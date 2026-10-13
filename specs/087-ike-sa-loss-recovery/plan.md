# Implementation Plan: ePDG IKE_SA-loss recovery, post-recovery P-CSCF sync, graceful teardown and Gm SA cleanup

**Branch**: `087-ike-sa-loss-recovery` | **Date**: 2026-10-13 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/087-ike-sa-loss-recovery/spec.md` (follow-up to GitHub #106; #120 and #107's selector fix out of scope)

## Summary

The strongswan supervisor treats a lost IKE_SA as a lost CHILD_SA. Its post-recovery P-CSCF sync reads the previous negotiation, and its teardown never waits for the ePDG's DELETE acknowledgement. A `pkill`ed IMS agent also leaves its Gm IPsec state behind. Together, these turn a carrier `DELETE for IKE_SA` into minutes of unreachability and a second, self-inflicted agent restart.

This plan makes five changes:

1. **Tell IKE_SA loss apart.** `swanctl --list-sas` output is split three ways: no IKE_SA, an IKE_SA without a usable CHILD_SA, and a usable CHILD_SA. "No IKE_SA" gets its own recovery: re-initiate, then an unconditional agent restart. The log line names the cause: carrier DELETE, dead-peer timeout, or unknown.
2. **Sync the P-CSCF from the new negotiation.** After every recovery that restarts the agent, the supervisor waits (≤20s) for a negotiation newer than a log marker taken before the tick. It records that negotiation's P-CSCF, then restarts the agent. A per-line gate keeps the agent loop from respawning in between.
3. **Graceful-first teardown.** A non-forced delete with a 5s wait. Only if the IKE_SA is still listed, or presence cannot be determined, is a forced delete issued. No fixed delay.
4. **Clear the line namespace's XFRM state** before each strongswan-engine agent spawn.
5. **Raise the tunnel-failure alert for a flapping tunnel**, meaning 3 or more rebuilds within `unhealthy_sec`. The alert text carries the last IKE_SA loss cause.

## Technical Context

**Language/Version**: Rust (stable, per `rust-toolchain.toml`), workspace crate `gsm-sip-bridge`  
**Primary Dependencies**: existing only: serde (control protocol), prometheus, strongSwan 5.9.3 `swanctl`/vici (Osmocom fork), iproute2 `ip xfrm`, BusyBox `timeout`  
**Storage**: N/A (in-memory supervisor and daemon state; charon filelog read as today)  
**Testing**: `cargo test` via `make test`; `make lint` (clippy `-D warnings`, whole workspace including tests)  
**Target Platform**: Linux container (Alpine runtime, privileged), x86_64 and arm64  
**Project Type**: single Rust workspace; the `gsm-sip-bridge` binary hosts `supervise`, the daemon and the agents  
**Performance Goals**: IKE_SA loss detected within 1 tick (30s), line re-registered within 75s (SC-001); teardown ≤ 28s worst case (8s graceful + 10s presence query + 10s forced)  
**Constraints**: no new config keys or metrics; recovery stays per connection (shared charon); no swanctl template or strongSwan build change; the shutdown path's teardown is unchanged  
**Scale/Scope**: 1–N VoWiFi lines per container, one shared charon; ~5 source files, release notes, one config-example comment

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Integration-First Testing | PASS, with justified mocks | Supervisor and engine logic is tested through the existing `MockCommandRunner` / fake engine. They are already justified at the mock site: charon, a live ePDG, a SIM and root network namespaces can't run in CI. Ingest and alert tests drive the real `apply_report`. Parser fixtures use real 5.9.3 line shapes. What CI cannot reach is covered by the blocking live gate (SC-007, quickstart). |
| II. Green-on-Commit | PASS | Every task group ends with `make format && make lint && make test` before its commit. |
| III. Frequent Atomic Commits | PASS | One commit per concern: detection, IKE_SA recovery, P-CSCF sync, teardown, Gm flush, wire fields, flapping alert, docs. |
| IV. Makefile-Driven Build | PASS | No new tooling; the live gate uses the existing `test/` container setup. |
| V. Simplicity | PASS | Reuses #118's `SteadyTracking`, `Reporter`, folded tunnel decision and alert phases. Adds one small window type (`RebuildWindow`) and one per-line flag; no new alert type, config or abstraction layer. |

Post-design re-check: still PASS. The rebuild-in-progress gate (R5) is the minimum needed for SC-002's "exactly once". Complexity Tracking is empty.

## Project Structure

### Documentation (this feature)

```text
specs/087-ike-sa-loss-recovery/
├── spec.md
├── plan.md                   # this file
├── research.md               # R1–R8
├── data-model.md
├── quickstart.md             # live gate (SC-007)
├── contracts/
│   └── observability.md      # log lines, wire fields, alert text
├── checklists/requirements.md
└── tasks.md                  # /speckit-tasks
```

### Source Code (repository root)

```text
gsm-sip-bridge/src/
├── supervise/
│   ├── engines.rs          # list_sas_has_ike_sa, ike_sa_loss_cause, extract_pcscf_after,
│   │                       # graceful-first StrongswanEngine::terminate, log_mark/fresh_pcscf_since,
│   │                       # steady_state_health → IkeSaMissing
│   ├── line_supervisor.rs  # SteadyStateHealth::IkeSaMissing, DegradeReason::IkeSaLost,
│   │                       # IkeSaLossCause, tick branch, recovery_restarts_agent, RebuildWindow
│   └── orchestrate.rs      # per-tick log mark, post-recovery P-CSCF wait, rebuild gate,
│                           # Gm flush before agent spawn, flapping + cause reporting
├── control/protocol.rs     # AgentState.tunnel_flapping, AgentState.ike_sa_loss_cause
└── metrics/ingest.rs       # flapping-driven tunnel streak, cause on the record, alert text
RELEASE_NOTES.md, CHANGELOG.md, config.toml.example ([alerts.tunnel_failure] comment)
```

**Structure Decision**: single existing crate. Every change lands next to the #118 code it extends. No new modules.

## Complexity Tracking

None.
