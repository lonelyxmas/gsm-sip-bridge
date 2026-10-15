# Data model: stuck CHILD_SA rekey rebuild (093)

## ChildSaLifetime (new, `supervise::line_supervisor`)

What one steady-state tick saw of the line's newest usable CHILD_SA.

| Field | Type | Meaning |
|---|---|---|
| `unique_id` | `u32` | The `#N` on the CHILD_SA header line (vici `uniqueid`). It changes whenever the CHILD_SA is replaced: by a rekey, a re-initiate, or a rebuild. |
| `expires_in` | `Option<i64>` | Seconds until the hard expiry, from `expires in Ns`. `None` when the field is absent or unparseable. |

Carried as `SteadyStateHealth::Ok(Option<ChildSaLifetime>)`. `None` there means the engine reports no lifetimes (SWu), so nothing is decided.

**Selection rule**: among the connection's CHILD_SA headers whose state is in `USABLE_CHILD_SA_STATES`, take the highest `unique_id`.

## SteadyTracking (extended)

| New field | Type | Set when | Cleared when |
|---|---|---|---|
| `rekey_stuck_rebuilt` | `Option<u32>` | A planned rebuild is issued for CHILD_SA `#N` | The newest usable `#N` differs, there is no usable CHILD_SA, or charon died |
| `rekey_stuck_deferral_logged` | `Option<u32>` | The deferral for `#N` is logged | Same |
| `lifetime_missing_logged` | `Option<u32>` | The missing lifetime for `#N` is logged | Same |

## DegradeReason::ChildSaRekeyStuck (new)

- `recovery_restarts_agent` → true. `recovery_renegotiates` → true. Both come from the existing rules; neither function changes.
- Logged by the orchestrator as `recovered from ChildSaRekeyStuck`, after the tick's own detailed line (contracts/logs.md).

## Decision table (one tick, structural checks passed)

Let `L` be the observation, `T = 300s`, `F = 90s`, and `call` be the call-state read.

| Condition | Outcome |
|---|---|
| `L` is `None` (engine reports none) | No decision |
| `L.expires_in` is `None` | Log once per `#N` (FR-011), no rebuild |
| `L.expires_in > T` | No rebuild |
| `≤ T`, `rekey_stuck_rebuilt == Some(#N)` | No rebuild (FR-006) |
| `≤ T`, `call` active, `expires_in > F` | Defer and log once per `#N` |
| `≤ T`, otherwise | Planned rebuild: log, `terminate`, `reinitiate`, set `rekey_stuck_rebuilt`, return `Recovered { ChildSaRekeyStuck }` |

## Call-state file (new, `ims::pcscf_files`)

`/tmp/pcscf-N.calls` beside `/tmp/pcscf-N`: the decimal active-call count, then a newline. contracts/files.md has the details.
