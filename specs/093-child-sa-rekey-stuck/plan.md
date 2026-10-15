# Implementation Plan: Rebuild the ePDG tunnel on a stuck CHILD_SA rekey

**Branch**: `093-child-sa-rekey-stuck` | **Date**: 2026-10-15 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/093-child-sa-rekey-stuck/spec.md`

## Summary

When a CHILD_SA has not been replaced and has less than 300s left before its hard expiry, its rekey has failed. Today nothing happens until the kernel deletes the CHILD_SA and the #118 two-strike path recovers about 60s later. With this change, the strongSwan steady-state tick reads each CHILD_SA's `expires in` from the `--list-sas` output it already fetches. Past the threshold it does one planned rebuild, using the existing terminate, reinitiate and agent-restart path, under a new `ChildSaRekeyStuck` reason. If the VoWiFi agent reports an active call, the rebuild waits until that call ends or 90s of lifetime remain, whichever comes first. The agent publishes its call count in a per-line file beside the P-CSCF files.

## Technical Context

**Language/Version**: Rust (workspace toolchain, `rust-toolchain.toml`)
**Primary Dependencies**: none new. strongSwan 5.9.3 (Osmocom fork, pinned) as an external process via `swanctl`
**Storage**: one new per-line file, `/tmp/pcscf-N.calls`
**Testing**: `cargo test` via `make test`. `FakeEngine` and `MockCommandRunner` harnesses already exist in `supervise/`
**Target Platform**: Linux container (amd64/arm64)
**Project Type**: single Rust workspace (`gsm-sip-bridge` crate)
**Performance Goals**: no extra per-tick process spawn (FR-001). The charon log is read only on a tick that rebuilds
**Constraints**: per-line isolation on the shared charon. Existing recovery precedence unchanged. No config keys
**Scale/Scope**: 1–4 lines per host. About 300 lines of production code across `supervise/{engines,line_supervisor,orchestrate}.rs`, `ims/{pcscf_files,observability,agent/mod}.rs`

## Constitution Check

| Principle | Status |
|---|---|
| I. Integration-first testing | Pass. The parsers are tested on real listing and log shapes. The tick decisions use the existing `FakeEngine` harness, whose justification for faking strongSwan is already documented at its definition. The orchestrator path is tested through `MockCommandRunner`, the established boundary for root-only commands. |
| II. Green-on-commit | Pass. `make format`, `make lint` and `make test` run before each commit. |
| III. Atomic commits | Pass. One commit per story (US1+US2 together, because they are one decision), then US3, then docs. |
| IV. Makefile-driven | Pass. No new targets needed. |
| V. Simplicity | Pass. It reuses the `ChildSaStuck` recovery verbatim and adds no config. The call state is a file rather than a new socket protocol (research R5). |

Re-checked after Phase 1 design: no change.

## Project Structure

### Documentation (this feature)

```text
specs/093-child-sa-rekey-stuck/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── logs.md
│   └── files.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code

```text
gsm-sip-bridge/src/
├── supervise/
│   ├── engines.rs          # newest_child_sa_lifetime() parser, child_sa_rejection_notify(),
│   │                       # StrongswanEngine::steady_state_health → Ok(Some(lifetime))
│   ├── line_supervisor.rs  # ChildSaLifetime, SteadyStateHealth::Ok(..), DegradeReason::ChildSaRekeyStuck,
│   │                       # SteadyTracking guards, decision in tick_steady_state_with_pool (+ call_active closure)
│   └── orchestrate.rs      # read the call-state file, pass call_active into the tick
└── ims/
    ├── pcscf_files.rs      # calls_path / write_active_calls / read_active_calls
    ├── observability.rs    # optional calls-file mirror in set_active_calls
    └── agent/mod.rs        # reset to 0 at run start; attach the path on the VoWiFi path
```

**Structure Decision**: existing modules only. The decision stays in `line_supervisor` beside the `ChildSaMissing` and `ChildSaStuck` logic it extends.

## Design notes

1. **Parsing (engines.rs).** Add `newest_child_sa_lifetime(sas_output, conn) -> Option<ChildSaLifetime>`. It walks the same two-space-indented CHILD_SA headers that `list_sas_has_child_sa` matches. For each usable header it records `#N` and reads `expires in Ns` from the next line (four-space indent). It returns the highest `#N`. `StrongswanEngine::steady_state_health` returns `Ok(newest_child_sa_lifetime(..))`. A usable CHILD_SA exists at that point, so `None` only happens if the header itself is malformed. SWu returns `Ok(None)`.
2. **Decision (line_supervisor.rs).** After the structural match, `if let Ok(Some(l))`, apply the data-model decision table. Clear the guards whenever the health is not `Ok(Some(l))` with the same `#N`. The process-died branch resets them with the other counters.
3. **Rebuild.** `engine.terminate`, then `engine.reinitiate`, then `Recovered { reason: ChildSaRekeyStuck }`, the same as the `ChildSaStuck` branch. The orchestrator's `renegotiated` branch does the rest unchanged.
4. **Rejection reason.** A new trait method `child_sa_rejection(&self, runner) -> Option<String>` defaults to `None`. strongSwan reads the charon log through `child_sa_rejection_notify`. It is called only when rebuilding.
5. **Call state.** `tick_steady_state_with_pool` gains `call_active: &dyn Fn() -> bool`. The tick evaluates it only when it has found a stuck rekey (contracts/files.md). The wrapper `tick_steady_state` and the SWu loop pass `&|| false`. The strongSwan orchestrator loop passes a closure that reads `pcscf_files::calls_path(&line.pcscf_source_path)` through the runner.

## Complexity Tracking

None.
