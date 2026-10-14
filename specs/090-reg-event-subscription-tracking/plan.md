# Implementation Plan: Reg-event SUBSCRIBE transaction and dialog tracking

**Branch**: `090-reg-event-subscription-tracking` | **Date**: 2026-10-14 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/090-reg-event-subscription-tracking/spec.md` (GitHub #124, PR 2 of 3; builds on 089, and 091 builds on this)

## Summary

Today the reg-event SUBSCRIBE is sent and forgotten. Its response is logged as "outside an active transaction".
Nothing notices if it goes unanswered. Every NOTIFY is answered `200` and acted on, whichever subscription it
belongs to.

The plan adds a pure, clock-injected state machine, `RegSubscription` (new `ims/agent/reg_subscription.rs`, in the
style of `ping.rs`). `LoopState` owns it, and the dispatch loop performs the I/O it asks for.

1. **Track** the subscription per line: its identifiers, the network tag, route set, expiry and confirmation
   state. Superseded subscriptions are remembered until their expiry plus 128 s, at most 32 per line.
2. **Correlate** every SUBSCRIBE response by method, Call-ID and CSeq before the keepalive matcher sees it. Act
   on 2xx, 423, 489, 503 and other errors.
3. **Run the timers.**
   - **Timer F** (128 s) leads to an immediate confirming Gm probe, sent even mid-call. If the probe goes
     unanswered, that is a qualifying endpoint failure: `force_renewal` is set, bypassing the rebuild ladder, and
     status reads `Failed`. If it is answered, it is a subscription failure on backoff.
   - **Timer N** (128 s, first NOTIFY) failing is a subscription failure only.
4. **Match NOTIFYs** by Call-ID and our tag before answering. A NOTIFY for the current subscription gets `200`
   and spec 089's policy. One for a superseded subscription gets `200` plus a log line. Anything else gets `481`.
   `Subscription-State` reasons are honoured.
5. **Refresh** within the dialog at `expires − renewal_headroom_for(granted, 300 s)`. Retries use a separate
   subscription backoff of 5 s doubling to 600 s.
6. **Report** subscription health in `vowifi-status` and `volte-status`, beside registration and Gm transport
   health. It does not change `can_answer`.

## Technical Context

**Language/Version**: Rust (stable, per `rust-toolchain.toml`), workspace crate `gsm-sip-bridge`  
**Primary Dependencies**: existing only (serde for the control protocol, tracing, chrono for rendering)  
**Storage**: N/A. All state is in-memory per agent process (spec assumption).  
**Testing**: `cargo test` via `make test`, and `make lint` (clippy `-D warnings` over the whole workspace, tests included)  
**Target Platform**: Linux container (Alpine runtime), x86_64 and arm64  
**Project Type**: a single Rust workspace; one binary hosts the VoWiFi and VoLTE IMS agents  
**Performance Goals**: one extra state-machine `poll` per idle tick (1 s idle, 100 ms in a call). Steady state is
one SUBSCRIBE per registration; the worst case is one per 600 s backoff interval plus one per Timer F.  
**Constraints**:
- Identical on VoWiFi and VoLTE (FR-021): both run `ims::agent::dispatch_loop`.
- Wire compatible: the new reply fields are `#[serde(default)]`.
- No new config, alert or metric.
- `respond_on_client` behaviour is unchanged.  
**Scale/Scope**: 1–N lines per container; about 7 source files plus docs

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Integration-First Testing | PASS | The state machine is tested on real `SipResponse`/`SipRequest` parses with injected `Instant`s. `LoopState` paths are tested over the existing real UDP socket pair (`udp_sink_pair`). No new mocks. Live gate Q1 covers carrier behaviour. |
| II. Green-on-Commit | PASS | Each task group ends with `make format && make lint && make test` before its commit. |
| III. Frequent Atomic Commits | PASS | One commit per step: sender refactor, response correlation, Timer F/probe, Timer N, NOTIFY matching, Subscription-State and refresh, status, docs. |
| IV. Makefile-Driven Build | PASS | No new tooling. |
| V. Simplicity | PASS, with one justified addition | There is one new module (see Complexity Tracking). Otherwise it reuses `next_backoff`, `renewal_headroom_for`, `uac_route_set`/`route_set_in_order`, `send_gm_ping`, `force_renewal` and `MaintenancePolicy`. There is no new thread, config key or abstraction for spec 091 beyond a named method and log line. |

Post-design re-check: still PASS.

## Project Structure

### Documentation (this feature)

```text
specs/090-reg-event-subscription-tracking/
├── spec.md
├── plan.md                 # this file
├── research.md             # R1–R13
├── data-model.md
├── quickstart.md           # Q0 CI, Q1 live steady state (blocking), Q2 dead path, Q3 status
├── contracts/
│   ├── observability.md    # log messages and fields
│   └── control.md          # status reply fields, CLI lines
├── checklists/requirements.md
└── tasks.md                # /speckit-tasks
```

### Source Code (repository root)

```text
gsm-sip-bridge/src/
├── ims/
│   ├── session.rs          # send_reg_subscribe → SentSubscribe; SubscribeParts.dialog (in-dialog refresh);
│   │                       # parse_subscription_state → SubState; handle_notify split: respond_notify + classify/log
│   ├── mod.rs              # SubscriptionHealth (+render); RegistrationStatus.reg_subscription(_last_failure)
│   └── agent/
│       ├── reg_subscription.rs  # NEW: RegSubscription, Subscription, Txn, SupersededSub, SubAction,
│       │                        # ResponseMatch, NotifyMatch, EndpointFailureCause; timers, backoff, Q4 memory
│       ├── mod.rs          # LoopState.reg_sub; first SUBSCRIBE sent from dispatch_loop (not run_inner);
│       │                   # handle_carrier_response routes SUBSCRIBE + probe responses first;
│       │                   # handle_reg_notify matches before answering; on_idle_tick polls the tracker
│       │                   # before the busy gate; on_qualifying_endpoint_failure; on_renewal_success → new sub;
│       │                   # sync_shared_status + status reply fields
│       ├── ping.rs         # PING_RESPONSE_TIMEOUT pub(super)
│       └── probe.rs        # adapt to send_reg_subscribe (fire-and-forget)
├── vowifi/
│   ├── control.rs          # RegistrationStatusReply.reg_subscription(_last_failure)
│   └── mod.rs              # vowifi-status prints the two lines
└── volte/
    └── bridge.rs           # volte-status prints the two lines
RELEASE_NOTES.md, CHANGELOG.md, docs/ (vowifi status / troubleshooting section)
```

**Structure Decision**: the single existing crate. One new sibling module beside `ping.rs`. Every other change sits
next to the code it replaces.

## Complexity Tracking

| Addition | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| New module `ims/agent/reg_subscription.rs` | About 10 fields, 3 timers, a backoff, and memory of superseded subscriptions, all of which must be testable without a socket or a real clock (research R1). | Fields on `LoopState` would add about a dozen members to an already 5,179-line file, and every timer test would need a whole `LoopState`. `ping.rs` is the precedent. |
