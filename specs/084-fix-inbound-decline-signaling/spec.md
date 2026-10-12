# Feature Specification: Correct decline signaling on inbound IMS calls

**Feature Branch**: `084-fix-inbound-decline-signaling`
**Created**: 2026-10-11
**Status**: Draft
**Input**: User description: "Fix inbound IMS call decline signaling (issue #108): map telephone-side outcomes to the correct carrier final response using PJSIP's real last_status (genuine reject 486/600/603 -> 486 Busy Here; no-answer, 404/408/5xx/transport failure/unreadable -> 480), correct the inverted setup-failure code (PBX unreachable at setup currently 486, should be 480), keep To-tag continuity between 180 Ringing and every subsequent final response (inbound.rs setup-failure paths for SDP and offerless INVITEs, and the agent mod.rs generic error path), and recognise ACKs for recently-declined transactions so log_ack does not warn. Out of scope: the issue's proposed failed-INVITE response cache; UDP Timer G retransmission tracked separately. Applies to both VoWiFi and VoLTE (shared agent). Open: live verification on T2 no-answer, Jio VoLTE reject, 486 vs 603 semantics."

## Why this exists

[GitHub issue #108](https://github.com/selvakn/gsm-sip-bridge/issues/108):
on T2 (Russia), over VoWiFi, a person rejects an incoming call on the
telephone side. About 11 seconds later the phone rings again. The caller
never redialled and heard ringback the whole time.

The reporter's trace shows what happened. The bridge answered the carrier
`480 Temporarily Unavailable`, and the carrier acknowledged it. The carrier
then sent a **new** INVITE (new Call-ID, new From tag, new Via branch) for
the same caller. The bridge correctly treated it as a new call.

Root cause: every telephone-side outcome before answer is reported to the
carrier as `480`. That includes a person pressing reject.

- RFC 3261 §21.4.18 defines `480` as "unavailable… the user could also be
  available elsewhere". Networks are allowed to try another route, and T2
  does. It probably uses terminating access-domain selection (3GPP TS 23.292
  §7.4.3), but the trace can't prove that.
- RFC 3261 §13.3.1.3 says a UAS that is "not willing or able to take
  additional calls" SHOULD send `486 Busy Here`. With `486`, the network
  treats the call as busy (busy tone, or forward-on-busy) and doesn't retry.
- The reporter's local change to `486` stopped the repeat ring on T2.

Triage found four more problems the issue doesn't cover fully:

- **Every pre-answer telephone-side hangup is treated as a rejection.** That
  includes 404, 408, 5xx, transport failure, and losing track of the leg.
  Mapping all of these to `486` (the issue's suggestion) would report a
  broken phone leg to callers as "busy". The telephone-side SIP status is
  available and should decide the response.
- **The setup-failure code is backwards.** When the telephone side can't be
  reached during setup, the bridge says `486 Busy Here`. When a person
  actually rejects, it says `480`.
- **The To-tag changes between responses.** `180 Ringing` creates an early
  dialog with one To-tag. Three failure paths then send their final
  response with a new random tag. The final response then doesn't match the
  early dialog the carrier saw.
- **Metrics count no-answers as declines.** The telephone side's 45 s ring
  timer normally fires before the agent's 50 s one. Every failure reported
  while ringing is counted as `pbx_declined`, and so is "PBX unreachable at
  setup". The metrics can't tell a reject from a no-answer.

The issue's proposed failed-INVITE response cache is **out of scope**:

- It doesn't address the reported symptom. The second INVITE was a new
  transaction.
- Over TCP, clients never retransmit an INVITE. Over UDP, they stop once a
  provisional response arrives (RFC 3261 §17.1.1.2).
- The real UDP gap is resending the final response until the ACK arrives
  (Timer G, §17.2.1). That gap is tracked separately.

History: the decline-while-ringing path, and its blanket `480`, arrived with
`4e21ebd` (2026-07-13, v6.0.0). The setup-failure `486` and the first To-tag
mismatch date from `03e39e1` (v6.0.0). The generic-error-path tag mismatch
dates from `ced38f5` (v7.0.0).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A rejected call stays rejected (Priority: P1)

A person on the telephone side (PBX extension or SIP phone) sees an incoming
mobile call ringing and rejects it. The caller hears busy treatment (busy
tone, or voicemail if the subscriber has forward-on-busy). The phone does
not ring again for that call attempt.

**Why this priority**: This is the reported defect. A phone that rings
again after it was rejected looks broken, and the caller is left on
ringback with no feedback.

**Independent Test**: Place a call to a bridged line and reject it on the
telephone side. Check that the carrier receives a busy final response on
the same early dialog, acknowledges it, and offers no follow-up INVITE
within the observation window.

**Acceptance Scenarios**:

1. **Given** an inbound call ringing on the telephone side, **When** the
   telephone side rejects it with a busy or decline response, **Then** the
   carrier receives `486 Busy Here`, carrying the same To-tag as the
   earlier `180 Ringing`.
2. **Given** the rejection above, **When** the carrier acknowledges it,
   **Then** the bridge records the ACK as matching a call it declined, with
   no "unmatched ACK" warning.
3. **Given** the rejection above, **When** the same caller genuinely
   redials, **Then** the new call is admitted and rings normally. Redials
   are never suppressed.

---

### User Story 2 - Failures are not reported as "busy" (Priority: P1)

When the telephone side can't take the call for a reason other than a
person declining, the caller gets "temporarily unavailable" treatment, not
"busy". Those reasons are: it never answers, the extension is unknown,
the PBX times out or errors, the transport fails, or the bridge can't
build the telephone-side leg.

**Why this priority**: Without this, the P1 fix makes things worse for
every failure case. A broken PBX would look like a busy subscriber,
callers would be sent to busy-forwarding (often voicemail) for the wrong
reason, and logs would hide the real fault.

**Independent Test**: Drive each telephone-side failure in turn (no answer
until timeout, 404, 408, 503, leg unreachable at setup, leg state
unreadable). Check that the carrier receives `480 Temporarily Unavailable`
for each, with the early dialog's To-tag.

**Acceptance Scenarios**:

1. **Given** an inbound call ringing on the telephone side, **When**
   nobody answers before the ring timeout, or the telephone side gives up
   on its own timer with `480` or `408`, **Then** the carrier receives `480`
   and the call is recorded as no answer.
2. **Given** an inbound call ringing, **When** the telephone side ends the
   leg with a non-busy failure (for example 404 or a 5xx), or the
   leg's outcome can't be determined, **Then** the carrier receives `480`.
3. **Given** an inbound call, **When** the telephone-side leg can't be
   placed at all during setup, **Then** the carrier receives `480`, not
   `486`.
4. **Given** any of the above, **When** an operator looks at the call
   record and status output, **Then** "declined by a person", "never
   answered" and "telephone side failed" are distinguishable.

---

### User Story 3 - One dialog per call attempt (Priority: P2)

Every response the bridge sends for one inbound INVITE after the early
dialog is created carries one consistent To-tag. That includes provisional
responses, failures during setup, failures while ringing, cancellation
(`487`) and unexpected internal errors.

**Why this priority**: A mismatched tag is a protocol defect. Some
carriers tolerate it today, but it can confuse dialog matching upstream.
It doesn't cause the reported symptom on its own.

**Independent Test**: For each failure path reachable after `180 Ringing`,
capture the responses and check that the final response's To-tag equals
the `180`'s.

**Acceptance Scenarios**:

1. **Given** `180 Ringing` was sent, **When** the telephone side reports
   setup failure (SDP-bearing or offerless INVITE), **Then** the final
   response carries the `180`'s To-tag.
2. **Given** `180 Ringing` was sent, **When** call handling fails with an
   unexpected internal error, **Then** the fallback final response carries
   the `180`'s To-tag.
3. **Given** a final response sent before any provisional response that
   carries a tag (for example `420`, `415` or `488` for an unacceptable
   offer), **When** it is sent, **Then** behaviour is unchanged.

---

### Edge Cases

- **The caller cancels at the same moment the telephone side rejects.**
  Exactly one final response goes to the INVITE. If the CANCEL is processed
  first, the existing `200`/`487` handling applies and no `486` follows.
- **The telephone side answers 486 to say a genuinely busy line**, as
  opposed to a person pressing reject. This is treated as busy too, which
  is the correct meaning either way.
- **The telephone side sends `600 Busy Everywhere` or `603 Decline`.** It is
  treated as declined, and the carrier receives `486` (FR-002). The original
  status is kept in the call record.
- **The telephone side returns a status this spec doesn't name** (for
  example 401, 404, 407 or 488). It is treated as a failure (`480`) and logged
  with its original status.
- **No telephone-side status is available** (the leg vanished, or its
  status couldn't be read). It is treated as a failure (`480`) and never as
  busy.
- **The telephone side's own no-answer timer fires first** (for example a
  PBX that stops ringing after 30 s and answers `480` or `408`). This is
  recorded as no answer, not failed. The carrier receives `480`.
- **The bridge's own ring timeout fires before the telephone side reports
  anything.** This stays a no-answer outcome (`480`), and the telephone
  side is told to stop ringing.
- **The carrier offers a fresh INVITE after a `480`** (T2's behaviour) for
  no-answer or failure cases. It is still admitted as a new call. This spec
  doesn't try to suppress carrier re-attempts; see Open questions.
- **An ACK arrives for a declined call after the record of it has expired.**
  It is logged at the existing warning level, which is acceptable.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The bridge MUST classify every pre-answer end of the
  telephone-side leg into exactly one outcome: **declined** (the telephone
  side answered busy or decline: 486, 600 or 603), **no answer** (the
  bridge's ring timeout expired, or the telephone side gave up on its own
  no-answer timer and answered 480 or 408), **failed** (any other final
  status, transport failure, or an outcome that can't be determined) or
  **caller gone** (the caller cancelled first). The classification MUST be based on
  the telephone side's actual final status, not on the leg simply having
  ended.
- **FR-002**: For a **declined** outcome, the bridge MUST answer the
  carrier's INVITE with `486 Busy Here` (RFC 3261 §13.3.1.3). This applies
  whether the telephone side sent 486, 600 or 603. A telephone-side
  600/603 is recorded with its original status (FR-007), but the carrier
  still receives `486`.
- **FR-003**: For **no answer** and **failed** outcomes, the bridge MUST
  answer the carrier's INVITE with `480 Temporarily Unavailable`.
- **FR-004**: When the telephone-side leg can't be placed during call setup,
  the bridge MUST answer `480 Temporarily Unavailable`, replacing today's
  `486 Busy Here`. This applies to both SDP-bearing and offerless INVITEs.
- **FR-005**: Once the bridge has sent any provisional response carrying a
  To-tag for an inbound INVITE, every later response to that INVITE MUST
  carry the same To-tag. This includes the final response on every failure
  path and the fallback response after an unexpected internal error.
- **FR-006**: The bridge MUST keep a short-lived record of inbound INVITEs
  it declined. It MUST log an ACK that matches one of those records as an
  expected acknowledgement, not as an unmatched ACK. The record MUST be
  bounded in time and size, and MUST NOT affect whether any new INVITE is
  admitted.
- **FR-007**: The call record and live status output MUST distinguish the
  declined, no-answer and failed outcomes. For the failed outcome, the
  record MUST include the telephone side's original status when one was
  available.
- **FR-007a**: The `bridge_failed` metric's reason MUST follow the same
  classification:
  - **declined** → `pbx_declined`, and nothing else maps to it
  - **no answer** (from any timer) → `ring_timeout`
  - **failed** → `pbx_failed`, a new reason value
  - **telephone-side leg can't be placed during setup** →
    `bridge_setup_failed`
  - `caller_cancelled`, `agent_unreachable` and `one_way_audio` are
    unchanged

  The new value MUST be documented wherever the reason set is documented.
- **FR-008**: Behaviour MUST be identical on VoWiFi and VoLTE lines, since
  both use the same inbound call handling.
- **FR-009**: Existing responses that are already correct MUST NOT change:
  - `486` when another call already occupies the line
  - `480` when the PBX registration is down
  - `487` after a caller's CANCEL
  - `420`, `415`, `488` and `580` for unacceptable INVITEs
  - the `200 OK` path for answered calls
- **FR-010**: The bridge MUST NOT suppress or delay admission of an inbound
  INVITE because of the caller's identity or a recent decline. Genuine
  redials MUST always ring.
- **FR-011**: Releasing this change MUST be gated on live verification on
  two rigs:
  - **Local Vodafone rig:** a reject, a no-answer and a setup failure.
  - **Jio VoLTE Pi:** a reject and a no-answer.

  Each must show the required final response, a matching ACK, and no
  follow-up INVITE. T2 can't be tested from here, so the issue reporter is
  asked to confirm the T2 reject and no-answer behaviour after release.

### Key Entities

- **Telephone-side outcome**: why the telephone-side leg ended before
  answer. It is one of declined, no answer, failed or caller gone, plus the
  original telephone-side status when known. It decides the carrier final
  response and is shown in call records.
- **Declined-call record**: a short-lived note of an inbound INVITE the
  bridge answered with a failure. It holds enough to recognise that
  INVITE's ACK, and is used for diagnostics only.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On a carrier that re-offers after "temporarily unavailable"
  (T2), a call rejected on the telephone side rings the telephone side
  exactly once. There are zero follow-up offers within 60 seconds in 10 of
  10 attempts.
- **SC-002**: In automated tests, each telephone-side outcome (declined via
  486, 600 and 603; no answer via the bridge's timer and via a
  telephone-side 480 or 408; failed via 404, 503, transport and unknown;
  setup failure) produces the response this spec requires, in
  100% of cases.
- **SC-003**: For every failure path reachable after `180 Ringing`, the
  final response's To-tag equals the `180`'s, in 100% of automated test
  cases.
- **SC-004**: A normal rejected call produces zero "unmatched ACK" warnings
  in the bridge log.
- **SC-005**: An operator can tell from the call record or the
  `bridge_failed` metric alone, without reading SIP traces, whether an unanswered inbound call was declined by a
  person, never answered, or failed on the telephone side.
- **SC-006**: A genuine redial by the same caller within 5 seconds of a
  rejection rings normally, in 100% of test cases.

## Assumptions

- The telephone side's final SIP status is available to the bridge when the
  leg ends, from the SIP stack's call information. If it can't be read,
  the outcome is "failed" (`480`). It is never assumed to be busy.
- Keeping `480` for no-answer is correct. RFC 3261 §21.4.18 and carrier
  no-reply forwarding both expect it. Whether T2 also re-offers after a
  no-answer `480` is an open question, not a requirement here.
- A person pressing reject on a PBX or SIP phone is signalled to the bridge
  as 486, 600 or 603. These are what common PBXs and phones send. Gigaset
  defaults to 603, and most others send 486.
- The decline record (FR-006) only needs to cover the normal ACK delay:
  seconds, at most Timer H (about 32 s). It doesn't survive a bridge
  restart.
- No real subscriber numbers appear in tests or fixtures. Synthetic numbers
  such as `+919000000000` are used.

## Out of scope

- The failed-INVITE response cache proposed in issue #108, and replaying a
  stored response to a retransmitted INVITE.
- Resending the final response over UDP until the ACK arrives (RFC 3261
  §17.2.1 Timer G). Tracked separately.
- Suppressing carrier re-attempts by caller identity.
- Any change to outbound (mobile-originated) call failure mapping.

## Clarifications

### Session 2026-10-11

- Q: What should the carrier receive when the telephone side declines (486,
  600 or 603)? → A: Always `486 Busy Here`.
- Q: Which live verification gates release? → A: The local Vodafone rig and
  the Jio VoLTE Pi. The T2 reporter confirms after release.
- Q: How is a telephone side that answers `480` or `408` (its own no-answer
  timer) classified? → A: As no answer. Other non-busy codes are failed.
- Q: Should the `bridge_failed` metric reasons be corrected? → A: Yes:
  - `pbx_declined` counts only declines.
  - No-answer counts as `ring_timeout`, from any source.
  - A new reason `pbx_failed` counts other telephone-side failures.
  - PBX unreachable at setup counts as `bridge_setup_failed`.

## Open questions (not blocking the spec)

- Does T2 also re-offer after a no-answer `480`? If it does, an unanswered
  call rings twice (about 90 s in total). This needs a live test by the
  reporter.
- Should a telephone-side `603` ever be passed through as `603`? Decided
  no for now (always `486`). Revisit only if a carrier is seen treating
  `486` badly.
- GSMA IR.92's exact expectation for a user-rejected call (486 or 603)
  hasn't been confirmed from the source text.
