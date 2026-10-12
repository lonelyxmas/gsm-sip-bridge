# Research: Correct decline signaling on inbound IMS calls

All decisions below were checked against the code on `main` at `d7cbc1e`.

## R1. Where the telephone side's final status comes from

**Decision**:
- Capture PJSIP's `last_status` in the call-state callback when a call
  reaches `DISCONNECTED`.
- Store it keyed by a per-call token, not by call id. `Call::make` passes
  the token to PJSUA as the call's `user_data`. PJSUA reuses call ids, and
  all lines share one endpoint, so another line's call can take a freed id
  before the first call's owner reads its status (PR #110 review). Tokens
  are never reused. The store keeps at most the last 64 entries.
- Expose it as `pjsua_safe::Call::last_status() -> Option<u16>`. This reads
  only the stored entry, never the live call info, which may by then belong
  to the call that reused the id. For the same reason, `poll_state` reports
  `Disconnected` once the id's `user_data` is no longer this call's token.
- Record every disconnect, with or without a status, and have `poll_state`
  report `Disconnected` for a placed call only once that record exists.
  PJSIP marks the call `DISCONNECTED` before it runs the callback, so
  without this an instant reject could be read before its status was
  captured. The live release gate caught exactly that: a `603` sent
  within a millisecond of ringing went to the carrier as `480`.
- Stub builds keep a settable field instead.

**Rationale**: Agent B polls `poll_state()` every 100 ms. As far as we know
PJSIP's behaviour, PJSUA resets the call slot right after the `DISCONNECTED`
callback returns. `pjsua_call_get_info` then fails, and `poll_state` already
treats that failure as `Disconnected` (`pjsua-safe/src/call.rs:300`). So by
the time Agent B sees `Disconnected`, the live call info is usually gone.
The callback is the only point where `last_status` is guaranteed readable,
and the existing disconnect log line (`pjsua-safe/src/endpoint.rs:916`)
already reads it there.

**Alternatives considered**:
- *Read `last_status` from `pjsua_call_get_info` after `poll_state`.* This
  usually fails because the slot has been reset. Genuine rejects would then
  be classified "failed" (`480`), which brings the T2 symptom back.
- *A callback trait on `Call`.* The crate deliberately has none (see
  `INCOMING_CALLS` docs). A static map follows the existing `BRIDGE_PAIRS`
  idiom.

## R2. Where the classification happens, and the control wire format

**Decision**: Agent B classifies the outcome and picks the `BridgeFailed`
reason:
- `pbx_rejected` for 486, 600 or 603
- `pbx_no_answer` for its own 45 s timer, or a telephone-side 480 or 408
- `pbx_failed`, a new constant, for everything else, including an unknown
  status

Agent A maps the reason to the carrier response: `pbx_rejected` gives `486`,
anything else gives `480`. `ControlMessage::BridgeFailed { call_id, reason }`
is unchanged.

**Rationale**:
- No wire-format change means no version-skew question between the two
  halves.
- Agent A already treats `reason` as a free-form string (`control.rs`), and
  any unknown reason falls through to `480`. That is the safe, non-busy
  default.
- The original numeric status only matters for Agent B's own call record
  (FR-007), and Agent B holds it.

**Alternatives considered**:
- *Add `sip_status: Option<u16>` to `BridgeFailed` and classify in Agent A.*
  More moving parts, a serde default to maintain, and no consumer for the
  number on Agent A's side.

## R3. Making the To-tag consistent

**Decision**:
- `LoopState::handle_inbound_invite` generates the To-tag once and passes it
  into `inbound::handle_invite`, which passes it on to
  `handle_offerless_invite`.
- Every response those functions send uses it: the `180`, the setup-failure
  decline, and the pre-ringing rejects.
- The generic `Err` fallback in `handle_inbound_invite` uses the same tag.

**Rationale**:
- The `Err` path (`agent/mod.rs:2356`) can't know the tag today because it
  is created inside `handle_invite`. Creating it one level up is the
  smallest change that makes the tag available on every path.
- The pre-ringing rejects (`420`/`415`/`488`/`580`) are sent before any
  provisional response carries a tag, so any tag is valid for them. Using
  the shared one is simpler than keeping a second source.

**Alternatives considered**:
- *Return the tag inside the error type.* That means threading it through
  `BridgeError`, which is used everywhere.
- *Have `handle_invite` send its own fallback `480`.* Two places would then
  own "send a final response on error", and they could both send one.

**Known residue, out of scope**: an `Err` raised after the `200 OK` has been
sent leads the fallback to send a `480` too. Examples are a relay spawn
failure (`inbound.rs:558`) and an offerless ACK wait failure. The tag is
consistent after this change, but the extra final response predates this
feature. Recorded for a separate fix.

## R4. Recognising the ACK for a declined call

**Decision**: `LoopState` gets a small `RecentDeclines` list of Call-IDs:
- It holds at most 64 entries for at most 32 s (64·T1).
- An entry is recorded whenever `handle_inbound_invite` finishes without an
  active call, which covers every failure response it sends.
- `log_ack` checks it after the active call and logs a match at `debug`.

**Rationale**:
- An ACK to a non-2xx response has the INVITE's Call-ID (RFC 3261
  §17.1.1.3), so Call-ID alone is enough for a diagnostic.
- The list never affects admission (FR-010). It is not the response cache
  the issue proposed.

**Alternatives considered**:
- *Match the full transaction key (Via branch plus CSeq).* More precise than
  a log line needs.

## R5. Metric reasons

**Decision**:
- `BridgeFailureReason` gets a new `PbxFailed` variant (`"pbx_failed"`).
- `map_bridge_failure_reason` maps:
  - `pbx_rejected` → `PbxDeclined`
  - `pbx_no_answer` → `RingTimeout`
  - `pbx_failed` → `PbxFailed`
  - `pbx_unreachable` → `BridgeSetupFailed`
- The ringing path (`inbound.rs:474` and `:821`) reports
  `map_bridge_failure_reason(reason)` instead of a hard-coded `PbxDeclined`.

**Rationale**:
- Today Agent B's 45 s no-answer arrives as `BridgeFailed` and is counted as
  `pbx_declined`. "PBX unreachable" is counted as `pbx_declined` too. Both
  contradict FR-007a.
- The enum is internal to the binary; the observability protocol runs
  between processes of the same build. No alert or dashboard in the repo
  uses `pbx_declined`.

## R6. RFC basis (from triage)

- RFC 3261 §13.3.1.3: a UAS unwilling or unable to take the call SHOULD send
  `486`.
- RFC 3261 §21.4.18: `480` means "the user could also be available
  elsewhere", which licenses the network to retry. This is T2's observed
  behaviour, plausibly T-ADS (3GPP TS 23.292 §7.4.3).
- RFC 3261 §17.1.1.2: an INVITE client stops retransmitting once a
  provisional response arrives, and never retransmits over TCP. This is why
  the issue's response cache is out of scope.
- RFC 3261 §17.2.1, Timer G: the real gap for UDP. Tracked separately.
