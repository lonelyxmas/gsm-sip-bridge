# Data Model: One P-CSCF probe per line

No persisted data changes. These are the in-memory and config shapes this feature adds or changes.

## `[vowifi]` configuration

| Field | Type | Default | Validation | Notes |
|---|---|---|---|---|
| `idle_keepalive` | bool | `false` | serde bool; unknown keys are rejected by strict parsing | **New.** Per deployment only, not on `[[vowifi.line]]`. The only switch for the keepalive loop |
| `keepalive_interval_sec` | u64 | `30` (was `20`) | `1..=3600`, always checked | Used only when `idle_keepalive = true` |

Derived: `idle_keepalive_interval(&VowifiConfig) -> Option<Duration>` returns `Some(Duration::from_secs(keepalive_interval_sec))` when `idle_keepalive`, else `None`.

## `AgentStatusProbe` (`supervise/runner.rs`)

The outcome of `CommandRunner::agent_status(addr)`.

| Variant | Meaning |
|---|---|
| `Reply(ControlMessage)` | One message was read before the timeouts. It may be any variant; the classifier decides |
| `NoAnswer(String)` | Bad address, connect refused or timed out (1s), read timed out (1s), EOF, or unparsable JSON. The string is the error text, for the fault message |

## `LineFault` (`commands/healthcheck.rs`)

| Variant | Status | Fields | Display |
|---|---|---|---|
| `TunnelInterfaceHasNoAddress` | unchanged | `iface` | unchanged |
| `PcscfUnreachable` | **removed** | — | — |
| `AgentNotResponding` | **new** | `addr: String` (`ip:5071`), `reason: String` | `IMS agent at {addr} not responding: {reason}` |
| `GmConnectionFailed` | **new** | `gm_connection: String` (as rendered by the agent), `pcscf: Option<String>` (`PcscfStatusReply.active`, when non-empty) | `IMS agent reports Gm connection {gm_connection}` plus ` (P-CSCF {pcscf})` when known |
| `RegistrationExpired` | unchanged | `module`, `ago_seconds` | unchanged |

## Classification: `classify_agent_reply(addr, AgentStatusProbe) -> Option<LineFault>`

| Input | Result |
|---|---|
| `NoAnswer(r)` | `AgentNotResponding { addr, reason: r }` |
| `Reply(m)` where `m` is not `RegistrationStatusReply` | `AgentNotResponding { addr, reason: "unexpected reply: <variant>" }` |
| `RegistrationStatusReply { gm_connection, .. }` with `gm_connection.starts_with("failed")` | `GmConnectionFailed { gm_connection, pcscf }` |
| `RegistrationStatusReply` with any other `gm_connection` (`up`, `reconnecting …`, `""`), whatever `state`, `can_answer`, `blocked_reason` | `None` |

## `check_line` order (per line, unchanged except step 3)

1. The tunnel interface has no address → `TunnelInterfaceHasNoAddress`. Stop.
2. The P-CSCF file is missing or blank → `None`. Stop.
3. **New:** `classify_agent_reply(addr, runner.agent_status(addr))`. **Was:** `tcp_connect_ok_in_netns(netns, pcscf, 5060)` → `PcscfUnreachable`.

`RegistrationExpired` from `/metrics`, configured-line-not-found, metrics-endpoint-down and the VoWiFi/CS-disabled shortcuts stay in `evaluate_with_metrics`, in their current order.
