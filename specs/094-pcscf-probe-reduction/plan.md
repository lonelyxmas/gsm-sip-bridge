# Implementation Plan: One P-CSCF probe per line

**Branch**: `094-pcscf-probe-reduction` | **Date**: 2026-10-16 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/094-pcscf-probe-reduction/spec.md`

## Summary

Each VoWiFi line probes its P-CSCF's TCP 5060 from three places. This change keeps only the supervisor's. The idle-tunnel keepalive loop in `start_line_tail` stays, but starts only when the new `[vowifi].idle_keepalive` (default `false`) is `true`. Its interval comes from `keepalive_interval_sec`, whose default becomes 30. The container HEALTHCHECK stops calling `tcp_connect_ok_in_netns`. At the same point in `check_line`, it asks the line's IMS agent for its status on the agent's existing status port (`veth_local_addr:5071`, the query `vowifi-status` sends). It faults the line only on no usable answer or on Gm `failed`. The query has its own short connect and read timeouts, and lines are checked concurrently, so one hung agent cannot run the whole check past Docker's 5s timeout. The supervisor probe, `repair_default_route` and the `PcscfUnreachable` rebuild are not touched.

## Technical Context

**Language/Version**: Rust (workspace toolchain, `rust-toolchain.toml`)
**Primary Dependencies**: none new. Uses the existing newline-JSON agent control protocol (`vowifi::control`)
**Storage**: none. One new config key, `[vowifi].idle_keepalive`
**Testing**: `cargo test` via `make test`. `MockCommandRunner` (the established boundary for root-only and network effects) gains a recorder for TCP connect attempts and a seeded agent-status reply. The bounded status query itself is tested against a real loopback `TcpListener`
**Target Platform**: Linux container (amd64/arm64)
**Project Type**: single Rust workspace (`gsm-sip-bridge` crate)
**Performance Goals**: with default config, one P-CSCF connect per line per 30s (SC-001). HEALTHCHECK finishes inside Docker's `--timeout=5s` with up to `max_lines` (default 8) lines, even with every agent hung
**Constraints**: supervisor probe path byte-for-byte unchanged (FR-007). `tcp-probe` subcommand kept (FR-014). No Dockerfile `HEALTHCHECK` change. VoLTE untouched
**Scale/Scope**: 1–8 lines per host. About 150 lines of production code across `config/{raw,build,mod}.rs`, `commands/{config,healthcheck}.rs`, `supervise/{orchestrate,runner}.rs`, `vowifi/mod.rs`, plus docs

## Constitution Check

| Principle | Status |
|---|---|
| I. Integration-first testing | Pass. The bounded status query is tested end to end against a real loopback listener: a reply, a silent peer, a refused port. The HEALTHCHECK decision and the keepalive gate go through `MockCommandRunner`, whose justification for standing in for namespaces and root-only commands is already documented at its definition. The new agent-status seam follows the existing `http_get` precedent, for the same reason: the agent listens on a veth address that only exists inside a running deployment. |
| II. Green-on-commit | Pass. `make format`, `make lint` and `make test` before every commit. |
| III. Atomic commits | Pass. Four commits: config key and interval default; keepalive gate; HEALTHCHECK via agent; docs and changelog. Live verification is recorded in the spec folder afterwards. |
| IV. Makefile-driven | Pass. No new targets. |
| V. Simplicity | Pass. The keepalive loop is moved and gated, not rewritten. The HEALTHCHECK reuses the agent's existing status query and `GmConnectionState::render` strings instead of adding a new endpoint. One boolean key, no per-line override. |

Re-checked after Phase 1 design: no change.

## Project Structure

### Documentation (this feature)

```text
specs/094-pcscf-probe-reduction/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── config.md        # [vowifi].idle_keepalive, keepalive_interval_sec, config export
│   ├── healthcheck.md   # verdict table, fault strings, timing budget
│   └── logs.md          # startup log line
├── checklists/requirements.md
└── tasks.md
```

### Source Code

```text
gsm-sip-bridge/src/
├── config/
│   ├── raw.rs              # RawVowifi.idle_keepalive (default false); keepalive_interval_sec default 20 → 30
│   ├── build.rs            # carry idle_keepalive through to VowifiConfig
│   └── mod.rs              # VowifiConfig.idle_keepalive + doc; default-value test updated
├── commands/
│   ├── config.rs           # IDLE_KEEPALIVE in the shell-env export
│   └── healthcheck.rs      # check_line: agent query replaces tcp_connect_ok_in_netns;
│                           # LineFault::{AgentNotResponding, GmConnectionFailed} replace PcscfUnreachable;
│                           # classify_agent_reply(); per-line checks run concurrently
├── supervise/
│   ├── orchestrate.rs      # idle_keepalive_interval(cfg) gate + idle_keepalive_once(); start_line_tail
│   │                       # spawns the loop only when enabled; one startup log line in start_vowifi_subsystem
│   └── runner.rs           # CommandRunner::agent_status(addr) -> AgentStatusProbe (Real + Mock);
│                           # Mock: tcp_connect_attempts() recorder, set_agent_status()
└── vowifi/
    └── mod.rs              # query_status_bounded(addr, connect, read); query_status delegates (unchanged behaviour)

config.toml.example         # both keys commented out, note pointing at #120
docs/configuration.md       # new row; keepalive_interval_sec row clarified
docs/operations.md          # what the HEALTHCHECK checks per VoWiFi line now
CHANGELOG.md                # Unreleased → Changed
```

**Structure Decision**: existing modules only. The gate lives beside the loop it gates in `orchestrate.rs`. The agent-status seam sits on `CommandRunner` next to `http_get`, which already exists so the HEALTHCHECK can be tested as a pure decision over real-world effects.

## Design notes

1. **Config (R2).** `RawVowifi` gains `idle_keepalive: bool` (default `false`). The field name is the TOML key, so the macro-generated `KEYS` list picks it up and strict parsing rejects typos. A non-boolean is a serde type error naming the key. `keepalive_interval_sec` keeps its `1..=3600` validation and gets a new default of 30 in both `RawVowifi::default` and the `VowifiConfig` test default. `RawVowifiLine` does **not** get the key: it is per deployment.
2. **Keepalive gate (R1).** Add `fn idle_keepalive_interval(v: &VowifiConfig) -> Option<Duration>`, returning `Some(keepalive_interval_sec)` only when `idle_keepalive`. Move the loop body to `fn idle_keepalive_once(runner, netns, pcscf_path)`, which reads the file and, if it is non-empty, makes one `tcp_connect_ok_in_netns` to `PCSCF_SIP_PORT` and discards the result. `start_line_tail` spawns `loop { once; sleep(interval) }` only when the gate returns `Some`. The long comment explaining the 2026-08-17 outage and the zombie-process history moves with the loop. `initial_pcscf` is still passed in and still unused by the loop. `start_vowifi_subsystem` logs the state once (contracts/logs.md).
3. **Agent status seam (R3).** `vowifi::query_status_bounded(addr, connect, read) -> BridgeResult<ControlMessage>` sets `set_read_timeout(Some(read))` on the stream before reading. `query_status` becomes `query_status_bounded(addr, 3s, None)`, so `vowifi-status` behaves exactly as before. `CommandRunner::agent_status(&self, addr: &str) -> AgentStatusProbe` calls it with 1s and 1s and maps `Err` to `NoAnswer(reason)`, `Ok(msg)` to `Reply(msg)`. The mock answers from a map seeded by `set_agent_status(addr, probe)`; an unseeded address is `NoAnswer("unseeded")`.
4. **HEALTHCHECK decision (R4).** `check_line` keeps the interface-address check and the empty-P-CSCF-file gate. In place of `tcp_connect_ok_in_netns` it calls `runner.agent_status(&format!("{}:{}", line.veth_local_addr, AGENT_A_STATUS_PORT))` and passes the result to the pure `classify_agent_reply(addr, probe) -> Option<LineFault>`, whose verdicts are in contracts/healthcheck.md. Gm `failed` is detected by `gm_connection.starts_with("failed")`, matching `GmConnectionState::render`. `PCSCF_SIP_PORT` in `healthcheck.rs` goes away.
5. **Time budget (R5).** `evaluate_with_metrics` runs `check_line` for every resolved line inside `std::thread::scope`, one thread per line, and gathers results in line order. The agent phase is bounded at about 2s regardless of line count. Combined with the existing 3s `/metrics` timeout this fits under 5s; in the common case both take milliseconds. `CommandRunner` is already `Send + Sync`.
6. **Mock recorder.** `MockCommandRunner` records every `tcp_connect_ok`/`tcp_connect_ok_in_netns` call as `(Option<netns>, host, port)`, read back by `tcp_connect_attempts()`. The HEALTHCHECK tests assert it stays empty, and the keepalive tests assert exactly one attempt per `idle_keepalive_once` with a P-CSCF present.

## Complexity Tracking

None.
