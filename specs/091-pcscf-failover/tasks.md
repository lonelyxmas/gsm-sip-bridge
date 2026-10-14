---
description: "Task list for specs/091-pcscf-failover"
---

# Tasks: P-CSCF pool and failover

**Input**: `specs/091-pcscf-failover/`: plan.md, spec.md (with 2026-10-14 clarifications), research.md (R1–R13),
data-model.md, contracts/files.md, contracts/control.md, contracts/observability.md, quickstart.md
**Tests**: Included. The constitution makes TDD the default. Write each test first and watch it fail before you
implement.

**Every commit** (CLAUDE.md, constitution II): run `make format && make lint && make test` first, with a private
`CARGO_TARGET_DIR=$PWD/target` (the shared target dir is clobbered by other worktrees). Commit only if all three pass.
**Never commit real MSISDNs, IMSIs or carrier addresses.** Use:
- P-CSCFs `10.0.0.10` (A), `10.0.1.10` (B), `10.0.2.10` (C), `2402:8100::10` (IPv6);
- locally, `127.0.0.1` (A) and `127.0.0.2` (B);
- IMPU `sip:+919000000000@ims.mnc000.mcc000.3gppnetwork.org`.

All paths are relative to the repo root:

| Shorthand | Path |
|---|---|
| `engines.rs` | `gsm-sip-bridge/src/supervise/engines.rs` |
| `line_supervisor.rs` | `gsm-sip-bridge/src/supervise/line_supervisor.rs` |
| `orchestrate.rs` | `gsm-sip-bridge/src/supervise/orchestrate.rs` |
| `transport.rs` | `gsm-sip-bridge/src/ims/transport.rs` |
| `ims/mod.rs` | `gsm-sip-bridge/src/ims/mod.rs` |
| `session.rs` | `gsm-sip-bridge/src/ims/session.rs` |
| `agent/mod.rs` | `gsm-sip-bridge/src/ims/agent/mod.rs` |
| `ping.rs` | `gsm-sip-bridge/src/ims/agent/ping.rs` |
| `pcscf_pool.rs` | `gsm-sip-bridge/src/ims/agent/pcscf_pool.rs` (new) |
| `watchdog` | `gsm-sip-bridge/src/` watchdog module (`watchdog::Phase`) |
| `control.rs` | `gsm-sip-bridge/src/vowifi/control.rs` |
| `vowifi/mod.rs` | `gsm-sip-bridge/src/vowifi/mod.rs` |
| `carrier_agent.rs` | `gsm-sip-bridge/src/volte/carrier_agent.rs` |
| `bridge.rs` | `gsm-sip-bridge/src/volte/bridge.rs` |

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- Stories:
  - **[US1]** (P1): failover to another advertised P-CSCF (FR-007, FR-008, FR-011, FR-012, FR-016, R7–R9).
  - **[US2]** (P1): never fail over for the wrong reasons (FR-002–FR-004, FR-009, FR-017).
  - **[US3]** (P2): an exhausted pool leads to a bounded rebuild (FR-006, FR-010, FR-013, FR-014, R11).
  - **[US4]** (P2): a restart does not walk back into a dead P-CSCF (FR-015, Q3, R3, R8).
  - **[US5]** (P3): status and logs (FR-019, FR-020).
- FR-018 (per-line isolation) holds by construction: every file and every `PcscfPool` is per line. It is asserted in
  T014 and T020.

---

## Phase 1: Setup

- [X] T001 Confirm the baseline on branch `091-pcscf-failover` is green: from the repo root, run
  `CARGO_TARGET_DIR=$PWD/target make lint && CARGO_TARGET_DIR=$PWD/target make test`. Stop if anything fails before
  any code is touched.

---

## Phase 2: Foundational (blocks every story)

**Purpose**: extract and publish the pool, read it in the agent, and route every registration through a
`PcscfPool`. **Behaviour is unchanged at the end of this phase**: the same address is chosen, the same failures
take the same paths, and nothing is marked yet.

- [X] T002 [P] Write failing tests in `engines.rs` (beside `a_new_negotiation_replaces_the_previous_pcscf_batch`):
  - `extract_latest_pcscf_pool(log, "ims0")` returns all four addresses of the latest batch, in log order and
    deduplicated. Use the existing facility-tagged charon fixture, including two IPv6 and two IPv4 addresses.
  - A second negotiation replaces the first batch (FR-003).
  - Another connection's lines are ignored.
  - `extract_pcscf_pool_after(log, conn, mark)` returns `None` for an unclosed batch and for a batch before `mark`.
  - `extract_swu_pcscf_pool(log)` collects every `P-CSCF IPV4 ADDRESS` and `P-CSCF IPV6 ADDRESS` address, in order.
- [X] T003 Implement T002 in `engines.rs`:
  - The three functions, built on `latest_pcscf_batch`.
  - `TunnelEngine::latest_pcscf_pool(&self, runner) -> Vec<String>` in `line_supervisor.rs`, implemented for
    `StrongswanEngine` and `SwuEngine`, and for the test engine in the `line_supervisor.rs` tests (it returns its
    configured pool).
  - `StrongswanEngine::fresh_pcscf_pool_since(runner, mark)`.

  Leave `extract_latest_pcscf`, `extract_pcscf_after` and `pick_stable_pcscf` untouched (FR-004). Commit.
- [X] T004 [P] Write failing tests in `orchestrate.rs` with the existing `MockCommandRunner` for a new
  `publish_pcscf_pool(runner, pcscf_source_path, pool)`:
  - It writes `<path>.pool.tmp`, then renames it to `<path>.pool`. Add `CommandRunner::rename` if the runner has no
    rename; the mock records it.
  - Content is one address per line with a trailing newline.
  - It prints `[supervise] line N: P-CSCF pool: a, b` (contracts/observability.md).
- [X] T005 Implement `publish_pcscf_pool` in `orchestrate.rs` and call it **before** each of the existing selection
  writes: establishment, `PcscfChanged`, and the post-recovery `fresh` sync, in `start_vowifi_line_strongswan` and
  `start_vowifi_line_swu`. Use `engine.latest_pcscf_pool` / `fresh_pcscf_pool_since`. Add `let mut current_pool` beside
  `current_pcscf`. Commit.
- [X] T006 [P] Write failing tests in `transport.rs`:
  - `EpdgTransport::prepare` with `/tmp/pcscf-test-*` and `/tmp/pcscf-test-*.pool` temp files returns
    `ImsTransportHandle { pcscf, pool: [A, B, IPv6] }`.
  - A missing `.pool` file gives `pool == [selection]`.
  - Blank and garbage lines are skipped.
  - Existing selection-file tests are unchanged.
- [X] T007 Implement T006 in `transport.rs`: add `ImsTransportHandle.pool: Vec<IpAddr>` and a reusable
  `read_pcscf_pool(path) -> Vec<IpAddr>`, which is also used before each renewal in T019. Commit.
- [X] T008 [P] Write failing tests in the new `pcscf_pool.rs` for the parts that preserve behaviour
  (data-model.md):
  - `PcscfPool::new(selection, pool, state_path)` builds `candidates()` as: selection first, then IPv4 in pool order,
    then IPv6, with duplicates removed.
  - A selection absent from the pool is still first (spec edge case).
  - An empty pool gives `[selection]` (FR-006).
  - `select(now)` with no marks returns the selection with `SelectReason::Startup` the first time, and `Sticky` on
    later calls once `on_registered(addr)` has set `active`.
  - Use injected `SystemTime`s only.
- [X] T009 Implement T008: create `pcscf_pool.rs` with `PcscfPool`, `Mark`, `Selection`, `SelectReason`, `FailureCause`
  (variants and kebab-case `Display`), `new`, `candidates`, `select`, `on_registered` and `snapshot`. Leave marks
  empty for now. Register `mod pcscf_pool;` in `agent/mod.rs` and add a row to the module table at the top of that
  file. Commit.
- [X] T010 Add `ImsRegisterConfig::with_pcscf(&self, addr: IpAddr) -> Self` in `ims/mod.rs`, a clone with only
  `pcscf_addr` replaced, with a unit test. Add `InboundParams.pcscf_pool: PcscfPool` in `agent/mod.rs`. Wire the two
  callers:
  - VoWiFi `run_inner`: from the transport handle; state path `format!("{}.state", config.pcscf_source_path)`.
  - VoLTE `carrier_agent.rs`: a one-element pool `[line.settings.pcscf.ip()]`; state path
    `/tmp/volte-pcscf-<card_id>.state`.

  `serve_inbound` calls `pool.select(SystemTime::now())`, registers through `reg_cfg.with_pcscf(sel.addr)`, logs
  `"P-CSCF selected"` and `"registration through P-CSCF"` (contracts/observability.md), calls `on_registered` on
  success, and moves the pool into `LoopState.pool`. `on_idle_tick` does the same per renewal. Every existing test
  must pass unchanged. Commit.

**Checkpoint**: `.pool` files appear beside `/tmp/pcscf-N`, every registration goes through `PcscfPool`, and nothing
fails over yet.

---

## Phase 3: User Story 1: move to another advertised P-CSCF when the current one stops answering (P1) 🎯 MVP

**Goal**: a qualifying failure on the active P-CSCF marks it, and the very next attempt goes to the next unmarked
candidate, with no tunnel rebuild.

**Independent Test**: quickstart Q1. Against a refused or silent `127.0.0.1` and a `127.0.0.2`, a renewal moves to
`127.0.0.2` on the next attempt, with no backoff step. Live: quickstart Q2.

### Tests for User Story 1

- [X] T011 [P] [US1] Write failing tests in `ims/mod.rs` for `classify_register_failure(&BridgeError) -> FailureClass`
  (research R4), using real errors where CI allows:
  - `SipTransport::connect` over TCP to a closed port on `127.0.0.1` (real `ECONNREFUSED`) gives `Transport`.
  - `register_session` with `use_tcp: false` against a bound UDP socket that never answers gives `Timeout`.
    `imsi`/`imei` overrides are set, so no modem is touched before the send.
  - `io::Error::from_raw_os_error(101)` and `(113)`, formatted as the connect error, give `NoRoute`.
  - `"connection closed by peer with no data (0 bytes read)"` gives `PeerClosed`.
  - An `AT+CSIM failed` error and a nonce error give `NotPcscf`.
- [X] T012 [P] [US1] Write failing tests in `pcscf_pool.rs` for marks:
  - `on_failure(active, RegisterTimeout, now)` gives `until = now + 330s` and `failures = 1`. Repeats give 360, 420,
    540, 780, 1260 and 2100 s, then stay at 2100 s (data-model.md formula).
  - `RetryAfter(600s)` gives `until = now + 600s` and increments `failures`.
  - After marking A: `select` returns B with `Failover`, and `has_unmarked` is true.
  - `on_registered(B)`, then A's mark expiring, then `select` still returns B, `Sticky` (FR-012, US1 AS4).
  - `on_registered(A)` resets A's `failures` to 0.
- [X] T013 [P] [US1] Write failing tests in `session.rs`: `attempt_renewal` returns
  `Err(RenewalFailure { status: Some(503), retry_after: Some(600s), .. })` for a local fake registrar on `127.0.0.1`
  that answers the first REGISTER with `503 Service Unavailable` plus `Retry-After: 600`. Without the header,
  `retry_after: None`. `Retry-After: 600 (maintenance)` parses as 600 s.
- [X] T014 [US1] Write failing integration tests in `agent/mod.rs`, in the style of
  `a_persistently_failing_renewal_exits_instead_of_retrying_forever`. Use `LoopState`, a pool `[127.0.0.1,
  127.0.0.2]`, A as a closed TCP port, and B as a listener that records connection attempts:
  - After a renewal against A fails with `Transport`:
    - A is marked;
    - `next_renewal_attempt` is `None` and `force_renewal` is true;
    - `consecutive_renewal_failures` is 1;
    - `backoff` is still `RETRY_INITIAL_BACKOFF`;
    - the next idle tick connects to B.
  - The same holds for a silent A (`Timeout`) and for a `503` with `Retry-After: 600`.
  - The same scenario on a second `LoopState` with its own pool and state file leaves the first pool untouched
    (FR-018).
- [X] T015 [P] [US1] Write failing tests in `agent/mod.rs`:
  - `on_qualifying_endpoint_failure` with the session's P-CSCF equal to the active one marks it with
    `EndpointFailure(cause)`.
  - With `busy()` true, the mark is immediate but no renewal runs until the call ends (FR-016, US1 AS5;
    `MaintenancePolicy` defers).
  - In `ping.rs`: `probe_gm_connection` returns `true` exactly on the call that sets `force_renewal` after
    `MAX_RECONNECT_ATTEMPTS`, and `false` otherwise.
- [X] T016 [P] [US1] Write failing tests in `line_supervisor.rs` (research R9):
  - `tick_steady_state` with a dead `current_pcscf` but a pool member that answers `tcp_connect_ok_in_netns` never
    returns `Recovered { PcscfUnreachable }` across 2× `pcscf_unreachable_strikes` ticks.
  - With every pool member dead, it still does.
  - A healthy selection probes no other member: the mock records exactly one connect per tick.

### Implementation for User Story 1

- [X] T017 [US1] Implement `FailureClass` and `classify_register_failure` in `ims/mod.rs` per R4. Match `os error N`
  first, then the English text. Add `route_to(addr: IpAddr, port: u16) -> Result<(), io::Error>`: a UDP socket
  `connect()` with nothing sent (R5). Make T011 pass. Commit.
- [X] T018 [US1] Implement `on_failure`, mark durations and `has_unmarked` in `pcscf_pool.rs` (T012). Implement
  `RenewalFailure` and a `retry_after(headers) -> Option<Duration>` helper in `session.rs`/`ims/mod.rs` (T013). Update
  the call sites of `attempt_renewal`. Commit.
- [X] T019 [US1] Implement renewal failover in `on_idle_tick` (`agent/mod.rs`), per R7:
  1. Before the attempt, re-read `.pool` with `read_pcscf_pool`. If the set changed, call `pool.replace_pool` (stubbed
     to a plain replace until T031).
  2. On `Err(RenewalFailure)`, classify. `Timeout`/`Transport`, or a `503` with `retry_after > TIMER_F` (128 s), on
     the active address, *and* `route_to(addr)` OK, gives `pool.on_failure`.
  3. Increment `consecutive_renewal_failures`, and exit if the budget is spent.
  4. If `has_unmarked`, log `"failing over to the next P-CSCF now"`, set `next_renewal_attempt = None` and
     `force_renewal = true`. Otherwise call `schedule_renewal_retry()`.
  5. Non-qualifying failures take exactly today's path.

  Make T014 pass. Commit.
- [X] T020 [US1] Implement startup failover in `serve_inbound` (`agent/mod.rs`), per R8. Loop `select` →
  `register_session(reg_cfg.with_pcscf(..))`. On a qualifying failure, mark and try the next unmarked candidate in
  the same process. On a non-qualifying failure, or once no unmarked candidate remains, return `Err` as today. Add a
  test: a closed `127.0.0.1` at startup leads to an attempt on `127.0.0.2` in the same call, asserted via the B
  listener's recorded connect. Commit.
- [X] T021 [US1] Wire the other failure sources in `agent/mod.rs` and `ping.rs`:
  - `on_qualifying_endpoint_failure` calls `pool.on_failure(session P-CSCF, EndpointFailure(cause), now)` after
    `route_to`.
  - `probe_gm_connection` returns the escalation `bool`, and `on_idle_tick` marks with `GmKeepalive`.
  - Make T015 pass. Update the doc comment on `on_qualifying_endpoint_failure`, which currently says "spec 091
    builds on this", to describe what it now does. Commit.
- [X] T022 [US1] Implement R9 in `line_supervisor.rs` and `orchestrate.rs`:
  - `tick_steady_state(engine, runner, current_pcscf, current_pool, tracking, poll)` counts reachable as "the
    selection answers, else any other member does".
  - Print `P-CSCF A unreachable; pool member B answers` once per episode, tracked in `SteadyTracking`.
  - Update both steady loops and the test shim `tick_steady_state` in `line_supervisor.rs` tests.
  - Make T016 pass. Commit.

**Checkpoint**: US1 is complete. A dead active P-CSCF is replaced by the next candidate within one idle tick, at
startup and on renewal, and the supervisor no longer rebuilds the tunnel over it.

---

## Phase 4: User Story 2: failover never fires for the wrong reasons (P1)

**Goal**: tunnel loss, reorders, additions, network refusals, deregistration notices, peer closes and stale events
never mark or move.

**Independent Test**: each non-qualifying situation below leaves the active P-CSCF and the marks unchanged (spec US2
AS1–AS5).

### Tests for User Story 2

- [X] T023 [P] [US2] Write failing tests in `agent/mod.rs`, driving the T014 harness with each non-qualifying
  outcome. For each, assert no mark, the same active P-CSCF, and today's ladder:
  - a `NoRoute` connect error (US2 AS1);
  - `route_to` failing for an otherwise-qualifying `Timeout` (edge case: tunnel drops at the same moment);
  - `403` (AS4);
  - `503` with `Retry-After: 60` (R12);
  - `503` without `Retry-After`;
  - `PeerClosed` (Q2);
  - a reg-event `rejected` NOTIFY through `on_network_deregistration` (AS5).
- [X] T024 [P] [US2] Write failing tests in `pcscf_pool.rs` for FR-017:
  - `on_failure(addr, ..)` with `addr != active` returns `None`, changes nothing, and logs
    `"failure not attributed to the P-CSCF; not marking"` with `why=not-active`.
  - After `on_registered(B)`, a late `on_failure(A, GmKeepalive)` does not mark B or A.
- [X] T025 [P] [US2] Write failing tests in `orchestrate.rs` (mock runner) for a steady tick:
  - A reorder of the same set (`[B, A]` for published `[A, B]`) writes nothing, runs no `pkill`, and does not print
    `P-CSCF changed` (US2 AS2, SC-002).
  - Adding C (`[A, B, C]`) rewrites `.pool` only, with no `pkill`, and prints
    `P-CSCF pool changed (set): …; not restarting` (US2 AS3).

### Implementation for User Story 2

- [X] T026 [US2] Make T023 pass in `agent/mod.rs`. The classification gate from T019 should already cover most of it.
  Fix what doesn't, and log `"failure not attributed to the P-CSCF; not marking"` with `why=no-route` when
  `route_to` fails. Commit.
- [X] T027 [US2] Make T024 pass in `pcscf_pool.rs`. Commit.
- [X] T028 [US2] Implement the set-change publication in both steady loops in `orchestrate.rs`: after each tick,
  compare the `engine.latest_pcscf_pool` set with the `current_pool` set; when they differ and the selection did not
  change, call `publish_pcscf_pool` only. Make T025 pass. Commit.

**Checkpoint**: US1 and US2 hold together. Failover fires only on a P-CSCF's own fault.

---

## Phase 5: User Story 3: an exhausted pool leads to a bounded, orderly rebuild (P2)

**Goal**: with every candidate marked, attempts follow the existing ladder, target the soonest-expiring mark, and
exit to the supervisor at the existing budget. A new negotiation replaces candidates and marks.

**Independent Test**: spec US3 AS1–AS4, with an injected clock; quickstart Q0.

### Tests for User Story 3

- [X] T029 [P] [US3] Write failing tests in `pcscf_pool.rs`:
  - With A and B both marked, and B's mark expiring first, `select` returns B with `AllUnavailable`. With equal
    expiries, it returns A (candidate order).
  - `replace_pool([C, D], C)` drops the marks for A and B, drops `last_registered`, and clears `active` (US3 AS4,
    FR-014).
  - `replace_pool([B, A, C], A)` keeps the marks for A and B.
  - A one-element pool's single address is marked and still selected (US3 AS2).
- [X] T030 [US3] Write failing tests in `agent/mod.rs` with the T014 harness and both A and B dead:
  - The attempts go A, B, then ladder steps of 5, 10, 20, 40 and 80 s (`RETRY_INITIAL_BACKOFF` doubling), each to
    the soonest-expiring mark.
  - The agent returns `Err` when `consecutive_renewal_failures` reaches `MAX_CONSECUTIVE_RENEWAL_FAILURES` (US3 AS1,
    SC-003).
  - No two attempts are closer together than the ladder step, except the immediate A→B.

### Implementation for User Story 3

- [X] T031 [US3] Implement the all-marked branch of `select` and the full `replace_pool` in `pcscf_pool.rs` (T029).
  Log `"P-CSCF pool replaced"` when the set changes and `"every P-CSCF is unavailable; retrying on backoff"` in
  `agent/mod.rs`. Make T029 and T030 pass. Commit.
- [X] T032 [US3] Confirm VoLTE parity in `carrier_agent.rs` with a test: a one-element pool with a qualifying failure
  marks it and takes `schedule_renewal_retry()` (today's ladder) (US3 AS2, R11). Commit.

**Checkpoint**: US1–US3 hold. Two dead P-CSCFs can never be retried faster than the ladder.

---

## Phase 6: User Story 4: a restart does not walk back into a dead P-CSCF (P2)

**Goal**: marks and the last working P-CSCF survive an agent restart (FR-015, clarification Q3), and a fully marked
pool at startup cannot be retried faster than the ladder (R8).

**Independent Test**: spec US4 AS1–AS3; quickstart Q3.

### Tests for User Story 4

- [X] T033 [P] [US4] Write failing tests in `pcscf_pool.rs` with a real temp dir (contracts/files.md):
  - Round trip: `save` then `load` keeps marks (`until_unix`, `failures`, `cause`) and `last_registered`.
  - The write is atomic: a `.state.tmp` is renamed, and no partial file is ever observed.
  - Load against a pool that lacks A drops A's mark (FR-014).
  - An expired mark keeps `failures`, but A is eligible.
  - Corrupt JSON, `version: 2`, and a missing file all load as empty state, with one `WARN` for the first two.
  - A save into an unwritable directory logs `"could not save P-CSCF state"` and does not panic.
  - Scenarios:
    - A marked for 10 more minutes, then restart: `select` gives B (US4 AS1).
    - A expired and `last_registered = B`, then restart: B (US4 AS2).
    - A expired and B marked, then restart: A (US4 AS3).
- [X] T034 [P] [US4] Write failing tests in `agent/mod.rs` for the startup wait (R8):
  - With every candidate marked and the soonest expiry 30 s away, `serve_inbound`'s selection step waits 30 s
    before its single attempt.
  - At 600 s away, it waits `RETRY_MAX_BACKOFF` (120 s).
  - The wait runs under `watchdog::Phase::PcscfWait` and advances progress every second.
  - Drive this through an injected sleep, so no test sleeps for real.

### Implementation for User Story 4

- [X] T035 [US4] Implement `load` and `save` in `pcscf_pool.rs`: serde `PersistedPool { version, last_registered,
  marks }`, atomic temp-then-rename, and reconciliation on load. Call `save` from `on_failure`, `on_registered` and
  `replace_pool`. Have `PcscfPool::new` call `load`. Implement `candidates()` putting `last_registered` first when it
  is unmarked. Make T033 pass. Commit.
- [X] T036 [US4] Implement the startup wait in `serve_inbound` (`agent/mod.rs`) and add `Phase::PcscfWait` to the
  watchdog module (a budget of `RETRY_MAX_BACKOFF` + 10 s). Log `"every P-CSCF is unavailable; waiting before the
  startup attempt"`. Make T034 pass. Commit.

**Checkpoint**: US1–US4 hold. An agent restart honours marks and stays on the working P-CSCF.

---

## Phase 7: User Story 5: operators can see which P-CSCF a line uses and why (P3)

**Goal**: one status query shows the active P-CSCF, the pool and the marks. Every selection and its outcome is
logged.

**Independent Test**: spec US5 AS1–AS2; quickstart Q2 steps 5–6.

### Tests for User Story 5

- [X] T037 [P] [US5] Write failing tests in `control.rs`: a `RegistrationStatusReply` round trip with `pcscf_active`,
  `pcscf_pool` and `pcscf_unavailable` set. A reply JSON *without* those fields (an older agent) still parses, with
  the defaults.
- [X] T038 [P] [US5] Write failing tests in `vowifi/mod.rs` for `format_pcscf_lines(active, pool, unavailable) ->
  [String; 2]`:
  - `pcscf: 10.0.1.10 (pool: 10.0.1.10, 10.0.0.10)`;
  - `pcscf_unavailable: 10.0.0.10 until 2026-10-14T10:08:41+00:00`;
  - `none` when nothing is marked;
  - `pcscf: unknown` for an empty active;
  - several marks, comma-separated (contracts/control.md).
- [X] T039 [P] [US5] Write failing tests in `pcscf_pool.rs`: `snapshot(now)` lists only marks with `until > now`, in
  candidate order.

### Implementation for User Story 5

- [X] T040 [US5] Implement the status path:
  - `PcscfSnapshot` and `RegistrationStatus.pcscf` in `ims/mod.rs`, in memory only, with a default.
  - Sync it in `sync_shared_status` and once after the startup registration (`agent/mod.rs`).
  - Add the three `#[serde(default)]` reply fields in `control.rs`, filled in `run_status_listener`.
  - `format_pcscf_lines` in `vowifi/mod.rs`, printed after `reg_subscription_last_failure` in `vowifi-status`.
  - The same in `volte-status` (`bridge.rs`).

  Make T037–T039 pass. Commit.
- [X] T041 [US5] Audit every log line in contracts/observability.md against the code (messages and field names).
  Add any that are missing, for example `previous`/`candidates` on `"P-CSCF selected"`, and suppress the repeated
  same-address `sticky` line. Add one test asserting that a failover emits, in order: selected, failed, marked,
  failing over, selected. Capture with the existing tracing test-capture helper if one exists; otherwise assert on
  the state transitions and leave the log order to quickstart Q1. Commit.

**Checkpoint**: every user story works independently.

---

## Phase 8: Polish & cross-cutting

- [X] T042 [P] Add a P-CSCF section to the operator docs under `docs/`, next to the vowifi status and troubleshooting
  section that 090 extended. Cover:
  - the `.pool` and `.state` files;
  - the two new status lines;
  - what marks and what never does;
  - how to clear state (delete `/tmp/pcscf-N.state` and restart the agent).
- [X] T043 [P] Update `RELEASE_NOTES.md` and `CHANGELOG.md`. Bold only the summary (memory: release notes bold
  scope), for example `**P-CSCF failover**`, followed by a plain-text description.
- [X] T044 Run quickstart Q0 in full. Confirm `make lint` is clean across all targets and that no test or fixture
  contains a real address or number (`grep -rn` the diff for anything outside the placeholders listed at the top).
- [X] T045 Live gate quickstart Q2 and Q3 on the Jio Pi (blocking: SC-001, SC-004, SC-005, R9). Record the
  `"P-CSCF marked unavailable"` / `"failing over"` lines and the `vowifi-status` output (addresses redacted) in the
  PR description.
- [ ] T046 Live gate quickstart Q4: 6 h steady state on the Vodafone rig (SC-002). There must be no marks and no
  `P-CSCF changed` lines.

---

## Dependencies & execution order

- **Phase 1 → Phase 2**: everything depends on T001.
- **Phase 2 (T002–T010)**: blocks all stories.
  - T002 → T003 → T005 (needs `latest_pcscf_pool`).
  - T004 → T005.
  - T006 → T007.
  - T008 → T009 → T010.
  - The chains T002/T003, T004/T005, T006/T007 and T008/T009 are independent of each other.
- **US1 (T011–T022)**: after Phase 2.
  - T011 → T017.
  - T012 + T013 → T018.
  - T017 + T018 + T014 → T019 → T020.
  - T015 → T021 (needs T019).
  - T016 → T022 (independent of the agent tasks; can run beside T017–T021).
- **US2 (T023–T028)**: after T019 (needs the classification gate). T025 → T028 needs only T005.
- **US3 (T029–T032)**: after T019. T029 and T030 → T031 → T032.
- **US4 (T033–T036)**: after T018. T033 → T035; T034 → T036 (needs T020).
- **US5 (T037–T041)**: needs only Phase 2 and T009, so it can run beside US2–US4. T041 comes last, after all the log
  lines exist.
- **Polish**: after the stories it documents. T045 and T046 come last.

### Parallel opportunities

- Phase 2: T002, T004, T006 and T008 touch four different files. Write all four failing tests together.
- US1: T011 (`ims/mod.rs`), T012 (`pcscf_pool.rs`), T013 (`session.rs`) and T016 (`line_supervisor.rs`) run in
  parallel. T022 (supervisor) can proceed while T017–T021 (agent) are in flight.
- US2: T023, T024 and T025 run in parallel.
- US5: T037, T038 and T039 run in parallel, and the whole story can run beside US3/US4 if another developer is
  available.

### Parallel example: User Story 1

```text
Together:  T011 classify tests (ims/mod.rs)   T012 mark tests (pcscf_pool.rs)
           T013 RenewalFailure tests (session.rs)   T016 reachability tests (line_supervisor.rs)
Then:      T017 → T018 → T014/T019 → T020 → T015/T021      ‖      T022
```

## Implementation strategy

- **MVP = Phase 1 + Phase 2 + US1 (T001–T022)**. A dead P-CSCF is replaced live by the next candidate, which is the
  capability the standard requires. Gate it with quickstart Q1, and Q2 if the Pi is available.
- **Then US2** (also P1): the guardrails that make US1 safe to ship. Do not merge US1 without US2.
- **Then US3 and US4** (P2): bounded exhaustion and restart memory. They can land in either order.
- **Then US5** (P3): status and log polish. Its fields can land earlier if they help with debugging US1.
- Commit after every task or test+implementation pair, with `make format && make lint && make test` green each time.

---

## Implementation notes (where the build departed from the task text)

- **T013**: `register_session` opens the modem before it sends anything, so a REGISTER against a local fake registrar
  cannot run in CI. Two things cover it instead:
  - the `Retry-After` parsing tests (`ims::tests::retry_after_reads_seconds_and_ignores_comments`);
  - the `cause_of` table (`pcscf_pool::tests::only_the_pcscfs_own_failures_amount_to_a_cause`).

  `register_checked` reads the header before `cleanup()`.
- **T014/T020**: the agent tests drive `register_via_pool` and `LoopState::on_renewal_failure` directly. The attempt
  itself is a real connect to `127.0.0.1`/`127.0.0.2`: a refused TCP connect, or a silent UDP REGISTER. Only what
  follows the transport (SIM, AKA) is stubbed, with the justification at the stub.
- **T015**: the endpoint-failure and keepalive marks are covered through `mark_active_pcscf`. Two things have no new
  test:
  - `probe_gm_connection`'s new return value needs a live session and Gm listener;
  - the mid-call deferral is the existing `MaintenancePolicy` path, unchanged.
- **T036**: the startup wait uses the existing `watchdog::Phase::Dormant` ("asleep by design, never a stall") and
  re-enters `Startup` afterwards, rather than adding a `PcscfWait` phase.
- **T040**: the reply carries the P-CSCF status as one boxed `pcscf: Option<PcscfStatusReply>`, not three flat
  fields. Three inline lists tripped `clippy::large_enum_variant` on `ControlMessage`. contracts/control.md is
  updated to match.

## Live gate T045 (Jio Pi, 2026-10-14, image `pcscf-failover-091-arm64`)

The negotiation advertised four P-CSCFs: two IPv4, A (the selection) and B, and two IPv6. The host has no
`iptables`, so A was black-holed with a /32 route to the line's veth peer inside the agent's namespace. New connects
to A time out, and the route probe still finds a route.

| UTC | Event |
|---|---|
| 17:11:48 | A black-holed |
| 17:12:47 | `Gm keepalive went unanswered` |
| ~17:13 | Supervisor: `P-CSCF A unreachable; pool member <IPv6> answers`; no rebuild (R9) |
| 17:14:43 | `Gm reconnect exhausted`, then `P-CSCF marked unavailable` (A, `gm-keepalive`, 330 s), then `P-CSCF selected` (B, `failover`) |
| 17:14:44 | `registration through P-CSCF` (B, `registered`); reg-event subscription confirmed on B |
| 17:15:22 | Agent killed inside A's mark: first selection B (SC-005, US4 AS1) |
| 17:20:22 | Route removed; A's mark expired 17:20:13 |
| 17:20:36 | Agent killed after expiry: stays on B (US4 AS2, Q3) |

- **SC-001**: registered through B 2 min 56 s after the black hole (keepalive interval plus 3 reconnects), with no
  tunnel rebuild.
- **SC-004**: zero marks outside the deliberate black hole.
- **Rebuilds**: zero `recovered from …` and zero `P-CSCF changed` lines since the deploy.
- **Not exercised live**: an inbound call after the failover. Registration, the confirmed reg-event subscription
  and `can_answer: true` on B were checked instead.
