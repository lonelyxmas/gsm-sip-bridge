# Feature Specification: Check the default route on every steady-state tick

**Feature Branch**: `096-per-tick-route-check`  
**Created**: 2026-10-17  
**Status**: Draft  
**Input**: User description: "Check the VoWiFi line's default route on every steady-state supervisor tick, not only after the P-CSCF TCP probe fails (#120 finding 3). Today the DefaultRouteMissing repair (repair_default_route, family-aware since #133) runs only inside the pcscf-unreachable branch of tick_steady_state_with_pool in gsm-sip-bridge/src/supervise/line_supervisor.rs, so with the probe removed a deleted default route is never repaired (probes-off soak: line down 10 min, agent crash-looped 44 times). Move the check ahead of the probe so it is structural: runs every tick once health is Ok, skipped when no P-CSCF is known yet, repairs within one tick with no SA renegotiation, falls through to the existing probe/strike escalation when the route is present or the repair fails. Prerequisite for removing the supervisor's 30s probe later; agent-liveness reporting and DPD tuning are out of scope."

## Background

Each strongSwan VoWiFi line runs a steady-state supervisor tick every 30 seconds. After the structural checks pass (charon alive, VICI answering, tunnel interface present, IKE_SA and CHILD_SA installed, rekey not stuck), the tick opens a TCP connection to the P-CSCF's port 5060 inside the line's namespace. **Only when that probe fails** does the tick ask whether the namespace has lost its default route through the tunnel. If the route is gone, the tick reinstates it in milliseconds and reports `DefaultRouteMissing`, with no renegotiation. If the route is present, the tick counts a strike, and after about 90s rebuilds the tunnel as `PcscfUnreachable`.

A missing default route is a known, repeated failure. It caused the 6-hour outage on 2026-08-19. It is also the one fault where every structural check passes while nothing can be reached. #133 (merged in PR #135) made the route check look at the address family of the P-CSCF the line actually uses, so an IPv6 line such as Vodafone IN no longer mistakes a surviving IPv4 route for a healthy one.

Issue #120 aims to remove the supervisor's P-CSCF probe, which looks unlike a handset and keeps strongSwan's DPD quiet. Its probes-off soak (run C) showed what happens to the route repair when the probe goes. The test deleted the netns default route on both carriers. **Nothing repaired it.** The line stayed down until the test put the route back by hand after 10 minutes, and on Vodafone the IMS agent crash-looped 44 times in that window. The repair was reachable only through the probe's failure.

This feature makes the route check structural. It runs on every tick in its own right, so it doesn't depend on the probe. It is the first of the three replacements #120 requires before the probe can be removed. The probe itself stays unchanged for now.

## Clarifications

### Session 2026-10-17

- Q: What happens to the IMS agent after a route repair, now that it can be caught within one tick? → A: Unchanged: `DefaultRouteMissing` still restarts the agent, as today. A call in progress is dropped and the line re-registers. Leaving the agent alone is deferred until the supervisor gets the agent's liveness (#120 item 2).
- Q: What live verification is required before merge? → A: On both rigs (Vodafone, IPv6 Gm; Jio Pi, IPv4 Gm): delete the route and see `DefaultRouteMissing` within ≤30s with no new IKE_SA, then ~30 min of healthy idle with zero `DefaultRouteMissing`. In addition, on one rig, a throwaway build with the P-CSCF probe forced to always succeed must still repair a deleted route within one tick.
- Q: What should happen when a line's route keeps disappearing, so that every tick repairs it and restarts the agent? → A: Keep today's behaviour: repair it and restart the agent on every tick, with no new escalation. Expose how many repairs have been done as a metric, so an operator can see it happening.
- Q: Should the metric count only route repairs, or every supervisor recovery? → A: Every supervisor recovery, labelled by reason. A counter per line and per reason, with the reason drawn from a closed set that mirrors the supervisor's recovery reasons.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A lost default route is repaired on the next tick, whatever the probe says (Priority: P1)

An operator runs VoWiFi lines with default configuration. Something removes a line's default route through its tunnel: an address teardown, a WAN blip, or an operator's mistake. The tunnel, the SA and the P-CSCF are otherwise healthy. On its next steady-state tick, the supervisor notices the route is missing, puts it back, and logs a `DefaultRouteMissing` recovery. It doesn't rebuild the tunnel. The line is reachable again within one tick, and that happens whether the P-CSCF probe would have passed, failed, or not run at all.

**Why this priority**: This is the whole feature, and finding 3 of #120. Without it, removing the probe brings back the class of outage seen on 2026-08-19.

**Independent Test**: Unit tests over the steady-state tick with a fake engine:
- The route is missing and the P-CSCF still answers. Today this case is never repaired. It must now be repaired.
- The route is missing and the P-CSCF does not answer. This is repaired today, and must still be.
- In both cases, no terminate, re-initiate or interface recreation happens, and the strike counter stays at zero.

Live: on a registered line, delete the default route in the line's namespace and confirm a `DefaultRouteMissing` recovery within 30 seconds, with no new IKE_SA in charon's log.

**Acceptance Scenarios**:

1. **Given** a line whose structural health is OK and whose default route, in the current P-CSCF's address family, is missing, **When** the steady-state tick runs, **Then** the route is reinstated, the tick reports `DefaultRouteMissing`, and no terminate, re-initiate or interface recreation is issued.
2. **Given** that same state, but with the P-CSCF probe answering (for example over a route that survived in the other family, or a probe that has been removed), **When** the tick runs, **Then** the outcome is the same as scenario 1. The probe's result doesn't decide whether the route is checked.
3. **Given** a route repair that succeeds, **When** the tick returns, **Then** the line's P-CSCF-unreachable strike count is zero.
4. **Given** a line with the route present, **When** the tick runs, **Then** the route check changes nothing, and the tick goes on to the P-CSCF probe and the strike logic exactly as before this feature.
5. **Given** an IPv6-P-CSCF line (Vodafone IN) whose IPv6 route is gone while its IPv4 route survives, **When** the tick runs, **Then** the IPv6 route is repaired (the #133 behaviour, now reached without a probe failure).

---

### User Story 2 - Everything else in the steady-state tick behaves as before (Priority: P1)

The same operator sees no other change in how lines recover. A dead process is still restarted, a vanished interface is still recreated, a lost IKE_SA is still re-initiated, a stuck rekey is still rebuilt, and a P-CSCF change is still picked up. A P-CSCF that is unreachable for reasons other than the route still leads to a `PcscfUnreachable` rebuild after the same ~90s window.

**Why this priority**: The route check moves to an earlier point in a tick that guards several recoveries. Moving it must not hide or re-order any of them.

**Independent Test**: The existing steady-state test suite passes unchanged, except for the one test that asserted the old design (that a reachable line is never checked for its route). New tests pin the cases below.

**Acceptance Scenarios**:

1. **Given** any structural fault (process died, VICI broken, interface vanished, IKE_SA missing, CHILD_SA missing, rekey stuck), **When** the tick runs, **Then** that fault's existing recovery is taken first, and the route check doesn't run on that tick.
2. **Given** the connection now reports a different P-CSCF, **When** the tick runs, **Then** it returns `PcscfChanged` as before, and the route check doesn't run on that tick.
3. **Given** the route is present and the P-CSCF (and every pool sibling) is unreachable, **When** enough ticks pass to fill the existing unreachable window, **Then** the tunnel is rebuilt as `PcscfUnreachable`, exactly as today.
4. **Given** the route repair runs but the route still isn't there afterwards, **When** the tick goes on, **Then** the failed repair isn't reported as a recovery. The tick falls through to the probe and strike logic, so a line that can't be repaired this way still gets rebuilt.
5. **Given** a line using the swu engine, **When** the tick runs, **Then** nothing changes. That engine manages its own routing and has no route repair.

---

### User Story 3 - An operator can see how often each line has been repaired, and why (Priority: P2)

An operator watching the bridge's metrics endpoint can see how many times the supervisor has recovered each VoWiFi line, broken down by the reason for each recovery: route repair, unreachable-P-CSCF rebuild, IKE_SA loss, CHILD_SA loss, stuck rekey, dead process, and so on. A line whose route keeps disappearing shows up as a rising route-repair count, instead of only appearing in the logs.

**Why this priority**: a repeated route repair restarts the agent on every tick (Clarification Q3) and doesn't count as flapping. Without a counter, that state can only be seen by reading logs. P2 because Story 1 is safe without it.

**Independent Test**: unit tests over the report ingest. A supervisor report carrying a recovery event increments the counter for that line and reason by one. A report with no events leaves it unchanged. A restart of the reporting process doesn't rewind it. Live: after the route-delete test, the metrics endpoint shows the line's route-repair count one higher than before.

**Acceptance Scenarios**:

1. **Given** a strongSwan line, **When** the supervisor reports any `Recovered{reason}` outcome, **Then** that line's recovery counter for `reason` goes up by exactly one.
2. **Given** a line whose route is repaired on three consecutive ticks, **When** the metrics are scraped, **Then** that line's route-repair count reads three more than before.
3. **Given** a healthy line (`StillUp` and `PcscfChanged` only), **When** ticks run, **Then** none of its recovery counters change.
4. **Given** the supervisor process restarts, **When** it reports again, **Then** the counters continue from where they were. Counters only ever move forward.

---

### Edge Cases

- **No P-CSCF known yet** (empty current P-CSCF): the route check is skipped on that tick, just as the probe is. Without a P-CSCF there's no family to check, and checking both families could report a phantom missing route on a single-family line.
- **A P-CSCF address that doesn't parse**: the existing family-aware repair requires both families' routes (#133 behaviour). Since the check is now skipped for an empty P-CSCF, this only applies to a non-empty, malformed address, which charon's log isn't expected to produce.
- **The route disappears between the route check and the probe in the same tick**: the probe fails and a strike is counted. The next tick's route check repairs it and resets the strikes. That costs at most one strike, well short of the rebuild window.
- **The repair keeps failing** (for example, `ip route replace` is refused): every tick logs the attempt, finds the route still missing, and falls through to strike counting. The ~90s `PcscfUnreachable` rebuild still eventually runs, as today.
- **A call is in progress when the route goes**: the repair doesn't renegotiate, so the SA survives. However, the existing agent restart on `DefaultRouteMissing` still applies, so the call is dropped and the line re-registers, exactly as when today's probe-gated repair fires.
- **The route keeps disappearing** (repaired every tick, gone again by the next): every tick repairs it and restarts the agent, as today, with no new escalation. The recovery counter (Story 3) makes the rate visible.
- **Several lines**: each line's tick checks only its own namespace and its own current P-CSCF.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: On every strongSwan steady-state tick whose structural health is OK and that hasn't returned `PcscfChanged`, the supervisor MUST check whether the line's default route through its tunnel exists, in the current P-CSCF's address family. It MUST do this **before**, and independently of, the P-CSCF reachability probe.
- **FR-002**: When that check finds the route missing and reinstates it, the tick MUST report a `DefaultRouteMissing` recovery and reset the P-CSCF-unreachable strike count to zero. It MUST NOT issue a terminate, re-initiate or interface recreation.
- **FR-003**: When the route is present, or the repair didn't bring it back, the tick MUST continue to the P-CSCF probe, sibling probing and strike escalation, with unchanged behaviour and unchanged timing (the same ~90s window before `PcscfUnreachable`).
- **FR-004**: The route check MUST be skipped on a tick where the current P-CSCF is empty.
- **FR-005**: The route check MUST NOT run on a tick that has already returned for a structural fault or a P-CSCF change. All existing recovery paths and their order MUST be unchanged.
- **FR-006**: The route check MUST NOT run a second time within the same tick. Specifically, it moves out of the unreachable branch rather than being duplicated there.
- **FR-007**: The route check MUST NOT add any traffic on the tunnel or towards the carrier. It only inspects, and when needed modifies, local routing state in the line's namespace.
- **FR-008**: Behaviour for the swu engine MUST be unchanged.
- **FR-009**: The `DefaultRouteMissing` outcome MUST keep its existing downstream meaning: it is not a renegotiation, and it does not count towards the flapping window. It still restarts the line's IMS agent, as today.
- **FR-010**: The existing route-repair log line MUST still be emitted when a route is reinstated, so an operator can tell a route repair from a rebuild.

- **FR-011**: Every steady-state `Recovered{reason}` outcome on a strongSwan VoWiFi line MUST increment a per-line, per-reason recovery counter exposed on the bridge's metrics endpoint. The reason label MUST come from a closed set that mirrors the supervisor's recovery reasons. The counter MUST only ever increase, including across a restart of the reporting process.
- **FR-012**: Repeated route repairs MUST NOT trigger any new escalation. The behaviour on each tick stays as in FR-002 and FR-009.

### Key Entities

- **Steady-state tick**: one 30s pass of a line's supervisor over its health. It returns `StillUp`, `PcscfChanged` or `Recovered{reason}`.
- **Default route (per family)**: the namespace's `default dev <tun>` route for IPv4 or IPv6. The family that matters is that of the line's current P-CSCF.
- **Recovery counter**: per VoWiFi line and recovery reason, a monotonic count of steady-state recoveries the supervisor has performed.
- **P-CSCF-unreachable strikes**: consecutive ticks on which neither the selected P-CSCF nor any pool sibling answered. Once they fill the ~90s window, the tunnel is rebuilt.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With the default route deleted from a healthy line's namespace, the line is reachable again within one steady-state interval (≤30s), on both the Vodafone (IPv6 Gm) and Jio (IPv4 Gm) rigs.
- **SC-002**: That repair causes zero new IKE_SA or CHILD_SA negotiations, so there is no tunnel rebuild. The agent restart and its re-REGISTER are expected.
- **SC-003**: With the P-CSCF probe forced to always succeed (a stand-in for its removal), a deleted default route is still repaired within one interval. In #120 run C, the same fault stayed unrepaired for 10 minutes. This is verified live on one rig with a throwaway build that is never merged, as well as in unit tests.
- **SC-004**: A healthy line sends no additional packets on the tunnel or to the carrier compared with before this feature.
- **SC-005**: Every existing steady-state recovery test passes, except for the single test that encoded "never check the route of a reachable line". That test is replaced by one asserting the opposite.
- **SC-006**: Over ~30 minutes of healthy idle on each rig, the line logs zero `DefaultRouteMissing` recoveries: the per-tick check never misfires on a line whose route is present.
- **SC-007**: After each live route-delete test, that line's route-repair count on the metrics endpoint has gone up by exactly one, and no other reason's count has moved.

## Assumptions

- The supervisor's P-CSCF probe stays in place in this feature. Removing it, reporting the agent's liveness to the supervisor (#120 item 2), and DPD tuning (#120 item 3) are separate, later work.
- One extra local `ip route show` per strongSwan line per 30s tick is an acceptable cost. It runs inside the namespace and puts nothing on the wire.
- `DefaultRouteMissing` keeps its current `recovery_restarts_agent = true` / `recovery_renegotiates = false` semantics (see Clarifications). Leaving the agent running on a route repair is deferred until #120 item 2 gives the supervisor the agent's own liveness signal.
- The family-aware repair from #133 (`repair_default_route(pcscf)`) is reused as-is. Its contract (returns `true` only if the route was missing and is now present) is unchanged.
- The recovery counter is reported by the supervisor through the existing agent-report channel to the daemon's metrics registry, as an event (a delta). The supervisor serves no metrics endpoint of its own.
- No configuration option is added. The check is always on for strongSwan lines, like the other structural checks.
- Live verification uses the existing rigs: the Vodafone rig, and the Jio line on the remote Pi. The route is deleted with `ip -n <ns> [-6] route del default dev <tun>`, as in the #120 fault tests. The forced-probe throwaway build runs on the Vodafone rig (local host, pjsip-linked build runs directly), with Jio as the fallback if that rig is unavailable.
