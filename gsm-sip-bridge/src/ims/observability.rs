//! Agent A's (`ims::agent`) side of restoring call observability under
//! VoWiFi (specs/014-vowifi-metrics-restore). One `AgentObservability`
//! instance lives for the process's lifetime, wrapping the
//! `observability::reporter::Reporter` that carries events to the daemon
//! and (best-effort) the `StoreHandle` that writes call history rows
//! directly, independent of whether the daemon is reachable.
//!
//! All gauge state (`active_calls`, `registered`, `tunnel_up`) is cached
//! here and re-sent in full on every report, since `AgentState` is
//! "absolute, latest-wins" on the wire (contracts/observability-protocol.md)
//! — a caller only ever needs to say what changed, not reconstruct the rest.

use crate::control::protocol::{
    AgentState, BridgeFailureReason, CallStatus, ObservedEvent, RegistrationStatus,
};
use crate::observability::reporter::Reporter;
use crate::store::calls::CallRecord;
use crate::store::{StoreCommand, StoreHandle, Transport};
use crate::vowifi::control::reason;
use chrono::{DateTime, Utc};
use std::sync::Mutex;

pub struct AgentObservability {
    reporter: Reporter,
    module_id: String,
    /// `None` when the store could not be opened — reporting must never be
    /// able to take an agent down (FR-018), so a missing store degrades to
    /// "no history for this run" rather than a startup failure.
    store: Option<StoreHandle>,
    /// Reused for every call record (`[bridge].sip_destination`) — the same
    /// destination Agent B dials, since there is exactly one PBX destination
    /// for the whole bridge.
    sip_destination: String,
    /// Which transport this agent's call rows are filed under. Both the VoWiFi
    /// and VoLTE paths run this same code, so it must be told which it is or
    /// their history collapses into one — the store counterpart of the metric
    /// `transport` label.
    transport: Transport,
    state: Mutex<AgentState>,
    /// Where the active-call count is mirrored for this line's tunnel
    /// supervisor, which holds a stuck-rekey rebuild for a call (specs/093
    /// `contracts/files.md`). VoWiFi only; `None` on VoLTE.
    calls_file: Option<std::path::PathBuf>,
    /// Calls being set up, which [`Self::calls_file`] counts as occupying the
    /// line though the `active_calls` gauge does not: an inbound call ringing
    /// or an outbound one being placed would be cut by a rebuild as surely as
    /// a connected one.
    setup: Mutex<CallSetup>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct CallSetup {
    inbound: bool,
    outbound: bool,
    /// What the calls file was last written with, so an unchanged state is
    /// not rewritten on every dispatch-loop pass.
    written: Option<u32>,
}

impl AgentObservability {
    pub fn new(
        reporter: Reporter,
        module_id: String,
        store: Option<StoreHandle>,
        sip_destination: String,
        transport: Transport,
    ) -> Self {
        Self {
            reporter,
            module_id,
            store,
            sip_destination,
            transport,
            state: Mutex::new(AgentState::default()),
            calls_file: None,
            setup: Mutex::new(CallSetup::default()),
        }
    }

    /// Also mirror every active-call count into `path`.
    pub fn with_calls_file(mut self, path: std::path::PathBuf) -> Self {
        self.calls_file = Some(path);
        self
    }

    fn push(&self, events: Vec<ObservedEvent>) {
        let state = *self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.reporter.report(state, events);
    }

    pub fn set_active_calls(&self, n: u32) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active_calls = Some(n);
        self.sync_calls_file();
        self.push(Vec::new());
    }

    /// An inbound call is ringing (`true`) or its setup has ended either way.
    pub fn set_inbound_setup(&self, on: bool) {
        self.setup.lock().unwrap_or_else(|e| e.into_inner()).inbound = on;
        self.sync_calls_file();
    }

    /// An outbound call is being placed (`true`) or is not. Cheap to call on
    /// every dispatch-loop pass: the file is only rewritten on a change.
    pub fn set_outbound_setup(&self, on: bool) {
        self.setup
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .outbound = on;
        self.sync_calls_file();
    }

    /// Mirrors "how many calls occupy this line" (connected plus being set
    /// up) into the calls file, for the tunnel supervisor (specs/093).
    fn sync_calls_file(&self) {
        let Some(path) = &self.calls_file else {
            return;
        };
        let active = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active_calls
            .unwrap_or(0);
        let mut setup = self.setup.lock().unwrap_or_else(|e| e.into_inner());
        let n = active.max(u32::from(setup.inbound || setup.outbound));
        if setup.written == Some(n) {
            return;
        }
        // Best-effort like every other report: a write that fails costs only
        // the deferral, and the supervisor reads a missing file as "no call".
        match crate::ims::pcscf_files::write_active_calls(path, n) {
            Ok(()) => setup.written = Some(n),
            Err(e) => {
                tracing::debug!(error = %e, path = %path.display(), "could not write the active-call count")
            }
        }
    }

    pub fn set_registered(&self, registered: bool) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .registered = Some(registered);
        self.push(Vec::new());
    }

    /// Report when this line's registration lapses
    /// (specs/039-at-stall-watchdog, FR-018).
    ///
    /// Absolute, and reported only when it changes — once per renewal. The
    /// daemon turns it into a countdown at scrape time, so an operator can see
    /// at a glance that a binding lapsed hours ago rather than inferring it
    /// from a raw timestamp nobody converts by hand.
    pub fn set_registration_expiry(&self, expires_at: std::time::SystemTime) {
        let unix = expires_at
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .registration_expires_at = Some(unix);
        self.push(Vec::new());
    }

    pub fn set_tunnel_up(&self, up: bool) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .tunnel_up = Some(up);
        self.push(Vec::new());
    }

    /// Report the Gm signaling connection's health (specs/028). Called on
    /// state changes only — never per idle poll — so the reporter isn't asked
    /// to coalesce a steady stream of identical values.
    pub fn set_gm_connection_up(&self, up: bool) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .gm_connection_up = Some(up);
        self.push(Vec::new());
    }

    pub fn report_registration_attempt(&self, status: RegistrationStatus) {
        self.push(vec![ObservedEvent::RegistrationAttempt { status }]);
    }

    /// A call that never reached an answer: declined outright, the PBX
    /// leg failed to establish, the extension rang out, or the caller gave
    /// up while it rang. `status` is `Missed` when the PBX extension was
    /// actually reached and rang (a human could have picked up), `Failed`
    /// when the call never got that far (no usable codec, Agent B
    /// unreachable, an internal error).
    pub fn report_call_not_answered(
        &self,
        status: CallStatus,
        reason: BridgeFailureReason,
        caller: &str,
        started_at: DateTime<Utc>,
    ) {
        self.push(vec![
            ObservedEvent::CallCompleted {
                status,
                duration_seconds: 0.0,
            },
            ObservedEvent::BridgeFailed { reason },
        ]);
        self.insert_call_row(caller, started_at, 0.0, status);
    }

    /// Reports a call that reached an answer and then ended, judged by its
    /// media (FR-017): a call that carried audio both ways is `Answered`; one
    /// that carried one-way or no audio is a **failure**, recorded as `Failed`
    /// with the direction named, never as a success.
    pub fn report_call_answered_and_ended(
        &self,
        caller: &str,
        started_at: DateTime<Utc>,
        duration_seconds: f64,
        media: super::media_stats::DirectionVerdict,
    ) {
        if media.is_success() {
            self.push(vec![ObservedEvent::CallCompleted {
                status: CallStatus::Answered,
                duration_seconds,
            }]);
            self.insert_call_row(caller, started_at, duration_seconds, CallStatus::Answered);
        } else {
            self.push(vec![
                ObservedEvent::CallCompleted {
                    status: CallStatus::Failed,
                    duration_seconds,
                },
                ObservedEvent::BridgeFailed {
                    reason: BridgeFailureReason::OneWayAudio,
                },
            ]);
            self.insert_call_row(caller, started_at, duration_seconds, CallStatus::Failed);
        }
    }

    /// Reports one source's media-quality figures for a just-ended call
    /// (specs/046-rtcp-reporting FR-008a) — call once per source that
    /// actually has something to report; a source that never produced any
    /// figures (the far end never sent RTCP, or no media was received at
    /// all) is simply not reported, rather than reported as zero (FR-009's
    /// "never reported" distinction, carried through to the metrics
    /// surface as well as the log line).
    pub fn report_media_quality(
        &self,
        source: crate::control::protocol::QualitySource,
        loss_percent: f64,
        jitter_seconds: f64,
        round_trip_seconds: Option<f64>,
    ) {
        self.push(vec![ObservedEvent::MediaQuality {
            source,
            loss_percent,
            jitter_seconds,
            round_trip_seconds,
        }]);
    }

    /// A call proceeded with no RTCP endpoint obtainable at all (FR-017a).
    pub fn report_rtcp_unavailable(&self) {
        self.push(vec![ObservedEvent::RtcpUnavailable]);
    }

    fn insert_call_row(
        &self,
        caller: &str,
        started_at: DateTime<Utc>,
        duration_seconds: f64,
        status: CallStatus,
    ) {
        let Some(store) = &self.store else {
            return;
        };
        let record = CallRecord {
            module_id: self.module_id.clone(),
            caller_id: caller.to_string(),
            started_at: started_at.to_rfc3339(),
            duration_seconds,
            status: status.as_str().to_string(),
            sip_destination: self.sip_destination.clone(),
            transport: self.transport,
        };
        if let Err(e) = store.sender().send(StoreCommand::InsertCall(record)) {
            tracing::error!(error = %e, "failed to send call record to store");
        }
    }
}

/// Maps the free-form reason strings carried by `vowifi::control`'s
/// `ControlMessage::BridgeFailed`/`CallEnded` (`vowifi::control::reason`)
/// onto the closed `BridgeFailureReason` set (FR-014) — new carrier-facing
/// failure text must never be able to mint a new metric label; anything
/// unrecognised collapses to `BridgeSetupFailed`.
pub fn map_bridge_failure_reason(raw: &str) -> BridgeFailureReason {
    match raw {
        // Only a genuine decline is `pbx_declined` (specs/084 FR-007a): a PBX
        // that could not even be reached at setup is a setup failure, and a
        // PBX leg that died before answer is `pbx_failed`, not a busy human.
        reason::PBX_REJECTED => BridgeFailureReason::PbxDeclined,
        reason::PBX_UNREACHABLE => BridgeFailureReason::BridgeSetupFailed,
        reason::PBX_FAILED => BridgeFailureReason::PbxFailed,
        reason::PBX_NO_ANSWER => BridgeFailureReason::RingTimeout,
        reason::CALLER_CANCELLED => BridgeFailureReason::CallerCancelled,
        reason::VETH_LEG_FAILED => BridgeFailureReason::BridgeSetupFailed,
        reason::TRANSPORT_ERROR => BridgeFailureReason::AgentUnreachable,
        _ => BridgeFailureReason::BridgeSetupFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_in_progress_or_being_set_up_are_mirrored_into_the_calls_file() {
        let dir = std::env::temp_dir().join(format!("obs-calls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pcscf-0.calls");
        let obs = AgentObservability::new(
            Reporter::spawn(
                dir.join("no-daemon.sock").display().to_string(),
                crate::control::protocol::AgentKind::Ims,
                "card0".into(),
                std::time::Duration::from_secs(3600),
            ),
            "card0".into(),
            None,
            "sip:pbx@192.0.2.1".into(),
            Transport::Vowifi,
        )
        .with_calls_file(path.clone());
        let read = || std::fs::read_to_string(&path).unwrap();
        obs.set_active_calls(1);
        assert_eq!(read(), "1\n");
        obs.set_active_calls(0);
        assert_eq!(read(), "0\n");
        // A call being set up occupies the line before the gauge counts it.
        obs.set_inbound_setup(true);
        assert_eq!(read(), "1\n", "ringing");
        obs.set_active_calls(1);
        obs.set_inbound_setup(false);
        assert_eq!(read(), "1\n", "answered: still occupied");
        obs.set_active_calls(0);
        assert_eq!(read(), "0\n");
        obs.set_outbound_setup(true);
        assert_eq!(read(), "1\n", "dialing");
        obs.set_outbound_setup(false);
        assert_eq!(read(), "0\n", "the attempt failed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn map_bridge_failure_reason_covers_every_known_reason_constant() {
        assert_eq!(
            map_bridge_failure_reason(reason::PBX_UNREACHABLE),
            BridgeFailureReason::BridgeSetupFailed
        );
        assert_eq!(
            map_bridge_failure_reason(reason::PBX_FAILED),
            BridgeFailureReason::PbxFailed
        );
        assert_eq!(
            map_bridge_failure_reason(reason::PBX_REJECTED),
            BridgeFailureReason::PbxDeclined
        );
        assert_eq!(
            map_bridge_failure_reason(reason::PBX_NO_ANSWER),
            BridgeFailureReason::RingTimeout
        );
        assert_eq!(
            map_bridge_failure_reason(reason::CALLER_CANCELLED),
            BridgeFailureReason::CallerCancelled
        );
        assert_eq!(
            map_bridge_failure_reason(reason::VETH_LEG_FAILED),
            BridgeFailureReason::BridgeSetupFailed
        );
        assert_eq!(
            map_bridge_failure_reason(reason::TRANSPORT_ERROR),
            BridgeFailureReason::AgentUnreachable
        );
    }

    #[test]
    fn map_bridge_failure_reason_falls_back_for_unknown_strings() {
        assert_eq!(
            map_bridge_failure_reason("some_future_reason_never_seen_before"),
            BridgeFailureReason::BridgeSetupFailed
        );
    }
}
