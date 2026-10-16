# Research: One P-CSCF probe per line

No `NEEDS CLARIFICATION` remained after `/speckit-clarify`. This file records the design decisions the plan rests on, and the code facts behind them.

## R1. Gate the keepalive, do not delete it

- **Decision**: Keep the loop, extracted into `idle_keepalive_once` and spawned from `start_line_tail` only when `idle_keepalive_interval(&config.vowifi)` returns `Some`.
- **Rationale**: The user asked for an escape hatch rather than deletion. #120 found no eviction with the probes off (finding 1), but #106 recorded an S2b eviction on another carrier, so an operator may need the traffic back without a rebuild. Extracting the body gives a testable unit without changing what one cycle does.
- **Alternatives considered**: Deleting the thread, as #120 suggested: rejected by the user. Gating it at the top of the loop (spawn always, skip when off): leaves an idle thread per line for nothing.
- **Code facts**: The loop is at `supervise/orchestrate.rs` (`start_line_tail`, "Idle-tunnel keepalive" block). It hard-codes `Duration::from_secs(30)` and never reads `keepalive_interval_sec`. Only `start_vowifi_line_strongswan` and `start_vowifi_line_swu` call `start_line_tail`, so VoLTE is unaffected.

## R2. A separate boolean switch; interval default 30

- **Decision**: `[vowifi].idle_keepalive = false` is the only switch. `keepalive_interval_sec` keeps its range check, and its default goes from 20 to 30 (spec Clarifications Q2).
- **Rationale**: `config.toml.example` ships `keepalive_interval_sec = 20` uncommented, so most live configs set it. Overloading it (`0` = off) would turn the keepalive back on for nearly every deployment after an upgrade. Today the value has no effect except in `config` export (`KEEPALIVE_INTERVAL`), so changing its default changes no running behaviour.
- **Alternatives considered**: Interval `0` means off: would re-enable the keepalive for most deployments. Deprecating the interval and keeping 30s hard-coded: throws away a knob that is about to become meaningful.

## R3. Query the agent through its existing status port, with a bounded read

- **Decision**: Reuse `ControlMessage::StatusQuery` → `RegistrationStatusReply` on `veth_local_addr:AGENT_A_STATUS_PORT` (5071), exactly what `vowifi-status` sends. Add `query_status_bounded(addr, connect, read)` and route it through a new `CommandRunner::agent_status`.
- **Rationale**: Agent A answers status queries from `run_status_listener`, a dedicated thread separate from its SIP dispatch loop ("a status query never competes with call signaling"), so a line in a call answers promptly. The reply already carries `gm_connection`, the agent's own verdict on P-CSCF reachability. The existing `query_status` bounds only the connect (3s) and then blocks in `read_msg` with no read timeout. A peer that accepts and never writes would hang the HEALTHCHECK past Docker's 5s timeout, and Docker would report a bare timeout that names no line.
- **Alternatives considered**: Reading `/metrics` for a Gm gauge: there is no per-line Gm-state series today, and adding one is a larger change than reusing the reply. Reading a state file written by the agent: a new file contract for data the agent already serves. A new HTTP health endpoint on the agent: more moving parts (constitution V).
- **Seam**: a `CommandRunner` method, following the `http_get` precedent ("routed through the runner like every other real-world effect, so the healthcheck's decision logic stays a pure function"). The veth address exists only inside a running deployment, so unit tests cannot reach a real agent. The bounded query itself gets real-socket tests on loopback.

## R4. Which agent states are faults

- **Decision**: A fault on no usable answer (`AgentNotResponding`) or on a `gm_connection` that starts with `failed` (`GmConnectionFailed`). Everything else, including `reconnecting`, a non-registered `state`, `can_answer = false` and an empty `gm_connection`, is not a fault (spec Clarifications Q1, FR-012).
- **Rationale**: `GmConnectionState` moves `Up` → `Reconnecting` on each unanswered 120s ping, escalates to re-registration after 3 attempts (`MAX_RECONNECT_ATTEMPTS`), and becomes `Failed` only when that escalated renewal fails (`ims/agent/mod.rs`, FR-010b: "Failed is not terminal"). `can_answer` also folds in `busy` (in a call) and `pbx_registered` (the PBX leg), neither of which is about the P-CSCF. Lapsed registration is already a HEALTHCHECK fault through `/metrics` (spec 039 FR-020).
- **Matching**: `GmConnectionState::render` yields exactly `up`, `reconnecting since <rfc3339> (attempt N)` or `failed since <rfc3339>`. Matching on the `failed` prefix uses the existing wire format without a protocol change. An older agent sends `""` (serde default), which is not a fault.

## R5. Fit the 5s Docker timeout with up to 8 lines

- **Decision**: 1s connect and 1s read per agent query. Lines are checked concurrently with `std::thread::scope`, and results are gathered in line order.
- **Rationale**: Docker's `HEALTHCHECK --timeout=5s` covers the whole run. `/metrics` already has a 3s timeout and is fetched first. Done sequentially, 8 hung agents at 2s each would take 16s. Today's sequential 3s P-CSCF probe already exceeds the budget with 2 dead lines. On a healthy deployment the agent answers over a local veth in milliseconds, so 1s is generous. `CommandRunner: Send + Sync` and every `check_line` input is borrowed, so scoped threads need no `Arc`.
- **Alternatives considered**: A sequential shared deadline: hung early lines would use up the budget and later lines would be reported as not responding when they were never asked. A thread pool or async: more machinery than one scoped spawn per line.

## R6. Where the startup log line goes

- **Decision**: `start_vowifi_subsystem` logs once, before lines start (contracts/logs.md).
- **Rationale**: `start_line_tail` runs once per line and again on re-discovery, so logging there would repeat. FR-005 asks for one line.
