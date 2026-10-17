# Research: per-tick default-route check

There were no NEEDS CLARIFICATION items in Technical Context. The decisions below record the choices made.

## R1. Where in the tick the route check goes

- **Decision**: after the structural `match health` (which only falls through on `Ok(..)`) and after the `PcscfChanged` early return, immediately before `engine.pcscf_reachable(runner, current_pcscf)`.
- **Rationale**:
  - A structural fault has its own remedy, and a missing interface or SA makes the route question meaningless (FR-005).
  - `PcscfChanged` must stay ahead of anything P-CSCF-specific: the old address's family may not be the new one's. The next tick checks the new one.
  - Putting the check before the probe is what makes it independent of the probe (FR-001).
- **Alternatives considered**:
  - *Inside `steady_state_health`* (an engine-level structural check that returns a new health variant). Rejected: it would need the current P-CSCF threaded into the engine's health call, and it would change the `SteadyStateHealth` match for both engines, for a check that already has its own trait method.
  - *Keep the in-branch call as well*. Rejected (FR-006): the route was checked milliseconds earlier in the same tick.

## R2. An empty current P-CSCF

- **Decision**: skip the check when `current_pcscf.is_empty()`.
- **Rationale**: `StrongswanEngine::pcscf_reachable` already treats an empty P-CSCF as nothing to test. `RouteFamily::of_addr("")` returns `None`, and the #133 repair then requires **both** families. On an IPv6-only line that would report a phantom missing IPv4 route.
- **Alternative**: change `repair_default_route` so an empty address means "check nothing". Rejected because it changes the #133 method's contract for one caller.

## R3. Recovery-counter transport

- **Decision**: a new `ObservedEvent::TunnelRecovered { reason: TunnelRecoveryReason }`, sent by the strongSwan line's existing `child_sa_reporter` (`AgentKind::Supervisor`). It goes in the `events` of a report whose `state` is the line's current `TunnelSignal`. Ingest increments `VOWIFI_TUNNEL_RECOVERIES_TOTAL{module,reason}`.
- **Rationale**:
  - The supervisor serves no `/metrics`; the daemon does.
  - `events` are deltas by design (`metrics/ingest.rs` module docs), so a supervisor restart can't rewind the counter (FR-011).
  - Sending the full current signal as `state` keeps the reporter's latest-state heartbeat correct. A default (all-`None`) state would become the heartbeat and stop re-asserting `child_sa_up`.
- **Alternatives considered**:
  - *A gauge holding a running total in `AgentState`*. Rejected: it rewinds when the supervisor restarts.
  - *Counting in the daemon from `child_sa_up` transitions*. Rejected: most recoveries, including `DefaultRouteMissing`, never change that signal.

## R4. The reason label set

- **Decision**: `TunnelRecoveryReason` is a closed serde enum (`snake_case`) in `control/protocol.rs`, with one variant per `DegradeReason` variant (11). A total `From<DegradeReason>` lives in `line_supervisor.rs`, and `as_str()` gives the label.
- **Rationale**:
  - The wire module keeps no dependency on `supervise`, the same layering as `OutboundAttemptOutcome`.
  - Cardinality is bounded (FR-014 of specs/014).
  - A total mapping means a new `DegradeReason` fails to compile until it has a label.
  - `PcscfChanged` is included only for totality: steady-state `Recovered` never carries it, so its series simply never appears.
- **Alternative**: reuse `DegradeReason` on the wire. Rejected: it isn't serde, and it would couple the protocol to the supervisor.

## R5. Swu lines

- **Decision**: no counter for swu lines.
- **Rationale**: the swu steady loop (`orchestrate.rs` around `:2932`) has no supervisor reporter. FR-011 scopes the counter to strongSwan lines, and adding a reporter there is out of scope.

## R6. Live verification of the "probe removed" case (SC-003)

- **Decision**: a throwaway build on the Vodafone rig with `StrongswanEngine::pcscf_reachable` returning `true`, never committed. Delete the IPv6 default route and expect `DefaultRouteMissing` within one tick.
- **Rationale**: this is the closest stand-in for the probe's removal without writing the removal itself. The local host runs the pjsip-linked build directly.
