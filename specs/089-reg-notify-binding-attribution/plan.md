# Implementation Plan: Reg-event NOTIFY binding attribution and per-event deregistration policy

**Branch**: `089-reg-notify-binding-attribution` | **Date**: 2026-10-14 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/089-reg-notify-binding-attribution/spec.md` (GitHub #124, PR 1 of 3; 090 and 091 build on it)

## Summary

A renewal registers a new contact address. The network then retires the old binding with a reg-event NOTIFY, and
that NOTIFY still carries our IMEI. `find_own_contacts` claims any entry that mentions our IMEI, so it treats the
retired binding as ours. The line reports the current binding as lost and re-registers, which retires the binding
it just made, and the loop repeats about once per renewal (#124).

The plan makes four changes:

1. **Attribute by address.** An entry naming our current contact address belongs to the current binding. An
   IMEI-echoing entry that names a *different* address is a **retired** previous binding: it gets an info log line
   and no action. An IMEI-echoing entry with no checkable address keeps today's treatment (research R1).
2. **Apply the TS 24.229 §5.1.1.7 policy for each event**, choosing the most restrictive event in the NOTIFY:
   - `deactivated`: re-register at once. The attempt still counts as an early loss (clarification Q2).
   - `unregistered` / `probation`: wait on the backoff ladder, and never less than the network's retry-after.
   - `rejected`: end the call, report Failed, and start a cooldown that does not count against the failure budget
     (default 30 min, configurable per line). Make one attempt when it ends.
3. **Distinguish identity-level from binding-level deregistration in the log**, and log every reg-event NOTIFY
   with its attribution, the current contact address, the applied event and the body.
4. **Show the rejection cooldown** in `vowifi-status` and `volte-status`.

The whole policy reuses the existing `next_renewal_attempt` gate and the `force_renewal` flag. The cooldown is a
far-off `next_renewal_attempt` that never passes through `schedule_renewal_retry`, so it cannot spend the exit
budget (FR-012).

## Technical Context

**Language/Version**: Rust (stable, per `rust-toolchain.toml`), workspace crate `gsm-sip-bridge`  
**Primary Dependencies**: existing only (serde for the control protocol, tracing); no XML crate, since the
hand-rolled reginfo scanner is extended instead  
**Storage**: N/A. The cooldown is in-memory and does not survive an agent restart (spec assumption).  
**Testing**: `cargo test` via `make test`, and `make lint` (clippy `-D warnings` over the whole workspace, tests included)  
**Target Platform**: Linux container (Alpine runtime), x86_64 and arm64  
**Project Type**: single Rust workspace; one binary hosts the VoWiFi and VoLTE IMS agents  
**Performance Goals**: no change to the per-NOTIFY cost (a linear scan of a document of a few kB)  
**Constraints**: identical behaviour on VoWiFi and VoLTE (FR-018), since both run `ims::agent::dispatch_loop`. Wire
compatible: the new status field is `#[serde(default)]`. No new alert or metric.  
**Scale/Scope**: 1–N lines per container; about 8 source files plus docs

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Integration-First Testing | PASS | Attribution and event selection are tested on real reginfo bodies, through the real classification function (`classify_reg_notify`) that `handle_notify` delegates to, as the existing `find_own_contacts` tests do. The policy is tested on the real `LoopState` with injected `Instant`s. No new mocks. The live gate (quickstart Q1/Q2) covers the carrier behaviour that CI cannot reach. |
| II. Green-on-Commit | PASS | Each task group ends with `make format && make lint && make test` before its commit. |
| III. Frequent Atomic Commits | PASS | One commit each for: attribution, event parsing, policy, call teardown on `rejected`, config, status, docs. |
| IV. Makefile-Driven Build | PASS | No new tooling. |
| V. Simplicity | PASS | No new timer or loop branch, because the cooldown reuses `next_renewal_attempt`. One new enum, one return struct, one `LoopState` field, one config key. |

Post-design re-check: still PASS, and Complexity Tracking is empty.

## Project Structure

### Documentation (this feature)

```text
specs/089-reg-notify-binding-attribution/
├── spec.md
├── plan.md                 # this file
├── research.md             # R1–R10
├── data-model.md
├── quickstart.md           # Q0 CI, Q1/Q2 live gates, Q3 status
├── contracts/
│   ├── observability.md    # log fields, status lines
│   └── config.md           # reg_rejected_cooldown_sec
├── checklists/requirements.md
└── tasks.md                # /speckit-tasks
```

### Source Code (repository root)

```text
gsm-sip-bridge/src/
├── ims/
│   ├── session.rs          # ReginfoContact{addr,retry_after,registration_state}, OwnContacts.retired,
│   │                       # address-first find_own_contacts, DeregEvent, Deregistration, classify_reg_notify, handle_notify → Option<Deregistration>
│   ├── mod.rs              # RegistrationStatus.rejected_until
│   └── agent/
│       ├── mod.rs          # LoopState.rejection, on_network_deregistration(event policy),
│       │                   # handle_reg_notify(&mut session, call/origination teardown), DispatchParams.reg_rejected_cooldown,
│       │                   # status reply field
│       └── origination.rs  # fail_origination (CallFailed + per-step teardown)
├── vowifi/
│   ├── control.rs          # RegistrationStatusReply.rejection_cooldown_until; reason::REGISTRATION_REJECTED
│   └── mod.rs              # vowifi-status prints rejection_cooldown
├── volte/
│   ├── bridge.rs           # volte-status prints rejection_cooldown
│   └── carrier_agent.rs    # pass the resolved per-line cooldown
└── config/
    ├── raw.rs              # [vowifi]/[volte]/[[*.line]] reg_rejected_cooldown_sec
    ├── build.rs            # range validation, inheritance
    └── mod.rs              # VowifiConfig/VolteConfig/VolteLineOverride fields
config.toml.example, docs/configuration.md, RELEASE_NOTES.md, CHANGELOG.md
```

**Structure Decision**: single existing crate. Every change sits next to the code it replaces. No new modules.

## Complexity Tracking

None.
