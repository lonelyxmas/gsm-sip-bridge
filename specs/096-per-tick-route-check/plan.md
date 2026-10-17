# Implementation Plan: Check the default route on every steady-state tick

**Branch**: `096-per-tick-route-check` | **Date**: 2026-10-17 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/096-per-tick-route-check/spec.md`

## Summary

`tick_steady_state_with_pool` (`supervise/line_supervisor.rs`) now calls `engine.repair_default_route(runner, current_pcscf)` on every structurally healthy tick: after the `PcscfChanged` early return and **before** the P-CSCF probe, and only when a P-CSCF is known. The call inside the unreachable branch goes. When the route was missing and is repaired, the tick returns `Recovered{DefaultRouteMissing}` as before. When the route is present, or the repair didn't take, it falls through to the probe and strike logic, which are unchanged. Nothing else about `repair_default_route` (the #133 family-aware engine method), the swu no-op or the orchestrator's handling of `DefaultRouteMissing` changes.

Clarifications Q3 and Q4 add a counter, `gsm_sip_bridge_vowifi_tunnel_recoveries_total{module,reason}`. Every steady-state `Recovered{reason}` on a strongSwan line sends a new `ObservedEvent::TunnelRecovered { reason }` on the supervisor's existing reporter. The daemon's ingest turns it into a counter increment. `reason` is a new closed wire enum, `TunnelRecoveryReason`, which mirrors `DegradeReason`.

## Technical Context

**Language/Version**: Rust (workspace toolchain, `rust-toolchain.toml`)
**Primary Dependencies**: none new. Uses the existing `prometheus` registry, the observability reporter, and the control-socket protocol
**Storage**: none
**Testing**: `cargo test` via `make test`:
- The steady-tick decision is tested with the module's existing `FakeEngine`. It is the established seam for the tick, because the real engine needs charon, netns and root.
- The counter is tested end to end in a new integration test: a real control server, a real `Reporter` (`AgentKind::Supervisor`) and the real registry, following `tests/test_vowifi_health_metrics.rs`.
- The wire enum gets serde round-trip tests in `control/protocol.rs`.

**Target Platform**: Linux container (amd64/arm64)
**Project Type**: single Rust workspace (`gsm-sip-bridge` crate)
**Performance Goals**: one extra local `ip [-6] route show default dev <tun>` per strongSwan line per 30s tick, and nothing on the wire (SC-004)
**Constraints**:
- The order of every other tick check is unchanged (FR-005).
- `DefaultRouteMissing` keeps its `recovery_restarts_agent`/`recovery_renegotiates` semantics (FR-009).
- No new escalation (FR-012).
- The counter's label cardinality stays bounded: one closed enum of 11 reasons.
- The swu engine is untouched (FR-008). The swu loop has no supervisor reporter, so it gets no counter.

**Scale/Scope**: 1–8 lines per host. About 30 lines of production change in `line_supervisor.rs`, about 60 across `control/protocol.rs`, `metrics/{mod,ingest}.rs` and `supervise/orchestrate.rs`, plus tests, docs and the changelog

## Constitution Check

| Principle | Status |
|---|---|
| I. Integration-first testing | Pass. The counter is tested through the real reporter, socket, ingest and registry path. The tick's decision logic uses the existing `FakeEngine`, whose justification (the real engine needs charon, a netns and root) is already documented at its definition. No new mocks. |
| II. Green-on-commit | Pass. `make format`, `make lint` and `make test` before every commit. |
| III. Atomic commits | Pass. Three code commits: (1) the per-tick route check and its tests; (2) the recovery counter (wire enum, metric, ingest, orchestrator send, integration test); (3) docs and changelog. The spec artifacts and live verification are committed separately. |
| IV. Makefile-driven | Pass. No new targets. |
| V. Simplicity | Pass. The route check moves; the repair method is reused as-is. The counter reuses the existing event channel instead of adding a metrics endpoint to the supervisor. The reason enum has no per-call or per-address values. |

Re-checked after Phase 1: unchanged. The design adds no layers beyond the one wire enum.

## Project Structure

### Documentation (this feature)

```text
specs/096-per-tick-route-check/
├── spec.md
├── plan.md               # this file
├── research.md
├── data-model.md
├── quickstart.md         # live verification recipe (both rigs + forced-probe build)
├── contracts/
│   ├── steady-tick.md    # order of checks in one tick, and outcomes
│   └── metrics.md        # the new counter and its wire event
├── checklists/requirements.md
├── tasks.md              # /speckit-tasks
└── live-verification.md  # results, written after the rig runs
```

### Source Code (repository root)

```text
gsm-sip-bridge/src/
├── supervise/
│   ├── line_supervisor.rs   # tick order change; DegradeReason -> TunnelRecoveryReason; tests
│   └── orchestrate.rs       # strongSwan steady loop sends TunnelRecovered on each Recovered
├── control/protocol.rs      # TunnelRecoveryReason + ObservedEvent::TunnelRecovered (+ serde tests)
└── metrics/
    ├── mod.rs               # VOWIFI_TUNNEL_RECOVERIES_TOTAL
    └── ingest.rs            # apply_event arm
gsm-sip-bridge/tests/
├── test_vowifi_tunnel_recovery_metrics.rs   # new: reporter -> socket -> ingest -> registry
└── test_metrics_endpoint.rs                 # the new name is exported
docs/observability.md                        # metric table row
CHANGELOG.md
```

**Structure Decision**: single crate. Changes go in the modules that already own each concern: tick decision in `line_supervisor`, wire format in `control::protocol`, registry in `metrics`.

## Complexity Tracking

No violations.
