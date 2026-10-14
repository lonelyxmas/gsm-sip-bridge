# Research: P-CSCF pool and failover

The Technical Context had no NEEDS CLARIFICATION items. The entries below record the design decisions that reading
the code forced. Each one cites the code it rests on.

---

## R1. Where the pool comes from

**Decision**: Read the pool from the source each engine already parses.

- **strongswan**: `latest_pcscf_batch` in `supervise/engines.rs` already returns the latest negotiation's whole batch
  and whether it is closed. `pick_stable_pcscf` reduces that batch to one address. Two new functions expose the
  batch itself, deduplicated and in log order:
  - `extract_latest_pcscf_pool(log, conn)`;
  - `extract_pcscf_pool_after(log, conn, mark)`, which, like `extract_pcscf_after`, accepts only a closed batch.
- **swu**: `extract_swu_pcscf` takes the first `P-CSCF IPV4 ADDRESS` line, else the first IPv6 one. A new
  `extract_swu_pcscf_pool` collects every address from both kinds of line, in log order.
- **Trait**: `TunnelEngine` gains `latest_pcscf_pool(&self, runner) -> Vec<String>`. `StrongswanEngine` gains
  `fresh_pcscf_pool_since(runner, mark)` beside `fresh_pcscf_since`.

**Rationale**: The batch logic, including the "a new negotiation clears the previous batch" rule that FR-003 needs,
already exists and is tested (`a_new_negotiation_replaces_the_previous_pcscf_batch`). The selection and the pool
come from one parse, so they can never disagree about which negotiation they describe.

**Alternatives**: Have the agent parse the charon log itself. Rejected: the log belongs to the supervisor, it is
shared by all lines, and the agent runs inside the netns.

## R2. How the pool is published (FR-001–FR-004)

**Decision**: `/tmp/pcscf-N.pool` sits beside the existing `/tmp/pcscf-N` (the path is
`format!("{pcscf_source_path}.pool")`). It holds one address per line in negotiation order and is written
atomically: a temp file, then a rename.

A new `publish_pcscf_pool(runner, line, pool)` in `orchestrate.rs` writes it.

- **When**: the same three places that write the selection today (initial establishment, `PcscfChanged`, and the
  post-recovery sync), on both engines.
- **Order**: the pool is always written *before* the selection, so an agent that reads the selection never reads a
  pool older than it.
- **Set changes**: the steady loop keeps `current_pool` beside `current_pcscf`. After each tick, it compares the set
  from `engine.latest_pcscf_pool` with the set it last published. If they differ, it rewrites only the `.pool` file:
  no `pkill`, no log line about refreshing. That covers US2 AS3, where a third address joins without a move.
- A reorder alone (same set) writes nothing.

**Rationale**:
- `healthcheck.rs` (`[ -s "$pcscf_path" ]`), the keepalive loop in `start_line_tail`, and `EpdgTransport` all read
  the selection file as a single address. Leaving it alone is FR-004 by construction.
- Comparing sets rather than sequences is the same reasoning as `pick_stable_pcscf`: the carrier reorders freely.

**Alternatives**: Send the pool to the agent over the control channel. Rejected: the agent reads its P-CSCF before
the control channel is up, and a file survives the agent's restart for free.

## R3. Where marks and the last working P-CSCF live across restarts (FR-015, clarification Q3)

**Decision**: The agent owns `/tmp/pcscf-N.state` (`format!("{pcscf_source_path}.state")`). It is JSON, written
atomically after every mark and every successful registration:

```json
{"version":1,"last_registered":"10.0.1.10",
 "marks":[{"addr":"10.0.0.10","until_unix":1792000000,"failures":2}]}
```

On load, the file is reconciled against the freshly read pool:

- Marks for addresses not in the pool are dropped (FR-014).
- A `last_registered` that is not in the pool is dropped.
- An expired mark keeps its `failures` count, so the next failure grows the delay (spec edge case), but it no longer
  blocks selection.
- A missing, corrupt or wrong-version file reads as empty state, with one `WARN`.

VoLTE uses `/tmp/volte-pcscf-<card_id>.state`.

**Rationale**:
- Wall-clock (`SystemTime`) expiry is required because the state crosses processes; `Instant` cannot.
- `/tmp` matches the spec assumption: survive an agent restart, not a host reboot. A container *restart* (not a
  recreate) keeps `/tmp`, so state can be up to 35 min stale (memory: switching transports, recreate). That is
  harmless: expiry and pool reconciliation both bound it, and a stale mark only reorders candidates. It never
  prevents an attempt.

**Alternatives**: Keep the state in the supervisor (needs a new control message plus restart ordering), or in the
SQLite store (a schema change, and the wrong lifetime). Both rejected under Principle V.

## R4. Classifying a registration failure (FR-007–FR-009, clarification Q2)

**Decision**: `classify_register_failure(&BridgeError) -> FailureClass` in `ims/mod.rs`, a text heuristic in the
style of `map_registration_error`. `register_session` builds every transport error with the `io::Error` `Display`,
which ends in `(os error N)`.

| Evidence in the error text | Class |
|---|---|
| `os error 101` / `Network is unreachable`, `os error 113` / `No route to host` | `NoRoute`: never marks |
| `TCP connect to … failed` / `UDP connect to … failed` with `os error 111` (refused) or `110` / `timed out` (connect timeout) | `Transport`: qualifying |
| `send failed` (a send on a connection opened by this attempt) | `Transport`: qualifying |
| `timed out waiting for a SIP response` | `Timeout`: qualifying (REGISTER transaction timeout, R5) |
| `connection closed by peer`, `recv failed` | `PeerClosed`: not qualifying (Q2) |
| anything else (AKA, SIM, `AT+CSIM`, parse) | `NotPcscf`: never marks |

The match is on the specific `os error N` first and the English text second, so a localized libc message still
classifies.

**Rationale**:
- `register_session` is a 2,000-line function built on `?`. Giving it a typed error would touch every return for
  one consumer.
- Each row is tested on a real error: a connect to a closed port on `127.0.0.1` (real `ECONNREFUSED`) and a REGISTER
  to a bound but silent UDP socket (real timeout). Only `ENETUNREACH`, which cannot be produced without
  `CAP_NET_ADMIN` (memory: sandbox blocks root network testing), is built with `io::Error::from_raw_os_error(101)`.

**Alternatives**: A `BridgeError::Transport { kind, … }` variant. Deferred: worth doing if a third consumer appears.

## R5. "Timer F" in this implementation, and the tunnel-up gate

**Decision (timeout)**: The REGISTER transaction timeout that counts as Timer F here is the existing one:
`recv_response` gives up after `RECV_TIMEOUT` (5 s) with `timed out waiting for a SIP response`. It is not changed
to 128 s.

- The 128 s figure stays as the named constant `TIMER_F`, used only as FR-008's `Retry-After` threshold.
- Acceptance tests drive "no response" rather than a literal 128 s wait.

**Rationale**: Today a 5 s silence already fails the attempt and moves the line onto the ladder. Stretching it to
128 s would slow every startup and renewal failure path that works now, and the spec's intent is "the attempt timed
out", not the number. TS 24.229 derives 128 s from 64 × T1 for UDP retransmission, which this client does not do.

**Decision (tunnel up)**: Before a qualifying failure marks anything, `route_to(addr)` must succeed. It opens a UDP
socket and `connect()`s it to the P-CSCF; no packet is sent, the kernel only does a route lookup. `ENETUNREACH` or
`EHOSTUNREACH` means the tunnel or attachment is down, and the failure does not mark (FR-009, edge case "tunnel
drops at the same moment").

- On VoLTE, a failed `pre_renewal` hook already returns before any REGISTER is attempted, so nothing marks there.
- A silently blackholed tunnel (route present, nothing passes) cannot be told apart from a dead P-CSCF. Each
  candidate then gets marked once, the ladder runs, and the supervisor's reachability check (R9) rebuilds the tunnel
  within ~90 s. That bounds the damage to marks on a pool that the rebuild usually replaces anyway.

**Decision (stale events, FR-017)**: Every failure carries the P-CSCF address of the session or attempt it was seen
on. `PcscfPool::on_failure` ignores, with an `INFO` line, any failure whose address is not the current active one.
Spec 090 already drops superseded-subscription events, and `ping.reset()` on renewal already drops old keepalives,
so this is a second guard, not the only one.

## R6. `PcscfPool`: a pure module (FR-005, FR-006, FR-010–FR-014)

**Decision**: `ims/agent/pcscf_pool.rs`. No I/O except `load`/`save`, which take a path. The clock is injected as a
`SystemTime` on every call.

- **Candidate order**:
  1. `last_registered`, if it is in the pool and unmarked (Q3);
  2. the supervisor's selection;
  3. the rest of the pool, IPv4 first, then IPv6, each in pool order.

  Duplicates are removed. A selection that is not in the pool (inconsistent files) is still candidate 2 (spec edge
  case).
- **`select(now)`**:
  1. If the active P-CSCF is unmarked, keep it (FR-012).
  2. Otherwise take the first unmarked candidate.
  3. If every candidate is marked, take the soonest-expiring mark, with ties broken by candidate order (Q4).

  It returns a `Selection { addr, reason }`.
- **Mark length**: `min(1800, 30 · 2^(k−1)) s + 300 s`, where k is the address's consecutive failures including this
  one, so 330 s, 360 s, 420 s, … up to 2,100 s. A qualifying `503` uses `Retry-After` instead (FR-008), and still
  increments k.
  - This takes the spec's "base 30 s" as the first failure's delay. RFC 5626's literal exponent would make the first
    one 60 s.
  - RFC 5626's 50–100 % jitter is omitted: there is one UE per line and no herd to spread, and determinism makes the
    tests exact.
- **`on_registered(addr)`**: sets the active and last-registered P-CSCF, removes that address's mark, and resets its
  count (FR-010).
- **`replace_pool(pool, selection)`**: drops marks and `last_registered` for addresses that are gone (FR-014).

**Rationale**: Startup (`serve_inbound`) and renewal (`on_idle_tick`) must make exactly the same choice, and the
whole rule set is testable with injected clocks. `reg_subscription.rs` is the precedent (090 R1).

## R7. Renewal failover in `on_idle_tick` (FR-011, FR-013, FR-016, clarification Q1)

**Decision**:

- **Before an attempt**: re-read `.pool`. If the set changed, `replace_pool`. Then `pool.select(now)`, and build a
  per-attempt config with `reg_cfg.with_pcscf(addr)`. `ImsRegisterConfig` is `Clone`; only `pcscf_addr` changes.
- **`attempt_renewal` result**: it now returns `RenewalFailure { error, status: Option<u16>, retry_after:
  Option<Duration> }`, reading `Retry-After` from the rejected session's headers before `cleanup()`.
- **On a qualifying failure on the active address**:
  - `pool.on_failure(...)` and `save`.
  - Count the failure on the existing budget: `consecutive_renewal_failures += 1`. If the budget is spent, exit as
    today.
  - If an unmarked candidate remains: set `next_renewal_attempt = None` and `force_renewal = true`, so the next idle
    tick (≤1 s) tries it. **The backoff does not advance.**
  - Otherwise: `schedule_renewal_retry()`, the ladder (FR-013).
- **On a non-qualifying failure**: exactly today's path.
- **During a call**: a failure observed mid-call (090's probe, a NOTIFY-side event) marks at once. The *move* happens
  at the next renewal, which `MaintenancePolicy` already defers until the call ends (FR-016).

**Rationale**: Counting each failed attempt keeps SC-003 within the existing budget
(`MAX_CONSECUTIVE_RENEWAL_FAILURES` = 6). Skipping only the delay gives SC-001: a dead A is replaced by B within one
idle tick of the failure being seen.

## R8. Startup selection in `serve_inbound`

**Decision**: Replace the single `register_session(reg_cfg)` with a loop over `pool.select`.

- **Qualifying failure**: mark the address, save, and try the next unmarked candidate immediately. The pool bounds
  the loop to N attempts.
- **Non-qualifying failure**: return `Err` as today, so the supervisor's `sim_recovery` still sees the same exits.
- **Every candidate marked at startup**: wait until the soonest mark expires, capped at `RETRY_MAX_BACKOFF` (120 s),
  then make a single attempt; on failure, exit as today.
  - The wait sleeps in 1 s slices under a dedicated `watchdog::Phase::PcscfWait`, so the stall watchdog does not
    fire.
  - This is what holds SC-003 across agent restarts. Without it, the supervisor's ~5 s respawn would retry a fully
    marked pool faster than the ladder.

**Rationale**: Clarification Q1 says move immediately. At startup, "immediately" means within the same process.
Exiting and letting the respawn pick the next candidate would also work, but each exit costs the 5 s respawn delay
and counts towards `sim_recovery`'s classification.

## R9. The supervisor's reachability check must consider the pool

**Decision**: `tick_steady_state` takes `current_pool: &[String]` as well as `current_pcscf`.

- The reachability verdict is "the selection answers TCP :5060, or, only if it does not, any other pool member
  does".
- `PcscfUnreachable` (a rebuild after ~90 s of strikes) fires only when no pool member answers.

**Rationale**: Without this, a dead A with a healthy B and tunnel trips `PcscfUnreachable` after
`PCSCF_UNREACHABLE_WINDOW` (90 s) and rebuilds the tunnel. That defeats US1 and breaks SC-001's "without a tunnel
rebuild". The check exists to catch a dead data path (#107, 2026-08-17), and one answering pool member proves the data
path. When the whole pool is dead, the rebuild still happens, and that is the spec's "rediscovery" (US3).

**Alternatives**: Have the agent report its active P-CSCF to the supervisor so that one address is probed. Rejected:
it needs a new channel, and "any member" answers the question the check actually asks.

## R10. Status and logs (FR-019, FR-020)

**Decision**:

- **Status reply** (`RegistrationStatusReply`): three `#[serde(default)]` fields:
  - `pcscf_active: String`;
  - `pcscf_pool: Vec<String>`;
  - `pcscf_unavailable: Vec<(String, u64)>`, each address with its `until` in Unix seconds.
- **`RegistrationStatus`**: gains `pcscf: PcscfSnapshot`, in memory only, like `reg_subscription`. It is synced in
  `sync_shared_status`, plus once at startup before the dispatch loop.
- **CLI**: `vowifi-status` and `volte-status` print two lines from one shared `format_pcscf_lines`; see
  contracts/control.md.
- **Logs**: one `INFO` "P-CSCF selected" per selection, carrying a reason; one "registration through P-CSCF" outcome
  line; and `WARN` "P-CSCF marked unavailable". See contracts/observability.md.
- **PII**: addresses are carrier infrastructure, not subscriber identifiers. No IMPU or IMSI is added to any line.

## R11. VoLTE (FR-006)

**Decision**: `carrier_agent.rs` builds a one-element pool from `line.settings.pcscf`, with state at
`/tmp/volte-pcscf-<card_id>.state`. The same `serve_inbound` and `dispatch_loop` code runs. With one candidate, a
qualifying failure marks it and every attempt takes the ladder: today's behaviour, plus the mark and the status
lines (US3 AS2).

**Rationale**: The spec keeps VoLTE discovery out of scope. Sharing the code path is what FR-021-style parity in 090
already relies on.

## R12. A `503` whose `Retry-After` is 128 s or less (left open by clarification)

**Decision**: No mark. The existing ladder applies unchanged; today `Retry-After` on a REGISTER is ignored.

**Rationale**: FR-008 marks only above Timer F. FR-009 lists "4xx/5xx/6xx without Retry-After" as non-qualifying,
and a short one is the same "come back soon" signal. Honouring a short `Retry-After` as a ladder floor would be a
separate behaviour change with no evidence asking for it.

## R13. Testing approach

- **`PcscfPool`**: table tests with injected `SystemTime`. Persistence round-trips through a real temp dir,
  including corrupt and wrong-version files and pool reconciliation.
- **Classification**: real errors, per R4.
- **Renewal failover**: `attempt_renewal` against two local "P-CSCF" sockets on `127.0.0.1`/`127.0.0.2`:
  - A is closed (refused) or silent (timeout);
  - B answers with a canned `401`, which fails AKA. That is a `NotPcscf` failure, so the test asserts *which address
    was tried*, not a full registration.
- **`LoopState` paths**: the existing `udp_sink_pair` harness, driving `on_qualifying_endpoint_failure` and the Gm
  escalation.
- **Supervisor**: the existing `MockCommandRunner` fixtures for publication points and reachability over the pool.
  Charon and swu log fixtures already exist in `engines.rs` tests.
- **Live**: quickstart Q2 on the Jio Pi blocks the active P-CSCF with an in-netns `iptables` `DROP` and watches
  `vowifi-status`.
