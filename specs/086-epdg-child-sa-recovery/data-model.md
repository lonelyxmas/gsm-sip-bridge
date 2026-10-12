# Data Model: ePDG CHILD_SA health detection and bounded recovery

Everything here is in-memory process state or wire/metric shape. Nothing is persisted.

## SteadyTracking (supervisor, per line, loop-owned)

Replaces the bare `unreachable_streak: u32` threaded through `tick_steady_state`.

| Field | Type | Meaning | Reset |
|---|---|---|---|
| `pcscf_unreachable` | `u32` | Consecutive ticks the P-CSCF was unreachable (existing behaviour, renamed) | As today |
| `child_sa_missing` | `u32` | Consecutive ticks the health check returned `ChildSaMissing` | Any other health result; on escalation |
| `child_sa_usable` | `bool` | Whether this tick positively confirmed a usable CHILD_SA (`SteadyStateHealth::Ok`) | Overwritten every tick |

`Default` gives all zero/false. The strongSwan loop sets `child_sa_usable = true` at "tunnel UP"
before entering the loop.

### CHILD_SA miss state machine (per line, one step per 30s tick)

```
            health != ChildSaMissing
   ┌──────────────────────────────────────────┐
   ▼                                          │
[0 misses] ──ChildSaMissing──▶ [1 miss] ──────┤
   ▲                re-initiate      │        │
   │                (ChildSaMissing) │ChildSaMissing
   │                                 ▼
   └──────── reset ◀── terminate --force + re-initiate
                        + agent restart (ChildSaStuck)
```

No backoff: while the CHILD_SA stays missing, the cycle repeats every two ticks (FR-006a).

## SteadyStateHealth (unchanged variants, sharpened meaning)

`ChildSaMissing` now means: the line's IKE_SA may be up, but no CHILD_SA named for this
connection is in INSTALLED / REKEYING / REKEYED / UPDATING.

## DegradeReason (+1 variant)

| Variant | New? | Restarts IMS agent | Meaning |
|---|---|---|---|
| `ChildSaMissing` | no | no | First miss; re-initiated inside the existing IKE_SA |
| `ChildSaStuck` | **yes** | **yes** | Second consecutive miss; IKE_SA rebuilt (#107) |

Logged by the existing `[supervise] line N: recovered from <reason>` line.

## Wire: AgentKind (+1 variant)

| Variant | serde | Reports |
|---|---|---|
| `Supervisor` | `"supervisor"` | `child_sa_up` only; never calls, registration or Gm |

## Wire: AgentState (+1 field)

| Field | Type | serde | Meaning |
|---|---|---|---|
| `child_sa_up` | `Option<bool>` | `skip_serializing_if = "Option::is_none", default` | `Some(true)`: usable CHILD_SA confirmed by the last check. `Some(false)`: not confirmed (missing, establishing, interface gone, vici/charon down). `None`: not reported (all non-supervisor agents, swu) |

## Daemon: AgentRecord (+1 field)

| Field | Type | Meaning |
|---|---|---|
| `tunnel_unhealthy_since` | existing | For a Supervisor record: when `child_sa_up` last turned false |
| `tunnel_alert_generation` | **new** `Option<Instant>` | The effective (combined) `since` that the record's TunnelFailure decision was last taken against. `record_alert_outcome` matches TunnelFailure deliveries against this instead of the raw `tunnel_unhealthy_since`. Only set on records that own the line's tunnel decision |

### Tunnel-failure decision ownership (per module M)

| Supervisor record for M exists? | Owner of TunnelFailure decision | Input |
|---|---|---|
| no | each record, as today | its own `tunnel_unhealthy_since` |
| yes | the Supervisor record only | `min(Ims.tunnel_unhealthy_since, Supervisor.tunnel_unhealthy_since)` over the `Some` values; `None` only when both are healthy |

## Metric: `gsm_sip_bridge_vowifi_child_sa_up`

| Label | Value |
|---|---|
| `module` | line's `card_id` (same as the other `vowifi_*` gauges) |

- 1 or 0, from the last `child_sa_up` received from `AgentKind::Supervisor` for that module.
- Set to 0 on scrape when that Supervisor record is stale (more than 3× `[metrics].agent_report_interval_seconds` since its last report, 45s by default).
- Absent for modules no supervisor reports on (swu lines, VoLTE).
