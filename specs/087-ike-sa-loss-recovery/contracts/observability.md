# Contract: observability for specs/087-ike-sa-loss-recovery

The operator-facing surface this feature adds or changes. The log wording is stable, so it is safe to grep. `N` is the line index, `<conn>` the swanctl connection (`imsN`).

## Supervisor log lines (stdout, `[supervise]` prefix)

| When | Line |
|---|---|
| IKE_SA missing on a health check | `line N: <conn> IKE_SA missing; re-initiating` |
| IKE_SA-loss recovery done | `line N: recovered from IkeSaLost (cause: carrier DELETE \| dead-peer timeout \| unknown)` |
| graceful teardown acknowledged | `line N: IKE_SA deleted (acknowledged)` |
| graceful teardown not acknowledged | `line N: ePDG did not acknowledge DELETE within 5s; forced` |
| P-CSCF taken from the new negotiation | `line N: P-CSCF synced from new negotiation: <addr>` |
| no new negotiation within the bound | `line N: no new negotiation within 20s; restarting agent with P-CSCF <addr>` |
| leaked Gm state cleared (only when nonzero) | `line N: cleared K stale Gm SAs / P policies before agent start` |
| flapping latched | `line N: tunnel flapping (K rebuilds within Ws)` |
| flapping unlatched | `line N: tunnel no longer flapping` |

These are unchanged: `recovered from <Reason>` for every other reason, `<conn> CHILD_SA missing; re-initiating`, and `P-CSCF changed (a -> b); refreshing`. After this feature, the last one should no longer follow an agent-restarting recovery on the next tick (SC-002).

## Control protocol (`AgentState`, supervisor reports only)

```json
{ "child_sa_up": true, "tunnel_flapping": false, "ike_sa_loss_cause": "carrier_delete" }
```

- `tunnel_flapping` (bool, optional): true while the line has needed 3 or more IKE_SA rebuilds within `unhealthy_sec` and has not had a full quiet window since.
- `ike_sa_loss_cause` (optional): one of `carrier_delete`, `dead_peer`, `unknown`. The cause of the most recent IKE_SA loss on this line.
- Both are omitted when `None`. Missing on input means "not reported", and the ingest side leaves its state untouched.

## Metrics

No new metrics. `gsm_sip_bridge_vowifi_child_sa_up` reads 0 for the duration of an IKE_SA loss, exactly as for a CHILD_SA loss (FR-014).

## Alerts (`[alerts.tunnel_failure]`, no new keys)

| Trigger | Failure text |
|---|---|
| continuous outage ≥ `unhealthy_sec` | `VoWiFi line's tunnel non-established for over {unhealthy_sec}s (last IKE_SA loss: {cause})` |
| ≥3 IKE_SA rebuilds within `unhealthy_sec` | `VoWiFi line's tunnel rebuilt repeatedly (3+ times within {unhealthy_sec}s; last IKE_SA loss: {cause})` |

- The `(…last IKE_SA loss…)` suffix is omitted when no cause has been reported.
- `{cause}` is `carrier DELETE`, `dead-peer timeout` or `unknown`.
- One Failure per episode. A continuous outage and flapping that overlap share one episode.
- Recovered text is unchanged (`VoWiFi line's tunnel re-established`). It is sent once the CHILD_SA is up and a full `unhealthy_sec` window has passed with no rebuild.
