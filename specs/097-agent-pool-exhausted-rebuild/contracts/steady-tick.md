# Contract: where the exhaustion check sits in one steady-state tick

The order is unchanged apart from row 7, which is new:

| # | Check | Outcome |
|---|---|---|
| 1 | Process dead | `Recovered{ProcessDied}` |
| 2 | Structural health (VICI, tun, IKE_SA, CHILD_SA) | the existing reasons |
| 3 | Stuck rekey | `Recovered{ChildSaRekeyStuck}` or deferred |
| 4 | Latest P-CSCF differs | `PcscfChanged` |
| 5 | Route missing and repaired (spec 096) | `Recovered{DefaultRouteMissing}` |
| 6 | (route present or repair failed) | continue |
| **7** | **`exhausted_pool(...)` is `Some` and no call is up** | **recreate the interface. If that fails: `Recovered{TunUnavailable}`. Otherwise terminate, re-initiate, `Recovered{PcscfPoolExhausted}`, strikes reset to 0, and `tracking.pool_exhausted` filled** |
| 7a | `exhausted_pool(...)` is `Some` and a call is up | log the deferral once per episode, then continue to row 8 |
| 8 | Probe, siblings, strikes | unchanged; `Recovered{PcscfUnreachable}` after ~90s |
| 9 | otherwise | `StillUp` |

## Invariants

1. Row 7 runs before the probe, so a probe success cannot suppress it (FR-005).
2. Row 7 rebuilds on the first tick that sees exhaustion (FR-006). There is no strike count.
3. When the pool state input returns `None`, the tick is identical to today's.
4. `tracking.pool_exhausted` is cleared at the start of every tick, so it describes only the tick that set it.
5. `pool_exhausted_deferral_logged` is cleared on any tick where the pool is not exhausted.

## Orchestrator duties

| When | Set `negotiated_at = now` |
|---|---|
| Entering the steady loop | yes |
| After `pkill` in a renegotiated `Recovered` | yes |
| After `pkill` in `PcscfChanged` | yes |
| `DefaultRouteMissing`, `ChildSaMissing`, `TunUnavailable` | no |

The strongSwan loop's `TunnelRecovered` event carries the addresses from `tracking.pool_exhausted` as `pcscfs`. The `Recovered` log line for this reason includes each member's cause, its `until`, and the baseline.
