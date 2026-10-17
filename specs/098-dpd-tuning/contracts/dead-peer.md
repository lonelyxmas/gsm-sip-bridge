# Contract: dead-peer classification

## Rule

`G = 165s` (charon's default retransmit schedule). On the tick at `T` that sees the IKE_SA gone with cause `DeadPeer`, using the last structurally healthy tick's snapshot `(S, L)` where `L = S − last_in_age`:

| Condition | Verdict |
|---|---|
| no snapshot | `unknown` |
| `L` is `None` (nothing ever received on the listed SAs) | `real` |
| `L > T − G` | `false` |
| `L ≤ S − G` | `real` |
| otherwise | `unknown` |

Applied only when the cause is `DeadPeer`. `CarrierDelete`, `Unknown` and every other recovery reason leave `dead_peer` unset.

## Log line (strongSwan loop)

```text
[supervise] line 0: recovered from IkeSaLost (cause: dead_peer)
[supervise] line 0: dead peer verdict: false (inbound ESP 42s before the loss, 1834 packets, snapshot 12s old; charon retransmits for 165s)
```

`unknown` with no snapshot: `dead peer verdict: unknown (no inbound ESP snapshot)`.

## Wire

`ObservedEvent::TunnelRecovered` gains `dead_peer`, absent when `None`:

```json
{"event":"tunnel_recovered","reason":"ike_sa_lost","dead_peer":"false"}
```

Older daemons ignore the field; older supervisors never send it.

## Metric

```text
# HELP gsm_sip_bridge_vowifi_dead_peer_losses_total IKE_SA losses to an unresponsive ePDG, by whether inbound ESP was still arriving while charon retransmitted (false), was not (real), or could not be told (unknown).
# TYPE gsm_sip_bridge_vowifi_dead_peer_losses_total counter
gsm_sip_bridge_vowifi_dead_peer_losses_total{module="0",verdict="false"} 1
```

Counted per event, so a supervisor restart cannot rewind it. No alert.
