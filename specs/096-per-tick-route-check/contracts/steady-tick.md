# Contract: one strongSwan steady-state tick

This is the behaviour `tick_steady_state_with_pool` guarantees after this feature. Each row is pinned by a unit test in `line_supervisor.rs`.

| # | State going into the tick | Route check runs? | Outcome | Side effects |
|---|---|---|---|---|
| 1 | process dead | no | `Recovered{ProcessDied}` | restart process |
| 2 | any structural fault (VICI, tun, IKE_SA, CHILD_SA, stuck rekey) | no | that fault's existing outcome | that fault's existing remedy |
| 3 | healthy, and `latest_pcscf` differs from current | no | `PcscfChanged{new}` | none |
| 4 | healthy, current P-CSCF empty | no | `StillUp` (the probe treats empty as reachable) | none |
| 5 | healthy, route missing in the P-CSCF's family, repair succeeds, **probe would pass** | yes | `Recovered{DefaultRouteMissing}` | route reinstated; strikes = 0; **no probe**, no terminate, reinitiate or recreate |
| 6 | healthy, route missing, repair succeeds, probe would fail | yes | `Recovered{DefaultRouteMissing}` | same as 5 |
| 7 | healthy, route present, P-CSCF answers | yes (finds nothing) | `StillUp` | strikes = 0 |
| 8 | healthy, route present, P-CSCF and every sibling silent | yes (finds nothing) | `StillUp`, then `Recovered{PcscfUnreachable}` once the strikes fill the ~90s window | existing rebuild |
| 9 | healthy, route missing, repair fails (the route still isn't there) | yes | falls through to rows 7/8 | repair attempt logged; strike counted if unreachable |

Invariants:

- `repair_default_route` is called **at most once per tick** (FR-006).
- On rows 5 and 6, `pcscf_reachable` is **not** called (the check is ahead of the probe).
- The swu engine's `repair_default_route` always returns `false`, so for swu, rows 5, 6 and 9 collapse into 7 and 8 (FR-008).

Orchestrator (unchanged): `Recovered{DefaultRouteMissing}` kills the line's IMS agent. It is not a renegotiation, and it is not recorded in the flapping window.
