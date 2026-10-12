# Implementation Plan: ePDG CHILD_SA health detection and bounded recovery

**Branch**: `086-epdg-child-sa-recovery` | **Date**: 2026-10-12 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/086-epdg-child-sa-recovery/spec.md` (GitHub #107, items A, B, C, E)

## Summary

The strongSwan supervisor misdiagnoses a missing CHILD_SA as healthy, because it matches
the IKE_SA header in `swanctl --list-sas`. Its CHILD_SA recovery never escalates, and its IKE_SA
teardown can block for as long as charon's retransmits last. Together these turned #107's
CHILD_SA hard expiry into a 170s outage that no metric showed.

This plan makes four changes:
1. Parse the indented CHILD_SA entry and its state.
2. Escalate a second consecutive miss to `terminate` + `reinitiate` with an IMS agent restart, with no backoff.
3. Run every steady-state teardown as `timeout 10 … swanctl --terminate --ike <conn> --force`.
4. Have the supervisor report a new per-line `gsm_sip_bridge_vowifi_child_sa_up` gauge over the
   existing observability protocol. It reads down from the start of supervision, and the signal
   also feeds the existing tunnel-failure alert without double alerts.

Tunnel negotiation is unchanged (FR-018).

## Technical Context

**Language/Version**: Rust (stable, workspace edition as in `Cargo.toml`)  
**Primary Dependencies**: existing only: serde/serde_json (control protocol), prometheus
(`register_gauge_vec!`), strongSwan 5.9.3 `swanctl` and BusyBox `timeout` in the runtime image  
**Storage**: N/A (in-memory supervisor and daemon state)  
**Testing**: `cargo test` via `make test`; `make lint` (clippy `-D warnings`, whole workspace including tests)  
**Target Platform**: Linux container (Alpine runtime image), x86_64 and arm64  
**Project Type**: single Rust workspace; the `gsm-sip-bridge` binary hosts `supervise`, the daemon and the agents  
**Performance Goals**: detect within 1 tick (30s); rebuild under way within 2 ticks (~60s); teardown ≤ 10s  
**Constraints**: no change to swanctl template or strongSwan build; recovery strictly per connection (shared charon); no extra `swanctl` query per tick  
**Scale/Scope**: 1–N VoWiFi lines per container, one shared charon; ~6 source files, 1 doc, 1 config comment

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Integration-First Testing | PASS, with justified mocks | Supervisor logic is tested through the existing `FakeEngine` / `MockCommandRunner`. Their justification is already at the mock sites: charon, a live ePDG and a SIM can't run in CI. Parser fixtures are real 5.9.3 output. Ingest, alert and metrics tests use the real `apply_report` and the real control socket. The behaviour that can't be reached in CI is covered by the blocking live gate (SC-008). |
| II. Green-on-Commit | PASS | Every task ends with `make format && make lint && make test` before its commit. |
| III. Frequent Atomic Commits | PASS | Tasks are grouped into one commit per concern: A, B, C, E-protocol, E-alert, docs. |
| IV. Makefile-Driven Build | PASS | No new tooling; the live gate uses `docker build` against the existing Dockerfile, as the `test/` folder already does. |
| V. Simplicity | PASS | One struct replaces a positional `&mut u32` (justified in research R2). No new abstraction layers. Reuses `Reporter`, `decide_transition` and the alert phase machinery. |

Post-design re-check: still PASS. The one added structure, the per-line ownership of the
tunnel-failure decision (R6), is the minimum needed to meet FR-017a's "exactly one alert"
across two reporters. Complexity Tracking is empty.

## Project Structure

### Documentation (this feature)

```text
specs/086-epdg-child-sa-recovery/
├── spec.md
├── plan.md              # this file
├── research.md          # R1–R7
├── data-model.md
├── quickstart.md        # automated + SC-008 live gate; live results appended here
├── contracts/
│   └── observability.md # protocol, metric, alert, log-line contract
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
gsm-sip-bridge/src/
├── supervise/
│   ├── engines.rs          # A: CHILD_SA parser; C: bounded --force terminate
│   ├── line_supervisor.rs  # B: SteadyTracking, CHILD_SA_MISSING_STRIKES, ChildSaStuck
│   └── orchestrate.rs      # B: thread SteadyTracking (strongswan + swu loops); E: per-line Supervisor reporter
├── control/protocol.rs     # E: AgentKind::Supervisor, AgentState.child_sa_up
├── metrics/
│   ├── mod.rs              # E: VOWIFI_CHILD_SA_UP gauge
│   ├── ingest.rs           # E: apply child_sa_up; per-line tunnel-failure ownership (R6)
│   └── server.rs           # E: zero the gauge for stale Supervisor records
├── ims/agent/mod.rs        # E: exhaustive AgentKind matches only
└── vowifi/mod.rs           # E: exhaustive AgentKind matches only

gsm-sip-bridge/tests/
├── test_observability_ingest.rs   # E: supervisor report over the real socket
└── test_metrics_endpoint.rs       # E: new gauge exposed

docs/observability.md               # E: new gauge; tunnel-failure alert now covers CHILD_SA
config.toml.example                 # E: [alerts.tunnel_failure] comment
```

**Structure Decision**: single-project layout. All changes live in existing modules of the
`gsm-sip-bridge` crate and need no new modules.

## Implementation notes per item

- **A** (research R1): `list_sas_has_child_sa` matches `"  {conn}: #"` with exactly two leading
  spaces, splits on `", "`, and checks field 2 against the usable-state set. The doc comment drops
  the "1:1 port of grep" claim and explains why. Its tests replace
  `list_sas_detection_matches_the_grep_pattern`.
- **B** (R2): `SteadyTracking` replaces `unreachable_streak` in both loops and in the test helper
  shadow. In the `ChildSaMissing` arm, the first miss re-initiates and the second does
  terminate + re-initiate and returns `ChildSaStuck`. The existing missing-CHILD_SA log line stays,
  and a second line is added for the escalation. `recovery_restarts_agent(ChildSaStuck) == true`.
- **C** (R3): `StrongswanEngine::terminate` builds
  `["timeout", "10", "env", STRONGSWAN_CONF=…, "swanctl", "--terminate", "--ike", conn, "--force"]`
  through `runner.run`, with a named const `STEADY_TERMINATE_TIMEOUT_SECS = 10`.
- **E-report** (R4/R5): `start_vowifi_line_strongswan` spawns the reporter and reports
  `child_sa_up: Some(false)` before `establish_line_tunnel`, `Some(true)` at "tunnel UP", and
  `Some(tracking.child_sa_usable)` after every tick. The swu path doesn't change.
- **E-ingest/alert** (R6): Supervisor `child_sa_up` sets the gauge and the Supervisor record's
  `tunnel_unhealthy_since`. The tunnel-failure decision is owned per module by the Supervisor
  record when one exists, uses the combined `min(since)`, and `tunnel_alert_generation`
  scopes delivery callbacks. Alert text names the line, not the reporter kind.

## Complexity Tracking

No violations to justify.
