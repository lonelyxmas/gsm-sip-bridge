---
description: "Task list for specs/086-epdg-child-sa-recovery"
---

# Tasks: ePDG CHILD_SA health detection and bounded recovery

**Input**: `specs/086-epdg-child-sa-recovery/` — plan.md, spec.md, research.md (R1–R7), data-model.md, contracts/observability.md, quickstart.md
**Tests**: Included. The constitution makes TDD the default, and spec SC-008 makes automated tests plus a live gate a merge condition. Write each test first and watch it fail.

**Every commit** (CLAUDE.md, constitution II): `make format && make lint && make test` must pass first. Never commit real IMSIs, MSISDNs or IPs from the live line. Use synthetic identities (`001010000000001@nai.epc.mnc001.mcc001.3gppnetwork.org`, `198.51.100.10`, `2001:db8::1`).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- **[US1]** detection + escalation (A+B) · **[US2]** bounded teardown (C) · **[US3]** CHILD_SA gauge + alert (E)

---

## Phase 1: Setup

- [X] T001 Confirm the baseline on branch `086-epdg-child-sa-recovery` is green: run `make lint && make test` from the repo root and record any pre-existing failures before touching code (there should be none; stop if there are)

---

## Phase 2: Foundational (blocks US1 and US3)

**Purpose**: replace the positional `unreachable_streak: &mut u32` with a struct that can carry the new per-line state (research R2, data-model "SteadyTracking"). Pure refactor, no behaviour change.

- [X] T002 In `gsm-sip-bridge/src/supervise/line_supervisor.rs`, add `#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)] pub struct SteadyTracking { pub pcscf_unreachable: u32, pub child_sa_missing: u32, pub child_sa_usable: bool }` with a doc comment (loop-owned, survives between ticks, keeps the tick a pure decision over its inputs). Change `tick_steady_state`'s `unreachable_streak: &mut u32` parameter to `tracking: &mut SteadyTracking`, and replace every use of `*unreachable_streak` with `tracking.pcscf_unreachable`. Leave `child_sa_missing`/`child_sa_usable` unused for now
- [X] T003 Update the test-helper shadow `tick_steady_state` and every test call site in `gsm-sip-bridge/src/supervise/line_supervisor.rs`: `&mut 0` becomes `&mut SteadyTracking::default()`. Tests that read the streak afterwards (e.g. the swu reachability-window tests using `let mut streak`) now hold a `SteadyTracking` and read `.pcscf_unreachable`
- [X] T004 Update both callers in `gsm-sip-bridge/src/supervise/orchestrate.rs`: in the strongswan steady-state loop (~line 1623, `let mut unreachable_streak = 0u32;`) and the swu loop (~line 2089), use `let mut tracking = line_supervisor::SteadyTracking::default();` and pass `&mut tracking`. Run `make format && make lint && make test` and commit: `refactor(supervise): thread steady-state tracking through a struct`

**Checkpoint**: behaviour identical, all tests green.

---

## Phase 3: User Story 1 — A lost CHILD_SA is detected and repaired within about a minute (P1) 🎯 MVP

**Goal**: detect a missing CHILD_SA under a live IKE_SA (#107 A), re-initiate on the first miss, rebuild the IKE_SA and restart the agent on the second consecutive miss with no backoff (#107 B).

**Independent test**: unit tests below pass. Live, `swanctl --terminate --child ims0` on the Vodafone line is detected within 30s and re-initiated (quickstart §2c).

### Tests for US1 (write first, must fail)

- [X] T005 [P] [US1] In `gsm-sip-bridge/src/supervise/engines.rs` tests, replace `list_sas_detection_matches_the_grep_pattern` with table-driven tests of `list_sas_has_child_sa`, using fixtures shaped like the live 5.9.3 output in research.md R1 (synthetic identities). Expected results:
  - IKE header only, `ESTABLISHED` → false (#107 regression)
  - IKE header only, `CONNECTING` → false
  - IKE header plus child `  ims0: #4, reqid 1, INSTALLED, TUNNEL-in-UDP, ESP:AES_CBC-128/HMAC_SHA1_96` → true
  - rekey overlap (child `#3 … REKEYED` plus child `#4 … INSTALLED`) → true
  - child `REKEYING` only → true
  - child `UPDATING` only → true
  - child `INSTALLING` only → false
  - child `DELETING` only → false
  - `ims0` IKE header with a child named `ims1` → false
  - a deeper-indented line (four spaces) containing `ims0: #` → false
  - an output listing two IKE_SAs, `ims1` with an INSTALLED child and `ims0` with none, queried for `ims0` → false (FR-003/FR-008: another line's healthy SA never masks this one's)
  - empty string → false
- [X] T006 [P] [US1] In `gsm-sip-bridge/src/supervise/engines.rs` tests, add an engine-level test: `MockCommandRunner::set_run_output` maps the `--list-sas` argv to the IKE-header-only fixture with exit 0, and the `ip link show` check succeeds. Assert `strongswan_engine().steady_state_health(&runner) == SteadyStateHealth::ChildSaMissing`. Repeat with the INSTALLED fixture and assert `Ok`. `set_run_output` is keyed by the **full argv joined with single spaces**, i.e. `env STRONGSWAN_CONF=/etc/strongswan-shared.conf swanctl --list-sas --ike ims0` for the default test engine. Read `strongswan_engine()`'s `shared.strongswan_conf` to build the key exactly. Build the `Output` the way `set_netns_output` does (`empty_output(0)` with `stdout` set). The `ip link show` check goes through `run_in_netns`, which returns success by default; confirm this in the mock, or seed it with `set_netns_output`
- [X] T007 [P] [US1] In `gsm-sip-bridge/src/supervise/line_supervisor.rs` tests, using `FakeEngine` with `health = ChildSaMissing` and one shared `SteadyTracking` across calls, add:
  - (a) first tick returns `Recovered{ChildSaMissing}` with reinitiate=1, terminate=0, restart=0 (keep the existing test passing);
  - (b) second consecutive tick returns `Recovered{ChildSaStuck}` with terminate=1, reinitiate=2, and `tracking.child_sa_missing == 0` afterwards;
  - (c) miss, then `Ok` (set `engine.health` to `Ok` between ticks), then miss: never returns `ChildSaStuck`, terminate=0;
  - (d) four consecutive misses alternate ChildSaMissing, ChildSaStuck, ChildSaMissing, ChildSaStuck (no backoff, FR-006a);
  - (e) with `health = ChildSaMissing` **and** the fake reporting no default route / unreachable P-CSCF, the tick returns a ChildSa* reason and `repair_default_route` is never called (add a call counter to `FakeEngine` if none exists);
  - (f) `health = TunVanished` resets `tracking.child_sa_missing` to 0.
- [X] T008 [P] [US1] In `gsm-sip-bridge/src/supervise/line_supervisor.rs`, extend `only_child_sa_missing_and_tun_unavailable_skip_the_agent_restart`: add `DegradeReason::ChildSaStuck` to the "restarts the agent" list and to the exhaustive `match`, under the arm asserting `recovery_restarts_agent(reason)`

### Implementation for US1

- [X] T009 [US1] Rewrite `list_sas_has_child_sa` in `gsm-sip-bridge/src/supervise/engines.rs` per research R1. A line qualifies when it starts with exactly two spaces (`l.strip_prefix("  ")` and the remainder does not start with a space), the remainder starts with `format!("{conn_name}: #")`, and the third `", "`-separated field is in `["INSTALLED", "REKEYING", "REKEYED", "UPDATING"]` (hold these in a named `const USABLE_CHILD_SA_STATES: &[&str]`). Replace the doc comment's "1:1 port of grep" claim: explain that the old match hit the IKE_SA header, so a CHILD_SA lost under a live IKE_SA was never detected (#107), and why REKEYING/REKEYED count as usable. Update the `SteadyStateHealth` doc in `line_supervisor.rs` (it mentions "the `ims:` CHILD_SA line's presence"). T005 and T006 now pass. Commit: `fix(supervise): detect a missing CHILD_SA, not just the IKE_SA (#107)`
- [X] T010 [US1] In `gsm-sip-bridge/src/supervise/line_supervisor.rs`:
  - add `DegradeReason::ChildSaStuck` with a doc comment (#107: the ePDG ignored CREATE_CHILD_SA in the surviving IKE_SA; only a fresh IKE_SA recovers; the agent must re-register because the new IKE_SA brings a new VIP/P-CSCF pool);
  - add `pub const CHILD_SA_MISSING_STRIKES: u32 = 2;`, documented as tick-based on purpose (only the strongswan engine, polled every 30s, ever reports `ChildSaMissing`, so 2 ≈ 60s), with no backoff (spec clarification);
  - in `tick_steady_state`, the `ChildSaMissing` arm does `tracking.child_sa_missing += 1`. Below the strike count it keeps today's re-initiate and returns `ChildSaMissing`. At the strike count it resets to 0, calls `engine.terminate(runner); engine.reinitiate(runner);` and returns `Recovered{ChildSaStuck}`. Add **no** new `println!` in the tick: per contracts/observability.md §4, the engine already logs each miss, and `orchestrate.rs` logs `recovered from ChildSaStuck` with the line index;
  - every other `SteadyStateHealth` value sets `tracking.child_sa_missing = 0` before its existing handling;
  - `recovery_restarts_agent` already returns true for anything not excluded, so confirm `ChildSaStuck` falls through to true.

  T007 and T008 now pass. Commit: `fix(supervise): rebuild the IKE_SA when a CHILD_SA stays missing (#107)`

**Checkpoint**: US1 complete and independently shippable.

---

## Phase 4: User Story 2 — Tearing down a stuck IKE_SA never blocks recovery (P1)

**Goal**: every steady-state IKE_SA teardown is `--force` (doesn't wait on a pending exchange) and bounded by a 10s outer timeout (#107 C; clarification "all recovery paths").

**Independent test**: the argv test below. Live, the recovery log shows the teardown returning within seconds.

- [X] T011 [US2] Test first in `gsm-sip-bridge/src/supervise/engines.rs`: extend `terminate_is_scoped_to_this_lines_connection` (~line 879), or add a sibling test, asserting that exactly one `run` call has argv starting `["timeout", "10", "env", "STRONGSWAN_CONF=…", "swanctl", "--terminate", "--ike", "ims0", "--force"]` and does **not** contain `--timeout` (swanctl's own `--timeout` disables `--force`'s no-wait behaviour, research R3). Keep the existing "not bare `ims`" assertion
- [X] T012 [US2] Implement in `gsm-sip-bridge/src/supervise/engines.rs`:
  - add `const STEADY_TERMINATE_TIMEOUT_SECS: u32 = 10;` with a doc comment citing #107's 52s wait and pointing at `shutdown.rs`'s `TERMINATE_TIMEOUT_SECS` as the precedent;
  - rewrite `StrongswanEngine::terminate` to build the argv above through `runner.run` (inline, like `shutdown.rs`'s `TerminateIke` step), with a comment on why `--force` and not swanctl's `--timeout`;
  - update the `TunnelEngine::terminate` doc in `line_supervisor.rs` to say it is non-waiting and bounded.

  Commit: `fix(supervise): never let an IKE_SA teardown wait on a stuck exchange (#107)`

**Checkpoint**: US1 and US2 together meet SC-001 to SC-003.

---

## Phase 5: User Story 3 — The operator can see whether the encrypted tunnel actually exists (P2)

**Goal**: per-line `gsm_sip_bridge_vowifi_child_sa_up`, reported by the supervisor, reads 0 from the start of supervision, is zeroed when stale, and feeds `[alerts.tunnel_failure]` with exactly one alert per outage (#107 E; contracts/observability.md).

**Independent test**: metrics endpoint shows the gauge; ingest/alert tests below; live quickstart §2a and §2c.

### Protocol and gauge

- [X] T013 [P] [US3] Test first in `gsm-sip-bridge/src/control/protocol.rs` tests: an `AgentReport` with `agent: AgentKind::Supervisor` and `state.child_sa_up: Some(false)` round-trips through serde_json as `"agent":"supervisor"` and `"child_sa_up":false`. A JSON `state` without `child_sa_up` deserialises to `None`. A `None` field is omitted from the serialised output
- [X] T014 [US3] Implement in `gsm-sip-bridge/src/control/protocol.rs`:
  - add `AgentKind::Supervisor` (doc: the `supervise` process; reports only `child_sa_up`; specs/086) and `as_str` → `"supervisor"`;
  - add `child_sa_up: Option<bool>` to `AgentState` with `#[serde(skip_serializing_if = "Option::is_none", default)]` and a doc comment per data-model.md;
  - update the `AgentKind` doc comment that says "always one of these two";
  - fix every non-exhaustive `match` the compiler reports. Expect `gsm-sip-bridge/src/metrics/ingest.rs` (`transport_label`, the registered/tunnel_up/gm arms), `gsm-sip-bridge/src/ims/agent/mod.rs` (`store::Transport` match ~line 791) and `gsm-sip-bridge/src/vowifi/mod.rs`. The supervisor owns no calls, registration or Gm state: follow the `AgentKind::VolteSip => {}` pattern for gauge arms. For `transport_label` and `store::Transport`, map `Supervisor` to the vowifi value with a comment that it never reports calls.
- [X] T015 [P] [US3] Add `VOWIFI_CHILD_SA_UP` GaugeVec (`gsm_sip_bridge_vowifi_child_sa_up`, help text per contracts/observability.md §2, label `["module"]`) in `gsm-sip-bridge/src/metrics/mod.rs`. Add a doc comment contrasting it with `VOWIFI_TUNNEL_UP` (registration-liveness proxy vs. supervisor-observed CHILD_SA). Make sure it is registered/forced wherever the other `vowifi_*` gauges are, so it appears on `/metrics` (follow how `VOWIFI_TUNNEL_UP` is touched at startup, if it is)
- [X] T016 [P] [US3] Tests first in `gsm-sip-bridge/src/metrics/ingest.rs` tests, mirroring the `tunnel_up` tests near line 1256:
  - a Supervisor report with `child_sa_up: Some(false)` sets `VOWIFI_CHILD_SA_UP{module}` to 0, and `Some(true)` sets it to 1;
  - a report with `None` leaves the gauge untouched;
  - an `Ims` report never touches `VOWIFI_CHILD_SA_UP`.
- [X] T017 [US3] Implement in `gsm-sip-bridge/src/metrics/ingest.rs` `apply_state`: on `state.child_sa_up`, for `AgentKind::Supervisor` only, set `VOWIFI_CHILD_SA_UP{module}` and maintain the record's `tunnel_unhealthy_since` (None when true; set to now on the first false), exactly like the `tunnel_up` block does for Ims. T016 now passes
- [X] T018 [US3] Test and implement stale zeroing in `gsm-sip-bridge/src/metrics/server.rs::refresh_agent_liveness`: when `state.agent == AgentKind::Supervisor && !state.up`, set `VOWIFI_CHILD_SA_UP{module}` to 0. Keep this branch separate from the existing `ACTIVE_CALLS` zeroing: the supervisor has no calls, but zeroing `active_calls{…,"vowifi"}` for its module would wrongly zero the agent's gauge, so exclude `Supervisor` from that line. Add or extend the server liveness test following the existing pattern. Commit (T013–T018): `feat(metrics): export per-line CHILD_SA state reported by the supervisor`

### Alert folding (R6)

- [X] T019 [US3] Tests first in `gsm-sip-bridge/src/metrics/ingest.rs` tests. Use backdated `Instant`s and the existing alert test helpers (e.g. `seed_pending_record`), plus whatever the existing tests use to capture dispatched transitions without Discord. For one module with both an `Ims` and a `Supervisor` record:
  - (a) only Supervisor `child_sa_up=false` past the threshold → exactly one TunnelFailure Failure;
  - (b) only Ims `tunnel_up=false` past the threshold → exactly one Failure;
  - (c) both unhealthy → exactly one Failure, and the Ims record's own `tunnel_alert_phase` stays Idle;
  - (d) one recovers while the other is still unhealthy → no Recovered; both healthy → exactly one Recovered;
  - (e) no Supervisor record for the module (swu/VoLTE) → today's per-record behaviour (existing tests stay green);
  - (f) `record_alert_outcome` for the owning Supervisor key matches on `tunnel_alert_generation`, and a stale generation is a no-op.
- [X] T020 [US3] Implement in `gsm-sip-bridge/src/metrics/ingest.rs` per research R6 and data-model "Tunnel-failure decision ownership":
  - add `tunnel_alert_generation: Option<Instant>` to `AgentRecord` (and to every struct literal, including tests and `seed_pending_record`).
  - In `apply_report`, under the existing lock: when a Supervisor record for `module_id` exists, or this report is from the Supervisor, compute `effective_since = min` over the `Some` values of the Ims record's and the Supervisor record's `tunnel_unhealthy_since` (using this report's freshly applied value for its own kind). Run `decide_transition` once, on the Supervisor record's `tunnel_alert_phase`. Store `effective_since` in its `tunnel_alert_generation`. Push the transition with key `(Supervisor, module)` and generation `effective_since`. Skip the Ims record's own TunnelFailure decision for that module.
  - When applying an Ims report that changes the decision, write the updated phase/generation back to the Supervisor record via `guard.get_mut`.
  - In `record_alert_outcome`, for TunnelFailure compare against `tunnel_alert_generation` when it is `Some`, else fall back to `tunnel_unhealthy_since` (existing records).
  - In `dispatch_transition`, for TunnelFailure keys whose kind is `Ims` or `Supervisor`, use `"VoWiFi line's tunnel non-established for over {}s"` / `"VoWiFi line's tunnel re-established"`. Keep the existing `key.0.as_str()` wording for every other kind (e.g. `Volte`) unchanged, since this feature doesn't touch VoLTE alerting. Update any existing alert-text test that pins the old `ims line's tunnel` string.

  T019 now passes. Commit: `feat(alerts): count a missing CHILD_SA toward the tunnel-failure alert, once per outage`

### Supervisor reporting

- [X] T021 [US3] Test first in `gsm-sip-bridge/src/supervise/line_supervisor.rs` tests: after a tick with `health = Ok`, `tracking.child_sa_usable == true`. After ticks with `ChildSaMissing`, `TunVanished` or `ViciBroken`, it is `false`. After the process-died branch (`is_process_alive` false) it is `false`. After `Ok` health with an unreachable P-CSCF it stays `true` (research R5)
- [X] T022 [US3] Implement in `gsm-sip-bridge/src/supervise/line_supervisor.rs`: `tick_steady_state` sets `tracking.child_sa_usable = false` at the top of the process-died branch. After `steady_state_health` it sets `tracking.child_sa_usable = matches!(health, SteadyStateHealth::Ok)` before the match. T021 now passes
- [X] T023 [US3] Implement in `gsm-sip-bridge/src/supervise/orchestrate.rs` `start_vowifi_line_strongswan` (~line 1584):
  - before `establish_line_tunnel`, create `let child_sa_reporter = crate::observability::reporter::Reporter::spawn(ctx.config.control.socket_path.clone(), AgentKind::Supervisor, line.card_id.clone(), Duration::from_secs(ctx.config.metrics.agent_report_interval_seconds));` and report `AgentState { child_sa_up: Some(false), ..Default::default() }` with no events;
  - after "tunnel UP", set `tracking.child_sa_usable = true` and report `Some(true)`;
  - after every `tick_steady_state` call (every outcome arm, or once right after the call), report `Some(tracking.child_sa_usable)`. To avoid redundant sends, skip the report when the value is unchanged: the `Reporter` already heartbeats the last state (research R4).

  If establishment returns `None` (bounded attempts exhausted), the reporter is dropped and its record goes stale, which zeroes the gauge per T018 and is the desired outcome. Add a short comment saying so. Leave the swu path (`orchestrate.rs` ~line 2089) without a reporter (FR-015). Verify the exact field paths (`config.control.socket_path`, `config.metrics.agent_report_interval_seconds`) against `gsm-sip-bridge/src/config/mod.rs`, as `ims/agent/mod.rs:784` uses them. Commit: `feat(supervise): report each line's CHILD_SA state from the start of supervision`
- [X] T024 [P] [US3] Integration test in `gsm-sip-bridge/tests/test_observability_ingest.rs`: following that file's existing pattern (real control socket / real `apply_report`), send a Supervisor `AgentReport` with `child_sa_up: Some(false)` and assert the gauge reads 0, then `Some(true)` and assert 1
- [X] T025 [P] [US3] In `gsm-sip-bridge/tests/test_metrics_endpoint.rs`, add `gsm_sip_bridge_vowifi_child_sa_up` to the list of metric names asserted present (next to `gsm_sip_bridge_vowifi_tunnel_up`, ~line 126). If a metric-rename guard exists in `gsm-sip-bridge/tests/test_metric_renames.rs`, update it too

### Docs

- [X] T026 [P] [US3] `docs/observability.md`: document `gsm_sip_bridge_vowifi_child_sa_up` next to `vowifi_tunnel_up`, with the value table from contracts/observability.md §2 and a sentence on how it differs from `tunnel_up`. Note the new `agent_up{agent="supervisor"}` series. State that the tunnel-failure alert now fires on either signal, once per outage
- [X] T027 [P] [US3] `config.toml.example` `[alerts.tunnel_failure]` comment (~line 86): "A VoWiFi line's ePDG/IPsec tunnel non-established, **or its IPsec CHILD_SA missing**, continuously for unhealthy_sec." Also update the same comment in any copy that a config test pins (grep for `tunnel non-established`). Commit (T024–T027): `docs: describe the CHILD_SA gauge and the widened tunnel-failure alert`

**Checkpoint**: all three stories complete.

---

## Phase 6: Polish, live gate and release

- [ ] T028 Run the full `make format && make lint && make test`, and grep for leftovers: `rg -n "unreachable_streak|grep pattern|1:1 port of .grep -q '\^ims:'" gsm-sip-bridge/src` must return nothing stale. Then confirm FR-018 (scope guard): `git diff --stat main -- docker/` must be empty, because no swanctl template, strongSwan build or charon config change is allowed in this feature
- [ ] T029 Run the SC-008 live gate per `specs/086-epdg-child-sa-recovery/quickstart.md` §2:
  - build `gsm-sip-bridge:child-sa-recovery-local`, point `test/docker-compose.yaml` at it, and recreate the container;
  - §2a: gauge 0 during establishment, then 1;
  - §2b: at least one natural CHILD_SA rekey with zero `ChildSa*` recoveries, with the gauge sampled every 30s across it;
  - §2c: `swanctl --terminate --child ims0` is detected within 30s, re-initiated, the gauge goes 0 → 1, the line re-registers, and one inbound test call rings.
- [ ] T030 Append a "Live gate results" section to `specs/086-epdg-child-sa-recovery/quickstart.md` with timestamps, rekey count, the recovery log lines and gauge samples. Synthesise every identity from `swanctl`/logs and include no IMSI/MSISDN/real IPs. Restore `test/docker-compose.yaml`'s image tag if it shouldn't be committed. Commit: `docs(specs): record the 086 live gate results`
- [ ] T031 Open the PR against `main` (title e.g. `Detect and recover a lost ePDG CHILD_SA (#107)`). The body summarises A/B/C/E, links spec/plan/quickstart results, and states that the carrier-specific rekey selector fix (strongSwan patch / inner-address-family option) is out of scope. Then draft, **but do not post without the user's approval**, a reply on issue #107 explaining what landed and what remains tracked separately

---

## Dependencies & Execution Order

```
T001 ─▶ T002─T004 (Foundational) ─┬─▶ US1: T005–T008 (tests, parallel) ─▶ T009 ─▶ T010
                                   ├─▶ US2: T011 ─▶ T012            (independent of US1; needs only T001)
                                   └─▶ US3: T013/T015/T016 (parallel) ─▶ T014 ─▶ T017 ─▶ T018 ─▶ T019 ─▶ T020
                                                                         T021 ─▶ T022 ─▶ T023
                                                                         T024/T025/T026/T027 (parallel, after T020+T023)
all stories ─▶ T028 ─▶ T029 ─▶ T030 ─▶ T031
```

- US2 touches only `engines.rs` `terminate` and doesn't need Phase 2. It can land first or in parallel with US1. US1's T009 also edits `engines.rs`, so sequence the two edits, not the stories.
- US3's T021–T023 need Phase 2 (`SteadyTracking`). T013–T020 (protocol, metrics, alerts) don't, and can start right after T001.
- T014 must land before T016/T017 compile. Write the T016 tests against the new names and expect a compile failure as the "red" step.

## Parallel examples

- **US1 tests**: T005 (`engines.rs` parser tests), T006 (`engines.rs` engine test, same file as T005, so write them together), T007 and T008 (`line_supervisor.rs`).
- **US3 kick-off**: T013 (`protocol.rs`), T015 (`metrics/mod.rs`), T016 (`ingest.rs` tests), all different files.
- **US3 wrap-up**: T024, T025, T026, T027, all different files.

## Implementation strategy

1. **MVP = Phase 2 + US1**: it closes the detection gap and the endless re-initiate loop, which is most of #107's 170s.
2. **+ US2**: removes the 52s teardown wait (small, low risk; can go in the same PR).
3. **+ US3**: visibility and alerting.
4. **Live gate (T029)** runs once on the combined build. The PR is not mergeable until T030 is recorded (SC-008).

One PR for the whole feature, with one commit per task group as listed above (constitution III).
