# Feature Specification: Rebuild the tunnel when the agent has given up on every P-CSCF

**Feature Branch**: `097-agent-pool-exhausted-rebuild`  
**Created**: 2026-10-17  
**Status**: Draft  
**Input**: User description: "Report the IMS agent's own P-CSCF verdict to the VoWiFi line supervisor so a carrier with a single P-CSCF (Vodafone) can still trigger a tunnel rebuild without the supervisor's TCP probe (#120 item 2, finding 4). Today, when the agent's Gm OPTIONS keepalive (ims/agent/ping.rs, 120s) and the escalated re-registration fail, the agent marks the P-CSCF unavailable in its pool (pcscf_pool.rs, persisted to pcscf-N.state, mark >= 330s) and, with no eligible sibling, sits in "every P-CSCF is unavailable; retrying on backoff" forever; only the supervisor can rebuild the tunnel to get a new P-CSCF, and only its probe triggers that. Signal source: the agent's persisted pool state file, read by the supervisor every steady-state tick (like line_in_call reads pcscf-N.calls), NOT the status-port StatusQuery, because the status listener only starts after the first successful registration (ims/agent/mod.rs ~898-957), so an agent that fails startup registration after a rebuild that handed back the same dead P-CSCF never answers and the supervisor would get stuck. Trigger: every address in the supervisor's current pool carries a mark the agent set after the last tunnel negotiation; add a marked_at_unix field to PersistedMark (serde default) so marks that predate the last rebuild (marks survive agent restarts and same-address rebuilds) are ignored. Only RegisterTimeout and Transport causes count; a 503 Retry-After mark does not trigger a rebuild. No strike count: a mark is already debounced (3 dead pings plus a failed REGISTER). New DegradeReason (e.g. PcscfPoolExhausted) with its own TunnelRecoveryReason wire label for the #136 recovery counter, same remedy as PcscfUnreachable (recreate_interface then terminate+reinitiate, restarts agent, renegotiates). When the agent says every P-CSCF is dead but the probe finds one answering on unprotected 5060, the agent's verdict wins. Applies to both tunnel engines (strongswan 30s tick, swu 5s tick). Multi-P-CSCF carriers (Jio, 4 P-CSCFs) must keep failing over inside the agent without a rebuild. The supervisor's P-CSCF probe stays in place; removing it and DPD tuning (#120 item 3) are out of scope. Expected worst-case detection without the probe ~200s (120s ping + 3x10s + ~5s TCP connect timeout per pool member + <=30s tick) vs ~90-120s for the probe — accepted trade-off, to be confirmed live. Live verification: blackhole the Vodafone P-CSCF with a /32 route to the veth peer (not a blackhole route, which reads as no-route and never marks), once with a throwaway probe-off build to prove the agent path recovers alone, once with the probe on to see which reason fires first; and confirm Jio single-P-CSCF blackhole still fails over without a rebuild."

## Background

Each VoWiFi line has two processes that care whether the P-CSCF is alive:

- **The IMS agent** keeps the registration. While idle, it sends an `OPTIONS` keepalive over the protected Gm connection every 120s. Three unanswered keepalives (about 150s worst case) escalate to a re-registration. Any failure the agent can blame on the P-CSCF marks that P-CSCF unavailable in its pool, for at least 330s (RFC 5626 backoff plus 5 minutes, spec 091). The failures are a dead keepalive, a REGISTER timeout, a failed connect or send, or a spec 090 qualifying endpoint failure. The agent then fails over to the next unmarked P-CSCF straight away. The marks are saved to the line's pool state file and survive agent restarts.
- **The line supervisor** owns the tunnel. Only a tunnel rebuild gets a new P-CSCF list from the carrier. Today the only thing that triggers a rebuild when the P-CSCF is dead is the supervisor's own TCP probe of the P-CSCF's unprotected port 5060 (`PcscfUnreachable`, ~90s window).

#120 is working towards removing that probe. The soak's finding 4 showed what is lost without it:

- **Jio, 4 P-CSCFs, probe off:** the agent marked the dead P-CSCF and failed over to a sibling. Registered again in 193s, with no rebuild.
- **Vodafone, 1 P-CSCF, probe off:** the agent marked its only P-CSCF and logged "every P-CSCF is unavailable; retrying on backoff". It stayed that way, because nothing told the supervisor and only a rebuild could help. **It never recovered.**
- **Vodafone, probe on:** `PcscfUnreachable` rebuilt the tunnel until the carrier handed out a different P-CSCF. Back in 314s.

The agent already reaches the right verdict. This feature lets the supervisor act on it. It is the second of the three replacements #120 requires before the probe can go. The first, the per-tick route check, merged in PR #136. The probe itself stays in place for now.

**Why the pool state file and not the agent's status port.** The status listener starts only after the agent's first successful registration. Take a rebuild that hands back the same dead P-CSCF: the restarted agent fails its startup registration, exits and is respawned, so it never answers a status query. A supervisor listening on the status port would see silence and stop rebuilding. This is exactly the case the probe handles today ("until the carrier handed out a different P-CSCF"). The pool state file is written on every mark, in both the startup and the running phases.

## Clarifications

### Session 2026-10-17

- Q: What should happen when the pool is exhausted while a call is in progress? → A: Defer the rebuild until the call ends, with no cap. Rebuild on the first tick after the call ends if the pool is still exhausted. This differs from `PcscfUnreachable`, which doesn't wait for calls.
- Q: What should happen when rebuilds keep handing back the same dead P-CSCF? → A: The loop is unbounded. There is one rebuild per fresh exhaustion, spaced only by the agent's own retry (~2–2.5 min), with no backoff and no cap. In addition, a dedicated Discord alert names the condition. None exists today: `registration_loss` fires but doesn't say why, and `tunnel_failure`'s flapping alert needs 3 rebuilds within 300s, which the ~2.5 min spacing may never reach.
- Q: What live verification is required before merge? → A: All of SC-001 (Vodafone, throwaway probe-off build, including the SC-007 alert check), SC-002 (Vodafone, probe on, record which reason fires first), SC-003 (Jio, active P-CSCF black-holed, failover with no rebuild) and SC-004 (~30 min healthy idle on each rig). Results are recorded in the feature's live-verification notes before merge.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A single-P-CSCF line recovers from a dead P-CSCF without the probe (Priority: P1)

An operator runs a Vodafone VoWiFi line. The carrier advertises only one P-CSCF. That P-CSCF stops answering while the tunnel itself stays up. The agent's keepalive notices and the agent marks the P-CSCF unavailable. On its next steady-state tick, the supervisor sees that the agent has marked every P-CSCF in the line's pool since the tunnel was last negotiated. It rebuilds the tunnel, logs a `PcscfPoolExhausted` recovery, and the line registers again through whatever P-CSCF the new negotiation brings. This works even if the supervisor's own probe would have passed, or isn't running at all.

**Why this priority**: this is the whole feature, and #120 finding 4. Without it, removing the probe leaves a single-P-CSCF line down until someone restarts it by hand.

**Independent Test**: unit tests over the steady-state tick with a fake engine and a seeded pool state file:
- every pool member marked after the last negotiation → a rebuild with the new reason;
- one member unmarked → no rebuild;
- marks older than the last negotiation → no rebuild.

Live (Vodafone rig): on a throwaway build with the probe forced to always succeed, black-hole the P-CSCF with a /32 route to the veth peer. Confirm a `PcscfPoolExhausted` rebuild and a fresh registration.

**Acceptance Scenarios**:

1. **Given** a structurally healthy line whose pool has one address, which the agent marked unavailable after the current tunnel was negotiated, **When** the steady-state tick runs, **Then** the tunnel is rebuilt with the same remedy as `PcscfUnreachable` (interface recreated, then terminate and re-initiate), and the tick reports `PcscfPoolExhausted`.
2. **Given** the same state, but with the probe finding the P-CSCF (or a sibling) answering on its unprotected port, **When** the tick runs, **Then** the outcome is the same as scenario 1. The agent's verdict wins.
3. **Given** a rebuild that hands back the same dead P-CSCF, **When** the restarted agent fails its startup registration through it and marks it again, **Then** the supervisor rebuilds again on a later tick. The agent answering on its status port isn't needed for this. The cycle stops as soon as a registration succeeds.
4. **Given** a `PcscfPoolExhausted` rebuild, **When** the metrics endpoint is scraped, **Then** that line's recovery counter for `pcscf_pool_exhausted` has gone up by exactly one.

---

### User Story 2 - A multi-P-CSCF line keeps failing over inside the agent, with no rebuild (Priority: P1)

An operator runs a Jio VoWiFi line. The carrier advertises 4 P-CSCFs, and the one in use stops answering. The agent marks it and registers through a sibling, exactly as it does today. The supervisor doesn't rebuild the tunnel, because at least one pool member is still unmarked.

**Why this priority**: a rebuild drops the bearer and every other line's view of it for ~20s. Spec 091's whole point was that a dead P-CSCF on a multi-P-CSCF carrier costs no rebuild. This feature must not undo that.

**Independent Test**: unit tests where some, but not all, pool members are marked → no rebuild, whatever the probe says. Live (Jio rig): black-hole the active P-CSCF and confirm failover to a sibling with no new IKE_SA and no `PcscfPoolExhausted`.

**Acceptance Scenarios**:

1. **Given** a 4-member pool with 1, 2 or 3 members marked since the last negotiation, **When** the tick runs, **Then** no rebuild is caused by this feature.
2. **Given** a 4-member pool in which all 4 have been marked since the last negotiation (every advertised P-CSCF is dead, or the path to all of them is), **When** the tick runs, **Then** the tunnel is rebuilt as `PcscfPoolExhausted`.
3. **Given** a member the agent marked and later registered through successfully (which clears its mark), **When** the tick runs, **Then** that member counts as unmarked.

---

### User Story 3 - Everything else in the steady-state tick behaves as before (Priority: P1)

The same operator sees no other change: structural faults, P-CSCF changes, the route repair and the probe's own `PcscfUnreachable` rebuild all behave and are timed as before this feature.

**Why this priority**: the new check joins a tick that already guards a dozen recoveries. It must not re-order or hide any of them.

**Independent Test**: the existing steady-state test suite passes unchanged, and new tests pin the ordering cases below.

**Acceptance Scenarios**:

1. **Given** any structural fault, a P-CSCF change, or a missing default route, **When** the tick runs, **Then** that existing outcome is returned first, and the new check doesn't run on that tick.
2. **Given** a pool not exhausted by the agent and a probe that keeps failing, **When** the unreachable window fills, **Then** `PcscfUnreachable` fires exactly as today.
3. **Given** a missing, unreadable or unparseable pool state file, or one from an older agent whose marks carry no marking time, **When** the tick runs, **Then** the new check never triggers, and the tick goes on to the probe as before.

---

### User Story 4 - The operator is told on Discord when a line is stuck on a dead P-CSCF pool (Priority: P2)

An operator with Discord alerts configured gets one alert when a line has kept rebuilding as `PcscfPoolExhausted` for longer than the alert threshold. The alert names the line, its phone number and the dead P-CSCF(s), and says that the carrier hasn't handed out a working alternative. When the line registers again, the operator gets one all-clear. Repeated rebuilds within the same episode send nothing more.

**Why this priority**: the rebuild loop is unbounded (Clarifications Q2). On a carrier that keeps returning the same dead P-CSCF, the line stays down until the carrier fixes it. The operator needs to know, and to know that the cause is the carrier's P-CSCF rather than the bridge. P2 because recovery itself (Story 1) works without it.

**Independent Test**: unit tests over the alert decision. An episode shorter than the threshold sends nothing. An episode longer than the threshold sends exactly one failure, however many rebuilds it contains. A successful registration ends the episode and sends exactly one all-clear. A category disabled in config sends nothing. Live: during SC-001, with a test webhook, exactly one alert and one all-clear arrive.

**Acceptance Scenarios**:

1. **Given** a line whose first `PcscfPoolExhausted` rebuild was longer ago than the category's threshold (default 300s) and which hasn't registered since, **When** the threshold is crossed, **Then** one Discord alert is sent in the `pcscf_pool_exhausted` category, naming the line, its phone number (or `unknown`), the exhausted P-CSCF address(es) and the rebuild count so far.
2. **Given** that alert has been sent, **When** further `PcscfPoolExhausted` rebuilds happen in the same episode, **Then** no further alert is sent. They are counted as suppressed, like other categories.
3. **Given** an alerted episode, **When** the line's agent registers successfully, **Then** one all-clear is sent and the episode ends. A later exhaustion starts a new episode.
4. **Given** an episode that ends (the line registers) before the threshold, **When** the threshold would have been crossed, **Then** nothing is sent.
5. **Given** `[alerts.pcscf_pool_exhausted] enabled = false`, or no webhook configured, **When** an episode crosses the threshold, **Then** nothing is sent, consistent with the other categories.

---

### Edge Cases

- **Stale marks from before the last negotiation.** Marks survive agent restarts, and a rebuild that returns the same address keeps them. Only marks the agent set or renewed after the supervisor's most recent negotiation of this line's tunnel count, so a single episode cannot cause back-to-back rebuilds on old evidence.
- **Expired marks.** A mark whose end time has passed doesn't count, matching the agent's own view of availability.
- **A 503 with `Retry-After`.** That is the P-CSCF asking to be left alone for a while, not failing. A `retry-after` mark doesn't count towards exhaustion. The other mark causes all do: `gm-keepalive`, `register-timeout`, `transport` and `endpoint-failure:*`.
- **Failures the agent doesn't blame on the P-CSCF.** A missing route, a SIM or AKA fault, or a final response other than 503 leaves no mark, so it can't trigger a rebuild. A missing route is the per-tick route check's job (spec 096).
- **The agent's pool differs from the supervisor's.** Only addresses in the supervisor's current pool count. A mark on an address no longer advertised is ignored. If the supervisor knows no pool, only its current selection counts (for example, an engine that logs a selection but no list).
- **A call is in progress.** The rebuild is deferred while the line has an active call. The agent itself holds back its own re-registration during a call (spec 090 FR-022), and media can still be flowing even when signalling to the P-CSCF is dead. It runs on the first tick after the call ends, provided the marks still meet the condition.
- **The wall clock steps** (no RTC on the Pi; NTP correcting at boot). Marking times and negotiation times come from the same host clock. A backwards step can at worst hide one episode's marks until the agent marks again. It can't cause a rebuild that the marks wouldn't otherwise justify.
- **The rebuild hands back the same dead P-CSCF repeatedly.** Each cycle needs a new mark from the restarted agent, which waits for at most its startup backoff (≤120s) before trying. So rebuilds are spaced by the agent's own retry, not by the tick. The loop is unbounded, with no backoff or cap, like today's probe-driven loop. The operator is told through the `pcscf_pool_exhausted` alert (Story 4), not by slowing recovery down.
- **Several lines.** Each line's tick reads only its own pool state file.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The agent MUST record, for every P-CSCF mark it sets or renews, the wall-clock time at which it did so, alongside the mark's existing end time, failure count and cause. The record MUST remain readable by an agent that predates this field, and vice versa: an old record reads as having no marking time.
- **FR-002**: The supervisor MUST record, per line, the wall-clock time of the most recent tunnel negotiation it has published a P-CSCF for (initial establishment and every renegotiating recovery).
- **FR-003**: On every steady-state tick whose structural health is OK, which hasn't returned a P-CSCF change, and on which the route check (spec 096) didn't repair the route, the supervisor MUST read the line's pool state file and decide whether the pool is **exhausted**. Exhausted means every address in the supervisor's current pool (or its current selection, when no pool is known) has a mark that (a) hasn't yet ended, (b) was set at or after the time in FR-002, and (c) has a cause other than `retry-after`.
- **FR-004**: When the pool is exhausted and the line has no call in progress, the tick MUST rebuild the tunnel with the same remedy and the same conditions as `PcscfUnreachable`: recreate the interface, and only if that works, terminate and re-initiate. It MUST report a new recovery reason, `PcscfPoolExhausted`. If recreating the interface fails, it MUST report `TunUnavailable`, as `PcscfUnreachable` does.
- **FR-005**: The exhaustion decision MUST be taken before, and independently of, the P-CSCF probe. A probe success on the selection or on any sibling MUST NOT suppress it.
- **FR-006**: There MUST be no strike count or extra waiting window for exhaustion: one tick that observes it is enough. The P-CSCF-unreachable strike count MUST be reset to zero when this recovery fires.
- **FR-007**: When the pool is exhausted but a call is in progress, the tick MUST NOT rebuild, however long the call lasts (no cap). It MUST log, once per episode, that the rebuild is deferred for a call, and re-evaluate on every later tick. The first tick after the call ends on which the pool is still exhausted MUST rebuild. The deferral applies only to `PcscfPoolExhausted`. The probe's `PcscfUnreachable` rebuild keeps its current behaviour of not waiting for calls.
- **FR-008**: A missing, unreadable or unparseable pool state file, or one whose marks carry no marking time, MUST count as not exhausted. The check MUST NOT fail the tick.
- **FR-009**: `PcscfPoolExhausted` MUST restart the line's IMS agent, count as a renegotiation (wait for a fresh P-CSCF batch, and count towards the flapping window), and carry its own label, `pcscf_pool_exhausted`, on the per-line, per-reason recovery counter from spec 096.
- **FR-010**: The check MUST NOT add any traffic on the tunnel or towards the carrier. It only reads a local file.
- **FR-011**: The behaviour applies to both tunnel engines, each on its own steady-state cadence. The probe, its sibling logic and its `PcscfUnreachable` timing MUST be unchanged.
- **FR-012**: The rebuild MUST be logged with the pool, each member's mark cause and end time, and the negotiation time it was compared against, so an operator can tell an agent-driven rebuild from a probe-driven one.
- **FR-013**: `PcscfPoolExhausted` rebuilds MUST NOT be throttled, backed off or capped. Every tick that observes a fresh exhaustion (FR-003) with no call in progress rebuilds.
- **FR-014**: There MUST be a new Discord alert category, `pcscf_pool_exhausted`, configured like the existing health categories (`[alerts.pcscf_pool_exhausted]` with `enabled`, `unhealthy_sec` and an optional webhook override). It MUST default to enabled with a 300s threshold, and appear in the alert metrics' category label and in the configuration docs.
- **FR-015**: An episode MUST start at a line's first `PcscfPoolExhausted` rebuild and end when that line's agent next registers successfully. When an episode has lasted longer than `unhealthy_sec`, exactly one failure alert MUST be sent for it, carrying the line, its phone number (or `unknown`), the exhausted P-CSCF address(es) and the rebuild count so far. Exactly one all-clear MUST be sent when an alerted episode ends. Rebuilds within an already-alerted episode MUST NOT send again.
- **FR-016**: The alert MUST be independent of the existing `registration_loss` and `tunnel_failure` alerts. It doesn't replace or suppress them, and they keep firing on their own rules.

### Key Entities

- **Pool state file**: per line, written by the agent. It holds the last P-CSCF registered through and one mark per unavailable P-CSCF: address, end time, failure count, cause and (new) marking time. It survives agent restarts.
- **Last negotiation time**: per line, held by the supervisor. It is the wall-clock time of the most recent tunnel negotiation it published a P-CSCF for.
- **Exhausted pool**: the condition in FR-003. Every P-CSCF the supervisor considers current has been given up on by the agent, for a reason other than `retry-after`, since the tunnel was last negotiated.
- **`PcscfPoolExhausted`**: the new steady-state recovery reason. It shares its remedy with `PcscfUnreachable` and its metric label is `pcscf_pool_exhausted`.
- **Pool-exhaustion episode**: per line, from the first `PcscfPoolExhausted` rebuild to the agent's next successful registration. Its length, the rebuild count and the exhausted addresses drive the `pcscf_pool_exhausted` alert.

## Success Criteria *(mandatory)*

### Measurable Outcomes

SC-001 to SC-004 and SC-007 are live runs. All of them MUST pass and be recorded before merge (Clarifications Q3).

- **SC-001**: On the Vodafone rig, with the probe forced to always succeed (a throwaway build that is never merged), black-holing the only P-CSCF leads to a `PcscfPoolExhausted` rebuild within 200s of the black-hole. The line is registered again once the carrier hands out a reachable P-CSCF. In #120 run C the same fault never recovered.
- **SC-002**: On the Vodafone rig with the probe on, the same fault still recovers. The log and the recovery counter show which of `PcscfUnreachable` and `PcscfPoolExhausted` fired first. Either outcome is acceptable, and the result is recorded for the probe-removal decision.
- **SC-003**: On the Jio rig, black-holing the active P-CSCF leads to failover to a sibling with zero new IKE_SAs and zero `PcscfPoolExhausted` recoveries, as in #120 finding 4.
- **SC-004**: Over ~30 minutes of healthy idle on each rig, zero `PcscfPoolExhausted` recoveries are logged.
- **SC-005**: A healthy line sends no additional packets on the tunnel or to the carrier compared with before this feature.
- **SC-006**: Every existing steady-state and pool test passes unchanged. An agent pool state file written before this feature still loads, and its marks still steer failover.
- **SC-007**: During the SC-001 run, with a test webhook, exactly one `pcscf_pool_exhausted` failure alert arrives once the episode passes 300s, whatever the number of rebuilds. Exactly one all-clear arrives after the line registers again.

## Assumptions

- The supervisor's P-CSCF probe stays. Removing it and DPD tuning (#120 item 3) are separate, later work. This feature's counter label is what lets that decision compare agent-driven and probe-driven rebuilds.
- The expected detection time without the probe is about 200s worst case: ≤120s until the next keepalive, 3 × 10s response timeouts, ~5s per pool member for a TCP connect timeout on a dead path, and ≤30s until the next tick. With the probe it is about 90–120s. This is accepted, and SC-001 confirms it live.
- A dead data path to every P-CSCF also reaches this condition, because each REGISTER through a dead path times out and is marked. On a 4-member pool this costs roughly another 20s over the single-member case.
- Black-holing for the live tests uses a /32 route to the veth peer inside the line namespace, not a `blackhole` route. A `blackhole` route makes the agent's own route check fail, so the failure is not attributed to the P-CSCF and no mark is set.
- The pool state file's location and format are the agent's (spec 091). The supervisor only reads it. Concurrent writes are atomic (write-then-rename), so a read sees either the old file or the new one.
