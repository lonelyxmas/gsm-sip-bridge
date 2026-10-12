# Feature Specification: ePDG CHILD_SA health detection and bounded recovery

**Feature Branch**: `086-epdg-child-sa-recovery`  
**Created**: 2026-10-12  
**Status**: Draft  
**Input**: User description: "ePDG CHILD_SA health detection and bounded recovery (GitHub issue #107, items A, B, C, E). On a Tele2 RU ePDG, CHILD_SA rekeys were rejected with INVALID_SELECTORS until the CHILD_SA hard-expired, causing a ~170s VoWiFi outage that our supervisor amplified. A) the steady-state CHILD_SA check matches the IKE_SA header line, not the CHILD_SA entry, so a missing CHILD_SA under a live IKE_SA is never detected; B) the CHILD_SA-missing recovery only re-initiates inside the existing IKE_SA and never escalates; C) the steady-state IKE terminate is unbounded and waited 52s behind a pending exchange; E) the `vowifi_tunnel_up` gauge is a registration-liveness proxy that read 1 through the outage — export real CHILD_SA state as its own gauge. Out of scope: the carrier-specific rekey selector fix."

## Background

A VoWiFi line's data path is an **IKE_SA** (the control session with the carrier's ePDG) carrying a **CHILD_SA** (the encrypted ESP tunnel that actually carries SIP and RTP). The two have independent lifetimes: the CHILD_SA can be gone while the IKE_SA is still up and still answering the ePDG's liveness checks. In that state the line looks alive but cannot carry a call.

Issue #107 captured exactly this on a Tele2 RU ePDG. The ePDG rejected every CHILD_SA rekey (a carrier-specific selector incompatibility, out of scope here), the CHILD_SA hard-expired, and the line was dark for 170 seconds:

| Time | What happened | Why it took so long |
|---|---|---|
| T+0 | CHILD_SA hard-expired; tunnel address and route removed | — |
| T+23s | Supervisor reinstated the default route | The CHILD_SA check reported "present" because it matched the IKE_SA, so the missing tunnel was diagnosed as a missing route |
| T+23s → T+114s | Waited out the 90s "P-CSCF unreachable" window | The route repair reset that window's counter |
| T+114s | Supervisor asked for the IKE_SA to be terminated | — |
| T+166s | Terminate finally completed | It queued behind a CHILD_SA request the ePDG never answered |
| T+170s | Fresh IKE_SA + CHILD_SA up | — |

Throughout, `gsm_sip_bridge_vowifi_tunnel_up` read 1, because it reflects whether the IMS agent last registered successfully, not whether a CHILD_SA exists.

On the carriers this project runs on day to day (Vodafone, Jio, Airtel) rekeys succeed, so this path is rare. The point of this feature is that **when** the CHILD_SA is lost, for any reason, the bridge notices within one supervision tick, recovers within about a minute, and tells the operator the truth.

## Clarifications

### Session 2026-10-12

- Q: Should a sustained CHILD_SA outage raise the existing critical alert? → A: Yes. It feeds the existing tunnel-failure alert (`[alerts.tunnel_failure]`), with the same enable switch and the same `unhealthy_sec` threshold. No new alert type or setting.
- Q: Which recovery paths use the non-waiting, time-bounded IKE_SA teardown? → A: All of them (interface vanished, P-CSCF unreachable, stuck CHILD_SA, broken control channel). Each is tearing down to rebuild, so waiting on the old IKE_SA never helps.
- Q: How many consecutive CHILD_SA-missing checks before rebuilding the IKE_SA? → A: Two (~60s): one cheap in-IKE re-initiate first, then a full rebuild.
- Q: What happens when the rebuild itself keeps failing (e.g. ePDG unreachable)? → A: No backoff. The supervisor keeps rebuilding every two consecutive misses (~60s) for as long as the CHILD_SA stays missing.
- Q: Does the CHILD_SA metric report anything before a line's tunnel first comes up? → A: Yes. It reads "down" from the moment supervision of the line starts, through initial establishment, and "up" once the tunnel is established. A line that never establishes after boot is then visible to the gauge and to the tunnel-failure alert.
- Q: What has to pass before this merges? → A: Automated tests plus a blocking live gate on the local Vodafone line. The gate needs at least one natural CHILD_SA rekey with no false recovery, and one induced CHILD_SA loss that is recovered and visible on the gauge. Results are recorded in the spec folder.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A lost CHILD_SA is detected and repaired within about a minute (Priority: P1)

An operator runs a VoWiFi line. The CHILD_SA disappears while the IKE_SA stays up — a hard expiry after failed rekeys, an ePDG-side delete, anything. The bridge notices on its next health check, first tries the cheap repair (a new CHILD_SA inside the existing IKE_SA), and if that has not worked by the following check, tears the IKE_SA down and builds a fresh one. The line is back and registered without operator action, and incoming calls ring again.

**Why this priority**: This is the outage itself. Detection (#107 item A) and escalation (item B) are only useful together: detection alone would leave the line retrying the cheap repair forever, which is precisely the repair that does not work when the ePDG ignores CHILD_SA requests on the old IKE_SA.

**Independent Test**: On a running line, remove only the CHILD_SA while leaving the IKE_SA up. Confirm the supervisor logs the CHILD_SA as missing on its next check and re-initiates it. Separately (in automated tests, since a cooperative carrier cannot be made to ignore the request), confirm that a CHILD_SA still missing on the second consecutive check leads to a full IKE_SA rebuild and an IMS agent restart.

**Acceptance Scenarios**:

1. **Given** a line with an established IKE_SA and no usable CHILD_SA, **When** the supervisor's next health check runs, **Then** it reports the CHILD_SA as missing and requests a new CHILD_SA inside the existing IKE_SA, without tearing the IKE_SA down and without restarting the IMS agent.
2. **Given** the CHILD_SA was reported missing on the previous check, **When** the next check still finds no usable CHILD_SA, **Then** the supervisor terminates the line's IKE_SA, initiates a fresh one, and restarts the line's IMS agent so it re-registers over the new tunnel.
3. **Given** the CHILD_SA was reported missing once, **When** the next check finds it present again, **Then** no escalation happens and the miss counter starts from zero.
4. **Given** a CHILD_SA rekey is in progress (old and new CHILD_SA briefly coexist, or the old one is mid-rekey), **When** a health check runs, **Then** the CHILD_SA is reported present and no recovery is taken.
5. **Given** a line whose CHILD_SA is missing and whose default route is also missing, **When** a health check runs, **Then** the missing CHILD_SA is acted on, rather than the route being repaired and the line declared recovered.
6. **Given** two lines sharing one IKE daemon, **When** only line 1's CHILD_SA is missing, **Then** only line 1 is recovered and line 0's tunnel is untouched.

---

### User Story 2 - Tearing down a stuck IKE_SA never blocks recovery (Priority: P1)

When recovery decides to rebuild a line's tunnel, the teardown of the old IKE_SA completes promptly even if that IKE_SA is stuck waiting on an exchange the ePDG will never answer. The rebuild starts right after.

**Why this priority**: In #107 the teardown alone added 52 seconds. Without this, the escalation in Story 1 still inherits that wait. It shares P1 because the minute-scale recovery target depends on both.

**Independent Test**: In automated tests, confirm the teardown request is issued in its non-waiting form and bounded by a hard time limit. Live, confirm a recovery-driven teardown returns within seconds.

**Acceptance Scenarios**:

1. **Given** a line's IKE_SA with an outstanding unanswered request, **When** recovery tears that IKE_SA down, **Then** the teardown returns without waiting for the outstanding request to time out.
2. **Given** the IKE daemon's control interface is unresponsive, **When** recovery tears down an IKE_SA, **Then** the teardown is abandoned after a fixed time limit and recovery continues.
3. **Given** any recovery path that tears down an IKE_SA (missing interface, unreachable P-CSCF, stuck CHILD_SA, broken control channel), **When** it runs, **Then** it uses the same bounded teardown.

---

### User Story 3 - The operator can see whether the encrypted tunnel actually exists (Priority: P2)

An operator watching metrics, or a dashboard built on them, can see per line whether a usable CHILD_SA exists right now, separately from whether the line is registered. During an outage like #107's, that signal reads "down" while the registration signal may still read "up".

**Why this priority**: It does not shorten an outage, but it is how an operator distinguishes "tunnel gone" from "registration failed" and confirms the recovery in Stories 1–2 actually happens. Without it, #107's outage was invisible in metrics.

**Independent Test**: Scrape the metrics endpoint of a healthy line and confirm the new signal reads up. Remove the CHILD_SA and confirm it reads down at the next check, then up again after recovery.

**Acceptance Scenarios**:

1. **Given** a healthy VoWiFi line, **When** metrics are scraped, **Then** a per-line CHILD_SA signal reads up.
2. **Given** a line whose CHILD_SA is missing, **When** the next health check has run, **Then** that line's CHILD_SA signal reads down, while the existing tunnel/registration signals keep their current meaning.
3. **Given** the supervisor stops reporting (crashed or wedged), **When** its reports go stale, **Then** the CHILD_SA signal for its lines reads down rather than a frozen "up".
4. **Given** a line on the legacy SWu tunnel engine (no CHILD_SA visibility), **When** metrics are scraped, **Then** no CHILD_SA signal is reported for it rather than a misleading value.
5. **Given** a line whose tunnel has never come up since boot (still in initial establishment), **When** metrics are scraped, **Then** its CHILD_SA signal reads down.
6. **Given** the tunnel-failure critical alert is enabled, **When** a line's CHILD_SA has been down continuously for that alert's configured unhealthy duration, **Then** the operator receives the tunnel-failure alert, even if the line's registration signal still reads up.

---

### Edge Cases

- **Rekey overlap**: during every successful rekey the old and new CHILD_SA coexist for a moment, and a failing rekey leaves the old one in a "rekeying" state while it still carries traffic. Neither is an outage.
- **CHILD_SA mid-installation or mid-deletion**: transient states that last milliseconds; treated as not usable. A false "missing" here costs one cheap re-initiate, never an escalation, because escalation needs two consecutive misses ~30s apart.
- **IKE_SA itself re-establishing** (no CHILD_SA yet because the IKE_SA is still connecting): counts as missing. A second consecutive miss rebuilds, which is no worse than the daemon's own retry.
- **Tunnel interface missing at the same time**: the existing interface-vanished recovery takes precedence, as today; the CHILD_SA miss counter resets.
- **Both a stuck CHILD_SA and a P-CSCF change**: the CHILD_SA recovery runs first; the P-CSCF record is resynchronised after the rebuild, as existing recoveries already do.
- **Rebuild keeps failing** (ePDG unreachable, Wi-Fi down): the line rebuilds about every 60 seconds, each time restarting the IMS agent, until a CHILD_SA appears. This is accepted: a line with no CHILD_SA carries nothing either way, and the rebuilds stop on their own once a CHILD_SA is seen.
- **Metrics daemon not running**: supervisor reports are buffered and dropped per the existing observability rules; supervision itself is unaffected.
- **Older agent on the other end of the protocol**: a report without the new field must still be accepted, and the new field must not break an older reader. A daemon older than this feature would reject the supervisor's reports. That pairing cannot occur, because both come from the same binary (FR-016).

## Requirements *(mandatory)*

### Functional Requirements

**Detection (#107 A)**

- **FR-001**: The supervisor MUST judge a line's tunnel healthy only when a CHILD_SA belonging to that line's own connection exists in a usable state: installed, rekeying, rekeyed, or updating. An established IKE_SA with no such CHILD_SA MUST be reported as CHILD_SA missing.
- **FR-002**: A CHILD_SA in any other state (being created, being installed, being deleted, etc.), or no CHILD_SA at all, MUST count as missing.
- **FR-003**: Another line's CHILD_SA MUST never count toward this line's health.

**Escalation (#107 B)**

- **FR-004**: On the first consecutive check that finds the CHILD_SA missing, the supervisor MUST request a new CHILD_SA inside the existing IKE_SA, and MUST NOT tear down the IKE_SA or restart the IMS agent (unchanged from today).
- **FR-005**: When the CHILD_SA is found missing on two consecutive checks, the supervisor MUST tear down the line's IKE_SA, initiate a fresh one, and restart the line's IMS agent. This MUST be logged as its own distinct recovery reason, separate from a plain CHILD_SA re-initiate.
- **FR-006**: Any check that does not find the CHILD_SA missing MUST reset the consecutive-miss count. An escalation MUST also reset it.
- **FR-006a**: There is no backoff between escalations. While the CHILD_SA stays missing, the cycle "re-initiate on the first miss, rebuild on the second" repeats unchanged, so a rebuild (with an IMS agent restart) happens about every 60 seconds until a usable CHILD_SA is seen.
- **FR-007**: A missing CHILD_SA MUST be acted on before, and instead of, the route-repair and P-CSCF-reachability logic in the same check.
- **FR-008**: Recovery for one line MUST NOT affect any other line's IKE_SA or CHILD_SA.

**Bounded teardown (#107 C)**

- **FR-009**: Every supervisor-initiated IKE_SA teardown during steady state MUST NOT wait for outstanding exchanges on that IKE_SA to complete or time out.
- **FR-010**: Every such teardown MUST be abandoned after a fixed time limit of 10 seconds if the IKE daemon does not respond, after which recovery proceeds.

**Visibility (#107 E)**

- **FR-011**: The system MUST export a per-line metric stating whether that line currently has a usable CHILD_SA, reported by the supervisor from the same health check that drives recovery (no additional queries).
- **FR-012**: The new metric MUST read "down" from the moment the supervisor starts working on a line and stay "down" through initial tunnel establishment, including retries. It MUST switch to "up" when the tunnel is established, and be updated after every steady-state check from then on.
- **FR-013**: If the supervisor's reports for a line go stale, the metric MUST read "down" for that line, following the existing stale-reporter rules.
- **FR-014**: The existing `vowifi_tunnel_up` and `vowifi_registered` metrics MUST keep their current meaning and behaviour.
- **FR-015**: Lines on the SWu tunnel engine MUST NOT report the new metric.
- **FR-016**: The new state field MUST remain compatible in both directions: reports lacking it are accepted, and it does not break readers that do not know it. The new reporter identity (the supervisor) only needs to be understood by a daemon of the same release. The daemon and the supervisor ship as one binary in one image, so a mixed-version pair is not a supported deployment.
- **FR-017**: The new metric MUST be documented alongside the existing VoWiFi health metrics, including how it differs from `vowifi_tunnel_up`.
- **FR-017a**: A line's CHILD_SA being down MUST count toward the existing tunnel-failure critical alert. It uses that alert's enable switch and `unhealthy_sec` threshold, and its existing repeat/recovery-notice behaviour. The alert MUST fire when either signal (tunnel liveness or CHILD_SA) has been unhealthy continuously for the threshold, and MUST NOT fire twice for one outage seen by both signals.
- **FR-017b**: The alert's documentation and config comment MUST say that it now also covers a missing CHILD_SA.

**Scope guard**

- **FR-018**: The tunnel negotiation itself (requested inner address families, traffic selectors, proposals, the IKE daemon build) MUST NOT change. Carriers whose rekeys succeed today MUST see identical negotiation.

### Key Entities

- **Line tunnel health**: per line, per check — tunnel interface present; IKE daemon reachable; usable CHILD_SA present. Drives recovery and the new metric.
- **Consecutive CHILD_SA miss count**: per line, survives between checks, reset on any non-missing check or on escalation.
- **Recovery reason**: the logged label of each recovery. Gains one new value for "CHILD_SA still missing, IKE_SA rebuilt".
- **CHILD_SA metric**: per line (labelled by module, like the other VoWiFi gauges); up/down; owned by the supervisor's reports.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: When a line loses its CHILD_SA while its IKE_SA survives, the loss is detected within one supervision interval (≤ 30 seconds).
- **SC-002**: When the ePDG does not honour a new CHILD_SA inside the surviving IKE_SA, a fresh IKE_SA is being negotiated within about 60 seconds of the loss (two supervision intervals), against 114 seconds in #107.
- **SC-003**: A recovery-driven IKE_SA teardown completes within 10 seconds in every case, against 52 seconds in #107.
- **SC-004**: Across at least one natural CHILD_SA rekey on a healthy line, zero CHILD_SA-missing recoveries are triggered (no false positives from rekey overlap).
- **SC-005**: During a CHILD_SA outage, the per-line CHILD_SA metric reads down within one supervision interval, and reads up again within one interval of the tunnel being restored.
- **SC-006**: Lines on Vodafone/Jio/Airtel show no change in tunnel negotiation, rekey behaviour or recovery count after deployment. This is observed after release, not a merge gate. Before merge it is guaranteed structurally: the change touches no tunnel configuration or IKE daemon build (FR-018), and SC-008(a) checks rekeys on Vodafone.
- **SC-007**: With the tunnel-failure alert enabled, a CHILD_SA outage that outlasts the configured threshold produces exactly one alert, even while registration still reads up.
- **SC-008** (merge gate): Before merge, all automated tests pass, and a live run on the local Vodafone line is recorded in this spec folder showing:
  - (a) at least one natural CHILD_SA rekey with zero CHILD_SA-missing recoveries and the gauge holding at up;
  - (b) one induced CHILD_SA loss with the IKE_SA kept up, detected within one supervision interval, recovered without operator action, and seen on the gauge as down and then up again.
  The escalation (IKE_SA rebuild) path is exempt from the live gate and is proven by automated tests.

## Assumptions

- The supervision interval stays at 30 seconds for the strongSwan engine. The two-miss threshold is defined in checks, which is only meaningful because the SWu engine never reports a CHILD_SA state.
- The IKE daemon in the shipped image (strongSwan 5.9.3) supports a non-waiting IKE_SA teardown. Confirmed: its `swanctl --terminate` offers `--force`.
- Tearing an IKE_SA down without waiting means the ePDG may not receive our delete. That is acceptable: the ePDG detects the dead session through its own liveness checks, and a fresh IKE_SA for the same subscriber replaces the old one.
- The fresh IKE_SA may come with a new inner address and P-CSCF pool, which is why the IMS agent is restarted on escalation and why the existing post-recovery P-CSCF resync applies.
- The live merge gate (SC-008) runs on the local Vodafone line. Inducing a CHILD_SA loss with the IKE_SA kept up is possible there, but making the ePDG *ignore* the re-initiate is not, so the escalation path is verified by automated tests. The #107 reporter's Tele2 deployment is the only environment that reproduces the original failure end to end.
- Out of scope: the carrier-specific rekey rejection itself (the proposed strongSwan selector-cache patch, or an opt-in inner-address-family setting), which is tracked separately on #107.
