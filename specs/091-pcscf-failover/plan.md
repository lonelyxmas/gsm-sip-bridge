# Implementation Plan: P-CSCF pool and failover

**Branch**: `091-pcscf-failover` | **Date**: 2026-10-14 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/091-pcscf-failover/spec.md` (GitHub #124, PR 3 of 3; builds on 089 and 090)

## Summary

Today the supervisor reduces each negotiation's P-CSCF list to one address (`pick_stable_pcscf`) and writes it to
`/tmp/pcscf-N`. The agent reads that file once (`EpdgTransport::prepare`) and sends every REGISTER there for the
life of the process.

The plan keeps that file and its "P-CSCF changed" semantics exactly as they are, and adds three things beside it.

1. **Publish the pool.** The supervisor writes the latest negotiation's full list to `/tmp/pcscf-N.pool`, at every
   point it writes the selection today. It also writes it on any steady-state tick where the *set* changed but the
   selection did not; that tick neither restarts nor moves the line (R1, R2).
2. **Choose and remember per line.** A pure, clock-injected `PcscfPool` (new `ims/agent/pcscf_pool.rs`, in the style
   of `reg_subscription.rs`) does three jobs:
   - It orders the candidates: the last P-CSCF that registered successfully, then the supervisor's selection, then
     the rest of the pool, IPv4 before IPv6.
   - It holds the unavailability marks: RFC 5626 §4.5 delay + 300 s, or `Retry-After`.
   - It picks the target for each attempt.

   It persists to `/tmp/pcscf-N.state`, so marks and the last working P-CSCF survive an agent restart (R3, R6).
3. **Act only on failures the P-CSCF caused.** These count:
   - a REGISTER transaction timeout;
   - a connect refused or connect timeout;
   - a send failure on a new connection;
   - spec 090's qualifying endpoint failure;
   - the Gm keepalive escalation;
   - a `503` whose `Retry-After` exceeds 128 s.

   Each one marks the P-CSCF the failing session actually used, and only if that is still the active one. A route
   probe must also find a route (R4, R5). Errors meaning "network unreachable" or "no route", attachment-down
   failures, other 4xx–6xx responses, reg-event deregistrations and bare peer closes never mark.

   After a mark, the next attempt goes **immediately** to the next unmarked candidate, still counted against the
   existing renewal budget. Only when every candidate is marked does the existing ladder run, each step targeting
   the soonest-expiring mark (R7, R8).

The supervisor's own reachability check (`tick_steady_state` → `PcscfUnreachable`, a ~90 s tunnel rebuild) is changed
to ask "does *any* pool member answer TCP?" rather than "does the selection?". Otherwise a dead A with a working B
would rebuild the tunnel before the agent could fail over. When every member is dead, the check still rebuilds, and
that rebuild is the spec's "rediscovery" (R9).

Status replies gain the active P-CSCF, the pool and the marks; `vowifi-status` and `volte-status` print them (R10).
VoLTE gets the same machinery over a one-element pool built from its configured or discovered P-CSCF (R11).

## Technical Context

**Language/Version**: Rust (stable, per `rust-toolchain.toml`), workspace crate `gsm-sip-bridge`  
**Primary Dependencies**: existing only (serde/serde_json for the state file and control protocol, tracing, chrono for rendering)  
**Storage**:
- Two small per-line files under `/tmp`, beside the existing `/tmp/pcscf-N`:
  - `.pool`: written by the supervisor; one address per line.
  - `.state`: written by the agent; JSON.
- Both are written atomically (write to a temp file, then rename).
- Neither file needs to survive a container recreate or a host reboot (spec assumption).

**Testing**: `cargo test` via `make test`; `make lint` (clippy `-D warnings`, whole workspace including tests)  
**Target Platform**: Linux container (Alpine runtime), x86_64 and arm64  
**Project Type**: a single Rust workspace; the supervisor and both IMS agents live in one binary  
**Performance Goals**:
- At most one extra small file read per registration attempt, and one write per mark or success.
- The supervisor's reachability probe tries the selection first. It probes other pool members only when the
  selection fails, so a healthy steady state costs exactly what it does today.

**Constraints**:
- The single-selection file, `pick_stable_pcscf` and `PcscfChanged` are unchanged (FR-004).
- Wire compatible: the new status-reply fields are `#[serde(default)]`.
- No new config key, alert or metric.
- Identical on VoWiFi and VoLTE: both run `ims::agent::serve_inbound`/`dispatch_loop`.
- The startup wait (R8) must keep the at-stall watchdog fed.

**Scale/Scope**: 1–N lines per container, pools of 1–4 addresses; about 12 source files plus docs

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. Integration-First Testing | PASS | `PcscfPool` is tested with injected `SystemTime`s over real temp-dir state files. Failure classification is tested on real `io::Error`s: a connect to a closed local port gives a real `ECONNREFUSED`, and a REGISTER to a silent local UDP socket gives a real timeout. Only `ENETUNREACH`, which cannot be produced in CI, is built with `from_raw_os_error`. Renewal failover is tested end to end through `attempt_renewal` against two local fake P-CSCF sockets. Supervisor publication reuses the existing `MockCommandRunner`, whose justification is already documented. Live gate Q2 covers the carrier. |
| II. Green-on-Commit | PASS | Each task group ends with `make format && make lint && make test` before its commit. |
| III. Frequent Atomic Commits | PASS | One commit per step: pool extraction, pool publication, supervisor reachability, `PcscfPool`, failure classification, startup selection, renewal failover, failure-source wiring, status, docs. |
| IV. Makefile-Driven Build | PASS | No new tooling. |
| V. Simplicity | PASS, with one justified addition | There is one new module (see Complexity Tracking). Everything else reuses existing pieces: `latest_pcscf_batch`, `next_backoff`, `schedule_renewal_retry`, `on_qualifying_endpoint_failure`, `MaintenancePolicy` and the status-reply pattern from 090. Classification is a text heuristic in the style of `map_registration_error`, not a refactor of `register_session`'s error type (R4). |

Post-design re-check: still PASS.

## Project Structure

### Documentation (this feature)

```text
specs/091-pcscf-failover/
├── spec.md
├── plan.md                 # this file
├── research.md             # R1–R13
├── data-model.md
├── quickstart.md           # Q0 CI, Q1 local two-P-CSCF rig, Q2 live failover (blocking), Q3 status/restart
├── contracts/
│   ├── files.md            # /tmp/pcscf-N.pool and /tmp/pcscf-N.state formats and writers
│   ├── control.md          # status reply fields, CLI lines
│   └── observability.md    # log messages and fields
├── checklists/requirements.md
└── tasks.md                # /speckit-tasks
```

### Source Code (repository root)

```text
gsm-sip-bridge/src/
├── supervise/
│   ├── engines.rs          # extract_latest_pcscf_pool / extract_pcscf_pool_after (strongswan, from
│   │                       # latest_pcscf_batch); extract_swu_pcscf_pool; TunnelEngine::latest_pcscf_pool;
│   │                       # StrongswanEngine::fresh_pcscf_pool_since
│   ├── line_supervisor.rs  # TunnelEngine trait gains latest_pcscf_pool; tick_steady_state reachability
│   │                       # = selection OR any pool member (R9)
│   └── orchestrate.rs      # publish_pcscf_pool() at the 3 existing write points + on a set change each
│                           # tick (both engines); pool written before the selection
├── ims/
│   ├── transport.rs        # EpdgTransport reads .pool beside the selection; ImsTransportHandle.pool
│   ├── mod.rs              # RegistrationStatus.pcscf (snapshot); classify_register_failure; retry_after()
│   │                       # header helper; ImsRegisterConfig::with_pcscf
│   ├── session.rs          # attempt_renewal → RenewalFailure { error, status, retry_after }
│   └── agent/
│       ├── pcscf_pool.rs   # NEW: PcscfPool, Mark, Selection, SelectReason, FailureCause, PersistedPool;
│       │                   # ordering, marks, RFC 5626 delay, persistence
│       ├── mod.rs          # InboundParams.pcscf_pool; serve_inbound tries candidates (R8); LoopState.pool;
│       │                   # on_idle_tick renews via pool.select (R7); on_qualifying_endpoint_failure and
│       │                   # Gm escalation mark (R5); sync_shared_status + status reply fields
│       └── ping.rs         # probe_gm_connection reports the escalation it raised
├── vowifi/
│   ├── control.rs          # RegistrationStatusReply.pcscf_active / pcscf_pool / pcscf_unavailable
│   └── mod.rs              # vowifi-status prints the P-CSCF lines; format_pcscf_lines
└── volte/
    ├── carrier_agent.rs    # one-element pool from the configured/discovered P-CSCF; state path
    └── bridge.rs           # volte-status prints the P-CSCF lines
RELEASE_NOTES.md, CHANGELOG.md, docs/ (vowifi status / troubleshooting section)
```

**Structure Decision**: the single existing crate. One new sibling module beside `reg_subscription.rs`. Every other
change sits next to the code it extends.

## Complexity Tracking

| Addition | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| New module `ims/agent/pcscf_pool.rs` | Ordering, marks with growing delays, persistence across restarts, and stale-event rejection, all of which must be testable without a socket or a real clock (R6). | Fields on `LoopState` would split the logic between startup (`serve_inbound`) and renewal (`on_idle_tick`), both of which must make the same choice. `LoopState` is already 5,800 lines, and `reg_subscription.rs` is the precedent. |
| A supervisor-written `.pool` file beside the selection file | The agent needs the whole list (FR-001). Changing the selection file's format would break `healthcheck` and the keepalive loop, which read it as one address. | Putting the list inside the selection file would change a contract other readers depend on (FR-004). |
| An agent-written `.state` file | Marks and the last working P-CSCF must survive an agent restart (FR-015, US4). The agent is a separate process that the supervisor restarts. | In-memory state is lost on every restart. Asking the supervisor to hold the state would need a new control-channel message, and that is more moving parts. |
