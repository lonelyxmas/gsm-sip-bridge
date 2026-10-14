# Contract: observability (logs)

Field names and messages are stable: operators grep for them, and SC-001, SC-002, SC-004 and SC-005 are measured with
them. No line adds a subscriber identifier. P-CSCF addresses are carrier infrastructure.

## Agent

| Message | Level | Fields | When |
|---|---|---|---|
| `"P-CSCF selected"` | INFO | `pcscf`, `reason` (`startup` / `sticky` / `failover` / `pool-replaced` / `all-unavailable`), `cause` (with `failover`: the cause that marked the previous P-CSCF), `previous`, `candidates` | Before every registration attempt whose target differs from the previous attempt's, and always at startup. A `sticky` renewal to the same address does not repeat the line. |
| `"registration through P-CSCF"` | INFO / WARN | `pcscf`, `outcome` (`registered` / `failed`), `class` (with `failed`: `timeout` / `transport` / `no-route` / `peer-closed` / `not-pcscf` / `rejected`), `status` (when a final response arrived) | After every registration attempt (FR-019 "outcome"). WARN when it fails. |
| `"P-CSCF marked unavailable"` | WARN | `pcscf`, `cause`, `failures`, `until` (RFC 3339), `unavailable_secs`, `remaining` (eligible candidates left) | `on_failure` applied a mark |
| `"failure not attributed to the P-CSCF; not marking"` | INFO | `pcscf`, `cause`, `why` (`no-route` / `not-active`) | A qualifying-shaped failure was found while the tunnel was down (route probe), or for an address that is no longer active (FR-017) |
| `"failing over to the next P-CSCF now"` | WARN | `from`, `to`, `cause` | R7: an unmarked candidate remains, so the backoff is skipped |
| `"every P-CSCF is unavailable; retrying on backoff"` | WARN | `pcscf` (the soonest-expiring mark), `retry_in_secs`, `pool` | R7 / FR-013 |
| `"every P-CSCF is unavailable; waiting before the startup attempt"` | WARN | `pcscf`, `wait_secs` | R8 |
| `"P-CSCF pool replaced"` | INFO | `pool`, `dropped_marks` | `replace_pool` changed the set |
| `"P-CSCF state unreadable; starting clean"` | WARN | `path`, `error` | `.state` was corrupt or the wrong version |
| `"could not save P-CSCF state"` | WARN | `path`, `error` | `.state` write failed |

The existing `"qualifying endpoint failure: re-registering"` line (spec 090) is unchanged. When it marks a P-CSCF, it
is followed by `"P-CSCF marked unavailable"`.

## Supervisor (stdout, `[supervise] line N:` prefix as today)

| Text | When |
|---|---|
| `P-CSCF pool: 10.0.0.10, 10.0.1.10` | Each time `.pool` is written at establishment, a P-CSCF change, or the post-recovery sync, printed after the existing `tunnel UP. P-CSCF: …` / `P-CSCF synced …` / `P-CSCF changed …` line |
| `P-CSCF pool changed (set): 10.0.0.10, 10.0.1.10, 10.0.2.10; not restarting` | A steady-state set change that left the selection unchanged (US2 AS3) |
| `P-CSCF 10.0.0.10 unreachable; pool member 10.0.1.10 answers` | The first tick on which the selection fails reachability but another member answers (R9). Printed once per episode, not every tick. |

The existing `P-CSCF changed (A -> B); refreshing` line is unchanged. A reorder must still never print it (SC-002).
