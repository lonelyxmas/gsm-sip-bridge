# Contract: `[vowifi].dpd_delay_sec`

```toml
[vowifi]
# Seconds without anything received from the ePDG before charon sends a DPD
# request. 0 turns charon-initiated DPD off. Default 120 (the Android IWLAN
# default). No effect while the line supervisor's P-CSCF probe runs.
dpd_delay_sec = 120
```

| Input | Result |
|---|---|
| absent | `120`; rendered `dpd_delay = 120s` |
| `0` | rendered `dpd_delay = 0s` (charon never initiates DPD; still answers the ePDG's) |
| `10` … `3600` | rendered as given |
| `1` … `9`, `3601`+, negative, non-integer | config error: `vowifi.dpd_delay_sec must be 0 or between 10 and 3600 (got …)` |
| under `[[vowifi.line]]` | config error: unknown key |

Shell export: `DPD_DELAY_SEC=<n>`.

Startup, once:

- strongSwan: `[supervise] strongSwan DPD: dpd_delay = <n>s ([vowifi].dpd_delay_sec)` or `… DPD off ([vowifi].dpd_delay_sec = 0)`
- swu: `[supervise] [vowifi].dpd_delay_sec ignored: the swu engine has no DPD setting`

The rendered connection differs from before only in the `dpd_delay` line.
