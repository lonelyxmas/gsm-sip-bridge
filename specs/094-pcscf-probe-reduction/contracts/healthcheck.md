# Contract: `gsm-sip-bridge healthcheck`

Invoked by the Docker `HEALTHCHECK` (`--interval=30s --timeout=5s --start-period=180s --retries=3`, unchanged). Exit 0 means healthy, anything else unhealthy. Faults go to stderr, one per line.

## Network effects

| Target | Before | After |
|---|---|---|
| `127.0.0.1:<metrics.port>/metrics` (HTTP GET, 3s) | yes | yes, unchanged |
| `ip addr show <tun iface>` inside each line's netns | yes | yes, unchanged |
| P-CSCF `:5060` TCP connect inside each line's netns | yes | **never** |
| Agent A `veth_local_addr:5071`, one `StatusQuery` (1s connect, 1s read) | no | yes, per line with a non-empty P-CSCF file, all lines concurrently |

## Per-line verdicts (VoWiFi enabled, metrics reachable)

| Line state | Verdict | stderr |
|---|---|---|
| Tunnel iface has no address | unhealthy | `line N: tunnel interface <iface> has no address` (unchanged) |
| No P-CSCF captured yet | healthy | — |
| Agent refuses, times out, closes, or sends garbage | unhealthy | `line N: IMS agent at <ip>:5071 not responding: <reason>` |
| Agent replies with a non-status message | unhealthy | `line N: IMS agent at <ip>:5071 not responding: unexpected reply: <variant>` |
| `gm_connection = "failed since <ts>"` | unhealthy | `line N: IMS agent reports Gm connection failed since <ts> (P-CSCF <addr>)`; the parenthesis is left out when the reply has no active P-CSCF |
| `gm_connection = "reconnecting since … (attempt N)"` | healthy | — |
| `gm_connection = "up"` | healthy | — |
| `gm_connection = ""` (older agent) | healthy | — |
| In a call (`can_answer = false`, `blocked_reason = "busy…"`) | healthy | — |
| PBX leg down, or any registration `state` | healthy for this check | — (lapsed registration still faults through `/metrics`, unchanged) |

All other verdicts (metrics endpoint down, configured line not running, `RegistrationExpired`, VoWiFi or CS disabled, zero resolved lines) are unchanged in wording and precedence.

## Timing

The worst case with every agent hung is about 3s for `/metrics`, if it is also slow, plus about 2s for the concurrent agent phase. That stays under the 5s Docker timeout for any line count up to `max_lines`. In the normal case both phases take milliseconds.
