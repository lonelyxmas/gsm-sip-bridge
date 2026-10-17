# Tasks: Rebuild the tunnel when the agent has given up on every P-CSCF (097)

**Input**: `specs/097-agent-pool-exhausted-rebuild/` (spec.md, plan.md, research.md, data-model.md, contracts/, quickstart.md)

**Tests**: required. The constitution defaults to TDD: write each test before the code it covers, and watch it fail first.

Paths are relative to `gsm-sip-bridge/src/` unless stated otherwise.

Before every commit:
- run `make format && make lint && make test` with `CARGO_TARGET_DIR=$PWD/target-097` (shared-target-dir memory);
- end the message with the Co-Authored-By trailer.

Fixtures use synthetic addresses only (`10.0.0.x`, `192.0.2.x`, `2001:db8::x`).

## Phase 1: Setup

- [ ] T001 Confirm the branch is `097-agent-pool-exhausted-rebuild` on top of `origin/main` (8393a6a), and that `make test` is green before any change. Commit the spec artifacts as `docs(097): spec, plan and tasks for the pool-exhaustion rebuild (#120)`.

## Phase 2: Foundational: the shared pool state format with `marked_at_unix` (blocks US1–US3)

**Goal**: [contracts/files.md](contracts/files.md).

- [ ] T002 Tests first, in `ims/pcscf_files.rs` `mod tests`:
  - `PoolState` with one mark round-trips through `serde_json`, including `marked_at_unix`;
  - a file written without `marked_at_unix` parses with it as 0;
  - `read_pool_state` returns `None` for: a missing file, garbage, and `version: 2`.
- [ ] T003 Move `PersistedPool`/`PersistedMark` and `STATE_VERSION` out of `ims/agent/pcscf_pool.rs` into `ims/pcscf_files.rs`, as `pub struct PoolState { version, last_registered, marks }` and `pub struct PoolStateMark { addr, until_unix, failures, cause, marked_at_unix }`.
  - Give `marks`, `cause` and `marked_at_unix` `#[serde(default)]`.
  - Add `pub const POOL_STATE_VERSION: u32 = 1`.
  - Add `pub fn read_pool_state(path: &Path) -> Option<PoolState>`, which returns `None` on any read or parse error and on a version mismatch.
  - Update the module doc to say the supervisor reads it too (specs/097).
- [ ] T004 In `ims/agent/pcscf_pool.rs`, tests first:
  - `on_failure` stamps `marked_at` = `now` and saves it;
  - a renewal re-stamps it;
  - `load` restores `marked_at`;
  - an old file without the field loads with its marks still steering failover;
  - `mark_durations_*` and every existing pool test stay green.

  Then:
  - add `marked_at: SystemTime` to `Mark`;
  - set it in `on_failure`;
  - make `save`/`load` use the shared types (missing field → `UNIX_EPOCH`).
- [ ] T005 Commit: `feat(ims): record when each P-CSCF mark was set (#120)`.

## Phase 3: User Story 1: a single-P-CSCF line recovers without the probe (P1) 🎯 MVP

**Goal**: rows 7/7a of [contracts/steady-tick.md](contracts/steady-tick.md).
**Independent test**: `line_supervisor` and `orchestrate` tests.

- [ ] T006 [US1] In `supervise/line_supervisor.rs`, tests first for the pure `exhausted_pool(pool, current, state, baseline, now)`:
  - **Exhausted:** a one-member pool with a fresh mark.
  - **Not exhausted:**
    - `None` state;
    - no baseline;
    - a mark older than the baseline;
    - `marked_at_unix == 0`;
    - an expired mark;
    - a `retry-after` mark.
  - **Pool handling:**
    - an empty pool falls back to `current`;
    - an empty pool and an empty `current` gives `None`;
    - an IPv6 member matches across textual forms (`2001:db8::1` vs `2001:0db8::1`);
    - a mark on an address outside the pool is ignored.
- [ ] T007 [US1] Implement `exhausted_pool` and `ExhaustedMember` (data-model.md).
  - Add `DegradeReason::PcscfPoolExhausted`, with a doc comment covering #120 finding 4 and the Vodafone case.
  - Add `TunnelRecoveryReason::PcscfPoolExhausted` in `control/protocol.rs`: the `ALL` array becomes 12 entries, label `pcscf_pool_exhausted`.
  - Map it in the exhaustive `From`.
  - Check that `recovery_restarts_agent` and `recovery_renegotiates` both return true for it, with a test.
- [ ] T008 [US1] Tick tests first, using `FakeEngine` and a pool-state closure:
  - `a_single_exhausted_pcscf_rebuilds_the_tunnel`: recreate, terminate and reinitiate, `Recovered{PcscfPoolExhausted}`, `tracking.pool_exhausted` filled.
  - `the_agent_verdict_wins_over_a_passing_probe`: `pcscf_reachable = true`, exhausted → rebuild, and the probe is never called.
  - `an_exhausted_pool_is_rebuilt_after_the_route_check`: route missing + exhausted → `DefaultRouteMissing` first.
  - `an_exhausted_pool_whose_interface_cannot_be_recreated_reports_tun_unavailable`.
  - `the_rebuild_resets_the_unreachable_strikes`.
  - `stale_marks_from_before_the_negotiation_never_rebuild`.
- [ ] T009 [US1] Add the tick input `agent_pool: &dyn Fn() -> Option<crate::ims::pcscf_files::PoolState>` to `tick_steady_state_with_pool`.
  - `tick_steady_state` passes `&|| None`.
  - Update all four test call sites.
  - Add `negotiated_at`, `pool_exhausted` and `pool_exhausted_deferral_logged` to `SteadyTracking`.
  - Insert row 7 after the route check, using `SystemTime::now()`.
- [ ] T010 [US1] Call-deferral tests first:
  - `an_exhausted_pool_waits_for_the_call_to_end`: `call_active` true → `StillUp` (or the probe's own outcome), no rebuild, the deferral logged once over two ticks.
  - The first tick with `call_active` false → rebuild.
  - The deferral flag clears when exhaustion clears.

  Then implement row 7a.
- [ ] T011 [US1] In `supervise/orchestrate.rs`:
  - **strongSwan loop:**
    - pass `&|| read_pool_state(&state_path(&line.pcscf_source_path))`;
    - set `tracking.negotiated_at = Some(SystemTime::now())` on loop entry, after the `pkill` in the renegotiated `Recovered` branch, and after the `pkill` in `PcscfChanged`;
    - extend `tunnel_recovered_event(reason, pcscfs)`.
  - **swu loop:**
    - the same closure;
    - `line_in_call` instead of `|| false`;
    - the baseline on entry, on `PcscfChanged`, and after the `pkill` in `Recovered` when `recovery_renegotiates(reason)`.
  - **Log line:** for `PcscfPoolExhausted`, log each member as `addr cause until`, plus the baseline.
  - Unit-test `tunnel_recovered_event` for both shapes.
- [ ] T012 [US1] Wire: `ObservedEvent::TunnelRecovered { reason, #[serde(default, skip_serializing_if = "Vec::is_empty")] pcscfs: Vec<String> }`.
  - The existing wire-shape test stays byte-identical.
  - Add a test for `{"event":"tunnel_recovered","reason":"pcscf_pool_exhausted","pcscfs":["10.0.0.11"]}`.
  - Update every construction site (ingest's match uses `{ reason, .. }`).
  - Commit: `feat(supervise): rebuild the tunnel when the agent has marked every P-CSCF (#120)`.

**Checkpoint**: US1 is shippable alone. With US1 in, the rebuild works and is counted.

## Phase 4: User Story 2: multi-P-CSCF lines keep failing over in the agent (P1)

- [ ] T013 [US2] Tests in `supervise/line_supervisor.rs`:
  - `a_partly_marked_pool_never_rebuilds`: a 4-member pool with 1, 2 or 3 fresh marks → no `PcscfPoolExhausted`, whatever the probe says.
  - `a_wholly_marked_pool_rebuilds`: all 4 marked → rebuild.
  - `a_member_cleared_by_a_registration_counts_as_unmarked`: its mark is removed in the state.

  These must pass against T009 with no new code. Any fix goes in `exhausted_pool`.

## Phase 5: User Story 3: everything else behaves as before (P1)

- [ ] T014 [US3] Tests:
  - `structural_faults_and_pcscf_changes_skip_the_exhaustion_check`: for each structural fault and for a changed P-CSCF, with an exhausted pool, the existing outcome wins.
  - `no_pool_state_leaves_the_tick_unchanged`: the full existing suite passes, and the new input returning `None` gives `StillUp`/`PcscfUnreachable` exactly as before.
  - `tracking.pool_exhausted` is empty after any non-exhaustion tick.

  Check that the existing tests in `engines.rs` and `orchestrate.rs` still pass. Fold into the US1 commit if done before it, otherwise commit as `test(supervise): pin the exhaustion check's place in the tick (#120)`.

## Phase 6: User Story 4: a Discord alert for a line stuck on a dead pool (P2)

**Goal**: [contracts/alerts.md](contracts/alerts.md).

- [ ] T015 [P] [US4] Config, tests first in `tests/test_config.rs`:
  - `[alerts.pcscf_pool_exhausted]` parses `enabled`, `unhealthy_sec` and the webhook override;
  - it defaults to enabled at 300s;
  - an out-of-range `unhealthy_sec` warns and falls back;
  - it isn't reported as an unknown section.

  Implement the six touch points, mirroring `gm_connection_lost`:
  - `config/raw.rs`: the field and the KEYS entry;
  - `config/mod.rs`: `pcscf_pool_exhausted: CategoryAlertConfig`, a `PcscfPoolExhaustedThresholds` struct (default 300), and the defaults;
  - `config/build.rs`: `category(raw, true)` and the thresholds block;
  - `alerts/mod.rs`: `AlertCategory::PcscfPoolExhausted`, `as_str`, and its `category_config` arm;
  - the active-incident set.
- [ ] T016 [US4] In `metrics/ingest.rs`, tests first. Each drives `apply_report_with` with an injected `AlertsConfig` and backdated `Instant`s, as the existing alert tests do:
  - **Start and threshold:** one supervisor report carrying `TunnelRecovered{PcscfPoolExhausted}` starts the episode, with no alert before the threshold. With the episode backdated past 300s, the next supervisor report yields exactly one `Failure`.
  - **Suppression:** further events in an alerted episode yield `Suppressed`, never a second `Failure`, and bump the rebuild count.
  - **Stale epoch:** an IMS report with `registered: Some(true)` from the epoch live at the rebuild does **not** end the episode.
  - **End:** one from a new epoch does. The next supervisor report then yields `Recovered`.
  - **Short episode:** it ends before the threshold, and nothing is ever sent.
  - **Other reasons:** `TunnelRecovered` with any other reason never starts an episode.
  - **Disabled:** with `enabled = false`, nothing is sent.
- [ ] T017 [US4] Implement in `metrics/ingest.rs`:
  - **Record fields:** add `pool_exhausted_since`, `pool_exhausted_alert_phase`, `pool_exhausted_rebuilds` and `pool_exhausted_stale_ims_epoch` to `AgentRecord`.
  - **Side map:** `module_id → Vec<String>`.
  - **Supervisor reports:** episode start and continue happen while the events are processed.
  - **IMS reports:** episode end is decided under the same lock, on the supervisor's record.
  - **Decision:** a fourth `decide_transition` arm, evaluated on supervisor reports.
  - **Dispatch:** `record_alert_outcome` and `build_alert` arms with the contract's message text.
  - **Exhaustiveness:** the `unreachable!` message updated.
- [ ] T018 [US4] Integration test, written first: `gsm-sip-bridge/tests/test_vowifi_pool_exhausted_alert.rs`, modelled on `tests/test_vowifi_tunnel_recovery_metrics.rs`. A real control server and a supervisor `Reporter` send two `TunnelRecovered{PcscfPoolExhausted, pcscfs}` events; the test waits for the counter to read 2 and asserts that `critical_event_active` stays 0 before the threshold. The episode-and-alert logic itself is covered by T016, since the 30s minimum threshold cannot run in an integration test.
- [ ] T019 [US4] Commit: `feat(alerts): alert when a line is stuck on an exhausted P-CSCF pool (#120)`.

## Phase 7: Polish

- [ ] T020 [P] `docs/configuration.md`: add a row for `[alerts.pcscf_pool_exhausted]` (`enabled`, `unhealthy_sec`, `webhook_url`, defaults), noting that swu lines don't alert (research R7). `config.toml.example`: the commented section.
- [ ] T021 [P] `docs/observability.md`: the new `category` value in both alert-metric rows, and the new `reason` value on `vowifi_tunnel_recoveries_total`. Wherever the supervisor's recovery order is described, add the agent-verdict step.
- [ ] T022 [P] `CHANGELOG.md`: an Unreleased entry in the existing style (bold summary only), with an upgrade note that the new alert defaults to enabled.
- [ ] T023 Run the full `make format && make lint && make test`. Commit: `docs: pool-exhaustion rebuild and its alert (#120)`.

## Phase 8: Live verification (Clarifications Q3; [quickstart.md](quickstart.md))

- [ ] T024 Vodafone rig, on a throwaway forced-probe build (a detached commit, never pushed): black-hole the P-CSCF and check SC-001 and SC-007. Then redeploy the branch image.
- [ ] T025 Vodafone rig, on the branch image: run the same black-hole and record which reason fires first (SC-002).
- [ ] T026 Jio Pi, on the branch image built via `arm-build-200` (back up the compose file): black-hole the active P-CSCF and check SC-003.
- [ ] T027 Both rigs: about 30 minutes of idle (SC-004).
- [ ] T028 Write `live-verification.md` with addresses redacted, and mark the tasks done. Commit: `docs(097): live verification on Vodafone and Jio (#120)`.

## Dependencies

- T001 → T002 → T003 → T004 → T005 (foundational).
- T006 → T007 → T008 → T009 → T010 → T011 → T012 (US1).
- T013 and T014 come after T009. They are tests only and can run in parallel with T010–T012.
- US4: T015 [P] is independent of US1. T016 and T017 depend on T007 (the reason enum) and T012 (the `pcscfs` field). T018 comes after T017.
- Polish (T020–T022 [P]) comes after T019.
- Live verification (T024–T028) comes after T023. T024 → T025 on the Vodafone rig; T026 can run in parallel.

## Parallel examples

- During US1: T013 and T014 (test-only additions in the same file; one author at a time, so a sequential edit is fine).
- After US1: T015 (the config files) in parallel with T016 (ingest tests).
- Polish: T020, T021 and T022 touch different files.

## Implementation strategy

1. **MVP: Foundational + US1.** It gives the rebuild, its counter label and the logs. That alone closes #120 finding 4 for single-P-CSCF carriers.
2. **US2 and US3.** They pin the safety properties. They are mostly tests, and should need no new production code.
3. **US4.** Adds the operator alert for the unbounded loop.
4. **Polish and live verification.** Both happen before merge.
