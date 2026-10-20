# Data model (098)

## `VowifiConfig.dpd_delay_sec: u64`

- Default `120`. Accepted: `0`, or `10..=3600`. Anything else fails config loading with an error naming `vowifi.dpd_delay_sec` and the accepted range.
- Deployment-wide; unknown under `[[vowifi.line]]`.
- Rendered as `dpd_delay = {n}s`. Applies on container recreate.

## `InboundEsp` (supervisor, per tick)

| Field | Type | Meaning |
|---|---|---|
| `last_in_age` | `Option<Duration>` | Youngest `Ns ago` across this connection's listed CHILD_SAs' inbound lines; `None` if none has received anything |
| `packets` | `u64` | Sum of inbound packets across those CHILD_SAs (for the log only) |
| `tun_rx_packets` | `Option<u64>` | The tunnel interface's RX packet counter on the same tick (`ip -s link show`); `None` if unread |

Produced by `engines::inbound_esp(sas_output, conn_name) -> Option<InboundEsp>`; `None` when no CHILD_SA of the connection is listed. The engine adds `tun_rx_packets` from the same tick's interface check, which also reads the counter on the loss tick (`TunnelEngine::tun_rx_packets`).

## `SteadyTracking` additions

| Field | Type | Set | Cleared |
|---|---|---|---|
| `inbound` | `Option<(Instant, InboundEsp)>` | every tick whose health is `Ok` and the engine reported a snapshot | every other tick (process died, any structural fault, `IkeSaMissing`) |
| `dead_peer` | `Option<DeadPeerClassification>` | the tick that returns `Recovered{IkeSaLost}` with cause `DeadPeer` | start of every tick |

## `DeadPeerClassification`

| Field | Type |
|---|---|
| `verdict` | `DeadPeerVerdict` (`False`, `Real`, `Unknown`) |
| `snapshot_age` | `Option<Duration>` — `T − S` |
| `last_in_before_loss` | `Option<Duration>` — `T − L` |
| `packets` | `Option<u64>` |
| `rx_since_snapshot` | `Option<u64>` — tunnel RX packets in `(S, T]`; `None` if either reading is missing or the counter went back |

## Wire: `DeadPeerVerdict`

`control::protocol::DeadPeerVerdict`, re-exported by `line_supervisor`. `snake_case`: `"false"`, `"real"`, `"unknown"`. Closed enum; `as_str` gives the metric label.

## State transitions

```text
tick Ok ──────────────► inbound = (now, snapshot)
tick IkeSaMissing ────► cause = engine.ike_sa_loss_cause()
   cause == DeadPeer ─► dead_peer = classify_dead_peer(inbound, now, engine.tun_rx_packets())
   otherwise ─────────► dead_peer = None
any tick start ───────► dead_peer = None
```
