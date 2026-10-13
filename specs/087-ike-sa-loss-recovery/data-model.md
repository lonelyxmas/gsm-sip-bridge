# Data Model: specs/087-ike-sa-loss-recovery

In-memory types only; nothing is persisted. Names are the intended Rust identifiers.

## Supervisor (`supervise/line_supervisor.rs`)

### `SteadyStateHealth` (enum, extended)

| Variant | Meaning | Source |
|---|---|---|
| `Ok` | usable CHILD_SA | unchanged |
| `ViciBroken` | `--list-sas` failed or timed out | unchanged |
| `TunVanished` | XFRM interface missing from netns | unchanged |
| **`IkeSaMissing`** | no IKE_SA header for this connection | **new** (R1) |
| `ChildSaMissing` | IKE_SA listed (any state, incl. `CONNECTING`), no usable CHILD_SA | narrowed meaning (R1) |

### `DegradeReason` (enum, extended)

- **`IkeSaLost`** (new). The agent restarts: `recovery_restarts_agent` returns true. Counts as an IKE_SA rebuild for `RebuildWindow`.
- `ChildSaMissing`: doc narrowed to "lost under a live IKE_SA". No agent restart, no rebuild count.
- Every other variant is unchanged. An IKE_SA rebuild is defined as `recovery_restarts_agent(reason) == true`.

### `IkeSaLossCause` (enum, new; shared with `control/protocol.rs`)

`CarrierDelete | DeadPeer | Unknown`. Serde `snake_case` (`carrier_delete`, `dead_peer`, `unknown`). Display: `carrier DELETE`, `dead-peer timeout`, `unknown`. Derived by `ike_sa_loss_cause` (R2). Used for logs and alert text only, never for decisions.

### `SteadyOutcome::Recovered`

Gains the cause for `IkeSaLost` without widening every match. Preferred shape: `Recovered { reason: DegradeReason, ike_sa_loss_cause: Option<IkeSaLossCause> }`, which is `Some` only for `IkeSaLost`. Alternative if it reads better in code: `DegradeReason::IkeSaLost(IkeSaLossCause)`. Either is fine, provided `DegradeReason` stays `Copy`.

### `SteadyTracking` (unchanged fields)

`child_sa_missing` resets on `IkeSaMissing`, through the existing `health != ChildSaMissing` reset. The doc for `child_sa_missing` changes to "consecutive ticks with an IKE_SA listed but no usable CHILD_SA".

### `RebuildWindow` (struct, new; per line, owned by the orchestrate steady loop)

| Field | Type | Notes |
|---|---|---|
| `window` | `Duration` | `config.alerts.tunnel_failure_thresholds.unhealthy_sec` |
| `threshold` | `usize` | `FLAP_REBUILDS = 3` |
| `rebuilds` | `VecDeque<Instant>` | pruned to `window` on every call |
| `latched` | `bool` | the reported `tunnel_flapping` |

The methods take `now` for testability, and return whether `latched` changed:

- `record(now) -> bool`: push `now`, prune, and latch if `len >= threshold`.
- `tick(now) -> bool`: prune, and unlatch if latched and the newest entry is at least `window` old.

| From | Event | To |
|---|---|---|
| idle (`len` < 3, not latched) | `record`, with `len` reaching 3 | latched |
| latched | `record` | latched (newest entry refreshed) |
| latched | `tick` with newest entry at least `window` old | idle |

## Engine (`supervise/engines.rs`)

| Item | Kind | Notes |
|---|---|---|
| `list_sas_has_ike_sa(output, conn) -> bool` | pure fn | column-0 `"{conn}: #"` (R1) |
| `ike_sa_loss_cause(log, conn) -> IkeSaLossCause` | pure fn | R2 |
| `latest_pcscf_batch` | existing fn | additionally returns whether the batch is closed |
| `extract_pcscf_after(log, conn, mark) -> Option<String>` | pure fn | closed batch in `log[mark..]`; whole log if `mark > len` (R4) |
| `StrongswanEngine::log_mark(&self, runner) -> usize` | method | charon log length; 0 if unreadable |
| `StrongswanEngine::fresh_pcscf_since(&self, runner, mark) -> Option<String>` | method | wraps the read |
| `StrongswanEngine::ike_sa_loss_cause(&self, runner) -> IkeSaLossCause` | method | wraps the read; `Unknown` if unreadable |
| `StrongswanEngine::terminate` | changed | graceful → presence check → forced (R3) |

`log_mark`, `fresh_pcscf_since` and `ike_sa_loss_cause` are inherent methods on `StrongswanEngine`, not `TunnelEngine` trait methods. Only the strongswan loop uses them, and the SWu engine stays untouched.

### Constants

| Name | Value | Where |
|---|---|---|
| `GRACEFUL_TERMINATE_WAIT_SECS` | 5 | swanctl `--timeout` |
| `GRACEFUL_TERMINATE_TIMEOUT_SECS` | 8 | outer `timeout` |
| `STEADY_LIST_SAS_TIMEOUT_SECS` | 10 (existing) | presence check |
| `STEADY_TERMINATE_TIMEOUT_SECS` | 10 (existing) | forced delete |
| `POST_RECOVERY_PCSCF_WAIT` | 20s | orchestrate |
| `POST_RECOVERY_PCSCF_POLL` | 1s | orchestrate |
| `FLAP_REBUILDS` | 3 | line_supervisor |

## Orchestrate (`supervise/orchestrate.rs`, strongswan path)

| Item | Notes |
|---|---|
| `mark` | taken before each `tick_steady_state` |
| `rebuild_in_progress: Arc<AtomicBool>` | per line; set and cleared by the steady loop around the P-CSCF wait and `pkill`; the agent loop waits on it before spawning (bounded at 25s, with shutdown checked) (R5) |
| `rebuilds: RebuildWindow` | per line; `record` on agent-restarting recoveries, `tick` each loop |
| Gm flush step | agent loop, before spawn, strongswan only (R6) |

## Wire (`control/protocol.rs`, `AgentState`)

| Field | Type | Sent by | Semantics |
|---|---|---|---|
| `tunnel_flapping` | `Option<bool>` | supervisor | `RebuildWindow::latched`; `None` = not reported |
| `ike_sa_loss_cause` | `Option<IkeSaLossCause>` | supervisor | last IKE_SA loss cause on this line; `None` = none yet |

Both are `#[serde(skip_serializing_if = "Option::is_none", default)]`, so older peers are unaffected in both directions.

## Ingest (`metrics/ingest.rs`, liveness record)

| Field | Type | Notes |
|---|---|---|
| `tunnel_flapping` | `bool` | last reported value (supervisor records only) |
| `last_ike_sa_loss_cause` | `Option<IkeSaLossCause>` | for the alert text |
| `streak_opened_by_flapping` | `bool` | set when flapping opened `tunnel_unhealthy_since` (R7); picks the alert wording |

Rules for `tunnel_unhealthy_since` on supervisor reports:

- healthy ⇔ `child_sa_up == Some(true) && !tunnel_flapping`
- If unhealthy, flapping, and no open streak: open it at `now - tunnel_threshold`, and set `streak_opened_by_flapping`.
- If unhealthy otherwise and no open streak: open it at `now` (as today).
- If healthy: close the streak and clear `streak_opened_by_flapping`.

The folded decision, phases and dispatch are unchanged.
