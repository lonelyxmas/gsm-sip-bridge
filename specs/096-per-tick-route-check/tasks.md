# Tasks: Check the default route on every steady-state tick (096)

**Input**: `specs/096-per-tick-route-check/` (spec.md, plan.md, research.md, data-model.md, contracts/, quickstart.md)

**Tests**: required. The constitution defaults to TDD. Write each test before the code it covers and watch it fail first.

Paths are relative to `gsm-sip-bridge/src/` unless stated otherwise. Before every commit, run `make format && make lint && make test` with a private `CARGO_TARGET_DIR` (for example `CARGO_TARGET_DIR=$PWD/target-096`), per the shared-target-dir memory. Every commit message ends with the Co-Authored-By trailer. Fixtures use synthetic addresses only (`10.0.0.x`, `192.0.2.x`, `2001:db8::x`).

## Phase 1: Setup

- [X] T001 Confirm the branch is `096-per-tick-route-check` on top of `origin/main` (2ae972c), and that `make test` is green before any change.

## Phase 2: User Story 1: a lost route is repaired on the next tick, whatever the probe says (P1) 🎯 MVP

**Goal**: rows 4–9 of [contracts/steady-tick.md](contracts/steady-tick.md).
**Independent test**: the `line_supervisor` tests below.

- [X] T002 [US1] Tests first, in `supervise/line_supervisor.rs` `mod tests`:
  - Replace `a_reachable_line_is_never_probed_for_its_route` with `a_missing_route_is_repaired_even_while_the_pcscf_answers`. Setup: `pcscf_reachable = true`, `route_missing = true`. Expect `Recovered{DefaultRouteMissing}`, `route_repairs == 1`, zero terminate/reinitiate/recreate, strikes 0, then `StillUp` on the next tick. This is contract rows 5 and 2-of-spec-US1. It must fail on today's code.
  - Add `the_route_is_checked_before_the_probe`. Setup: `route_missing = true`, `pcscf_reachable = false`. On the repairing tick, `engine.probed` is empty (contract invariant 2).
  - Add `an_empty_pcscf_skips_the_route_check`. Setup: `route_missing = true`, `current_pcscf = ""`. Expect `StillUp` and `route_repairs == 0` (FR-004).
  - Add `a_present_route_still_lets_the_probe_decide`. Setup: `route_missing = false`. The probe is called and `call_order` has no `repair_route` (contract row 7).
  - Add `structural_faults_and_pcscf_changes_skip_the_route_check`. For each of `ViciBroken`, `TunVanished`, `IkeSaMissing`, `ChildSaMissing` health, and for a `latest_pcscf` that differs, with `route_missing = true`: the outcome is that branch's existing one and `route_repairs == 0` (FR-005).
  - Add `the_route_is_repaired_at_most_once_per_tick`. `FakeEngine` gains a `repair_calls` counter that is incremented on **every** call, including when nothing is missing. Over a tick with the route present and the P-CSCF unreachable, `repair_calls == 1` (FR-006).
  - Keep `a_missing_default_route_is_repaired_without_touching_the_tunnel` and `unreachable_with_the_route_present_still_escalates_to_a_rebuild` unchanged.
- [X] T003 [US1] Implement in `tick_steady_state_with_pool`:
  - Insert the route check after the `PcscfChanged` early return and before `pcscf_reachable`: `if !current_pcscf.is_empty() && engine.repair_default_route(runner, current_pcscf) { tracking.pcscf_unreachable = 0; return Recovered{DefaultRouteMissing, None} }`.
  - Remove the in-branch call, and move its comment block (the 2026-08-19 outage, why a rebuild can't fix it, the family note) up to the new site. Add a line saying the check is structural, so it doesn't wait for the probe (#120 finding 3).
  - Update the `repair_default_route` trait doc: it is called on every healthy tick.
  - Update the `DegradeReason::DefaultRouteMissing` doc: drop "no dropped call", since the agent restart drops one (Clarification Q1).
- [X] T004 [US1] Make sure `git diff` leaves `engines.rs` and `epdg_iface.rs` untouched. Check that `engines.rs` tests at about `:1836` that drive `tick_steady_state` still pass. Commit: `fix(supervise): check the default route every tick, not only after the probe fails (#120)`.

**Checkpoint**: US1 is shippable alone.

## Phase 3: User Story 3: a per-reason recovery counter (P2)

**Goal**: [contracts/metrics.md](contracts/metrics.md).
**Independent test**: protocol serde tests and the new integration test.

- [X] T005 [P] [US3] In `control/protocol.rs`, tests first:
  - `TunnelRecoveryReason` round-trips through serde with every label in data-model.md.
  - `ObservedEvent::TunnelRecovered { reason: DefaultRouteMissing }` serialises to `{"event":"tunnel_recovered","reason":"default_route_missing"}`.

  Then add the enum (`Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`, `rename_all = "snake_case"`), its `as_str()`, and the `ObservedEvent` variant, with a doc comment covering the supervisor only, once per steady-state `Recovered`, and specs/096.
- [X] T006 [US3] In `supervise/line_supervisor.rs`, add `impl From<DegradeReason> for TunnelRecoveryReason`. It must be exhaustive, with no wildcard arm. Unit-test that every `DegradeReason` maps to the label of the same name.
- [X] T007 [US3] In `metrics/mod.rs`, add `VOWIFI_TUNNEL_RECOVERIES_TOTAL`: a `CounterVec` named `gsm_sip_bridge_vowifi_tunnel_recoveries_total` with labels `["module", "reason"]`. In `metrics/ingest.rs` `apply_event`, add an arm that increments it. Add the name to `tests/test_metrics_endpoint.rs`'s expected exports, following the existing VoWiFi counters there.
- [X] T008 [US3] Integration test, written first: `gsm-sip-bridge/tests/test_vowifi_tunnel_recovery_metrics.rs`, modelled on `tests/test_vowifi_health_metrics.rs`. Use a real `start_control_server` and a `Reporter::spawn(.., AgentKind::Supervisor, "test-tunnel-recoveries", 50ms)`. Then:
  - Report a state with `child_sa_up: Some(true)` and two `TunnelRecovered{DefaultRouteMissing}` events plus one `PcscfUnreachable`. Wait until the counter reads 2 and 1 respectively.
  - Send an events-free report, wait one interval, and assert the counters haven't moved.
  - Spawn a second `Reporter` for the same module (a supervisor restart), report one more `DefaultRouteMissing`, and expect 3, never a rewind.
- [X] T009 [US3] In `supervise/orchestrate.rs`, in `start_vowifi_line_strongswan`, split the reporter closure:
  - `send_tunnel(s: TunnelSignal, events: Vec<ObservedEvent>)` builds today's `AgentState` from `s`.
  - `report_tunnel = |s| send_tunnel(s, Vec::new())`.

  In the `SteadyOutcome::Recovered` arm, right after the existing `recovered from` println, call `send_tunnel(signal, vec![ObservedEvent::TunnelRecovered { reason: reason.into() }])`. Put the event construction in a small pure fn `tunnel_recovered_event(reason: DegradeReason) -> ObservedEvent`, unit-tested in `tunnel_signal_tests`. Commit: `feat(metrics): count supervisor tunnel recoveries per line and reason (#120)`.

## Phase 4: Polish

- [X] T010 [P] `docs/observability.md`: add a row for `gsm_sip_bridge_vowifi_tunnel_recoveries_total`, listing its labels and noting that a rising `default_route_missing` count means the route keeps disappearing. Mention, wherever the doc describes the supervisor's recovery order, that the route check now runs every tick.
- [X] T011 [P] `CHANGELOG.md`: add an Unreleased entry for the per-tick route check (#120) and the recovery counter, in the existing style (bold summary only).
- [X] T012 Run the full `make format && make lint && make test`. Commit: `docs: per-tick route check and the tunnel recovery counter (#120)`.

## Phase 5: Live verification (spec Clarification Q2; [quickstart.md](quickstart.md))

- [X] T013 Vodafone rig (local, `test/`): build the branch image and recreate the container. Do the route delete (SC-001, SC-002, SC-007), then about 30 minutes of idle (SC-006).
- [X] T014 Jio Pi: build arm64 via `arm-build-200`, load it onto the Pi, back up the compose file, recreate the container. Do the route delete and the 30 minutes of idle.
- [X] T015 Vodafone rig: a forced-probe throwaway build on a scratch branch that is never pushed. Delete the route and expect a repair within one tick (SC-003). Then redeploy the real branch image.
- [X] T016 Write `specs/096-per-tick-route-check/live-verification.md`, with P-CSCF addresses redacted, and mark the tasks done. Commit: `docs(096): live verification on Vodafone and Jio (#120)`.

## Dependencies

- T002 → T003 → T004.
- T005 → T006, T007 → T008 → T009.
- US3 depends on US1 only at the commit level, since both touch `line_supervisor.rs`.
- T010 and T011 can run in parallel after T009.
- Live verification (T013–T016) comes after T012. T013 and T014 can run in parallel; T015 comes after T013.
