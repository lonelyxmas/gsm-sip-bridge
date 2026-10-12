# Tasks: Correct decline signaling on inbound IMS calls

**Input**: Design documents from `specs/084-fix-inbound-decline-signaling/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/decline-signaling.md, quickstart.md

**Tests**: Included. The constitution requires TDD, so each story writes
its failing tests first.

**Commit rule**: run `make format && make lint && make test` before every
commit (CLAUDE.md). Make one commit per phase.

## Phase 1: Setup

No setup is needed. The workspace, toolchain and test harness already exist.

## Phase 2: Foundational (blocks US1 and US2)

**Purpose**: make the telephone side's final SIP status readable from a
`pjsua_safe::Call`. This covers research R1.

- [X] T001 Add a `last_status: Option<u16>` field to the stub (`not(feature = "pjsip-linked")`) `Call`, plus `pub fn set_last_status(&mut self, Option<u16>)`, and initialise it to `None` in `Call::make` and `Call::from_id`, in pjsua-safe/src/call.rs. Do not add the field to linked builds, or keep it unused there behind cfg.
- [X] T002 Add `static DISCONNECT_STATUS: LazyLock<Mutex<HashMap<i32, u16>>>` (pjsip-linked) in pjsua-safe/src/endpoint.rs:
  - In `on_call_state_cb`'s `DISCONNECTED` arm, insert `(call_id, info.last_status as u16)` when `last_status > 0`.
  - In every other state, remove `call_id`.
  - Add `pub(crate) fn disconnect_status(call_id: i32) -> Option<u16>`.
- [X] T003 Add `pub fn last_status(&self) -> Option<u16>` to `Call` in pjsua-safe/src/call.rs:
  - Linked builds: return `pjsua_call_get_info`'s `last_status` if the call still exists and the value is > 0, else `endpoint::disconnect_status(self.call_id)`.
  - Stub builds: return the field.
  - Add a doc comment explaining why the callback capture is needed (the slot resets after `DISCONNECTED`).
- [X] T004 Add a unit test in pjsua-safe/src/call.rs: a stub `Call`'s `last_status()` is `None` by default and round-trips `set_last_status`.

**Checkpoint**: `make test` passes. Commit "feat(pjsua-safe): expose a call's final SIP status".

## Phase 3: User Story 1 + User Story 2, decline outcomes (P1) 🎯 MVP

**Goal**:
- A telephone-side reject (486, 600 or 603) reaches the carrier as `486`.
- No-answer, other failures and setup failure reach the carrier as `480`.
- The metrics and call records distinguish the three outcomes.

US1 and US2 share one classification and one mapping, so they are built
together. Splitting them would mean shipping `486` for every failure in
between, the exact regression US2 exists to prevent.

**Independent test**: unit tests drive `await_pbx_answer` over a real local
UDP socket with each `BridgeFailed` reason and assert the status line. A
classification table covers every status.

### Tests first

- [X] T005 [P] [US1] Add a test `classify_pbx_disconnect_maps_every_status` in gsm-sip-bridge/src/vowifi/mod.rs:
  - 486, 600 and 603 → Rejected
  - 480 and 408 → NoAnswer
  - 404, 401, 488, 500, 503 and `None` → Failed
  - assert each `reason()` string: `pbx_rejected`, `pbx_no_answer`, `pbx_failed`
- [X] T006 [P] [US1] Add a test in gsm-sip-bridge/src/vowifi/mod.rs: `wait_for_pbx_answer` with a stub `Call` already `set_state(Disconnected)` and `set_last_status(Some(603))` returns Rejected. With `None` it returns Failed.
- [X] T007 [P] [US2] Add a test `decline_response_maps_reasons` in gsm-sip-bridge/src/ims/agent/inbound.rs:
  - `pbx_rejected` → `SIP/2.0 486 Busy Here`
  - `pbx_no_answer`, `pbx_failed`, `pbx_unreachable` and an unknown reason → `SIP/2.0 480 Temporarily Unavailable`
  - every response carries the given To-tag
- [X] T008 [P] [US1] Add a test in gsm-sip-bridge/src/ims/agent/inbound.rs: `await_pbx_answer`, given a `BridgeFailed { reason: "pbx_rejected" }` on the control channel, sends `486` with the call's To-tag to a real local UDP "carrier" socket. Repeat for `pbx_no_answer` → `480` and `pbx_failed` → `480`. Return `RingOutcome::PbxDeclined { reason }` carrying the reason.
- [X] T009 [P] [US2] Extend the tests in gsm-sip-bridge/src/ims/observability.rs: `pbx_rejected` → `PbxDeclined`, `pbx_no_answer` → `RingTimeout`, `pbx_failed` → `PbxFailed`, `pbx_unreachable` → `BridgeSetupFailed`. Update the existing assertion that maps `pbx_unreachable` to `PbxDeclined`.

### Implementation

- [X] T010 [US1] Add `pub const PBX_FAILED: &str = "pbx_failed";` with a doc comment in gsm-sip-bridge/src/vowifi/control.rs, and fix the `BridgeFailed` doc ("486 Busy Here"): Agent A picks 486 or 480 from the reason.
- [X] T011 [US1] Rework `PbxOutcome` in gsm-sip-bridge/src/vowifi/mod.rs:
  - Variants: `Rejected { status: u16 }`, `NoAnswer { status: Option<u16> }`, `Failed { status: Option<u16> }`, plus the existing `Answered` and `CallerGone`.
  - Add `fn classify_pbx_disconnect(status: Option<u16>) -> PbxOutcome`.
  - Add `fn status(&self) -> Option<u16>`.
  - Map `reason()` per data-model.md.
  - `wait_for_pbx_answer` returns `classify_pbx_disconnect(pbx_call.last_status())` on `Disconnected`.
- [X] T012 [US2] In gsm-sip-bridge/src/vowifi/mod.rs's decline branch of the incoming-call handler, write the call-record outcome as `declined:{reason}`, plus `:{status}` when `outcome.status()` is `Some`.
- [X] T013 [US2] Add `PbxFailed` (`"pbx_failed"`) to `BridgeFailureReason` and `as_str` in gsm-sip-bridge/src/control/protocol.rs.
- [X] T014 [US2] Correct `map_bridge_failure_reason` in gsm-sip-bridge/src/ims/observability.rs per research R5.
- [X] T015 [US1] In gsm-sip-bridge/src/ims/agent/inbound.rs:
  - Add `fn decline_response(invite, to_tag, reason) -> String`, returning `486 Busy Here` for `reason::PBX_REJECTED` and `480 Temporarily Unavailable` otherwise, built with `build_uas_response`.
  - Use it in `await_pbx_answer`'s `BridgeFailed` arm.
  - Change `RingOutcome::PbxDeclined` to carry `reason: String`.
  - Narrow `await_pbx_answer`'s `inbound: &Inbound` to `sip_rx: &mpsc::Receiver<(SipMessage, SipSink)>` so it can be tested.
- [X] T016 [US2] In gsm-sip-bridge/src/ims/agent/inbound.rs, at both `RingOutcome::PbxDeclined` call sites (SDP path and offerless path), report `observability::map_bridge_failure_reason(&reason)` instead of the hard-coded `BridgeFailureReason::PbxDeclined`.
- [X] T017 [US2] In gsm-sip-bridge/src/ims/agent/inbound.rs, at both setup-time `BridgeFailed` replies (SDP and offerless), send `decline_response(req, &to_tag, &fail_reason)` instead of `build_486_busy_here(req, &random_hex(4))`. Setup failure then becomes `480`, with the `180`'s tag.

**Checkpoint**: `make test` passes. Commit "fix(ims): answer a telephone-side reject with 486, everything else 480 (#108)".

## Phase 4: User Story 3, one To-tag per INVITE (P2)

**Goal**: every response to an inbound INVITE after the busy and PBX-down
checks carries one To-tag, including the `Err` fallback. This covers
research R3.

**Independent test**: a unit test shows the `Err` fallback response uses
the tag it is given. Phase 3's tests already cover the decline paths.

- [X] T018 [P] [US3] Add a test in gsm-sip-bridge/src/ims/agent/mod.rs: `bridge_error_response(req, "tagX")` is a `480 Temporarily Unavailable` whose To header carries `tag=tagX`.
- [X] T019 [US3] In gsm-sip-bridge/src/ims/agent/inbound.rs, add a `to_tag: &str` parameter to `handle_invite` and `handle_offerless_invite`, and use it for every response in place of each `random_hex(4)`: `420`, `415`, `488` transport, `580`, the `180`, the `200` and the declines. Remove the local `let to_tag = random_hex(4)` lines.
- [X] T020 [US3] In gsm-sip-bridge/src/ims/agent/mod.rs `handle_inbound_invite`:
  - Create `let to_tag = random_hex(4);` after the busy and PBX-down checks.
  - Pass it to `inbound::handle_invite`.
  - Send the `Err` fallback via a new `fn bridge_error_response(req, to_tag) -> String`.

**Checkpoint**: `make test` passes. Commit "fix(ims): keep the 180's To-tag on every final response".

## Phase 5: FR-006, ACK for a declined call (supports US1)

- [X] T021 [P] [US1] Add tests in gsm-sip-bridge/src/ims/agent/mod.rs:
  - `RecentDeclines` remembers a Call-ID, forgets it after the TTL (use an injectable `now`), and evicts the oldest beyond 64.
  - `ack_disposition(...)` returns `ActiveCall`, `DeclinedCall` or `Unmatched`.
- [X] T022 [US1] Add `struct RecentDeclines` (a `VecDeque<(String, Instant)>`, at most 64 entries, 32 s TTL, with `record(call_id, now)` and `contains(call_id, now)`) and a `LoopState.recent_declines` field initialised in `LoopState::new`, in gsm-sip-bridge/src/ims/agent/mod.rs.
- [X] T023 [US1] In gsm-sip-bridge/src/ims/agent/mod.rs, record the INVITE's Call-ID in `recent_declines` on every `handle_inbound_invite` exit that leaves no active call: busy `486`, PBX-down `480`, `Ok(None)` and `Err`. Change `log_ack` to log a declined-call ACK at `debug` and warn only for `Unmatched`.

**Checkpoint**: `make test` passes. Commit "fix(ims): recognise the ACK for a call we declined".

## Phase 6: Polish

- [X] T024 [P] Add `pbx_failed` to the documented reason set in specs/014-vowifi-metrics-restore/contracts/observability-protocol.md, noting that it was added by specs/084.
- [X] T025 [P] Add an Unreleased entry to CHANGELOG.md and RELEASE_NOTES.md (under `## Unreleased`, renamed at release time), following repo style: bold summary, plain description.
- [X] T026 Run `make format && make lint && make test` on the whole workspace and confirm they are clean.
- [X] T027 Live release gate (FR-011, quickstart.md). Run on the Vodafone rig and the Jio Pi. This is manual, so record the results in the PR.
  - Done 2026-10-12 on `df509b8`, both rigs on VoWiFi; results are in PR #110.
  - Reject: `603` → `486` with the `180`'s To-tag, including an instant reject. The first run caught a status-capture race, fixed in `df509b8`.
  - Setup failure → `480`.
  - The Jio PBX rejects with `403`, which stays `480` by decision.
  - Both networks cancel an unanswered call at 30 s (`Q.850;cause=19`), before the bridge's 45 s timer, so the no-answer `480` is covered by unit tests only.

## Dependencies

- Phase 2 → Phase 3 (T011 needs `Call::last_status`).
- Phase 3 → Phase 4 (T019 changes the same functions as T015 and T017).
- Phase 5 depends only on the `handle_inbound_invite` shape after Phase 4.
- Phase 6 comes last. T027 needs a deployed build.

## Parallel opportunities

- T005–T009 are tests in different files or modules and can be written in
  parallel.
- T013 and T014 can run alongside T010–T012, since they are in different
  files.
- T024 and T025 can run in parallel.

## Implementation strategy

MVP = Phases 2 and 3: the reported symptom fixed without regressing
failures. Phases 4 and 5 are protocol hygiene and diagnostics. Each phase
is a green, separately revertable commit.
