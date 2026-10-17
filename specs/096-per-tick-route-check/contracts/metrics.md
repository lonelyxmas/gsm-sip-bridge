# Contract: tunnel recovery counter

## Metric

| Name | Type | Labels | Meaning |
|---|---|---|---|
| `gsm_sip_bridge_vowifi_tunnel_recoveries_total` | Counter | `module` (line card id), `reason` | Steady-state recoveries the supervisor performed on this strongSwan VoWiFi line, by reason |

`reason` ∈ `process_died`, `vici_broken`, `tun_vanished`, `tun_unavailable`, `child_sa_missing`, `ike_sa_lost`, `child_sa_stuck`, `child_sa_rekey_stuck`, `pcscf_changed` (never emitted), `pcscf_unreachable`, `default_route_missing`.

- A series appears on the line's first recovery for that reason. Absence means zero.
- The counter is monotonic and survives a restart of the supervisor's reporter, because the wire carries deltas.
- It is not emitted for swu or VoLTE lines.
- `StillUp` and `PcscfChanged` outcomes never increment it.

## Wire event (control socket, `AgentReport.events`)

```json
{"event": "tunnel_recovered", "reason": "default_route_missing"}
```

- Sent only by `agent: "supervisor"`, exactly once per steady-state `Recovered{reason}`.
- It travels in a report whose `state` is the line's current tunnel signal (`child_sa_up`, `tunnel_flapping`, `ike_sa_loss_cause`).
- An older daemon that doesn't know `tunnel_recovered` fails to decode the report. This is the same compatibility stance as every earlier `ObservedEvent` addition: the bridge's processes always ship and run together in one image.
