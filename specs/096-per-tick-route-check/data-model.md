# Data Model: per-tick default-route check

## Steady-state tick (unchanged types, new order)

`tick_steady_state_with_pool(engine, runner, current_pcscf, current_pool, tracking, poll_interval, call_active) -> SteadyOutcome`

| Step | Check | Outcome when it fires |
|---|---|---|
| 1 | process alive? | `Recovered{ProcessDied}` |
| 2 | `steady_state_health` | `ViciBroken` / `TunVanished` / `TunUnavailable` / `IkeSaLost` / `ChildSaMissing` / `ChildSaStuck` / `ChildSaRekeyStuck` |
| 3 | `latest_pcscf != current_pcscf` | `PcscfChanged{new}` |
| **4 (new)** | `current_pcscf` non-empty **and** `repair_default_route(current_pcscf)` | `Recovered{DefaultRouteMissing}`; `pcscf_unreachable = 0` |
| 5 | probe the selection, then pool siblings | none; sets `pcscf_via_sibling` and resets strikes on success |
| 6 | strike count `>= pcscf_unreachable_strikes(poll)` | `Recovered{TunUnavailable}` or `Recovered{PcscfUnreachable}` |
| — | otherwise | `StillUp` |

Removed: the `repair_default_route` call in step 6, before the strike increment.

`SteadyTracking` has no new fields.

## TunnelRecoveryReason (new, wire)

Closed enum in `control/protocol.rs`, serde `snake_case`. It maps one-to-one from `DegradeReason`:

| DegradeReason | label |
|---|---|
| ProcessDied | `process_died` |
| ViciBroken | `vici_broken` |
| TunVanished | `tun_vanished` |
| TunUnavailable | `tun_unavailable` |
| ChildSaMissing | `child_sa_missing` |
| IkeSaLost | `ike_sa_lost` |
| ChildSaStuck | `child_sa_stuck` |
| ChildSaRekeyStuck | `child_sa_rekey_stuck` |
| PcscfChanged | `pcscf_changed` (never emitted; for totality) |
| PcscfUnreachable | `pcscf_unreachable` |
| DefaultRouteMissing | `default_route_missing` |

The label count follows `DegradeReason`'s variants. The `From` impl is exhaustive, so a new reason fails to compile until it has a label.

## ObservedEvent::TunnelRecovered (new, wire)

`{ "event": "tunnel_recovered", "reason": "<label>" }`. It is sent only by `AgentKind::Supervisor`, once per steady-state `Recovered{reason}`.

## Metric

`gsm_sip_bridge_vowifi_tunnel_recoveries_total{module, reason}`: a counter. `module` is the line's `card_id`, the same `module_id` the supervisor's `child_sa_up` gauge uses.
