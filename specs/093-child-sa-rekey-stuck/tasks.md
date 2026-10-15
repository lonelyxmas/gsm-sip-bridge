# Tasks: Rebuild the ePDG tunnel on a stuck CHILD_SA rekey (093)

**Input**: `specs/093-child-sa-rekey-stuck/` (spec.md, plan.md, research.md, data-model.md, contracts/, quickstart.md)

**Tests**: required. The spec's acceptance and the constitution's TDD default ask for them, and they are written before the code they cover.

All paths are relative to `gsm-sip-bridge/src/` unless stated otherwise. Before every commit: `make format && make lint && make test`, with a private `CARGO_TARGET_DIR` (memory: shared target dir). Every commit message ends with the Co-Authored-By trailer.

Fixtures use synthetic identities only (`001010000000001`, `192.0.2.x`, `203.0.113.x`, `2001:db8::/32`). Never paste the live sample's IMSI or addresses (CLAUDE.md).

## Phase 1: Setup

- [X] T001 Confirm the branch is `093-child-sa-rekey-stuck` on top of `origin/main`, and that `make test` is green before any change.

## Phase 2: Foundational (blocks US1–US3)

- [X] T002 In `supervise/line_supervisor.rs`, add `pub struct ChildSaLifetime { pub unique_id: u32, pub expires_in: Option<i64> }` (`Debug, Clone, Copy, PartialEq, Eq`), with a doc comment per data-model.md. Change `SteadyStateHealth::Ok` to `Ok(Option<ChildSaLifetime>)`, documented as: "`None` when the engine reports no CHILD_SA lifetimes (SWu)". Update every use: `matches!(health, SteadyStateHealth::Ok(_))` at the `child_sa_usable` line, the `SteadyStateHealth::Ok(_) => {}` match arm, the `FakeEngine` defaults and test assignments (`SteadyStateHealth::Ok(None)`), `supervise/engines.rs` (SWu returns `Ok(None)`; strongSwan temporarily `Ok(None)`), and the `engines.rs` test assertion. There is no behaviour change, so `make test` stays green.
- [X] T003 [P] In `supervise/engines.rs`, add tests first, then `pub fn newest_child_sa_lifetime(sas_output: &str, conn_name: &str) -> Option<ChildSaLifetime>` per plan.md design note 1. It matches CHILD_SA headers exactly as `list_sas_has_child_sa` does (factor out a shared header parser returning `(unique_id, state)` so the two cannot drift). It reads `expires in Ns` from the immediately following four-space-indented line, and returns the usable header with the highest `#N`. Add a test helper `child_with_life(conn, n, state, installed, rekeying, expires)` that renders the real 5.9.3 shape from research.md R1 (`    installed 1109s ago, rekeying in 2167s, expires in 2852s`). Table test `child_sa_lifetime_table` cases:
  - single `INSTALLED #12` → `Some{12, Some(2852)}`
  - natural overlap `REKEYED #12` (expires 300) plus `INSTALLED #13` (expires 3960) → `#13`, 3960
  - `REKEYING #12` with `expires in 290s` → `#12`, 290
  - `INSTALLED #12` plus non-usable `INSTALLING #13` → `#12`
  - no `expires in` on the details line → `Some{12, None}`
  - negative `rekeying in -40s` with `expires in 280s` → 280
  - only another connection's CHILD_SA → `None`
  - `ims10` is not matched when looking for `ims1`
  - empty input → `None`

  Also fix the existing `child()` helper's details line to the real `installed 10s ago` form.
- [X] T004 Wire `StrongswanEngine::steady_state_health` in `supervise/engines.rs` to return `SteadyStateHealth::Ok(newest_child_sa_lifetime(&sas_output, &self.conn_name))`. Extend the existing healthy-path engine test to assert the lifetime it now returns. Commit: `refactor(supervise): report the newest CHILD_SA's lifetime with a healthy tick (093)`.

## Phase 3: User Stories 1 + 2: rebuild a stuck rekey once, never on a natural rekey (P1) 🎯 MVP

**Goal**: with `expires_in ≤ 300s` on the same CHILD_SA, do exactly one planned rebuild with `ChildSaRekeyStuck`. A natural rekey never triggers one.
**Independent test**: the tick tests below, plus the orchestrator test (T011).

- [ ] T005 [P] [US1] In `supervise/engines.rs`, add tests first, then `pub fn child_sa_rejection_notify(charon_log: &str, conn_name: &str) -> Option<String>` (research R4). It uses the same session scoping as `child_rekey_invalid_selectors_session`: the latest `IKE_SA <conn>[N] established` resets it. It returns the last `N(...)` name from a `CREATE_CHILD_SA response` line whose payload list is only notifies, e.g. `parsed CREATE_CHILD_SA response 12 [ N(INVAL_SEL) ]` → `INVAL_SEL`. A success response (`[ SA No TSi TSr ]`) clears the reason. Table test cases: INVAL_SEL; TS_UNACCEPT; rejection then success → `None`; rejection in a previous session → `None`; another connection's line → `None`.
- [ ] T006 [US1] Add a `TunnelEngine` trait method `fn child_sa_rejection(&self, _runner: &dyn CommandRunner) -> Option<String> { None }` in `supervise/line_supervisor.rs`, and implement it for `StrongswanEngine` in `supervise/engines.rs` over the shared charon log (the same pattern as `rekey_invalid_selectors_session`).
- [ ] T007 [US1] In `supervise/line_supervisor.rs`, add:
  - `DegradeReason::ChildSaRekeyStuck`, with a doc comment: the same CHILD_SA outlived the natural-rekey window, rebuilt before its hard expiry (#127).
  - `pub const REKEY_STUCK_THRESHOLD: Duration = 300s` and `pub const REKEY_STUCK_FORCE_MARGIN: Duration = 90s`, with doc comments carrying research R2/R6's reasoning.
  - `SteadyTracking` fields `rekey_stuck_rebuilt`, `rekey_stuck_deferral_logged` and `lifetime_missing_logged` (all `Option<u32>`), documented per data-model.md.

  Assert in a test that `recovery_restarts_agent(ChildSaRekeyStuck)` and `recovery_renegotiates(ChildSaRekeyStuck)` are both true.
- [ ] T008 [US1] [US2] Write the failing tick tests in `supervise/line_supervisor.rs` `mod tests`, using `FakeEngine` with `health = Ok(Some(ChildSaLifetime{..}))`. Extend `FakeEngine` to record `child_sa_rejection` calls and return a configurable reason. Tests, all prefixed `rekey_stuck_`:
  - `..._rebuilds_at_threshold`: expires 300 → `Recovered{ChildSaRekeyStuck}`, one terminate, one reinitiate.
  - `..._above_threshold_is_still_up`: expires 301 → `StillUp`, no terminate.
  - `..._rebuilds_once_per_child_sa`: a second tick on the same `#N` (expires 270) → `StillUp`, still one terminate.
  - `..._new_child_sa_rearms`: `#13` with expires 3960, then `#13` at 290 → a second rebuild.
  - `..._natural_rekey_overlap_never_rebuilds`: a sequence of `#12` at 700, 400, 370, then `#13` at 3960 → all `StillUp`.
  - `..._structural_failure_takes_precedence`: health `ChildSaMissing` → the existing `ChildSaMissing` path, guards cleared.
  - `..._no_lifetime_logs_once_and_never_rebuilds`: `expires_in: None` twice → `StillUp` both times, and `lifetime_missing_logged == Some(12)`.
  - `..._swu_none_does_nothing`: `Ok(None)` → `StillUp`.
  - `..._process_died_clears_guards`.
- [ ] T009 [US1] [US2] Implement the decision in `tick_steady_state_with_pool` (`supervise/line_supervisor.rs`), right after the structural `match health` and before the P-CSCF-change check, following the data-model.md decision table. Log the lines in contracts/logs.md: rebuild, with the rejection reason when `engine.child_sa_rejection(runner)` returns `Some`, and the missing-lifetime line. The process-died branch and any non-`Ok(Some)` health reset the three guards. A guard whose `#N` differs from the current one is treated as unset. For now, `call_active` is not yet a parameter and the deferral rows are not implemented (US3). T008's tests pass.
- [ ] T010 [P] [US2] In `supervise/line_supervisor.rs` tests, add a 6-hour simulated soak: 720 ticks at 30s with a CHILD_SA that is naturally rekeyed every ~3300s (expires falls from 3960, and the `#N` changes when expires reaches 400–660). Assert zero `ChildSaRekeyStuck` (SC-002). Add a #107-style replay: the same `#N` from 3960 down to 0, ticking every 30s → exactly one rebuild, issued with expires ≥ 270 (SC-001/SC-003).
- [ ] T011 [US1] In `supervise/orchestrate.rs` tests, using the existing strongswan steady-state harness on `MockCommandRunner`, feed `--list-sas` output with `expires in 290s`. Assert that the `recovered from ChildSaRekeyStuck` path runs: one forced terminate scoped to this line's `--ike ims0`, one initiate, one `pkill` of `vowifi-ims-agent --line 0$`, and nothing issued for `ims1` (FR-008, SC-004). Commit US1+US2: `feat(supervise): rebuild a stuck CHILD_SA rekey before the hard expiry (093 US1, US2)`.

## Phase 4: User Story 3: defer during an active call, forced before expiry (P2)

**Goal**: an active call defers the rebuild until the call ends or 90s remain. Unknown call state never defers.
**Independent test**: the tick tests below plus the call-state file tests.

- [ ] T012 [P] [US3] In `ims/pcscf_files.rs`, add tests first, then `pub fn calls_path(selection_path: &str) -> PathBuf` (`<selection>.calls`), `pub fn render_active_calls(n: u32) -> String` (`"{n}\n"`), `pub fn parse_active_calls(contents: &str) -> Option<u32>` (trimmed, decimal) and `pub fn write_active_calls(path: &Path, n: u32) -> io::Result<()>` (via `write_atomic`). Tests: path beside the selection; round trip; garbage, empty and negative → `None`; the write is atomic and readable.
- [ ] T013 [US3] In `ims/observability.rs`, give `AgentObservability` an optional `calls_file: Option<PathBuf>` with `pub fn with_calls_file(mut self, path: PathBuf) -> Self`. `set_active_calls` writes it best-effort; ignore errors, `tracing::debug!` on failure. Test: an obs with a calls file in a temp dir writes `1` then `0`.
- [ ] T014 [US3] In `ims/agent/mod.rs`:
  - (a) At the start of `run_inner`, before `EpdgTransport::new`, best-effort `pcscf_files::write_active_calls(&calls_path(&config.pcscf_source_path), 0)`, with a comment citing research R5 (a killed agent's stale `1`).
  - (b) In `serve_inbound`, when `pcscf_source` is `Some`, attach `.with_calls_file(calls_path(src))` to the `AgentObservability` it builds. VoLTE (`None`) is unchanged.
- [ ] T015 [US3] Write failing tick tests in `supervise/line_supervisor.rs`:
  - `rekey_stuck_defers_during_a_call`: expires 290 with `call_active` true → `StillUp`, no terminate, `rekey_stuck_deferral_logged == Some(#N)`. A second tick does not log again.
  - `..._rebuilds_when_the_call_ends`: the next tick with `call_active` false → rebuild.
  - `..._forced_under_margin`: expires 90 with the call active → rebuild, and the log says "despite an active call".
  - `..._call_state_read_only_when_stuck`: with expires 3000, the closure is never called. Use a `Cell` counter.
- [ ] T016 [US3] Add the `call_active: &dyn Fn() -> bool` parameter to `tick_steady_state_with_pool`. `tick_steady_state` and the SWu loop in `supervise/orchestrate.rs` pass `&|| false`. Update the other test call sites. Implement the deferral rows of the decision table and the deferral log line (contracts/logs.md). T015's tests pass.
- [ ] T017 [US3] In the strongswan steady-state loop in `supervise/orchestrate.rs`, pass a closure that reads `pcscf_files::calls_path(&line.pcscf_source_path)` via `runner.read_file` and returns `parse_active_calls(..).is_some_and(|n| n > 0)`. Orchestrator test: with a mock file containing `1` and expires 290 → no terminate. With `0`, or with the file missing → rebuild. Commit: `feat(supervise): hold a stuck-rekey rebuild for an active call, never past the margin (093 US3)`.

## Phase 5: Polish

- [ ] T018 [P] Update `docs/configuration.md` (the `tunnel_address_family` row) and `config.toml.example` (its comment). The #107 symptom on a `dual` line is now an hourly `ChildSaRekeyStuck` planned rebuild, a few seconds without service, rather than an expiry followed by `ChildSaMissing`/`ChildSaStuck`. `ipv4` still removes it. `tests/test_config_docs.rs` must pass.
- [ ] T019 [P] In `RELEASE_NOTES.md`, add a `## Unreleased` entry with only the summary bold (memory). For example: **A failing tunnel rekey is now repaired before it drops the line** -- what changed for operators, the 300s/90s figures, call deferral, the new log line and reason, that it counts toward `[alerts.tunnel_failure]`, strongSwan engine only, no configuration change (#127). In `CHANGELOG.md`, add a `### Changed` entry under `[Unreleased]`.
- [ ] T020 Mark tasks done in this file. Run `make format && make lint && make test`. Commit: `docs(093): release notes, changelog and docs for the stuck-rekey rebuild`.
- [ ] T021 Live check (quickstart.md "natural rekey"): build an image and run it on the local Vodafone rig via `test/` (memory: local docker tests). Watch one natural rekey: `#N` increments and no `rekey overdue` line appears. Record the result here.
- [ ] T022 Push the branch and open the PR (`feat: rebuild the ePDG tunnel on a stuck CHILD_SA rekey (#127)`; the body links #127, #107 and #131 and covers the R1 finding about `REKEYING` sampling). Trigger a Greptile review. Fix its findings in code; never reply to Greptile comments (memory).

## Dependencies

- T002 → T003 → T004 → Phase 3 → Phase 4. Phase 5 needs Phase 3 (T018 and T019 describe Phase 4 too, so do them last).
- US2 has no code of its own. It is a set of constraints on the US1 decision, so the two share a phase and a commit.
- US3 depends on US1's decision code. T012–T014 (the agent side) are independent of T015–T016 (the supervisor side).

## Parallel opportunities

- T003 and T005 (separate parsers in `engines.rs`; edit sequentially, design in parallel).
- T012, T013 and T014 (agent side) alongside T015 and T016 (supervisor side).
- T018 and T019.

## Implementation strategy

The MVP is Phase 2 plus Phase 3: on its own it removes the unplanned expiry and the ~60s of strikes. Phase 4 adds the call protection. Phase 5 adds docs, the live check and the PR.
