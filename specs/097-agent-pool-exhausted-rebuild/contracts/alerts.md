# Contract: `[alerts.pcscf_pool_exhausted]`

```toml
[alerts.pcscf_pool_exhausted]
enabled = true            # default true: a line stuck on a dead pool is an incident
unhealthy_sec = 300       # 30..=3600, default 300
# webhook_url = "..."     # optional override of [alerts].webhook_url
```

This is a `RawUnhealthyCategory`, like `gm_connection_lost`. It has the same six config touch points as spec 028's contracts/config.md:
- raw field
- known-keys entry
- `category(raw, true)`
- the thresholds block with `30..=3600`
- the `AlertsConfig` fields
- `AlertCategory::PcscfPoolExhausted` with `as_str`, `category_config` and the active-incident gauge set

## Episode

- **Start**: the first `TunnelRecovered{reason: pcscf_pool_exhausted}` from a line's supervisor.
- **Continue**: each further such event increments the rebuild count, replaces the address list, and marks the IMS agent's current reporter epoch as stale.
- **End**: an IMS agent report for the same module with `registered = true`, from a reporter epoch other than the stale one.

## Messages

- **Failure**, once per episode after `unhealthy_sec`:
  `VoWiFi line's P-CSCF pool exhausted for over 300s (10.0.0.11; 4 tunnel rebuilds): the carrier has not handed out a working P-CSCF`
- **Recovered**, once, after an alerted episode ends:
  `VoWiFi line registered again after its P-CSCF pool was exhausted`

Phone number and instance come from the existing identity map (spec 034).

## Metrics

- `gsm_sip_bridge_critical_alerts_total{category="pcscf_pool_exhausted", outcome}`
- `gsm_sip_bridge_critical_event_active{category="pcscf_pool_exhausted", module}`
- `gsm_sip_bridge_vowifi_tunnel_recoveries_total{reason="pcscf_pool_exhausted", module}`

## Wire

```json
{"event":"tunnel_recovered","reason":"pcscf_pool_exhausted","pcscfs":["10.0.0.11"]}
```

Every other reason serializes as before, with no `pcscfs` key.
