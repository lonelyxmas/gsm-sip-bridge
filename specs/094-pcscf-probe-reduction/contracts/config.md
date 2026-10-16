# Contract: configuration

## `[vowifi]`

```toml
[vowifi]
# Idle-tunnel keepalive: a TCP connect to the P-CSCF's SIP port every
# keepalive_interval_sec, result discarded. Off by default since #120: the
# supervisor already probes the P-CSCF every 30s, and the extra traffic keeps
# strongSwan's DPD and NAT-T keepalives from running. Turn on only if the
# carrier drops an idle bearer (e.g. #106).
# idle_keepalive = false
# keepalive_interval_sec = 30
```

| Key | Accepts | Absent | Invalid |
|---|---|---|---|
| `idle_keepalive` | `true` / `false` | `false` | non-boolean: config load fails with an error naming `vowifi.idle_keepalive` |
| `keepalive_interval_sec` | integer `1..=3600` | `30` | out of range: config load fails naming `vowifi.keepalive_interval_sec` (unchanged), **even when `idle_keepalive = false`** |

- `idle_keepalive` under `[[vowifi.line]]` is an unknown key and is rejected by strict parsing.
- Setting `keepalive_interval_sec` never turns the keepalive on by itself.

## `gsm-sip-bridge config` shell export

One new line, after `KEEPALIVE_INTERVAL`:

```text
KEEPALIVE_INTERVAL=30
IDLE_KEEPALIVE=false
```

`IDLE_KEEPALIVE` is `true` or `false`.
