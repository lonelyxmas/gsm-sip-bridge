# Quickstart: validating 091

## Q0: Automated (CI)

```bash
CARGO_TARGET_DIR=$PWD/target make format && CARGO_TARGET_DIR=$PWD/target make lint \
  && CARGO_TARGET_DIR=$PWD/target make test
```

`CARGO_TARGET_DIR` keeps this worktree's build away from the shared absolute `target-dir` (memory: shared target-dir
clobbered by worktrees).

This covers the following, with injected `SystemTime`s, real temp-dir files and real local sockets:

- **Pool extraction and publication (FR-001–FR-004):**
  - Charon and swu fixtures yield the whole pool.
  - A new negotiation replaces the old batch.
  - `.pool` is written before the selection at all three points.
  - A set change rewrites `.pool` without a `pkill`.
  - A reorder writes nothing and never prints `P-CSCF changed` (SC-002).
- **Supervisor reachability (R9):** a dead selection with a live pool member is not `PcscfUnreachable`. An all-dead
  pool still is.
- **`PcscfPool` (FR-005, FR-006, FR-010–FR-015):**
  - Candidate order, including `last_registered` first and the selection missing from the pool.
  - Sticky selection after a mark expires.
  - Mark durations of 330, 360, 420, … up to 2,100 s, and `Retry-After`.
  - The count reset on success.
  - The soonest-expiring target when every candidate is marked.
  - Stale-address failures ignored (FR-017).
  - Persistence round trip, corrupt and wrong-version files, and reconciliation against a new pool (US3 AS4,
    US4 AS1–AS3).
- **Classification (FR-007–FR-009):**
  - A real `ECONNREFUSED` gives `Transport`.
  - A silent UDP socket gives `Timeout`.
  - `ENETUNREACH` gives `NoRoute`.
  - A peer close gives `PeerClosed`.
  - An AKA error gives `NotPcscf`.
  - A `503` with `Retry-After: 600` marks; one with `Retry-After: 60` and a `403` do not (R12).
- **Failover (US1, US3):**
  - A renewal against a refused or silent `127.0.0.1` "A" goes to `127.0.0.2` "B" on the very next attempt, with no
    backoff step.
  - With both dead, it follows the ladder, targets the soonest-expiring mark, and exits at the existing budget
    (SC-003).
  - Startup tries B in the same process.
  - A fully marked pool at startup waits ≤ 120 s, with the watchdog fed.
- **Failure sources (FR-007, FR-016):**
  - 090's qualifying endpoint failure and the Gm keepalive escalation mark the active P-CSCF.
  - Mid-call, the mark is immediate and the move waits for the call.
- **Status (FR-020):** the reply round trip with and without the new fields, and the CLI lines.

## Q1: Local two-P-CSCF rig (no carrier)

Driven by the integration tests in Q0. To watch it by hand, run one of the failover tests with logs on:

```bash
RUST_LOG=info cargo test -p gsm-sip-bridge pcscf_failover -- --nocapture
```

Expect, in order:

1. `"P-CSCF selected" reason=startup`
2. `"registration through P-CSCF" outcome=failed class=transport`
3. `"P-CSCF marked unavailable"`
4. `"failing over to the next P-CSCF now"`
5. `"P-CSCF selected" reason=failover`

## Q2: Live failover on Jio (blocking: SC-001, SC-004, R9)

Jio advertises four P-CSCFs per negotiation (two IPv4, two IPv6), so it is the carrier that can exercise a real move.
Use the remote Pi (memory: remote Pi Jio deployment; docker-compose v1).

1. Deploy the build. Recreate the container; do not restart it (memory: switching transports).
2. Wait for `registered`. Note the values:
   ```bash
   gsm-sip-bridge vowifi-status | grep -E 'pcscf'
   cat /tmp/pcscf-0 /tmp/pcscf-0.pool
   ```
   Expect `pcscf: A (pool: A, B, …)`, where `.pool` lists all four addresses and `/tmp/pcscf-0` is unchanged from
   today's choice.
3. Black-hole only A, inside the line's netns:
   ```bash
   ip netns exec <ns> iptables -I OUTPUT -d A -j DROP
   ```
4. Within ~2 minutes, the keepalive escalation (or the next renewal) should produce:
   - `"P-CSCF marked unavailable" pcscf=A`;
   - `"failing over to the next P-CSCF now" from=A to=B`;
   - `"registration through P-CSCF" pcscf=B outcome=registered`.
5. `vowifi-status` should show `pcscf: B …` and `pcscf_unavailable: A until <~5.5 min>`.
6. The supervisor log should show `P-CSCF A unreachable; pool member B answers`, and **no** `recovered from
   PcscfUnreachable` (R9).
7. Place an inbound test call to the line (memory: Jio outbound test destination). It must ring.
8. Remove the rule. After A's mark expires, the line **stays on B** (US1 AS4).
9. Restart only the agent: `pkill -f 'vowifi-ims-agent --line 0$'`. It must come back on B (Q3, US4 AS2).

**SC-004 check**: across the whole run, `grep -c 'P-CSCF marked unavailable'` must equal the number of deliberate
blackholes. Any mark logged during a tunnel rebuild is a bug.

## Q3: Restart within a mark window (SC-005)

On the Pi, while A is still blackholed and marked:

1. `pkill -f 'vowifi-ims-agent --line 0$'`.
2. Expect the restarted agent's first `"P-CSCF selected"` to be B, and no REGISTER to A before A's `until`.

## Q4: Steady state on Vodafone (SC-002)

On the local Vodafone rig, run for at least 6 hours with no fault injected. Expect:
- `grep -c 'P-CSCF changed'` = 0, unless the pool genuinely changed;
- `grep -c 'P-CSCF marked unavailable'` = 0;
- every `"P-CSCF selected"` line has `reason=startup` or `sticky`.
