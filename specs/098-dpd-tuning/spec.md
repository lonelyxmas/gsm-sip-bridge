# Feature Specification: Tune strongSwan DPD so one ignored request can't tear down a working tunnel

**Feature Branch**: `098-dpd-tuning`  
**Created**: 2026-10-18  
**Status**: Draft  
**Input**: User description: "DPD tuning for the strongSwan ePDG engine (#120 replacement 3, finding 2). Today swanctl-epdg.conf.template hard-codes dpd_delay = 30s. charon sends DPD only after dpd_delay with no inbound traffic (ESP counts); the supervisor's 30s TCP probe to the P-CSCF currently keeps DPD from ever firing. In #120's probes-off soak (run C, 4.5h, Vodafone and Jio) DPD fired ~every 30s and on both carriers the ePDG once ignored a DPD INFORMATIONAL and all 5 retransmits while ESP still flowed both ways; charon gave up (~165s) and tore down a healthy tunnel (~22s outage via IkeSaLost recovery). Goal: make the supervisor's P-CSCF probe removable without this regression. Scope: (1) Research — the run-C logs/captures are gone, so reproduce with a throwaway probe-forced-to-pass build and capture charon logs + outer UDP 4500 + in-tunnel traffic to establish whether the ignored DPD correlates with CHILD_SA/IKE_SA rekeys (message-ID/collision) or is random ePDG loss; also confirm what handsets use (Android IWLAN KEY_DPD_TIMER_SEC_INT default believed 120s). (2) Make DPD delay configurable via new [vowifi].dpd_delay_sec, default 120 (phone-like), 0 disables, rendered through render_swanctl_epdg as a template placeholder; docs/config example/snapshot updated. (3) Diagnostics — on IkeSaLost{DeadPeer} classify whether the CHILD_SA had recent inbound ESP (false dead peer) vs not, and expose it on metrics so the probe-removal decision has production data. (4) Live verification — 12–24h probes-off soak on both carriers with zero false dead-peer teardowns, plus a black-holed-ePDG test proving a real dead path is still detected and recording detection time; also chase #137's open finding (agent path 234s vs probe 97s detection). Out of scope: removing the supervisor probe itself (separate PR after this gate); patching charon in the fork (fallback only if 120s still shows false teardowns). DPD change has no production effect while the probe stays."

## Background

#120 is working towards removing the line supervisor's 30s TCP probe of the P-CSCF's unprotected port 5060. Its soak named three things the probe was silently doing, each of which needs a replacement before the probe can go:

1. Repairing a lost default route — replaced by the per-tick route check (PR #136).
2. Rebuilding the tunnel when a single-P-CSCF carrier's only P-CSCF dies — replaced by `PcscfPoolExhausted` (PR #137).
3. **Keeping strongSwan's dead-peer detection (DPD) quiet** — this feature.

**How DPD behaves today.** Each line's IKE connection is configured with a fixed 30s DPD delay. charon sends a DPD request only when it has received *nothing* from the ePDG for that long; inbound ESP counts as something. With the probe running, its TCP handshake brings inbound traffic every 30s, so DPD never fires in production. With all probes off (#120 run C), the only regular inbound traffic is the IMS agent's 120s `OPTIONS` answer, so DPD fired roughly every 30s — on the order of 500 exchanges in 4.5h.

**What went wrong in run C.** On **both** Vodafone and Jio, once in 4.5h, the ePDG never answered one DPD request or any of its 5 retransmits, while ESP kept flowing both ways inside the same tunnel (OPTIONS were answered; on Jio a full REGISTER went through during the retransmit window). charon gave up after ~165s and deleted a healthy tunnel. The existing `IkeSaLost` recovery re-registered the line in ~22s. Each such event is a ~22s outage, and drops any call in progress.

**What is not known.** The run-C logs and captures are gone. Whether the ignored request is random ePDG loss or is triggered by something specific — most plausibly a collision with a CHILD_SA or IKE_SA rekey, or a message-ID desync — decides the right fix. A longer DPD delay reduces the number of exchanges (and so the chances of hitting a random loss) proportionally; it does nothing for a rekey-triggered fault, which happens at the same rate whatever the delay.

**What a handset does.** Android's IWLAN stack is believed to default to a 120s DPD timer. That is unconfirmed and part of the research.

**What the DPD delay must still do.** With the probe gone, DPD is one of the two ways a *really* dead ePDG path is detected (the other being the agent's keepalive feeding `PcscfPoolExhausted`, measured at ~234s on Vodafone in #137). On Jio, with 4 P-CSCFs, the agent path walks every pool member before it gives up, so DPD may be the faster signal. DPD can't simply be switched off without measuring that.

**No production effect while the probe stays.** Because the probe keeps DPD from firing, changing the DPD delay changes nothing on a line running the current default build. Every behavioural claim in this spec has to be verified with the probe forced off.

## Clarifications

### Session 2026-10-18

- Q: The run-C logs are gone — reconstruct from memory, or reproduce? → A: Reproduce and capture on both rigs before deciding the fix.
- Q: Run as a speckit feature or straight to a branch? → A: speckit.
- Q: What if the 30s reproduction soak produces no teardown on a carrier in 12h? → A: Record the verdict as inconclusive and proceed with the 120s default; the probe-removal gate still needs SC-001, and the production false/real dead-peer metric keeps collecting evidence.
- Q: Should a false dead-peer teardown raise a Discord alert? → A: No. Metrics and the log line only; no new alert section and no change to existing alert text.
- Q: Can a fix for #137's detection gap land in this PR? → A: Yes, if small: confined to the IMS agent's keepalive/failover timing, no new config, covered by unit tests, and re-verified live (Vodafone single-P-CSCF black-hole plus Jio sibling failover). Anything larger becomes a filed follow-up issue.
- Q: How long must the 120s probe-off soak run? → A: 12h clean per carrier to merge this feature; the later probe-removal feature requires at least 48h probe-off per carrier with zero false dead-peer teardowns, plus the production false/real metric.

### Session 2026-10-20

- Q: SC-003's 300s bound is below DPD's worst case (120s delay + 165s retransmits + 30s poll). Widen it? → A: Yes, to 315s.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Know why the ePDG ignored a DPD request (Priority: P1)

The maintainer runs each live rig on a throwaway build with the supervisor's probe forced to pass and the current 30s DPD delay, with charon logging and packet capture on the outer IKE/NAT-T path and inside the tunnel. They run it until each carrier has produced at least one false dead-peer teardown, or for 12h if none occurs. For every teardown, they record the exact sequence: the DPD request's message ID, its retransmits, any rekey or other IKE exchange in the minutes around it (from either side), and the ESP traffic in both directions during the retransmit window. They also record how many DPD exchanges were answered in the same period, and confirm the handset DPD default from a primary source.

**Why this priority**: everything else depends on it. If the fault is rekey-triggered, a longer DPD delay is not the fix and the spec's default must change before anything ships.

**Independent Test**: delivered as a research note with the raw evidence (log excerpts, capture summaries) and a verdict: *random loss*, *rekey/collision-triggered*, or *inconclusive*, plus the measured rate (ignored requests per DPD exchange).

**Acceptance Scenarios**:

1. **Given** a 30s-DPD, probe-forced-to-pass soak on Vodafone and on Jio, **When** charon reports giving up on the ePDG, **Then** the research note shows, for that event, the timeline of IKE exchanges in the preceding 10 minutes, the ignored request and its retransmits, and inbound/outbound ESP counts during the retransmit window.
2. **Given** no teardown occurred in 12h on a carrier, **When** the soak ends, **Then** the note records that, with the number of DPD exchanges answered, and the verdict for that carrier is *inconclusive*.
3. **Given** the research verdict is *rekey/collision-triggered*, **When** planning continues, **Then** the DPD-delay default in User Story 2 is revisited before implementation and the spec is amended.
4. **Given** the verdict for a carrier is *inconclusive*, **When** planning continues, **Then** the 120s default proceeds unchanged; the reproduction is not extended or repeated as a blocker.

---

### User Story 2 - Operators can set the DPD delay, and the default is phone-like (Priority: P1)

An operator configures each deployment's DPD delay with one setting. If they don't set it, it defaults to 120s, matching a handset. Setting it to 0 turns charon-initiated DPD off (the line still answers the ePDG's own DPD). The value applies to every strongSwan line in the deployment and takes effect when the container is recreated. Startup states the effective value once.

**Why this priority**: it is the change that makes probe removal safe, assuming User Story 1 finds random loss. It is also the escape hatch if a carrier turns out to behave differently: an operator can lengthen it or turn it off without a rebuild of the image.

**Independent Test**: unit tests of config parsing (default, explicit value, 0, out-of-range rejected) and of the rendered IKE connection config for each. Live: with the probe forced off, count DPD requests on the outer path over 1h at the default.

**Acceptance Scenarios**:

1. **Given** no DPD setting in the config, **When** a strongSwan line starts, **Then** its IKE connection uses a 120s DPD delay and startup logs the effective value.
2. **Given** the setting is 0, **When** a line starts, **Then** charon never initiates DPD on that line, and still answers the ePDG's DPD requests.
3. **Given** a value that is negative, non-numeric or outside the accepted range, **When** config is loaded, **Then** startup fails with an error naming the setting and the accepted range.
4. **Given** the default build with the probe running, **When** a line idles for 1h, **Then** the outer traffic and every recovery behaviour are unchanged from before this feature (DPD still never fires).
5. **Given** a line on the swu engine, **When** the setting is present, **Then** it is accepted and ignored, and startup says so once.

---

### User Story 3 - A false dead-peer teardown is visible in production metrics (Priority: P2)

When charon tears down a tunnel because the ePDG stopped answering IKE, the supervisor classifies the loss: *false* if inbound ESP was still arriving during the period charon was retransmitting, *real* if not. The classification appears in the existing VoWiFi metrics so an operator — and the probe-removal decision — can see how often each happens on each line.

**Why this priority**: one soak per carrier is a small sample. The probe-removal decision, and any later choice between a longer delay and patching charon, should rest on long-run data from real deployments. It is P2 because the tuning itself (User Story 2) does not depend on it.

**Independent Test**: unit tests over the classification with synthetic SA traffic snapshots before the loss (ESP rising during the window → false; flat → real; no snapshot inside the window → unknown). Integration test that the classified loss reaches the metrics endpoint.

**Acceptance Scenarios**:

1. **Given** a dead-peer loss where the line's inbound ESP counter rose between the last snapshot before the first unanswered DPD request and the loss, **When** the supervisor handles the loss, **Then** it is recorded as a false dead peer, and the corresponding metric for that line goes up by one.
2. **Given** a dead-peer loss where inbound ESP did not rise in that window, **When** handled, **Then** it is recorded as a real dead peer.
3. **Given** a dead-peer loss with no traffic snapshot inside the window (e.g. the supervisor restarted mid-window), **When** handled, **Then** it is recorded as unknown, never as false.
4. **Given** a loss caused by the carrier deleting the IKE_SA, or by the bridge itself, **When** handled, **Then** this classification is not applied and existing behaviour is unchanged.
5. **Given** any classification, **When** the supervisor logs the loss, **Then** the log line names the classification and the inbound ESP evidence (packets/bytes during the window).

---

### User Story 4 - A really dead ePDG path is still detected in bounded time without the probe (Priority: P2)

With the probe forced off and the default DPD delay, the maintainer black-holes the ePDG's outer address (the tunnel's own path, not just the P-CSCF). The line notices and starts recovering within a bounded time, through whichever of DPD or the agent path fires first. The same test is repeated on Jio, where the agent walks a 4-member P-CSCF pool before giving up.

**Why this priority**: lengthening DPD trades false teardowns against slower real-failure detection. This story measures that trade-off so the probe-removal PR can state it.

**Independent Test**: live only; recorded in the live-verification notes with timestamps for fault injection, first unanswered DPD request, loss detection, rebuild, and re-registration once the path is restored.

**Acceptance Scenarios**:

1. **Given** a healthy Vodafone line with the probe forced off and default DPD, **When** the ePDG's outer address is black-holed, **Then** the supervisor detects the loss and begins a rebuild within 315s, and the time and the signal that fired first are recorded.
2. **Given** the same on Jio, **When** the ePDG is black-holed, **Then** the same bound and recording apply.
3. **Given** the black hole is removed, **When** the next rebuild attempt runs, **Then** the line re-registers without manual action.

---

### User Story 5 - Explain the agent path's slower dead-P-CSCF detection (Priority: P3)

#137 measured the agent's `PcscfPoolExhausted` path at ~234s against an estimate of ~200s and the probe's 97s; ~35s went missing between the first dead keepalive and the next send, and the agent log for that window was lost to a restart. The maintainer reproduces the Vodafone single-P-CSCF black-hole with the agent log preserved and accounts for the full detection time.

**Why this priority**: it affects how slow a dead P-CSCF is to recover once the probe goes, but it doesn't block the DPD change.

**Independent Test**: a research note with a timeline that sums to the measured detection time, and either a fix in this PR or a filed follow-up issue. A fix qualifies only if it is confined to the agent's keepalive/failover timing, adds no config, is covered by unit tests, and is re-verified live; otherwise it is filed.

**Acceptance Scenarios**:

1. **Given** the Vodafone P-CSCF black-holed with the probe forced to pass and the agent log retained across restarts, **When** `PcscfPoolExhausted` fires, **Then** the note accounts for every interval from the first unanswered keepalive to the rebuild, to within 5s.
2. **Given** a qualifying fix lands in this PR, **When** the same black-hole is repeated, **Then** detection is no slower than before and matches the note's corrected estimate to within 10s, and a Jio active-P-CSCF black-hole still fails over to a sibling with no rebuild.

---

### Edge Cases

- **A rekey overlaps the DPD retransmit window.** charon serialises IKE exchanges; a DPD request in flight delays a rekey and vice versa. The research must say whether the ignored request in run C was adjacent to one, and the classification (User Story 3) must not count a rekey's own ESP switch-over as evidence either way without looking at both the old and new CHILD_SA.
- **The ePDG initiates DPD itself.** Independent of our setting; the line must keep answering. Not affected by any value, including 0.
- **A call is in progress when DPD gives up.** Today the call drops. This feature reduces how often that happens but doesn't change the recovery; it is recorded, not prevented.
- **NAT-T keepalives.** They are outbound-only and independent of the DPD delay; they keep the NAT mapping alive and do not count as inbound traffic for DPD. No change.
- **Config changed without recreating the container.** The rendered IKE config is written at startup; the change takes effect on recreate, as with other `[vowifi]` deployment settings. Documented, not detected.
- **IKE_SA lost for a reason the supervisor can't attribute.** Classified as unknown, as today.
- **The research verdict is "rekey-triggered".** The 120s default is not shipped on that basis alone; the spec is amended (User Story 1, scenario 3), and the charon-patch fallback becomes the likely path in a follow-up.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The maintainer MUST reproduce the run-C false dead-peer teardown on both carriers (or run 12h without one) with the probe forced off and the current 30s DPD delay, capturing charon logs, outer IKE/NAT-T/ESP traffic and in-tunnel traffic, and record the evidence and a verdict before the default in FR-003 is finalised. Only a *rekey/collision-triggered* verdict blocks the 120s default; an *inconclusive* verdict does not.
- **FR-002**: The research MUST state the handset DPD default from a primary source (platform carrier-config definitions), or record that it could not be confirmed.
- **FR-003**: A deployment-wide DPD delay setting MUST exist for strongSwan lines, in seconds, defaulting to 120, where 0 disables charon-initiated DPD.
- **FR-004**: Out-of-range or malformed DPD values MUST fail config loading with an error naming the setting and the accepted range. The accepted range is 0, or 10 to 3600 inclusive.
- **FR-005**: Each strongSwan line's IKE connection MUST be rendered with the configured DPD delay; nothing else in the rendered connection may change.
- **FR-006**: Startup MUST log the effective DPD delay once per deployment, and say when it's ignored (swu engine).
- **FR-007**: With the probe running (the default build), outer traffic and every supervisor recovery behaviour MUST be unchanged from before this feature.
- **FR-008**: On every IKE_SA loss attributed to an unresponsive peer, the supervisor MUST classify it as false, real or unknown from the line's inbound ESP counters, using only counters observed during the window between the last answered IKE exchange (or the last snapshot before it) and the loss.
- **FR-009**: The classification MUST be exposed per line on the existing metrics endpoint as a counter that a supervisor restart cannot rewind, and included in the loss's log line together with the inbound ESP evidence.
- **FR-009a**: The classification MUST NOT raise a Discord alert or change the text of any existing alert.
- **FR-010**: Loss causes other than an unresponsive peer MUST NOT be classified and MUST keep their existing behaviour and metrics.
- **FR-011**: The configuration reference, the example config and the changelog MUST document the setting, its default, the meaning of 0, the recreate-to-apply rule, and the fact that it has no effect while the probe runs.
- **FR-011a**: A fix for the #137 detection gap MAY land in this feature only if it is confined to the IMS agent's keepalive/failover timing, adds no config, is unit-tested and is re-verified live on both rigs; otherwise a follow-up issue MUST be filed and linked from the research note.
- **FR-012**: The live verification (User Stories 2, 4 and the 12h-per-carrier soak in SC-001) MUST be recorded in the feature's live-verification notes before merge.

### Key Entities

- **DPD delay setting**: one deployment-wide value in seconds (0 or 10–3600, default 120) applied to every strongSwan line's IKE connection.
- **Dead-peer classification**: false / real / unknown, attached to an IKE_SA loss whose cause is an unresponsive peer; derived from the line's inbound ESP counters over the retransmit window.
- **Research verdict**: random loss / rekey-or-collision-triggered / inconclusive, per carrier, with the measured rate of ignored DPD requests per exchange.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With the probe forced off and the default DPD delay, each carrier runs at least 12h idle with **zero** false dead-peer teardowns, at least one REGISTER refresh, one CHILD_SA rekey and one IKE_SA rekey. 12h is the merge bar for this feature only; it is weak evidence on its own (at the expected ~1 per 18h rate a clean 12h is roughly a coin flip), which is why the probe-removal gate below is stricter.
- **SC-002**: At the default, the number of DPD requests sent over an idle hour (probe forced off) falls to at most one third of the 30s baseline measured in User Story 1.
- **SC-003**: With the probe forced off and the default DPD delay, a black-holed ePDG is detected and a rebuild begun within 315s on both carriers: the remaining DPD delay (up to 120s), charon's 165s of retransmits, and up to 30s until the supervisor's next poll.
- **SC-004**: With the default build (probe on), an idle hour on each rig shows no DPD exchanges, no new recoveries and the same outer traffic as before this feature.
- **SC-005**: The research note gives a verdict per carrier, with evidence, and the handset default is confirmed or explicitly marked unconfirmed.
- **SC-006**: Every dead-peer loss observed during the live runs is classified, and the classification matches the capture (false when ESP was inbound during the window, real for the black-hole test).
- **SC-007**: The #137 detection gap is accounted for to within 5s, with a fix or a filed follow-up.

## Assumptions

- The ePDG's ignoring of DPD requests is rare and roughly proportional to the number of DPD exchanges, unless User Story 1 shows otherwise; that is what makes a longer delay a fix and not a mitigation.
- charon's retransmit schedule (5 retransmits, ~165s before giving up) stays as it is. Shortening it would speed up real-failure detection but also make false teardowns more likely; changing it is out of scope unless the research shows a reason.
- The supervisor already queries each line's SAs every steady-state tick, and the query reports inbound byte/packet counters; classification can reuse it without new traffic.
- swu lines have no DPD setting of their own and no supervisor reporter; they are out of scope (same as #136 and #137).
- Removing the supervisor's probe is a separate feature, gated on this one's SC-001 and SC-003 **and** on its own probe-off soak of at least 48h per carrier with zero false dead-peer teardowns, alongside whatever the production false/real dead-peer metric has recorded by then. Patching charon to treat recent inbound ESP as proof of life is the fallback only if the research verdict or SC-001 rules out the delay change.
- Live tests use the existing rigs: Vodafone on the local host, Jio on the remote Pi; throwaway probe-forced builds are never pushed.
