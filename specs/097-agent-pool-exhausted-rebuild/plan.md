# Implementation Plan: Rebuild the tunnel when the agent has given up on every P-CSCF

**Branch**: `097-agent-pool-exhausted-rebuild` | **Date**: 2026-10-17 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/097-agent-pool-exhausted-rebuild/spec.md`

## Summary

The change has four parts.

**1. File format.** The agent's pool state file (`/tmp/pcscf-N.state`, spec 091) gains a per-mark `marked_at_unix`. The format types move from `ims/agent/pcscf_pool.rs` into `ims/pcscf_files.rs`, the module that already exists so the agent and the supervisor cannot disagree on a file format. That module gets a reader, `read_pool_state`, which the supervisor uses.

**2. Supervisor tick.** `tick_steady_state_with_pool` (`supervise/line_supervisor.rs`) gains one input, a closure returning the agent's pool state. After the route check (spec 096) and before the probe, a pure function `exhausted_pool` decides whether every member of the supervisor's pool carries an unexpired, non-`retry-after` mark set at or after `tracking.negotiated_at`.
- If it does and no call is up, the tick takes the `PcscfUnreachable` remedy (recreate, then terminate and re-initiate) and returns `Recovered{PcscfPoolExhausted}`. The exhausted members are recorded in `tracking.pool_exhausted`.
- If a call is up, the rebuild is deferred and logged once.

**3. Orchestrator.**
- **Baseline:** both steady loops set `tracking.negotiated_at` to "now" at the moment the supervisor restarts the agent onto a freshly negotiated P-CSCF. That happens at startup, after every renegotiating recovery, and on `PcscfChanged`. Marks a dying agent sets during a rebuild therefore never count.
- **Calls on swu:** the swu loop now passes the real `line_in_call`.
- **Event:** the strongSwan loop's `TunnelRecovered` event gains an optional `pcscfs` list, filled for this reason only.

**4. Alert.** A new `pcscf_pool_exhausted` Discord category, `[alerts.pcscf_pool_exhausted]`, is enabled by default with a 300s threshold. Its episode is tracked on the supervisor's ingest record:
- **Start:** the first `TunnelRecovered{PcscfPoolExhausted}` starts the episode.
- **Each further rebuild:** bumps the rebuild count, keeps the addresses for the alert text, and notes which IMS-agent reporter epoch was live at that moment.
- **End:** a `registered = true` report from an IMS-agent process with a different epoch, i.e. one started after the latest rebuild, ends the episode.
- **Decision:** the existing `decide_transition` and `record_alert_outcome` handle the threshold, the one-shot failure, suppression and the all-clear.

## Technical Context

**Language/Version**: Rust (workspace toolchain, `rust-toolchain.toml`)
**Primary Dependencies**: none new (serde_json, prometheus, the observability reporter, the alerts module)
**Storage**: the existing per-line pool state file. It keeps one additive field, so the `version` stays 1 (research R2).
**Testing**: `cargo test` via `make test`:
- **Tick:** the existing `FakeEngine` plus a closure-supplied pool state.
- **`exhausted_pool`:** a pure function with an injected clock.
- **File format:** round-trip, plus old-file compatibility in both directions.
- **Ingest:** `apply_report_with` with an injected `AlertsConfig`, as the existing two-reporter alert tests do.
- **End to end:** a new integration test from a real `Reporter` over a real control socket into the registry.
- **Config:** `tests/test_config_docs.rs` enforces that the new section is documented.

**Target Platform**: Linux container (amd64/arm64)
**Project Type**: single Rust workspace (`gsm-sip-bridge` crate)
**Performance Goals**: one extra local file read per line per tick (30s on strongSwan, 5s on swu), and nothing on the wire (FR-010, SC-005).
**Constraints**:
- The order of the existing checks is unchanged; the new check sits between the route check and the probe (FR-003, FR-005).
- The probe and its timing are untouched (FR-011).
- No throttling (FR-013).
- An old-format state file never triggers a rebuild (FR-008).
- swu has no supervisor reporter, so on swu the rebuild works but the counter and the alert do not. This matches spec 096, and research R7 has the details.

**Scale/Scope**: 1–8 lines per host. Production changes:
- about 80 lines in `line_supervisor.rs`;
- about 60 across `pcscf_files.rs` and `pcscf_pool.rs`;
- about 30 in `orchestrate.rs`;
- about 120 across `metrics/ingest.rs`, `control/protocol.rs`, `alerts/mod.rs` and `config/{raw,mod,build}.rs`.

Plus tests, docs and the changelog.

## Constitution Check

| Principle | Status |
|---|---|
| I. Integration-first testing | **Pass.** Format compatibility is tested on real serialized files, and the alert episode through the real ingest path. The tick uses the existing `FakeEngine`, whose justification (charon, a netns, root) is documented at its definition. The new tick input is a plain closure that tests feed a real `PoolState`, not a mock. |
| II. Green-on-commit | **Pass.** `make format && make lint && make test` before every commit, with a private `CARGO_TARGET_DIR`. |
| III. Atomic commits | **Pass.** Four code commits: (1) `marked_at` and the shared format; (2) the tick check, its orchestrator wiring and the counter label; (3) the alert category; (4) docs and changelog. The spec artifacts and live verification go in separate commits. |
| IV. Makefile-driven | **Pass.** No new targets. |
| V. Simplicity | **Pass.** It reuses an existing file, an existing event, an existing remedy and the existing alert state machine. The one new state machine is the episode, and it is a few fields on an existing record (research R5). |

Re-checked after Phase 1: unchanged.

## Project Structure

### Documentation (this feature)

```text
specs/097-agent-pool-exhausted-rebuild/
├── spec.md
├── plan.md               # this file
├── research.md
├── data-model.md
├── quickstart.md         # live recipe (Vodafone probe-off/probe-on, Jio, idle)
├── contracts/
│   ├── files.md          # pool state file v1 + marked_at_unix
│   ├── steady-tick.md    # where the check sits, and its outcomes
│   └── alerts.md         # [alerts.pcscf_pool_exhausted], episode, wire event
├── checklists/requirements.md
├── tasks.md              # /speckit-tasks
└── live-verification.md  # written after the rig runs
```

### Source Code (repository root)

```text
gsm-sip-bridge/src/
├── ims/
│   ├── pcscf_files.rs          # PoolState/PoolStateMark (moved here) + read_pool_state
│   └── agent/pcscf_pool.rs     # Mark.marked_at; save/load via the shared types
├── supervise/
│   ├── line_supervisor.rs      # exhausted_pool(), new tick input, DegradeReason::PcscfPoolExhausted
│   └── orchestrate.rs          # negotiated_at baseline, pool-state closure, swu line_in_call, pcscfs on the event
├── control/protocol.rs         # TunnelRecoveryReason::PcscfPoolExhausted; TunnelRecovered.pcscfs
├── metrics/ingest.rs           # episode on the supervisor record; decision; alert text
├── alerts/mod.rs               # AlertCategory::PcscfPoolExhausted
└── config/{raw,mod,build}.rs   # [alerts.pcscf_pool_exhausted]
gsm-sip-bridge/tests/
├── test_vowifi_pool_exhausted_alert.rs   # new: reporter -> socket -> ingest (episode + counter)
└── test_config.rs / test_config_docs.rs  # the new section
docs/{configuration,observability}.md, config.toml.example, CHANGELOG.md
```

**Structure Decision**: single crate. Each change goes in the module that already owns its concern: the file format in `ims::pcscf_files`, the tick decision in `line_supervisor`, the wire format in `control::protocol`, and alert decisions in `metrics::ingest`.

## Complexity Tracking

No violations.
