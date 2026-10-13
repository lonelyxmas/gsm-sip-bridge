# Feature Specification: Reg-event SUBSCRIBE transaction and dialog tracking

**Feature Branch**: `090-reg-event-subscription-tracking`  
**Created**: 2026-10-14  
**Status**: Draft  
**Input**: User description: "Reg-event SUBSCRIBE transaction and dialog tracking (GitHub issue #124, PR 2 of 3; builds on spec 089). Track the reg-event subscription per line; correlate SUBSCRIBE responses; Timer F on the transaction (qualifying endpoint failure → forced re-registration, later P-CSCF failover); Timer N for the first NOTIFY (subscription failure only); match NOTIFYs to their subscription dialog (superseded → ignore, unknown → 481); honour Subscription-State reasons; refresh before expiry; report registration, subscription and Gm transport health separately. Out of scope: P-CSCF failover (spec 091), attribution and per-event policy (spec 089)."

## Background

After every successful registration, each IMS line subscribes to its own registration-state event package (`Event: reg`, TS 24.229 §5.1.1.3). The network answers the SUBSCRIBE with a response and then sends NOTIFYs inside the subscription's dialog; those NOTIFYs are the only way the line learns that the network has dropped or changed its registration (spec 089 decides what each one means).

Today the SUBSCRIBE is sent and forgotten:

| Gap | Effect | Standard |
|---|---|---|
| The SUBSCRIBE's response is never matched to it; it is logged as "received response outside an active transaction". | An error response (`423`, `489`, `503`) changes nothing; the line stays without a subscription until its next registration. | RFC 6665 §4.1.2.1, TS 24.229 §5.1.2.2 |
| No transaction timeout (Timer F). | An unanswered SUBSCRIBE goes unnoticed. In #124 the line's last SUBSCRIBE got no response and no NOTIFY while the line reported `Registered`. TS 24.229 §5.1.2A.1.6 says such a timeout (or a fatal transport error) means the contact is no longer bound. | RFC 3261 §17.1.2.2 (64·T1; applies over TCP too), TS 24.229 §5.1.2A.1.6 |
| No wait for the first NOTIFY (Timer N). | A subscription accepted but never confirmed is indistinguishable from a working one. | RFC 6665 §4.1.2.4 |
| Every NOTIFY is answered `200 OK` and acted on, whichever subscription it belongs to. | A NOTIFY from an old subscription (e.g. `terminated;reason=noresource` after a renewal, seen ~478 times in #124) is applied to the current registration. | RFC 6665 §4.4.1 (unknown subscription → `481`) |
| `Subscription-State` is logged but ignored. | A terminated subscription is never re-established until the next registration, up to an hour later. | RFC 6665 §4.1.3 |
| The subscription is only ever re-created by a registration renewal. | If the network grants a shorter subscription than registration, the line silently loses its notifications partway through. | RFC 6665 §4.1.2.2 |

In the IMS, T1 is 2 s, so Timer F and Timer N are both 128 s (TS 24.229 table 7.7.1).

## Clarifications

### Session 2026-10-14

- Q: On a carrier that never answers the SUBSCRIBE while the Gm keepalive (OPTIONS) is still answered, should Timer F on the SUBSCRIBE alone force a re-registration? → A: Confirm with a probe first. When Timer F fires (or the send fails), send an immediate Gm keepalive. No answer within the keepalive's own deadline (or the probe cannot be sent) → qualifying endpoint failure and forced re-registration. Answered → the path is alive, so it is a subscription failure only: re-subscribe on backoff. A deliberate, narrow deviation from TS 24.229 §5.1.2A.1.6, accepted to avoid re-registration churn on carriers that ignore SUBSCRIBE.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A dead signalling path is detected through the unanswered SUBSCRIBE (Priority: P1)

An operator's line registers successfully, but the path to the P-CSCF dies before the line's SUBSCRIBE is answered (or the send fails outright). Within about two minutes the bridge concludes the registration is no longer usable, reports it, and re-registers, instead of sitting in `Registered` while unreachable.

**Why this priority**: This is the silent failure #124 ended in (`Registered` locally, no SUBSCRIBE answer, no inbound calls), and it is the trigger spec 091 needs for P-CSCF failover.

**Independent Test**: Send a SUBSCRIBE into a transport that accepts it but never answers, with an injected clock. Confirm nothing happens before 128 s; at 128 s a confirming keepalive is sent; if it also goes unanswered the registration is reported failed and a re-registration is forced, and if it is answered only a subscription failure is logged.

**Acceptance Scenarios**:

1. **Given** a SUBSCRIBE sent at t=0 and no response, **When** t reaches 128 s, **Then** an immediate Gm keepalive probe is sent; **and when** that probe also goes unanswered, **Then** the line reports a qualifying endpoint failure, shows registration failed, and a re-registration is forced.
2. **Given** the SUBSCRIBE send itself fails with a transport error, **When** the confirming probe also fails, **Then** the same qualifying endpoint failure is raised.
3. **Given** a SUBSCRIBE unanswered at 128 s, **When** the confirming keepalive probe is answered, **Then** no endpoint failure is raised, registration stays `Registered`, a subscription failure is logged, and the SUBSCRIBE is retried on backoff.
4. **Given** a final response arrives at t=100 s, **When** t reaches 128 s, **Then** nothing is raised.
5. **Given** a response arrives for an older SUBSCRIBE that has since been replaced, **When** it is processed, **Then** it neither completes nor fails the current SUBSCRIBE.

---

### User Story 2 - The subscription is confirmed, kept alive, and re-established when lost (Priority: P1)

The bridge knows whether its line actually has a working reg-event subscription: accepted by the network **and** confirmed by a first NOTIFY. It refreshes the subscription before it expires, and re-establishes it when the network ends it for a reason that allows that. A subscription that never gets confirmed is a subscription problem only; the registration is not torn down for it.

**Why this priority**: Without a working subscription, network-side deregistrations are invisible (the reason spec 089 exists at all).

**Independent Test**: Drive the subscription state machine with an injected clock through: 2xx with no NOTIFY; NOTIFY before 2xx; a granted expiry shorter than the registration; and each `Subscription-State: terminated` reason.

**Acceptance Scenarios**:

1. **Given** a `200 OK` to the SUBSCRIBE and no NOTIFY, **When** 128 s pass, **Then** a subscription-confirmation failure is logged, a new SUBSCRIBE is sent on backoff, and registration state stays `Registered`.
2. **Given** the first NOTIFY arrives before the SUBSCRIBE's `200 OK`, **When** the `200 OK` then arrives, **Then** the subscription is active and confirmed, and no Timer N failure follows.
3. **Given** the network granted `Expires: 600`, **When** the subscription nears expiry, **Then** it is refreshed in the same dialog before it expires, independent of the registration's own schedule.
4. **Given** `Subscription-State: terminated;reason=deactivated` (or `timeout`), **When** processed, **Then** a new subscription is started immediately.
5. **Given** `terminated;reason=probation;retry-after=300` (or `giveup`), **When** processed, **Then** a new subscription is started no earlier than 300 s later.
6. **Given** `terminated;reason=noresource` (or `rejected`), **When** processed for the current subscription, **Then** no new subscription is started for this registration; the next registration starts one.
7. **Given** `terminated` with no reason, **When** processed, **Then** a new subscription is started on backoff.
8. **Given** a registration renewal succeeds, **When** the new registration is in place, **Then** a fresh subscription is started and nothing still pending from the old one (timers, retries) can affect it.

---

### User Story 3 - NOTIFYs are applied only to the subscription they belong to (Priority: P2)

A NOTIFY is matched to its subscription dialog before anything acts on it. A NOTIFY for the line's current subscription is processed (spec 089). A NOTIFY for one of the line's earlier, replaced subscriptions is acknowledged and logged but never changes registration state. A NOTIFY that matches no subscription the line has made is refused with `481`.

**Why this priority**: Spec 089's address-based attribution already prevents the #124 loop; dialog matching is defence in depth and brings the bridge in line with RFC 6665 §4.4.1.

**Independent Test**: With a known current and previous subscription, deliver NOTIFYs carrying each one's identifiers plus one with unknown identifiers, and observe the response code and whether registration state changed.

**Acceptance Scenarios**:

1. **Given** a NOTIFY carrying the current subscription's Call-ID and the line's tag, **When** processed, **Then** it is answered `200 OK` and evaluated (spec 089).
2. **Given** a NOTIFY carrying a previous subscription's identifiers and reporting the line's binding `terminated/rejected`, **When** processed, **Then** it is answered `200 OK`, logged as belonging to a superseded subscription, and registration state is unchanged.
3. **Given** a NOTIFY whose identifiers match no subscription the line made, **When** processed, **Then** it is answered `481 Call/Transaction Does Not Exist` and has no other effect.

---

### User Story 4 - SUBSCRIBE error responses are handled (Priority: P2)

When the network refuses a SUBSCRIBE, the bridge reacts to the specific refusal instead of ignoring it.

**Why this priority**: Rare, but each one otherwise leaves the line without a subscription until its next registration.

**Independent Test**: Answer the SUBSCRIBE with each status below and observe the next SUBSCRIBE (if any) and its timing.

**Acceptance Scenarios**:

1. **Given** `423 Interval Too Brief` with `Min-Expires: 1800`, **When** processed, **Then** the SUBSCRIBE is re-sent at once asking for at least 1800 s.
2. **Given** `503 Service Unavailable` with `Retry-After: 120`, **When** processed, **Then** no SUBSCRIBE is sent for 120 s (TS 24.229 §5.1.2.2).
3. **Given** `489 Bad Event`, **When** processed, **Then** no further SUBSCRIBE is sent for this registration and the refusal is logged.
4. **Given** any other 4xx/5xx/6xx, **When** processed, **Then** the refusal is logged and the SUBSCRIBE is retried on bounded backoff.

---

### User Story 5 - Status separates registration, subscription and transport health (Priority: P3)

An operator querying line status sees three separate facts: whether the registration is accepted, whether the reg-event subscription is confirmed, and whether the Gm signalling transport is up. None of them alone is presented as proof that inbound calls will arrive.

**Why this priority**: Diagnosability. #124's reporter could not tell from status that the subscription had gone silent.

**Independent Test**: Put the line into each combination (registered + subscription unconfirmed, registered + transport down, all healthy) and read the status output.

**Acceptance Scenarios**:

1. **Given** a registered line whose SUBSCRIBE was answered but never confirmed, **When** status is queried, **Then** registration shows registered and subscription shows unconfirmed, with the time of the last failure.
2. **Given** a fully healthy line, **When** status is queried, **Then** all three show healthy.

---

### Edge Cases

- A NOTIFY arrives before the SUBSCRIBE's response (so the network's tag is first learned from the NOTIFY): accepted, and the later response is matched to the same dialog.
- The network forks or answers with a different tag in the `200 OK` than in the first NOTIFY: the dialog is identified by Call-ID and the line's own tag; the network tag from the first NOTIFY is kept.
- A call is in progress when Timer F fires: the failure is recorded, and the forced re-registration waits for the call to end through the existing maintenance deferral.
- Timer F fires while a re-registration is already pending (e.g. Gm keepalive escalation): no second escalation is stacked.
- The network never supports the reg event package and answers `489` every time: one SUBSCRIBE per registration, never a retry loop.
- A NOTIFY with no `Subscription-State` header: treated as `active` for matching purposes and logged as malformed.
- The line was answered `481` for a stale-looking NOTIFY that was actually for the current subscription (identifier mismatch on the carrier side): logged with both sets of identifiers so it can be diagnosed.
- Responses delivered on the "respond on client" path (`vowifi.respond_on_client`) are matched exactly the same way.
- A carrier never answers the SUBSCRIBE but answers the Gm keepalive: the confirming probe succeeds, so each Timer F is a subscription failure only, re-subscribed on backoff; the line is never re-registered every ~2 minutes for it.

## Requirements *(mandatory)*

### Functional Requirements

**Subscription tracking**

- **FR-001**: The system MUST keep, per line, the current reg-event subscription's identifiers (Call-ID, the line's tag, CSeq), when its SUBSCRIBE was sent, the network's tag once known, its granted expiry, and its state (pending, active, terminated).
- **FR-002**: The system MUST remember the identifiers of the line's previous subscriptions made by the running agent, enough to recognise their NOTIFYs as superseded.
- **FR-003**: A successful registration renewal MUST start a fresh subscription and discard the previous subscription's pending timers and retries.

**Responses to SUBSCRIBE**

- **FR-004**: The system MUST match responses to the current SUBSCRIBE by Call-ID and CSeq, and MUST ignore responses to any superseded SUBSCRIBE.
- **FR-005**: On a 2xx, the system MUST mark the subscription accepted and record the granted expiry.
- **FR-006**: On `423` with `Min-Expires`, the system MUST re-send the SUBSCRIBE requesting at least that interval.
- **FR-007**: On `503` with `Retry-After`, the system MUST NOT re-send the SUBSCRIBE before that interval elapses.
- **FR-008**: On `489`, the system MUST stop subscribing for the current registration and log it.
- **FR-009**: On any other 4xx, 5xx or 6xx, the system MUST log it and retry on bounded backoff.

**Timers**

- **FR-010**: The system MUST apply a 128 s transaction timeout (Timer F) to each SUBSCRIBE.
- **FR-011**: On Timer F expiry, or when sending the SUBSCRIBE fails with a transport error, the system MUST immediately send a Gm keepalive probe to confirm the signalling path.
- **FR-011a**: If the confirming probe goes unanswered within the keepalive's existing response deadline, or cannot be sent, the system MUST raise a qualifying endpoint failure: report the registration as failed and force a re-registration through the existing escalation path. The failure MUST be available as a distinct signal for P-CSCF failover (spec 091).
- **FR-011b**: If the confirming probe is answered, the system MUST treat the Timer F expiry as a subscription failure only: log it, keep registration state unchanged, and re-subscribe on bounded backoff.
- **FR-012**: The system MUST apply a 128 s confirmation timeout (Timer N) from sending the SUBSCRIBE until the first NOTIFY of its dialog, MUST accept a NOTIFY that arrives before the SUBSCRIBE's 2xx as satisfying it, and on expiry MUST log a subscription-confirmation failure and re-subscribe on backoff without changing registration state.

**NOTIFY handling**

- **FR-013**: The system MUST match every incoming NOTIFY to a subscription by Call-ID and the line's tag (the NOTIFY's To tag) before acting on it.
- **FR-014**: A NOTIFY for the current subscription MUST be answered `200 OK` and evaluated as specified in spec 089.
- **FR-015**: A NOTIFY for a superseded subscription of the line MUST be answered `200 OK`, logged, and MUST NOT change registration state.
- **FR-016**: A NOTIFY matching no subscription of the line MUST be answered `481 Call/Transaction Does Not Exist` and MUST have no other effect.
- **FR-017**: On `Subscription-State` `active` or `pending` with an `expires` parameter, the system MUST update the subscription's expiry.
- **FR-018**: On `Subscription-State: terminated`, the system MUST act on the reason: `deactivated` or `timeout` → re-subscribe immediately; `probation` or `giveup` → re-subscribe no earlier than `retry-after` (backoff if absent); `rejected` or `noresource` → no new subscription until the next registration; no reason → re-subscribe on backoff.

**Refresh**

- **FR-019**: The system MUST refresh an active subscription within its dialog before its granted expiry, independent of registration renewal.

**Status and scope**

- **FR-020**: Status queries MUST report registration acceptance, subscription confirmation (with the last failure and when it happened), and Gm transport health as three separate facts.
- **FR-021**: The behaviour MUST be identical for VoWiFi and VoLTE lines.
- **FR-022**: A qualifying endpoint failure raised during a call MUST NOT end the call; the forced re-registration waits for the call to end, as existing renewals do.

### Key Entities

- **Reg-event subscription**: one per registration; identifiers, network tag, granted expiry, state, confirmation status.
- **Superseded subscription**: an earlier subscription of the same line, kept only to recognise its late NOTIFYs.
- **Qualifying endpoint failure**: a SUBSCRIBE Timer F expiry or transport send failure, confirmed by an unanswered keepalive probe; means the registered contact can no longer be assumed bound.
- **Subscription-confirmation failure**: Timer N expiry; the subscription is unconfirmed but the registration is not in question.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A line whose signalling path dies right after registering reports the registration failed and starts recovery within 140 s (Timer F plus the confirming probe's deadline), instead of reporting `Registered` indefinitely.
- **SC-002**: A subscription that is accepted but never confirmed is visible in status within 130 s, and the line stays registered.
- **SC-003**: Over a 3-hour run including renewals, zero NOTIFYs belonging to a superseded subscription change registration state.
- **SC-004**: A carrier that grants subscriptions shorter than registrations keeps the line continuously subscribed (no gap longer than one retry interval) for 24 hours.
- **SC-005**: Every SUBSCRIBE response and every NOTIFY in a test run is accounted for in the log as matched, superseded, or unknown; none is logged as "outside an active transaction".
- **SC-006**: No combination of SUBSCRIBE error responses causes more than one SUBSCRIBE per backoff interval.

## Assumptions

- Spec 089 is in place: NOTIFY attribution is by contact address, and the per-event policy is applied to NOTIFYs this spec routes to it.
- T1 is the IMS value of 2 s, so Timer F and Timer N are 128 s; no per-line override is needed.
- Authentication challenges (`401`/`407`) are not expected on a SUBSCRIBE sent over an established Gm security association; if one arrives it is handled as "any other 4xx" (logged, retried on backoff), and the next registration renews credentials.
- The existing renewal backoff and maintenance deferral (no maintenance during a call) are reused; this spec adds no new alert category.
- Previous-subscription memory is in-process only; after an agent restart, NOTIFYs for subscriptions from before the restart get `481`, which is the correct response for them.
- P-CSCF failover on a qualifying endpoint failure is specified in spec 091; here the failure only forces a re-registration to the same P-CSCF.
