# Contract: observability additions

Extends the agent→daemon protocol (specs/014-vowifi-metrics-restore,
`contracts/observability-protocol.md`) and the `/metrics` surface.

## 1. Protocol (`control/protocol.rs`)

### `AgentKind`

```diff
 #[serde(rename_all = "snake_case")]
 pub enum AgentKind {
     Ims, Sip, Volte, VolteSip,
+    /// The `supervise` process, reporting per-line tunnel state it alone can
+    /// observe (specs/086). Reports `child_sa_up` and nothing else.
+    Supervisor,
 }
```

Wire value: `"supervisor"`. `as_str()` → `"supervisor"`.

### `AgentState`

```diff
+    /// specs/086: whether the supervisor's last health check confirmed a usable
+    /// CHILD_SA for this line. `None` = not reported (every non-supervisor agent).
+    #[serde(skip_serializing_if = "Option::is_none", default)]
+    pub child_sa_up: Option<bool>,
```

Example supervisor report:

```json
{"agent":"supervisor","module_id":"a1b2c3","epoch":1760270000123,"seq":7,
 "state":{"child_sa_up":false},"events":[],"dropped":0}
```

### Compatibility

- A report without `child_sa_up` parses exactly as before (`default`).
- A report with `child_sa_up` parses in a reader that predates it (unknown fields are ignored, as for
  every other optional field).
- `agent: "supervisor"` is rejected by a daemon that predates this feature. Not a supported
  pairing: the daemon and the supervisor are the same binary in one image.

## 2. Metrics

### New gauge

```
# HELP gsm_sip_bridge_vowifi_child_sa_up 1 if the supervisor's last check found a usable IPsec CHILD_SA for this VoWiFi line, 0 otherwise
# TYPE gsm_sip_bridge_vowifi_child_sa_up gauge
gsm_sip_bridge_vowifi_child_sa_up{module="a1b2c3"} 1
```

| Situation | Value |
|---|---|
| Supervision started, tunnel still establishing | 0 |
| Tunnel up, CHILD_SA INSTALLED / REKEYING / REKEYED / UPDATING | 1 |
| CHILD_SA missing, interface gone, vici or charon down | 0 |
| Supervisor reports stale (>30s) | 0 (zeroed at scrape) |
| swu engine line, VoLTE line | series absent |

### Unchanged

`gsm_sip_bridge_vowifi_tunnel_up` and `gsm_sip_bridge_vowifi_registered` keep their meaning:
they are reported by the IMS agent and track registration liveness.

### Side effect

`gsm_sip_bridge_agent_up{agent="supervisor",module=…}` and
`gsm_sip_bridge_agent_last_report_seconds{agent="supervisor",…}` appear for each strongSwan line.

## 3. Alerts (`[alerts.tunnel_failure]`)

- Same config keys (`enabled`, `unhealthy_sec`), and no new ones.
- For a VoWiFi line with a supervisor reporting, the alert fires once when **either** the agent's
  tunnel signal or the CHILD_SA signal has been unhealthy continuously for `unhealthy_sec`. It
  recovers once when **both** are healthy again.
- One outage seen by both signals produces exactly one failure notice and one recovery notice.
- The description names the line, not the reporter: e.g. `VoWiFi line's tunnel non-established for over 300s`.
- The config comment in `config.toml.example` and `docs/observability.md` say the alert also
  covers a missing CHILD_SA.

## 4. Supervisor log lines

Existing format, new reason value. No new log line is added: the engine already logs every miss,
and the loop already logs every recovery with its reason. A rebuild therefore appears as the
second consecutive pair below.

```
[supervise] line 0: ims0 CHILD_SA missing; re-initiating
[supervise] line 0: recovered from ChildSaMissing
[supervise] line 0: ims0 CHILD_SA missing; re-initiating
[supervise] line 0: recovered from ChildSaStuck
```

The engine's message is printed on each miss, before the tick decides between re-initiate and
rebuild. `recovered from ChildSaStuck` is the authoritative record that the IKE_SA was rebuilt.
