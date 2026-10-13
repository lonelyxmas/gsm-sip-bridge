---
description: "Task list for specs/087-ike-sa-loss-recovery"
---

# Tasks: ePDG IKE_SA-loss recovery, post-recovery P-CSCF sync, graceful teardown and Gm SA cleanup

**Input**: `specs/087-ike-sa-loss-recovery/` — plan.md, spec.md, research.md (R1–R8), data-model.md, contracts/observability.md, quickstart.md
**Tests**: Included. The constitution makes TDD the default, and SC-007 makes automated tests plus a live gate a merge condition. Write each test first and watch it fail.

**Every commit** (CLAUDE.md, constitution II): `make format && make lint && make test` must pass first. Never commit real IMSIs, MSISDNs or IPs from the live line. Use synthetic values: `001010000000001@nai.epc.mnc001.mcc001.3gppnetwork.org`, inner `198.51.100.10` / `2001:db8::1`, P-CSCF `192.0.2.10`, `192.0.2.11`, ePDG `203.0.113.1`.

All paths are relative to the repo root. `supervise/` means `gsm-sip-bridge/src/supervise/`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- **[US1]** IKE_SA loss → rebuild + agent restart, with cause, plus flapping alert (FR-001–003, 002a, 014, 014a) · **[US2]** post-recovery P-CSCF sync (FR-004–007) · **[US3]** graceful-first teardown (FR-008–011) · **[US4]** Gm state cleared before agent start (FR-012–013)

---

## Phase 1: Setup

- [x] T001 Confirm the baseline on branch `087-ike-sa-loss-recovery` is green: run `make lint && make test` from the repo root. Stop if anything fails before code is touched

---

## Phase 2: Foundational (blocks US1, US3 and the alerting phase)

**Purpose**: the two pure parsers and the shared cause enum that several stories use.

- [x] T002 [P] Write failing tests in the `#[cfg(test)]` module of `gsm-sip-bridge/src/supervise/engines.rs` for a new `pub fn list_sas_has_ike_sa(sas_output: &str, conn_name: &str) -> bool`. Reuse the existing `ike_header(..)` / `child(..)` fixture helpers near line 810. Cases:
  - An `ESTABLISHED` header with an `INSTALLED` child → true.
  - A header alone → true.
  - A `CONNECTING` header (`ims0: #2, CONNECTING, IKEv2, …`) → true.
  - Empty output → false.
  - Only another line's header (`ims1: #1, …`) → false.
  - Only an indented child line with no header → false.
  - `ims10: #1` must not match `ims1` (prefix is `"{conn}: #"`).
- [x] T003 Implement `list_sas_has_ike_sa` in `gsm-sip-bridge/src/supervise/engines.rs` next to `list_sas_has_child_sa`. Match a line that does not start with a space and starts with `format!("{conn_name}: #")`. The doc comment cites research R1 and #106, and the live shape confirmed 2026-10-13
- [x] T004 [P] Add `IkeSaLossCause { CarrierDelete, DeadPeer, Unknown }` in `gsm-sip-bridge/src/control/protocol.rs`:
  - Derives `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`.
  - `#[serde(rename_all = "snake_case")]`.
  - `impl Display`: `carrier DELETE`, `dead-peer timeout`, `unknown`.
  - Doc comment: closed enum, for logs and alert text only.
  - A test asserting the serde names and the Display strings.
- [x] T005 Write failing tests in `gsm-sip-bridge/src/supervise/engines.rs` for `pub fn ike_sa_loss_cause(charon_log: &str, conn_name: &str) -> IkeSaLossCause`. Fixture lines use the real prefix shape: `… 09[IKE] <ims0|3> IKE_SA ims0[3] established between …`, `… <ims0|3> received DELETE for IKE_SA ims0[3]`, `… <ims0|3> giving up after 5 retransmits`. Cases:
  - DELETE after the last establishment → `CarrierDelete`.
  - Retransmit give-up after the last establishment → `DeadPeer`.
  - Neither → `Unknown`.
  - A DELETE *before* the most recent establishment is ignored → `Unknown`.
  - Both present → the later one wins.
  - Another connection's (`<ims1|…>`) DELETE is ignored.
  - An empty log → `Unknown`.
- [x] T006 Implement `ike_sa_loss_cause` in `gsm-sip-bridge/src/supervise/engines.rs` using `lines_for_conn`. Scan forward: reset on a line containing `IKE_SA {conn}[` + `] established`; set the cause on `received DELETE for IKE_SA` / `giving up after`. Doc comment per research R2: both are DBG1 and present at `default = 1`. Run `make format && make lint && make test` and commit: `feat(supervise): parse IKE_SA presence and IKE_SA loss cause`

**Checkpoint**: parsers and enum exist, nothing uses them yet, all green.

---

## Phase 3: User Story 1 — A carrier-deleted IKE_SA is rebuilt and the line re-registers (P1) 🎯 MVP

**Goal**: "no IKE_SA" becomes its own health state. It re-initiates and always restarts the agent, and its log line names the cause.

**Independent Test**: unit tests below. Live: quickstart 2b (`swanctl --terminate --ike ims0` → `recovered from IkeSaLost (cause: unknown)`, one agent restart, registered).

### Tests for User Story 1

- [x] T007 [P] [US1] Write failing tests in the `#[cfg(test)]` module of `gsm-sip-bridge/src/supervise/line_supervisor.rs`, using the existing fake engine with `health: RefCell<SteadyStateHealth>` (as the #118 tests near lines 1139–1300 do):
  - (a) `IkeSaMissing` → outcome `Recovered { reason: IkeSaLost, .. }`, exactly one `reinitiate`, **no** `terminate`, `tracking.child_sa_missing == 0` even if it was 1 before, and `tracking.child_sa_usable == false`.
  - (b) `ChildSaMissing` twice still gives `ChildSaMissing` then `ChildSaStuck` (regression).
  - (c) `IkeSaMissing` between two `ChildSaMissing` ticks resets the strike count, so the second `ChildSaMissing` is a first strike.
  - (d) `recovery_restarts_agent(DegradeReason::IkeSaLost)` is true. Extend the exhaustive-variant test near lines 1388–1411 with the new variant.
- [x] T008 [P] [US1] Write failing tests in `gsm-sip-bridge/src/supervise/engines.rs` for `StrongswanEngine::steady_state_health` with a `MockCommandRunner` seeded with list-sas outputs (mirror the existing `seed(..)` helper near line 977):
  - Empty output → `IkeSaMissing`.
  - Header only, or a `CONNECTING` header with no child → `ChildSaMissing`.
  - Header plus `INSTALLED` child → `Ok`.
  - A failing list-sas → `ViciBroken` (unchanged).
  - The tun-missing check still runs first.

### Implementation for User Story 1

- [x] T009 [US1] In `gsm-sip-bridge/src/supervise/line_supervisor.rs`:
  - Add `SteadyStateHealth::IkeSaMissing` and `DegradeReason::IkeSaLost`. The doc says: carrier DELETE or DPD clear; a new IKE_SA brings a new VIP and P-CSCF pool; the agent always restarts (spec clarification Q1).
  - Change `SteadyOutcome::Recovered` to `Recovered { reason: DegradeReason, ike_sa_loss_cause: Option<IkeSaLossCause> }`, and update every construction site in this file (`None` everywhere except IkeSaLost) and every match in `supervise/orchestrate.rs` (`Recovered { reason, .. }`).
  - Add a `SteadyStateHealth::IkeSaMissing` arm in `tick_steady_state`: reset `tracking.child_sa_missing = 0`, call `engine.reinitiate(runner)`, and return `Recovered { reason: DegradeReason::IkeSaLost, ike_sa_loss_cause: Some(engine.ike_sa_loss_cause(runner)) }`. This needs a `fn ike_sa_loss_cause(&self, runner: &dyn CommandRunner) -> IkeSaLossCause` on the `TunnelEngine` trait, with a default body returning `IkeSaLossCause::Unknown` so the SWu engine and test fakes need no change.
  - Narrow the docs of `SteadyStateHealth::ChildSaMissing`, `DegradeReason::ChildSaMissing`, `SteadyTracking::child_sa_missing` and `recovery_restarts_agent` to "lost under a live (listed) IKE_SA".
- [x] T010 [US1] In `gsm-sip-bridge/src/supervise/engines.rs`:
  - In `StrongswanEngine::steady_state_health`, after the list-sas success branch: if `!list_sas_has_ike_sa(&sas_output, &self.conn_name)`, print `[supervise] line {idx}: {conn} IKE_SA missing; re-initiating` and return `SteadyStateHealth::IkeSaMissing`. This goes before the CHILD_SA check.
  - Override `ike_sa_loss_cause` on `StrongswanEngine`: read `self.shared.charon_log` and call the T006 parser, returning `Unknown` on read error.
- [x] T011 [US1] In `gsm-sip-bridge/src/supervise/orchestrate.rs`, strongswan steady loop `Recovered` arm (~line 1696): print `[supervise] line {idx}: recovered from IkeSaLost (cause: {cause})` when `ike_sa_loss_cause` is `Some`, otherwise the existing `recovered from {reason:?}`. Store the cause in a loop-local `last_ike_sa_loss_cause: Option<IkeSaLossCause>` for the alerting phase. Make T007/T008 pass. Run `make format && make lint && make test` and commit: `fix(supervise): rebuild a lost IKE_SA and restart the agent (#106)`

**Checkpoint**: IKE_SA loss detected and recovered, with the cause logged. The P-CSCF sync is still the old, racy one (fixed in US2).

---

## Phase 4: User Story 2 — A recovery never causes a false "P-CSCF changed" (P1)

**Goal**: after every agent-restarting recovery, wait (≤20s) for the *new* negotiation, record its P-CSCF, then restart the agent exactly once.

**Independent Test**: unit tests below. Live: quickstart 2b/2c (`P-CSCF synced from new negotiation`, no `P-CSCF changed` on the next tick, one agent restart).

### Tests for User Story 2

- [x] T012 [P] [US2] Write failing tests in `gsm-sip-bridge/src/supervise/engines.rs` for `pub fn extract_pcscf_after(charon_log: &str, conn_name: &str, mark: usize) -> Option<String>`:
  - An old batch before `mark` plus nothing after → `None`.
  - Old batch, then a new batch after `mark` closed by `CHILD_SA ims0{4} established` → the new batch's `pick_stable_pcscf` (IPv4 preferred, minimum).
  - A new batch after `mark` *not yet closed* → `None`.
  - `mark > log.len()` (truncated) with a closed batch → found.
  - Another connection's batch after `mark` → `None`.
  - `mark` landing mid-line or on a non-char boundary must not panic: snap to the next `\n`, or use `get(mark..)` falling back to the whole log.
- [x] T013 [P] [US2] Write failing tests in `gsm-sip-bridge/src/supervise/orchestrate.rs` (`#[cfg(test)]`, `MockCommandRunner`) for a new helper `fn await_fresh_pcscf(engine: &StrongswanEngine, runner: &dyn CommandRunner, mark: usize, wait: Duration, poll: Duration, stop: &dyn Fn() -> bool) -> Option<String>`. The mock's charon-log reads return a growing log; mock `sleep` returns immediately and is counted. Cases:
  - The new batch appears on the 3rd read → `Some(addr)` after 2 sleeps.
  - Never appears → `None` after `wait / poll` sleeps.
  - `stop()` true → returns `None` at once.
- [x] T014 [P] [US2] Write failing tests for the agent-loop spawn gate in `gsm-sip-bridge/src/supervise/orchestrate.rs`. Extract the wait as `fn wait_for_rebuild_gate(flag: &AtomicBool, runner: &dyn CommandRunner, bound: Duration, stop: &dyn Fn() -> bool)`. Cases:
  - Returns immediately when clear.
  - Returns once the flag clears.
  - Returns at the bound even if the flag never clears.
  - Returns at once on `stop()`.

### Implementation for User Story 2

- [x] T015 [US2] In `gsm-sip-bridge/src/supervise/engines.rs`:
  - Change `latest_pcscf_batch` to also report whether the batch was closed (e.g. return `(Vec<String>, bool)`). Keep `extract_latest_pcscf` behaviour identical.
  - Implement `extract_pcscf_after` (T012).
  - Add inherent methods on `StrongswanEngine`: `pub fn log_mark(&self, runner) -> usize` (the charon log's `len()`, 0 on error) and `pub fn fresh_pcscf_since(&self, runner, mark) -> Option<String>`.
  - Add constants `POST_RECOVERY_PCSCF_WAIT: Duration = 20s` and `POST_RECOVERY_PCSCF_POLL: Duration = 1s`, with a doc citing research R4.
- [x] T016 [US2] In `gsm-sip-bridge/src/supervise/orchestrate.rs`:
  - Implement `await_fresh_pcscf` (T013) and `wait_for_rebuild_gate` (T014).
  - Create one `Arc<AtomicBool>` "rebuild in progress" per strongswan line. Pass it to both the steady loop and the agent loop (the closure/thread around line 1845). Find where the two are spawned for the line and thread it through the same context they already share.
  - In the agent loop, call `wait_for_rebuild_gate(&flag, runner, POST_RECOVERY_PCSCF_WAIT + 5s, &|| *shutting_down.read().unwrap())` immediately before the spawn. Do not hold the shutdown guard while waiting: take it only after the wait, as the loop does today.
- [x] T017 [US2] In the strongswan steady loop in `gsm-sip-bridge/src/supervise/orchestrate.rs`:
  - Take `let mark = engine.log_mark(runner.as_ref());` before `tick_steady_state`.
  - In the `Recovered` arm, after `drop(guard)`, when `recovery_restarts_agent(reason)`:
    1. Set the flag.
    2. `match await_fresh_pcscf(&engine, runner, mark, POST_RECOVERY_PCSCF_WAIT, POST_RECOVERY_PCSCF_POLL, &stop)`.
    3. `Some(p)`: write `line.pcscf_source_path`, set `current_pcscf = p`, print `P-CSCF synced from new negotiation: {p}`.
    4. `None` (and not shutting down): print `no new negotiation within 20s; restarting agent with P-CSCF {current_pcscf}`.
    5. Then `pkill`, then clear the flag.
  - For non-restarting reasons, keep no sync and no pkill (the CHILD_SA path cannot change the P-CSCF).
  - Delete the old immediate `latest_pcscf` sync, and replace its 2026-08-14 comment with an accurate one citing #106 triage and research R4/R5.
  - Make T012–T014 pass. Run `make format && make lint && make test` and commit: `fix(supervise): sync the P-CSCF from the new negotiation before restarting the agent`

**Checkpoint**: one agent restart per rebuild, registered against the right P-CSCF, no false `PcscfChanged`.

---

## Phase 5: User Story 3 — Teardown waits for the ePDG's acknowledgement (P2)

**Goal**: graceful delete first (5s), then a presence check, then forced delete only if still present or unknown. No fixed delay.

**Independent Test**: unit tests below. Live: quickstart 2a/2c (`IKE_SA deleted (acknowledged)`, no `forced`).

### Tests for User Story 3

- [ ] T018 [P] [US3] Update and extend the terminate tests in `gsm-sip-bridge/src/supervise/engines.rs`, replacing `terminate_is_scoped_to_this_lines_connection`'s single-call expectation. With `MockCommandRunner`:
  - (a) Graceful call argv is exactly `["timeout","8","env",<prefix>,"swanctl","--terminate","--ike","ims0","--timeout","5"]`, then a list-sas with `timeout 10` returning no header → **no** `--force` call.
  - (b) List-sas still shows the `ims0: #` header → a third call `["timeout","10","env",<prefix>,"swanctl","--terminate","--ike","ims0","--force"]`.
  - (c) List-sas fails (non-zero) → `--force` is issued.
  - (d) No call ever passes both `--force` and `--timeout`.
  - (e) No `sleep` is recorded.

### Implementation for User Story 3

- [ ] T019 [US3] Rewrite `StrongswanEngine::terminate` in `gsm-sip-bridge/src/supervise/engines.rs` per research R3:
  - Add constants `GRACEFUL_TERMINATE_WAIT_SECS = 5` and `GRACEFUL_TERMINATE_TIMEOUT_SECS = 8`.
  - Factor the bounded list-sas invocation out of `steady_state_health` into a private `fn list_sas(&self, runner) -> Option<String>` (`None` on failure) and use it in both places.
  - Print `[supervise] line {idx}: IKE_SA deleted (acknowledged)` or `[supervise] line {idx}: ePDG did not acknowledge DELETE within 5s; forced`.
  - Update the doc comment: the worst case is 28s under the shutdown read guard; the shutdown path (`shutdown.rs`) is intentionally unchanged; and why there is no sleep. Make T018 pass. Run `make format && make lint && make test` and commit: `fix(supervise): delete the IKE_SA gracefully before forcing it`

**Checkpoint**: every steady-state teardown is acknowledged on a responsive ePDG, and bounded on an unresponsive one.

---

## Phase 6: User Story 4 — Restarting the IMS agent leaves no Gm IPsec state behind (P2)

**Goal**: clear the line namespace's XFRM state before each strongswan-engine agent spawn.

**Independent Test**: unit tests below. Live: quickstart 2d (counts stable across 3 forced agent restarts).

### Tests for User Story 4

- [ ] T020 [P] [US4] Write failing tests in `gsm-sip-bridge/src/supervise/orchestrate.rs` for a new `fn clear_line_gm_state(runner: &dyn CommandRunner, idx: u32, netns: &str) -> (usize, usize)`, using `MockCommandRunner`:
  - (a) State and policy dumps with 4 and 8 `src ` entries → `run_in_netns(netns, ["ip","xfrm","state","flush"])` and `["ip","xfrm","policy","flush"]` are both issued, and the function returns `(4, 8)`.
  - (b) Both dumps empty → no flush.
  - (c) A dump query fails → still flush (state unknown, and no agent is running), returning `(0, 0)`.
  - (d) No call is made outside the netns, i.e. no plain `runner.run(["ip","xfrm",…])`.

### Implementation for User Story 4

- [ ] T021 [US4] Implement `clear_line_gm_state` in `gsm-sip-bridge/src/supervise/orchestrate.rs` per research R6. Print `[supervise] line {idx}: cleared {s} stale Gm SAs / {p} policies before agent start` only when nonzero.
- [ ] T022 [US4] Call `clear_line_gm_state` in the agent loop, immediately before `runner.spawn(... "vowifi-ims-agent" ...)` (~line 1850) and after the rebuild gate from T016. Call it **only for strongswan-engine lines**: find how the loop knows its engine (the line config's engine field, or the caller that sets up this loop) and gate on it. Add a comment that the SWu dialer was not verified to be XFRM-free in the netns (R6).
  - Add a test that the SWu path issues no flush. If the loop is not unit-testable, assert at the call-site helper level by passing the engine kind into a small `fn should_clear_gm_state(engine) -> bool`.
  - Run `make format && make lint && make test` and commit: `fix(supervise): clear leaked Gm IPsec state before each agent start`

**Checkpoint**: one registration's worth of Gm state, however many restarts.

---

## Phase 7: User Story 1 (cont.) — Flapping alert and loss cause in alerts (FR-014a, FR-002a)

**Goal**: the tunnel-failure alert also fires on ≥3 IKE_SA rebuilds within `unhealthy_sec`, once per episode, and its text names the last loss cause. Depends on Phases 3 and 4 (rebuild recording happens in the `Recovered` arm).

**Independent Test**: SC-006a unit tests. Live: quickstart 2e.

### Tests

- [ ] T023 [P] [US1] Write failing tests in `gsm-sip-bridge/src/supervise/line_supervisor.rs` for `pub struct RebuildWindow` (data-model), all with explicit `Instant`s:
  - 2 records within the window → not latched.
  - A 3rd within the window → `record` returns `true` (changed) and latches.
  - A 4th → stays latched, returns `false`.
  - `tick` before the newest entry is `window` old → stays latched.
  - `tick` after → unlatches, returns `true`.
  - 3 records spread wider than the window → never latches.
- [ ] T024 [P] [US1] Write failing tests in `gsm-sip-bridge/src/control/protocol.rs`:
  - A supervisor `AgentState { tunnel_flapping: Some(true), ike_sa_loss_cause: Some(CarrierDelete), .. }` round-trips with JSON keys `"tunnel_flapping":true` and `"ike_sa_loss_cause":"carrier_delete"`.
  - Both fields are omitted when `None`, and absent keys deserialize to `None`. Mirror `supervisor_report_round_trips_child_sa_up` / `child_sa_up_is_optional_on_the_wire`.
- [ ] T025 [P] [US1] Write failing tests in `gsm-sip-bridge/src/metrics/ingest.rs`, at `apply_report` level, reusing `child_sa_report`, `tunnel_cfg()` (unhealthy_sec = 300) and the existing alert-capture helpers used by `a_missing_child_sa_alone_alerts_once`:
  - (a) A supervisor report with `child_sa_up: true, tunnel_flapping: true` → exactly one TunnelFailure Failure is decided on that report, and its description contains `rebuilt repeatedly` and `last IKE_SA loss: carrier DELETE` when the cause was reported.
  - (b) A further flapping report → no second Failure.
  - (c) `tunnel_flapping: false` with `child_sa_up: true` → one Recovered.
  - (d) A continuous outage (`child_sa_up: false` past threshold) overlapping flapping → exactly one Failure total.
  - (e) No cause ever reported → no `last IKE_SA loss` suffix.
  - (f) An IMS-agent report never sets or clears flapping.
  - (g) The existing #118 fold tests are unchanged and pass.

### Implementation

- [ ] T026 [US1] Implement `RebuildWindow` and `pub const FLAP_REBUILDS: usize = 3` in `gsm-sip-bridge/src/supervise/line_supervisor.rs` (T023), with docs citing research R7 and spec clarification Q2
- [ ] T027 [US1] Add `tunnel_flapping: Option<bool>` and `ike_sa_loss_cause: Option<IkeSaLossCause>` to `AgentState` in `gsm-sip-bridge/src/control/protocol.rs`, both `#[serde(skip_serializing_if = "Option::is_none", default)]`, with docs (contracts/observability.md). Fix every `AgentState { .. }` literal across the workspace that does not use `..Default::default()` (`cargo build` will list them)
- [ ] T028 [US1] In `gsm-sip-bridge/src/metrics/ingest.rs`, per research R7/R8 and data-model "Ingest":
  - Add `tunnel_flapping: bool`, `last_ike_sa_loss_cause: Option<IkeSaLossCause>` and `streak_opened_by_flapping: bool` to `AgentRecord` (~line 92) and its constructors and test literals.
  - In `apply_state`'s supervisor branch (~line 715): update the stored flapping and cause from `Some(..)` values; the supervisor's health is `child_sa_up && !tunnel_flapping`. On becoming unhealthy because of flapping with no open streak, set `tunnel_unhealthy_since = Some(now - tunnel_threshold)` and `streak_opened_by_flapping = true`; any other unhealthy start opens at `now`; on healthy, clear both. `apply_state` needs the tunnel threshold, so pass it in from `apply_report_with`, where `cfg` is available (~line 228).
  - In `build_alert` (~line 525), produce the two Failure texts from contracts/observability.md using the supervisor record's `streak_opened_by_flapping` and `last_ike_sa_loss_cause`. Look the record up by key under the liveness lock, or pass the values in from `dispatch_transition`'s caller.
  - Make T024/T025 pass.
- [ ] T029 [US1] In `gsm-sip-bridge/src/supervise/orchestrate.rs`, strongswan steady loop:
  - Create `RebuildWindow::new(Duration::from_secs(ctx.config.alerts.tunnel_failure_thresholds.unhealthy_sec))` per line.
  - In the `Recovered` arm, when `recovery_restarts_agent(reason)`, call `rebuilds.record(Instant::now())`.
  - Call `rebuilds.tick(Instant::now())` once per loop iteration.
  - On any latch change, print `tunnel flapping ({n} rebuilds within {w}s)` / `tunnel no longer flapping`.
  - Extend the existing `child_sa_reporter` reporting so every report carries the current `child_sa_up`, `tunnel_flapping` and `ike_sa_loss_cause` (the last loss cause from T011). Replace the `report_child_sa(bool)` closure with one that builds the full supervisor `AgentState`, and call it whenever any of the three changes.
  - Run `make format && make lint && make test` and commit: `feat(alerts): raise the tunnel-failure alert for a flapping ePDG tunnel, naming the loss cause`

**Checkpoint**: SC-006a covered by tests, and alerts carry the cause.

---

## Phase 8: Polish & Cross-Cutting

- [ ] T030 [P] Add Unreleased entries to `RELEASE_NOTES.md` and `CHANGELOG.md`, in the existing style: bold only the summary (memory: release-notes bold scope). Cover IKE_SA-loss recovery with cause, the P-CSCF sync fix, graceful-first teardown, Gm state cleanup, and the flapping alert. Reference #106 and #120 (out of scope). Commit with T031
- [ ] T031 [P] In `config.toml.example` under `[alerts.tunnel_failure]` (~line 85), extend the comment: it also fires when a line's tunnel is rebuilt 3+ times within `unhealthy_sec`, and the alert text names the last IKE_SA loss cause. No new keys. Run `make format && make lint && make test` and commit: `docs: release notes and config note for IKE_SA-loss recovery`
- [ ] T032 Run the live gate in `specs/087-ike-sa-loss-recovery/quickstart.md` §2 (steps 2a–2f, 2g if it happens) on the local Vodafone line. Recreate, don't restart. Fill in the §3 results table, redacted. If 2a shows `CONNECTING` is **not** listed, stop and revisit research R1 before merging
- [ ] T033 Final pass: `make format && make lint && make test` on the branch tip. Confirm every FR/SC in the mapping table below is covered. Open the PR referencing #106 (and noting #120 and #107 as out of scope), ending with the Claude Code attribution line

---

## Requirement coverage

| Requirement | Tasks |
|---|---|
| FR-001 three-way health | T002–T003, T008, T010 |
| FR-002 IKE_SA loss → reinitiate, unconditional agent restart | T007, T009, T011 |
| FR-002a cause in log + alert text | T004–T006, T009–T011, T025, T028–T029 |
| FR-003 #118 path unchanged incl. CONNECTING | T007(b,c), T008 |
| FR-004–007 P-CSCF wait, marker, timeout, shutdown | T012–T017 |
| FR-008–011 graceful → presence → forced, no delay, bound | T018–T019 |
| FR-012–013 Gm clearing, scope, SWu exclusion | T020–T022 |
| FR-014 gauge down during IKE_SA loss | T007(a) (`child_sa_usable` false), existing #118 reporting |
| FR-014a flapping alert | T023–T029 |
| FR-015 no new config keys | T027–T031 (wire fields only) |
| FR-016 release notes | T030 |
| SC-001, SC-002, SC-003, SC-005 | T032 (2b, 2c, 2d) |
| SC-004 teardown bound | T018–T019 |
| SC-006 no #118 regression | T007(b), T032 (2f) |
| SC-006a flapping alert | T025, T032 (2e) |
| SC-007 gates | every commit, T032–T033 |

## Dependencies & Execution Order

- **Phase 1 → Phase 2** blocks everything.
- **US1 (Phase 3)** needs T002–T006.
- **US2 (Phase 4)** is independent of US1's code, but its live verification uses US1's IKE_SA loss (2b). It can also be verified alone with 2c.
- **US3 (Phase 5)** needs T003 (presence parser) only.
- **US4 (Phase 6)** is independent. T022 goes after T016 because both edit the agent-loop spawn site.
- **Phase 7** needs Phases 3 and 4: both touch the `Recovered` arm, and rebuild recording sits beside the sync.
- **Phase 8** last.
- Within each phase, the tests come first and must fail before the implementation.
- `orchestrate.rs` is edited by T011, T016–T017, T021–T022 and T029. Do these sequentially, never in parallel.

### Parallel opportunities

- Phase 2: T002 ∥ T004, since they are different files. T005 can follow T002 in the same file.
- Phase 3: T007 ∥ T008.
- Phase 4: T012 ∥ T013 ∥ T014. T013 and T014 share a file, but as test-only additions they can be written together.
- After Phase 2: US3's T018–T019 (engines.rs terminate) can run alongside US1's line_supervisor.rs tasks. Avoid concurrent edits to engines.rs with T010/T015.
- Phase 7: T023 ∥ T024 ∥ T025 (three files).
- Phase 8: T030 ∥ T031.

## Implementation Strategy

- **MVP**: Phases 1–3 (US1). This alone fixes the #106 loop, the agent stuck on a dead address. Ship-ready after its commit if needed.
- **Then**: US2, which removes the double restart that US1 still suffers on pool-rotating carriers. Then US3 and US4 (robustness), then Phase 7 (alerting), then the live gate.
- Each phase ends green and committed. Stop at any checkpoint and the branch is still coherent.
