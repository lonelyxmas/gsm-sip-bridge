# Feature Specification: ePDG IKE_SA-loss recovery, post-recovery P-CSCF sync, graceful teardown and Gm SA cleanup

**Feature Branch**: `087-ike-sa-loss-recovery`  
**Created**: 2026-10-13  
**Status**: Draft  
**Input**: User description: "ePDG IKE_SA-loss recovery, post-recovery P-CSCF sync, and Gm SA leak (follow-up to #106). Scope: (1) detect a lost IKE_SA (carrier DELETE / DPD clear) separately from a CHILD_SA lost under a live IKE_SA, and recover with re-initiate plus IMS agent restart; (2) after any recovery that builds a new IKE_SA, sync the P-CSCF from the new negotiation before restarting the agent, so the next tick does not report a false P-CSCF change; (3) teardown terminates gracefully first and waits for the ePDG's DELETE acknowledgement, falling back to a bounded forced delete, with no fixed delay; (4) clear leaked Gm IPsec SAs/policies in the line namespace before each agent start. Out of scope: the 30s P-CSCF TCP probe (#120) and the carrier rekey-selector fix (#107)."

## Background

A VoWiFi line's data path is an **IKE_SA** (the control session with the carrier's ePDG) carrying a **CHILD_SA** (the ESP tunnel that carries SIP and RTP). The IKE_SA also owns the line's **inner address** (VIP) and the **P-CSCF list**: both are handed out once, when the IKE_SA is authenticated (RFC 7296 configuration payload in IKE_AUTH). A new CHILD_SA inside the same IKE_SA keeps both; a new IKE_SA usually gets a new inner address, and on some carriers (Jio) a different P-CSCF pool.

Over the IKE_SA, the IMS agent installs its own **Gm IPsec SAs** (3GPP TS 33.203 security agreement) inside the line's network namespace, bound to the inner address and per-registration ports.

Issue #106 reported a VoWiFi line looping after the carrier sent `DELETE for IKE_SA`. Triage against `main` (after #118) found the report's own diagnosis mostly wrong, but four real defects underneath it:

| # | Defect | Effect |
|---|---|---|
| 1 | A lost IKE_SA is classified as a lost CHILD_SA. That path's first step re-initiates without restarting the IMS agent, which is right for a CHILD_SA but wrong for an IKE_SA: the re-initiate builds a new IKE_SA with a new inner address. | The agent keeps a registration and Gm SAs bound to an address that no longer exists. It only notices via its keepalive ping (up to ~130s), three failed reconnects, then a re-registration. Inbound calls fail meanwhile, because the carrier's binding points at the old address. |
| 2 | After a recovery, the P-CSCF is re-read immediately, but the re-initiate runs in the background, so the read sees the **previous** negotiation's P-CSCF list. | The restarted agent registers against the old P-CSCF. On the next check (~30s later), the new list appears, is reported as a "P-CSCF change", and a healthy registration is killed. The existing code comment claims to prevent exactly this; it does not. |
| 3 | Teardown always force-deletes the IKE_SA without waiting for the ePDG to acknowledge the DELETE. #106 proposed a fixed 4s sleep before re-initiating to cover for this. | The ePDG can receive the new IKE_SA_INIT before it has processed the old DELETE. A fixed sleep guesses at a timing the ePDG can tell us directly. |
| 4 | The IMS agent is stopped by signal and never removes its Gm SAs and policies. | Every agent restart leaves its Gm SAs and policies behind in the namespace. If the same inner address and local port are reused, the new registration cannot install its own and fails. |

## Clarifications

### Session 2026-10-13

- Q: Should the IMS agent restart after every IKE_SA loss, or only when the inner address or P-CSCF changed? → A: Always. An IKE_SA deletion ends the ePDG/PGW session, so the IMS core normally drops the registration and the P-CSCF-side Gm SAs, even when the same address comes back. A conditional restart would leave the line unreachable until the agent's own ~130s+ liveness detection.
- Q: Should a flapping tunnel (rebuilt repeatedly, never continuously down for `unhealthy_sec`) raise an alert? → A: Yes, through the existing tunnel-failure alert. It fires when a line needs 3 or more IKE_SA rebuilds within `unhealthy_sec`, once per episode, with the all-clear after a full `unhealthy_sec` window without one. No new config keys, and no backoff between rebuilds.
- Q: Should the log say why the IKE_SA was lost? → A: Yes. The cause (carrier DELETE, dead-peer timeout, or unknown) goes in the recovery log line and in the tunnel-failure alert text. No new metric, and the recovery itself does not depend on the cause.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A carrier-deleted IKE_SA is rebuilt and the line re-registers on the new address (Priority: P1)

An operator runs a VoWiFi line. The carrier's ePDG deletes the whole IKE_SA (or dead-peer detection clears it). On its next health check the bridge recognises that the IKE_SA itself is gone, not just the CHILD_SA. It builds a new one and restarts the line's IMS agent, which registers from the new inner address. Incoming calls ring again without operator action.

**Why this priority**: This is the loop #106 reports, and the largest outage window of the four defects.

**Independent Test**: On a live line, delete the line's IKE_SA by hand (simulating the carrier's DELETE). Confirm the next health check reports an IKE_SA loss (not a CHILD_SA loss), the tunnel comes back, the agent is restarted exactly once and registers, and an inbound call rings.

**Acceptance Scenarios**:

1. **Given** a line with no IKE_SA at all for its connection, **When** the next health check runs, **Then** the supervisor reports an IKE_SA loss with its cause, requests a new IKE_SA, and restarts the line's IMS agent.
2. **Given** a line with a live IKE_SA but no usable CHILD_SA, **When** the next health check runs, **Then** behaviour is unchanged from #118: a CHILD_SA is re-initiated inside the IKE_SA, with no agent restart, escalating to a full rebuild on the second consecutive miss.
3. **Given** an IKE_SA that is still being set up (present but not yet established, no CHILD_SA), **When** a health check runs, **Then** it is treated as a missing CHILD_SA under a live IKE_SA (two-strike path), not as a lost IKE_SA, so an initiate already in progress is not re-fired on every check.
4. **Given** an IKE_SA loss was just recovered, **When** the following health check finds the IKE_SA and CHILD_SA present, **Then** the line is reported healthy and the missing-CHILD_SA counter is zero.
5. **Given** two lines sharing one IKE daemon, **When** only line 1's IKE_SA is lost, **Then** only line 1 is recovered and only line 1's agent restarts.
6. **Given** the IKE daemon logged that the ePDG deleted this line's IKE_SA, **When** the loss is recovered, **Then** the log line names `carrier DELETE` as the cause. If it logged a dead-peer timeout, the cause is `dead-peer timeout`. If neither appears, the cause is `unknown`.

---

### User Story 2 - A recovery never causes a false "P-CSCF changed" (Priority: P1)

After any recovery that builds a new IKE_SA, the bridge waits, for a bounded time, for the new negotiation to finish. It records the P-CSCF that negotiation advertised, and only then restarts the IMS agent. The agent registers against the right P-CSCF first time, and the next health check sees no change.

**Why this priority**: Without it, the agent restart from Story 1 is followed ~30s later by a second, self-inflicted one on any carrier that rotates its P-CSCF pool per IKE_SA. That double registration is the "false P-CSCF changed kill" in #106.

**Independent Test**: Trigger a recovery that rebuilds the IKE_SA (tunnel interface removed, or IKE_SA deleted). Confirm the log shows the P-CSCF taken from the new negotiation before the agent restart, the agent restarts once, and the next health check does not report a P-CSCF change.

**Acceptance Scenarios**:

1. **Given** a recovery that builds a new IKE_SA, **When** the new negotiation completes within the wait bound, **Then** the line's recorded P-CSCF is set to that negotiation's P-CSCF before the IMS agent is restarted.
2. **Given** a recovery that builds a new IKE_SA, **When** the new negotiation does not complete within the wait bound, **Then** the agent is restarted anyway, the timeout is logged, and the next health check's normal P-CSCF-change handling remains the fallback.
3. **Given** the recorded P-CSCF was synced from the new negotiation, **When** the next health check runs, **Then** no P-CSCF change is reported.
4. **Given** the P-CSCF list in the new negotiation is the same set as before, **When** the recovery completes, **Then** the recorded value is unchanged and no extra restart occurs.
5. **Given** a shutdown begins while the supervisor is waiting for the new negotiation, **When** the wait next checks, **Then** it stops promptly and the line shuts down normally.
6. **Given** the IKE daemon's log was rotated or truncated during the wait, **When** the supervisor looks for the new negotiation, **Then** it still finds it (it never reads a stale earlier negotiation as the new one).
7. **Given** a recovery that only re-initiates a CHILD_SA inside a live IKE_SA, **When** it completes, **Then** no wait and no agent restart happen (the P-CSCF and inner address cannot have changed).

---

### User Story 3 - Teardown waits for the ePDG's acknowledgement, with a bounded fallback (Priority: P2)

Whenever the supervisor tears down a line's IKE_SA to rebuild it, it first asks for a normal delete and waits a short, bounded time for the ePDG to acknowledge it. If the ePDG acknowledges, the old session is known to be gone and the rebuild starts at once. If it does not, the IKE_SA is force-deleted locally, as today, and the rebuild starts. No fixed sleep is added on either path.

**Why this priority**: It replaces a guessed delay with the ePDG's own confirmation. It is a robustness improvement rather than the cause of an observed outage, so it ranks below Stories 1 and 2.

**Independent Test**: On a live line, trigger an IKE_SA rebuild and confirm the log records an acknowledged delete with no forced delete. In automated tests, an ePDG that never answers leads to the forced delete within the bound.

**Acceptance Scenarios**:

1. **Given** a responsive ePDG, **When** the supervisor tears down the IKE_SA, **Then** the delete is acknowledged, no forced delete is issued, and the log says the delete was acknowledged.
2. **Given** an ePDG that does not answer the delete (or has an exchange stuck, as in #107), **When** the graceful wait bound expires, **Then** the IKE_SA is force-deleted, and the log says the ePDG did not acknowledge within the bound.
3. **Given** whether the IKE_SA is still present is the deciding signal, **When** the graceful attempt's own exit status is unreliable, **Then** the forced fallback is still taken whenever the IKE_SA remains, and skipped whenever it is gone.
4. **Given** either path, **When** teardown finishes, **Then** the rebuild is requested immediately, with no added fixed delay.
5. **Given** the worst case (no acknowledgement, slow forced delete), **When** teardown runs, **Then** it completes within its stated total bound and cannot stall the line's supervision indefinitely.

---

### User Story 4 - Restarting the IMS agent leaves no Gm IPsec state behind (Priority: P2)

Before every start of a line's IMS agent, the bridge clears any Gm SAs and policies a previous agent left in the line's namespace. The namespace then holds exactly the state of the current registration, however many times the agent has restarted.

**Why this priority**: The leak is slow and mostly harmless until an address-and-port collision makes a registration fail. Once Story 1 restarts the agent on every IKE_SA loss, restarts become more frequent, so the cleanup belongs in the same change.

**Independent Test**: Count Gm SAs in the line's namespace, force several agent restarts, and count again: the count equals one registration's worth.

**Acceptance Scenarios**:

1. **Given** Gm SAs and policies left by a previous agent, **When** the next agent starts, **Then** they are gone before it registers.
2. **Given** the clearing step, **When** it runs, **Then** it touches only this line's namespace: the line's ePDG tunnel and every other line are unaffected.
3. **Given** an agent is running and registered, **When** any supervision activity happens, **Then** its Gm state is never cleared underneath it (clearing happens only while no agent for that line runs).
4. **Given** a line using the legacy SWu engine, **When** its agent starts, **Then** clearing happens only if that engine is confirmed to keep no tunnel state of its own in the namespace; otherwise it is skipped for that engine.

---

### Edge Cases

- The carrier deletes the IKE_SA during an active call: the call drops (the tunnel is gone either way). Recovery must not be slower than for an idle line, and must not wait on the call.
- The ePDG refuses the new IKE_SA repeatedly: each health check sees an IKE_SA being set up (not lost), so the #118 two-strike path governs. There is no restart storm and no re-initiate on every check.
- The carrier deletes each new IKE_SA shortly after it is built (the #106 pattern): every rebuild succeeds, so the line is never continuously down for `unhealthy_sec`, but the rebuild count crosses the flapping threshold and the tunnel-failure alert fires once (FR-014a).
- The IKE daemon itself died: unchanged; the existing daemon-restart recovery runs and its agent restart now also benefits from the P-CSCF sync.
- The new negotiation advertises only IPv6 P-CSCFs, or both families: the same deterministic choice as today (IPv4 preferred, stable within a family) is applied to the new negotiation's list.
- Another line's negotiation completes during this line's wait: it must not be mistaken for this line's.
- The agent exits on its own (for example, its binding-loss budget from #119) at the same moment as a supervisor-initiated restart: one restart, with the namespace cleared before it.
- The graceful delete is acknowledged but the daemon still lists the IKE_SA briefly: the presence check decides, and a forced delete of an already-dying SA is harmless.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The supervisor MUST distinguish, on every strongswan health check, between (a) no IKE_SA for the line's connection, (b) an IKE_SA with no usable CHILD_SA, and (c) a usable CHILD_SA.
- **FR-002**: On (a), the supervisor MUST request a new IKE_SA, reset the missing-CHILD_SA counter, report the recovery with a reason distinct from a CHILD_SA loss, and restart the line's IMS agent. The restart is unconditional: it happens even if the new IKE_SA brings back the same inner address and P-CSCF.
- **FR-002a**: The IKE_SA-loss recovery log line MUST name the cause: `carrier DELETE` when the ePDG deleted the IKE_SA, `dead-peer timeout` when liveness checks gave up, and `unknown` otherwise. The cause is found from this line's own most recent IKE daemon log entries. When an IKE_SA loss contributes to a tunnel-failure alert, the alert text MUST include the most recent cause. The cause MUST NOT change the recovery taken, and no new metric is added for it.
- **FR-003**: On (b), behaviour MUST stay as delivered by #118, including when the IKE_SA is present but still being established.
- **FR-004**: After every recovery that restarts the IMS agent, the supervisor MUST wait up to a bounded time (default 20s) for a negotiation newer than the recovery to complete for this line. If one does, it MUST record that negotiation's P-CSCF for the line before restarting the agent.
- **FR-005**: "Newer than the recovery" MUST be judged against a marker taken before the health check that triggered the recovery. It MUST NOT be satisfied by any negotiation logged before that marker, and it MUST tolerate the log having been rotated or truncated.
- **FR-006**: If the bound in FR-004 expires, the supervisor MUST log that, restart the agent anyway, and leave the existing P-CSCF-change handling as the fallback.
- **FR-007**: The wait in FR-004 MUST stop promptly when shutdown begins, and MUST NOT hold the lock that shutdown waits on.
- **FR-008**: Every supervisor-initiated IKE_SA teardown on the strongswan steady-state path MUST first request a non-forced delete and wait a bounded time (default 5s) for completion.
- **FR-009**: After the graceful attempt, the supervisor MUST check whether the line's IKE_SA is still present. Only if it is MUST it issue the forced delete, under the existing 10s bound. If presence cannot be determined (the status query itself fails, as when the control channel is broken), the supervisor MUST treat the IKE_SA as present and force the delete.
- **FR-010**: Teardown MUST log which path completed it (acknowledged or forced), and MUST NOT add any fixed delay before the rebuild.
- **FR-011**: The total teardown time MUST have a documented upper bound.
- **FR-012**: Before each start of a strongswan-engine line's IMS agent, the supervisor MUST clear all IPsec SAs and policies inside that line's namespace, and log how many were cleared when nonzero.
- **FR-013**: The clearing in FR-012 MUST NOT run while an agent for that line is running, MUST NOT touch the host namespace or any other line's namespace, and MUST apply to the SWu engine only if that engine is confirmed to keep no kernel IPsec state there.
- **FR-014**: The CHILD_SA health gauge and tunnel-failure alert from #118 MUST report an IKE_SA loss as "down" for its duration, exactly as they report a CHILD_SA loss.
- **FR-014a**: The existing tunnel-failure alert MUST also fire when a line needs 3 or more IKE_SA rebuilds within `unhealthy_sec`. An IKE_SA rebuild is any recovery that builds a new IKE_SA, i.e. one that restarts the IMS agent. The alert uses the same enable switch as today, fires once per flapping episode, and sends its all-clear after a full `unhealthy_sec` window with no rebuild. A flapping episode and a continuous outage on the same line MUST NOT produce two alerts for overlapping time. Rebuilds are not slowed or backed off.
- **FR-015**: No new configuration keys are introduced. The bounds in FR-004 and FR-008 are internal constants, documented where they are defined.
- **FR-016**: Unreleased release-notes and changelog entries MUST describe the change in the project's usual style.

### Key Entities

- **Line health state**: per health check, one of: IKE_SA lost, CHILD_SA missing under a live IKE_SA, tunnel interface missing, control channel broken, healthy.
- **Recovery reason**: the reported reason for a recovery. Gains "IKE_SA lost", which restarts the IMS agent and carries a cause (carrier DELETE / dead-peer timeout / unknown) for logging and alert text only. "CHILD_SA missing" keeps its meaning: lost under a live IKE_SA, no agent restart.
- **Negotiation marker**: the position in the IKE daemon's log before a health check, used to tell the new negotiation's P-CSCF list from earlier ones.
- **Line Gm state**: the Gm IPsec SAs and policies in a line's namespace. Owned by that line's current registration only.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After a carrier-side (or hand-made) IKE_SA deletion on an idle line, the line is registered on its new inner address and rings for an inbound call within 75s, with no operator action. Today this takes several minutes and depends on the agent's own failure detection.
- **SC-002**: Every recovery that rebuilds the IKE_SA restarts the IMS agent exactly once. Zero "P-CSCF changed" reports occur on the check after such a recovery, measured over at least three induced recoveries on the live line.
- **SC-003**: On a responsive ePDG, 100% of supervisor teardowns in the live gate complete through the acknowledged path, with no forced delete.
- **SC-004**: Worst-case teardown time stays within its documented bound in automated tests with an unresponsive peer.
- **SC-005**: After any number of agent restarts, the line's namespace holds exactly one registration's worth of Gm IPsec state.
- **SC-006**: No regression from #118: a natural CHILD_SA rekey causes no recovery, and a CHILD_SA removed under a live IKE_SA is still re-initiated without an agent restart.
- **SC-006a**: In automated tests, a line whose IKE_SA is rebuilt 3 times within `unhealthy_sec` produces exactly one tunnel-failure alert, and one all-clear after a quiet window. A line rebuilt twice in that window produces none.
- **SC-007**: `make format`, `make lint` and `make test` pass on every commit. The live gate on the local Vodafone line passes and its results are recorded in this spec folder before merge.

## Assumptions

- strongswan's listing shows an IKE_SA for a connection from the moment an initiate begins, including while it is still being set up. So "no IKE_SA listed" reliably means the session is gone and no initiate is in progress.
- A CHILD_SA created inside an existing IKE_SA never changes the inner address or P-CSCF list (RFC 7296: configuration payloads are exchanged in IKE_AUTH only), so that path needs neither the wait nor an agent restart.
- The IKE daemon's log is shared across lines, and each line's entries carry its connection name; this attribution is already relied on.
- The line namespace holds no kernel IPsec state other than the IMS agent's Gm state for strongswan-engine lines: the ePDG tunnel's state lives in the host namespace, tied to the line by interface ID.
- 20s is enough for an IKE_SA with SIM-based EAP-AKA to establish on the carriers in use. Longer is handled by the fallback in FR-006.
- Container shutdown already tears the IKE_SA down with a non-forced delete under its own 5s bound, so FR-008 to FR-011 change only the steady-state recovery path.
- 5s is enough for a responsive ePDG to acknowledge a delete. A slower one is handled by the forced fallback.
- The live gate uses the local Vodafone line. The carrier's own DELETE is simulated by deleting the IKE_SA locally, which produces the same "no IKE_SA" state.

## Out of Scope

- The 30s TCP probe to the P-CSCF's unprotected port (#120).
- The carrier-specific CHILD_SA rekey selector fix (#107).
- A SIGTERM handler in the IMS agent for orderly Gm cleanup. The pre-start clearing makes it unnecessary for correctness.
- #106's other proposals: binding the tunnel interface to the WAN device, a 20s SIP keepalive, a fixed post-terminate sleep, flushing Gm state under a live agent, and the updown default-route changes. They were triaged as incorrect, harmful or already done.
