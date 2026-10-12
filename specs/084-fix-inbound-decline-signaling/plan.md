# Implementation Plan: Correct decline signaling on inbound IMS calls

**Branch**: `084-fix-inbound-decline-signaling` | **Date**: 2026-10-11 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/084-fix-inbound-decline-signaling/spec.md`

## Summary

Issue #108: T2 re-offers an inbound call after the bridge answers `480`
to a telephone-side reject. The fix has six parts:

1. **pjsua-safe**: make the telephone-side leg's final SIP status
   available by capturing it in PJSIP's disconnect callback.
2. **Agent B** (telephone side): classify the outcome as rejected,
   no-answer or failed, and report it in the existing `BridgeFailed.reason`
   string.
3. **Agent A** (IMS side): map the reason to the carrier response.
   `pbx_rejected` gives `486`; anything else, including setup failure,
   gives `480`.
4. **Agent A**: make the To-tag consistent by creating it once per INVITE,
   one level up, so every response, including the `Err` fallback, uses it.
5. **Agent A**: keep a bounded recent-declines list, so the ACK for a
   decline isn't logged as unmatched.
6. **Metrics**: correct the `bridge_failed` reasons and add `pbx_failed`.

The control message format doesn't change. See [research.md](research.md)
R1–R6.

## Technical Context

**Language/Version**: Rust (workspace toolchain pinned in `rust-toolchain.toml`)
**Primary Dependencies**: `pjsua-safe`/`pjsua-sys` (PJSIP FFI, `pjsip-linked` feature), std `mpsc`/`net`
**Storage**: N/A (in-memory call history only)
**Testing**: `cargo test` via `make test`. Stub (non-linked) PJSIP builds are used for unit tests, plus real local UDP sockets for SIP send paths.
**Target Platform**: Linux (x86_64 host, arm64 Pi), Docker
**Project Type**: Daemon. A two-agent IMS↔SIP bridge for VoWiFi and VoLTE.
**Performance Goals**: Unchanged. The decline reaches the carrier within one 100 ms poll of the telephone side ending.
**Constraints**:
- No change to the control wire format.
- No new dependencies.
- No real subscriber numbers in tests.
**Scale/Scope**: About 5 source files plus tests. One new reason constant and one new metric label value.

## Constitution Check

*GATE: must pass before Phase 0 research, and again after Phase 1 design.*

| Principle | Assessment |
|---|---|
| I. Integration-first testing | **Pass.** `await_pbx_answer` is tested against real local UDP sockets and real `mpsc` channels. The classification and mapping functions are pure. The only stand-in is the stub PJSIP `Call`, which this crate already uses because PJSIP and a PBX aren't available in CI; its justification is existing practice. |
| II. Green-on-commit | **Pass.** `make format`, `make lint` and `make test` run before every commit (CLAUDE.md checklist). |
| III. Frequent atomic commits | **Pass.** One commit per user-story slice (see tasks.md). |
| IV. Makefile-driven build | **Pass.** No new targets needed. |
| V. Simplicity | **Pass.** The wire format is reused (R2). The To-tag moves up one level instead of being threaded through errors (R3). The ACK aid is a bounded `VecDeque` (R4). The issue's response cache is rejected. |

Re-check after design: **pass**. No violations, so no complexity tracking is
needed.

## Project Structure

### Documentation (this feature)

```text
specs/084-fix-inbound-decline-signaling/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/decline-signaling.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code (files touched)

```text
pjsua-safe/src/
├── endpoint.rs        # DISCONNECT_STATUS (keyed by per-call token), filled in on_call_state_cb
└── call.rs            # Call::last_status(); stub field + set_last_status()

gsm-sip-bridge/src/
├── vowifi/control.rs  # reason::PBX_FAILED; BridgeFailed doc fix
├── vowifi/mod.rs      # PbxOutcome {Rejected, NoAnswer, Failed}{status}; classify_pbx_disconnect; call-record suffix
├── control/protocol.rs# BridgeFailureReason::PbxFailed
├── ims/observability.rs # map_bridge_failure_reason corrected
├── ims/agent/inbound.rs # decline_response(); to_tag parameter; await_pbx_answer takes a SIP rx; RingOutcome::PbxDeclined{reason}
└── ims/agent/mod.rs   # to_tag created in handle_inbound_invite; Err fallback uses it; RecentDeclines + log_ack
```

**Structure Decision**: Existing workspace layout. No new modules.

## Risks

- **R1 depends on PJSUA's callback order** (the slot resets after the
  `DISCONNECTED` callback). If the order were different, the live-info read
  in `last_status()` would cover it. In either case a missing status falls
  back to `480`, never `486`.
- **`486` on Jio and Vodafone VoLTE**: these carriers are sensitive to
  response details. The live release gate (FR-011) covers this.
