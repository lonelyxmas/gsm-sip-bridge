# Feature Specification: Rebuild the ePDG tunnel on a stuck CHILD_SA rekey, before the hard expiry

**Feature Branch**: `093-child-sa-rekey-stuck`  
**Created**: 2026-10-15  
**Status**: Draft  
**Input**: User description: "Rebuild the ePDG tunnel on a stuck CHILD_SA rekey, before the hard expiry (GitHub issue #127). When a carrier keeps rejecting CHILD_SA rekeys, charon keeps using the old SA until its hard lifetime expires, then the tunnel drops at an uncontrolled moment (possibly mid-call) and the supervisor needs ~60s of ChildSaMissing/ChildSaStuck strikes before rebuilding. Detect the stuck rekey early and do a planned rebuild (bounded terminate --force + re-initiate + agent restart) with a distinct DegradeReason (ChildSaRekeyStuck), while the old SA still carries traffic. Trigger must be carrier-agnostic and must not rely on sampling the transient REKEYING state; instead use the CHILD_SA's own lifetime from swanctl --list-sas. Natural rekeys must never trigger it. Exactly one planned rebuild per stuck SA. Per-line only. New reason visible in logs and counted toward [alerts.tunnel_failure] and the flapping window. Optional call-gating: defer the rebuild during an active call but force it before the hard expiry. Non-goals: no change to tunnel proposals/lifetimes/strongSwan build; existing ChildSaMissing/ChildSaStuck backstop unchanged."

## Background

A VoWiFi line's data path is an **IKE_SA** (the control session with the carrier's ePDG) carrying a **CHILD_SA** (the ESP tunnel that carries SIP and RTP). A CHILD_SA has two lifetimes. At the **soft** lifetime the IKE daemon asks the ePDG to replace it (a *rekey*). At the **hard** lifetime the kernel deletes it, whether or not a replacement exists. With the daemon's defaults, which this project does not override, the soft lifetime falls between 55 and 60 minutes after installation and the hard lifetime at 66 minutes. A healthy rekey therefore replaces the CHILD_SA with 6 to 11 minutes of its hard lifetime still left, and finishes within a second.

Issue #107 recorded what happens when an ePDG rejects every rekey. On that carrier each attempt was answered at once with `INVALID_SELECTORS`, and the daemon retried roughly every 6 seconds: 105 rejections over 11 minutes. The old CHILD_SA kept carrying calls the whole time. Then it hit its hard lifetime and the line lost its data path at a moment nobody chose, possibly mid-call. Recovery today starts only after that, through the #118 path: one health check (up to 30s) to notice the CHILD_SA is missing and re-initiate it inside the same IKE_SA, which an ePDG in this state does not answer, and a second check (another 30s) to escalate and rebuild the whole IKE_SA. The IMS agent then restarts and re-registers.

So the bridge has about 11 minutes of warning that the line will drop, and uses none of it.

#131 (`tunnel_address_family`, spec 092) removes the cause on the one carrier where it has been seen, once an operator sets `ipv4` for that line. This feature limits the damage from any rekey failure, on any carrier, with no operator action. The two complement each other.

### Why the trigger is the CHILD_SA's remaining lifetime

Issue #127 proposed counting health checks that see the CHILD_SA in the `REKEYING` state. That state is only shown while a rekey exchange is in flight. A rejected rekey is answered immediately, and the CHILD_SA most likely returns to `INSTALLED` until the next retry a few seconds later. A check every 30 seconds would then almost never see `REKEYING`, and the counter would keep resetting. A rekey the ePDG never answers at all looks different again.

What every failed rekey has in common is the outcome: the **same** CHILD_SA is still installed after the point where a healthy rekey would have replaced it. Its remaining lifetime, which the IKE daemon reports for every CHILD_SA, shows this directly, whatever the carrier sent back. It also gives the exact deadline that any deferral (User Story 3) has to meet.

## Clarifications

### Session 2026-10-15

- Q: Is deferring the rebuild during an active call part of this feature, or a follow-up? → A: Part of this feature, as User Story 3 (P2). Stories 1–2 deliver value on their own and can ship first; Story 3 adds the line's active-call state to the supervisor. A rebuild is never deferred past the forced-rebuild margin before the hard expiry.
- Q: Should a logged rekey rejection trigger the rebuild straight away, ahead of the lifetime threshold? → A: No. The lifetime threshold is the only trigger, so it works the same on every carrier, and a one-off rejection that a later retry fixes never causes a rebuild. When the IKE daemon's log shows why the rekeys failed in the current session (for example `INVALID_SELECTORS`), the planned-rebuild log line includes that reason.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A stuck rekey is rebuilt before the hard expiry (Priority: P1)

An operator runs a VoWiFi line on a carrier whose ePDG has started rejecting (or ignoring) CHILD_SA rekeys. While the old CHILD_SA still carries traffic, the bridge notices that the CHILD_SA has outlived the point where a healthy rekey would have replaced it. It rebuilds the line's tunnel then, at a time it chose, restarts the line's IMS agent, and the line re-registers. The hard expiry never happens, and the ~60 seconds of failed recovery that used to follow it never happen either.

**Why this priority**: This is the whole of #127. Without it, every rekey failure on any carrier ends in an unplanned outage at a random moment.

**Independent Test**: With recorded `--list-sas` output from a live line, replay a sequence in which the same CHILD_SA's remaining lifetime falls past the threshold. Confirm exactly one planned rebuild happens, with the new reason, and that the old CHILD_SA was still usable when it started. On a live line, force the same condition (for example by preventing the rekey from completing) and confirm the line re-registers before the old CHILD_SA's hard lifetime.

**Acceptance Scenarios**:

1. **Given** a line whose CHILD_SA is usable, with the same identity as on earlier checks, and whose remaining lifetime is below the stuck-rekey threshold, **When** the next health check runs, **Then** the supervisor logs a stuck rekey for that line, with the CHILD_SA's remaining lifetime, rebuilds the line's IKE_SA, and restarts the line's IMS agent.
2. **Given** a planned rebuild has just been done for a stuck rekey, **When** the next health checks run while the new tunnel is being built, **Then** no second planned rebuild is started for the same stuck CHILD_SA.
3. **Given** a planned rebuild has completed and a new CHILD_SA is installed, **When** that new CHILD_SA later gets stuck in the same way, **Then** it gets its own single planned rebuild.
4. **Given** two lines sharing one IKE daemon, **When** only line 1's CHILD_SA has a stuck rekey, **Then** only line 1's tunnel is rebuilt and only line 1's agent restarts. Line 2's tunnel, CHILD_SA and registration are untouched.
5. **Given** a line where the stuck-rekey rebuild was not done in time (for example the bridge was restarted during the window) and the CHILD_SA has hard-expired, **When** health checks run, **Then** the existing missing-CHILD_SA recovery from #118 handles it exactly as it does today.

---

### User Story 2 - A healthy rekey never triggers a rebuild (Priority: P1)

An operator runs a line on a carrier where rekeys work (Vodafone, Jio, Airtel today). Every hour the CHILD_SA is rekeyed. The bridge does nothing: no rebuild, no agent restart, no extra log noise.

**Why this priority**: A false positive here would turn every healthy line into one that drops and re-registers every hour, which is worse than the problem being fixed. It must hold from the first release.

**Independent Test**: Unit tests over real `--list-sas` output covering every shape a natural rekey takes. On the live Vodafone line, let at least one natural rekey happen and confirm no planned rebuild is logged.

**Acceptance Scenarios**:

1. **Given** a natural rekey where the old CHILD_SA shows as replaced beside the new installed one, **When** a health check samples that moment, **Then** no planned rebuild happens and the line stays healthy.
2. **Given** a natural rekey that a health check samples while the exchange is in flight, **When** the check runs, **Then** no planned rebuild happens.
3. **Given** a freshly rekeyed CHILD_SA (a new identity with a full lifetime), **When** health checks run, **Then** any stuck-rekey state held for the previous CHILD_SA is cleared.
4. **Given** a CHILD_SA whose remaining lifetime is still above the threshold, **When** health checks run, **Then** no planned rebuild happens, however long the line has been up.

---

### User Story 3 - A planned rebuild waits for an active call to end, but never past the hard expiry (Priority: P2)

An operator's line has a stuck rekey while a call is in progress. The bridge holds the planned rebuild until the call ends, so the call is not cut. If the call is still going as the hard expiry approaches, the bridge rebuilds anyway, because the call is about to be cut regardless, and a planned rebuild recovers faster than an expiry.

**Why this priority**: It turns "the break happens at a moment we choose" into "the break happens between calls", which is what matters to callers. It depends on the line's call state reaching the tunnel supervisor, which is new, so it can ship after Stories 1–2.

**Independent Test**: Feed the supervisor a stuck rekey with the line reported in a call. Confirm the rebuild is deferred and logged as deferred. Report the call as ended and confirm the rebuild follows at the next health check. Separately, keep the call active and confirm the rebuild is forced once the remaining lifetime falls below the forced-rebuild margin.

**Acceptance Scenarios**:

1. **Given** a stuck rekey on a line with an active call and a remaining lifetime above the forced-rebuild margin, **When** a health check runs, **Then** the rebuild is deferred and the deferral is logged once (not on every check).
2. **Given** a deferred rebuild, **When** the call ends, **Then** the rebuild happens at the next health check.
3. **Given** a deferred rebuild and a call that is still active, **When** the remaining lifetime falls below the forced-rebuild margin, **Then** the rebuild happens anyway and the log says it was forced during a call.
4. **Given** the line's call state is unknown (not reported, or stale), **When** a stuck rekey is found, **Then** the rebuild is not deferred.

---

### Edge Cases

- **Health check falls right at the threshold**: a check every 30 seconds sees the remaining lifetime fall past the threshold within at most one interval, which the threshold's margin to the hard expiry must cover, together with the rebuild time.
- **CHILD_SA identity changes without a rekey** (re-initiate after a loss, a rebuild by another recovery path): the new CHILD_SA has a fresh lifetime, and any stuck-rekey state is cleared.
- **Several CHILD_SAs listed for one line at once** (the brief overlap during a natural rekey): the decision uses the newest usable CHILD_SA, so the old one's short remaining lifetime during the overlap never counts as stuck.
- **Lifetime not reported or unparseable**: no planned rebuild, no error noise beyond one log line. The #118 backstop still covers the eventual expiry.
- **Bridge (or IKE daemon) restarts during the window**: the next health check judges the CHILD_SA on its own reported remaining lifetime. No state needs to survive a restart.
- **Planned rebuild fails to bring a new tunnel up**: the existing recovery paths (IKE_SA missing, CHILD_SA missing) take over on the following checks. This feature adds no retry loop of its own.
- **Repeated stuck rekeys**: each planned rebuild counts as a rebuild in the flapping window, so a carrier that breaks every new CHILD_SA within the window raises the existing tunnel-failure alert.
- **Other recovery due in the same check** (missing tunnel interface, missing IKE_SA, missing CHILD_SA, dead daemon): those take precedence. A stuck-rekey rebuild only runs on a check where the structural checks pass.
- **SWu (legacy) engine**: it reports no CHILD_SA lifetimes, so the feature does not apply to it and changes nothing for it.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: On each steady-state health check of a line using the strongSwan engine, the supervisor MUST read the remaining hard lifetime and the identity of the line's newest usable CHILD_SA from the same listing it already uses to judge the CHILD_SA's presence. It MUST NOT issue an extra query per check for this.
- **FR-002**: The supervisor MUST treat a CHILD_SA as having a **stuck rekey** when its remaining hard lifetime is below a fixed stuck-rekey threshold. The threshold MUST sit below the shortest remaining lifetime at which a natural rekey replaces the CHILD_SA under the daemon's default lifetimes (6 minutes), and far enough above zero to cover one health-check interval plus a tunnel rebuild. The default is 5 minutes.
- **FR-003**: The trigger MUST NOT depend on observing the CHILD_SA in a transient rekey state. A CHILD_SA in any usable state (installed, rekeying, replaced, updating) with a remaining lifetime under the threshold counts. A CHILD_SA with a remaining lifetime above it does not.
- **FR-004**: On a stuck rekey, the supervisor MUST perform a **planned rebuild**: the same bounded forced terminate, re-initiate and IMS agent restart as the existing stuck-CHILD_SA rebuild, including its wait for the new negotiation's P-CSCF pool before the agent restarts (spec 087).
- **FR-005**: The planned rebuild MUST be recorded under its own recovery reason, distinct from the existing missing-CHILD_SA and stuck-CHILD_SA reasons. The log line MUST name the line, the reason, and the CHILD_SA's remaining lifetime at the decision. When the IKE daemon's log shows a rekey rejection reason in the current session, the line MUST include it; a logged rejection MUST NOT trigger a rebuild by itself.
- **FR-006**: The supervisor MUST start at most one planned rebuild per stuck CHILD_SA, identified by its unique identity. The stuck-rekey state MUST clear as soon as a CHILD_SA with a different identity is the line's newest usable one.
- **FR-007**: A natural rekey MUST NOT trigger a planned rebuild in any of the shapes it takes in the listing: old and new CHILD_SAs side by side, the exchange in flight, or the new CHILD_SA alone.
- **FR-008**: The planned rebuild MUST act only on the affected line's connection and namespace. It MUST NOT terminate, re-initiate or restart anything belonging to another line on the shared IKE daemon.
- **FR-009**: The planned rebuild MUST count as an IKE_SA rebuild in the line's flapping window and toward `[alerts.tunnel_failure]`, exactly like the existing rebuild reasons. It MUST NOT add new alert types or config keys.
- **FR-010**: Existing recovery MUST be unchanged: the dead-daemon, broken-control-channel, missing-interface, missing-IKE_SA and missing-CHILD_SA checks MUST take precedence over the stuck-rekey check on the same health check, and the #118 two-strike recovery MUST still handle a CHILD_SA that hard-expires.
- **FR-011**: If the remaining lifetime is absent or unparseable, the supervisor MUST NOT rebuild on that check, MUST log the condition at most once per CHILD_SA, and MUST otherwise behave as it does today.
- **FR-012** *(User Story 3)*: When the line's call state shows an active call and the remaining lifetime is above a forced-rebuild margin, the supervisor MUST defer the planned rebuild and log the deferral once per stuck CHILD_SA. When the call ends, or the remaining lifetime falls below the forced-rebuild margin, it MUST perform the rebuild at that check. Unknown or stale call state MUST NOT defer the rebuild. The forced-rebuild margin MUST be large enough to cover one health-check interval plus a rebuild before the hard expiry. The default is 90 seconds.
- **FR-013**: The feature MUST NOT change tunnel proposals, lifetimes, traffic selectors, or the strongSwan build, and MUST NOT change the SWu engine's behaviour.

### Key Entities

- **CHILD_SA observation**: per line, per health check. The newest usable CHILD_SA's unique identity, its state, and its remaining hard lifetime, taken from the daemon's SA listing.
- **Stuck-rekey state**: per line, held between checks. The identity of the CHILD_SA already acted on (or deferred), so that each stuck CHILD_SA gets exactly one planned rebuild. It clears when the identity changes.
- **Recovery reason `ChildSaRekeyStuck`**: a new reason alongside the existing ones, visible in logs. It renegotiates and restarts the agent, so it counts as a rebuild for flapping and alerting.
- **Line call state** *(User Story 3)*: whether the line has an active call, as reported by the line's IMS agent, with enough freshness to tell "no call" from "no report".

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On a line whose ePDG rejects every rekey, the line's data path is restored before the old CHILD_SA's hard expiry. The outage it sees is one planned rebuild (target: under 15 seconds from the start of the rebuild to a usable CHILD_SA), instead of an expiry followed by about 60 seconds of failed recovery and a rebuild.
- **SC-002**: Over a 6-hour soak on a carrier with working rekeys (at least 5 natural rekeys), zero planned rebuilds are triggered.
- **SC-003**: Every stuck CHILD_SA causes exactly one planned rebuild, verified across the full replayed sequence of a #107-style failure window.
- **SC-004**: When one line has a stuck rekey, the other lines on the same host show zero tunnel rebuilds and zero agent restarts during the event.
- **SC-005**: An operator can tell a planned stuck-rekey rebuild from an expiry-driven one by reading the line's log alone, without the IKE daemon's log.
- **SC-006** *(User Story 3)*: A stuck rekey during an active call does not end the call early, unless the call is still running when the forced-rebuild margin is reached.

## Assumptions

- The project keeps the IKE daemon's default CHILD_SA lifetimes (soft lifetime 60 minutes with up to 6 minutes of randomisation, hard lifetime 66 minutes), as recorded in #107. If those are ever changed, the stuck-rekey threshold must be revisited. That is why it is stated relative to the natural-rekey window.
- The daemon's SA listing reports a remaining hard lifetime for each CHILD_SA, and a unique per-CHILD_SA identity that changes when a CHILD_SA is replaced. Both are visible in today's listing format. The exact behaviour of the reported values for an overdue rekey (for example a negative "rekeying in") will be confirmed against real output from the pinned daemon version during planning, by sampling a live line around a natural rekey.
- A carrier that rejects rekeys is not available on the project's test hardware (Vodafone, Jio). The stuck path is therefore proven with unit tests built from real listing output, plus a simulated stuck rekey on a live line. The natural-rekey path is proven live on Vodafone.
- A planned rebuild costs the same as today's stuck-CHILD_SA rebuild: a new IKE_SA, normally a new inner address, and an IMS re-registration. Keeping the old tunnel up while building the new one is out of scope, because the ePDG is assumed to allow only one session per subscriber.
- The IMS agent already tracks the number of active calls for metrics. User Story 3 assumes that count can be made available to the line's tunnel supervisor. It is not available there today.
- Health checks keep their current 30-second cadence on the strongSwan engine.
