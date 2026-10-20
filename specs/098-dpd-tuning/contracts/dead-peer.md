# Contract: dead-peer classification

## Rule

`G = 165s` (charon's default retransmit schedule). On the tick at `T` that sees the IKE_SA gone with cause `DeadPeer`, using the last structurally healthy tick's snapshot `(S, L)` where `L = S − last_in_age`:

| Condition | Verdict |
|---|---|
| no snapshot | `unknown` |
| `L` is `None` (no listed SA has received anything, e.g. a CHILD_SA rekeyed moments earlier) | `unknown` |
| `L > T − G` | `false` |
| the tunnel interface received ≥ 1 packet in `(S, T]`, and `T − S < G` | `false` |
| `L ≤ S − G`, and the tunnel interface received 0 packets in `(S, T]` | `real` |
| otherwise | `unknown` |

The interface count is the RX packet counter from `ip -s link show <tun>`, read on the snapshot's tick and on the loss tick. If either reading is missing, or the counter went back, the count is unknown and the `real` row cannot apply.

Applied only when the cause is `DeadPeer`. `CarrierDelete`, `Unknown` and every other recovery reason leave `dead_peer` unset.

## Log line (strongSwan loop)

```text
[supervise] line 0: recovered from IkeSaLost (cause: dead-peer timeout)
[supervise] line 0: dead peer verdict: false (inbound ESP 42s before the loss, 1834 packets, snapshot 12s old, 3 received on the tunnel since; charon retransmits for 165s)
```

`unknown` with no snapshot: `dead peer verdict: unknown (no inbound ESP snapshot)`; with no inbound use on any listed SA: `dead peer verdict: unknown (no inbound ESP on any listed CHILD_SA, 0 packets, snapshot 12s old, …; …)`. A missing interface count reads `tunnel counter unread`.

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
