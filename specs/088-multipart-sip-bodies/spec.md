# Feature Specification: Multipart SIP bodies and an RFC 3312-conformant precondition answer

**Feature Branch**: `088-multipart-sip-bodies`  
**Created**: 2026-10-13  
**Status**: Draft  
**Input**: User description: "Multipart SIP body handling and RFC 3312-conformant precondition answer (GitHub issue #103). Inbound IMS calls from T2 Russia fail with 415 because handle_invite rejects any Content-Type other than application/sdp; T2 sends multipart/mixed with an optional PIDF-LO part (Content-Disposition: render;handling=optional) plus the SDP offer. This is a regression from specs/045 SDP-05 (commit 6f425a3, v8.15.0) and violates RFC 5621 §4.2. Scope: (1) a shared MIME body decoder used by every SDP consumer — inbound INVITE, offerless-INVITE ACK, outbound INVITE 2xx; (2) fix the specs/048 precondition answer (emit a=des:qos, stop emitting a=conf:qos on the bridge's own segment); (3) side fix: a parameterised application/vnd.3gpp.sms MESSAGE body is decoded as binary. Out of scope: 580 failure SDP, PRACK/UPDATE, 100rel, RFC 6442."

## Background

A SIP message body can be a single session description, or a MIME
**multipart** container holding several parts — for example, a caller's
location (a PIDF-LO document) next to the session description. Each part
carries its own content type and, optionally, a **disposition** saying what
it is for (`session` for the session description, `render` otherwise) and
whether understanding it is `required` or `optional`.

RFC 5621 §4.2 makes multipart support mandatory for every SIP user agent:
UAs MUST parse multipart bodies including nested parts, and MUST support
`multipart/mixed` and `multipart/alternative`. It names "a legacy UA answers
415 to any multipart body" as the interoperability failure to avoid.

Issue #103 reported three inbound calls from a T2 Russia subscriber, all
declined `415 Unsupported Media Type` by the bridge itself within a
millisecond, never reaching the PBX. T2's INVITE body was `multipart/mixed`:
an optional PIDF-LO part (`render;handling=optional`) followed by the SDP
offer. Interleaved calls from a MegaFon subscriber with a plain SDP body were
bridged normally. Triage against `main` (v8.20.0) found:

| # | Defect | Effect |
|---|---|---|
| 1 | Any INVITE body not typed `application/sdp` is declined `415`, including every multipart body. Introduced by specs/045 SDP-05 (`6f425a3`, v8.15.0), whose design assumed no carrier sends multipart. Before v8.15.0 the same T2 body parsed to an offer identical to the bare SDP. | Complete inbound failure for any carrier that attaches location (or anything else) to its INVITE. A regression, and a violation of RFC 5621 §4.2. |
| 2 | The other two places a session description is read — the ACK answering an offerless INVITE, and the 2xx answering an outbound INVITE — check no content type and understand no MIME structure at all. | The same carrier behaviour on those paths is either misread or (for an ACK) tears the call down. RFC 5621 §4.2 applies to responses too. |
| 3 | When the offer carries QoS preconditions (RFC 3312), the answer omits every desired-status (`a=des:qos`) line and adds a confirmation request (`a=conf:qos`) on the bridge's own access segment. | RFC 3312 §5.2 requires desired-status lines in the answer. The confirmation request obliges the carrier (§7) to send a fresh offer by UPDATE, which the bridge refuses `405`. T2 would be the first carrier on this bridge to send preconditions, so fixing only #1 risks moving the failure from call setup to mid-call. |
| 4 | An SMS `MESSAGE` typed `application/vnd.3gpp.sms` with any parameter passes the content-type check but is not recognised as a 3GPP SMS body. | The binary SMS payload is relayed as if it were text. |

## Clarifications

### Session 2026-10-13

- Q: Should an unrecognised part that isn't marked optional (explicitly `handling=required`, or no disposition) refuse the whole INVITE, even when a valid SDP part is present? → A: No — lenient when an SDP part is present. Such parts are skipped with a warning log naming their content type and the call proceeds; `415` only when no usable session description is found. A deliberate deviation from RFC 5621 §8.4 (a SHOULD), because carriers attach 3GPP XML parts (`application/3gpp-ims+xml`, `application/vnd.3gpp.*`) and a strict rule would repeat the #103 outage for any of them missing `handling=optional`.

### Session 2026-10-14

- Q: When the offer shows the caller's own mandatory QoS segment not yet met, should the bridge ring, decline `580`, or hold ringing? → A: Ring the PBX immediately; the answer mirrors the caller's unmet status and an info log records it. RFC 3312 §6's "SHOULD NOT alert" is knowingly not followed: without UPDATE support the readiness report could never arrive, and in IMS deployments the network reserves resources before the INVITE reaches the bridge.
- Q: For an end-to-end (`e2e`) precondition, what current status should the answer report? → A: Met only in the directions the caller's own offer shows met — from its `a=curr:qos e2e` line, or its `a=curr:qos local` line when there is no e2e line — direction inverted per RFC 3312 Table 4; `none` otherwise. Replaces today's unconditional "met" claim, since the bridge's own segment is always ready and e2e is therefore met exactly when the caller's side is.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - An inbound call with a multipart INVITE reaches the PBX (Priority: P1)

A caller on a carrier that attaches location to its INVITEs (T2 Russia
today) dials a number served by the bridge. The bridge finds the session
description inside the multipart body, ignores the optional location part,
and rings the PBX exactly as it would for a plain-SDP INVITE.

**Why this priority**: This is the reported outage. Every such call fails
outright today, and the caller hears "subscriber unavailable".

**Independent Test**: Feed a synthetic INVITE shaped like the captured T2
request (optional PIDF part first, SDP second, unquoted boundary, per-part
Content-Length headers) into the inbound call path; the call proceeds to PBX
setup with the same negotiated media as the bare SDP would produce.

**Acceptance Scenarios**:

1. **Given** an INVITE whose body is `multipart/mixed` with an optional
   location part and an SDP part, **When** the bridge receives it, **Then**
   no `415` is sent, the PBX is rung, and the answer negotiates the same codec
   as for the SDP part delivered on its own.
2. **Given** the same parts in reverse order, **When** received, **Then** the
   outcome is identical.
3. **Given** a quoted boundary parameter, or mixed-case media-type and
   parameter names, **When** received, **Then** the outcome is identical.
4. **Given** an INVITE with a plain `application/sdp` body, no Content-Type,
   or no body at all (offerless), **When** received, **Then** behaviour is
   exactly as before this feature.

---

### User Story 2 - Bodies the bridge cannot honour are refused clearly (Priority: P1)

A carrier sends something the bridge genuinely cannot use: a malformed
multipart body, a part the bridge does not understand but is told it must,
or two competing session descriptions. The bridge refuses with a response
that says why, never guesses, and never rings the PBX.

**Why this priority**: Accepting multipart must not turn into silently
accepting whatever arrives — the exact misparse SDP-05 was written to stop.
Ships together with Story 1.

**Independent Test**: Feed each malformed or unsupported body shape into the
inbound path and check the response code, the `Accept` header where
applicable, and that no PBX call was attempted.

**Acceptance Scenarios**:

1. **Given** a multipart body containing no usable session description
   (only unknown parts, whatever their handling), **When** received, **Then**
   the bridge answers `415` with an `Accept` header listing `application/sdp`,
   `multipart/mixed` and `multipart/alternative`.
1a. **Given** a multipart body with a valid SDP part plus an unknown part
   marked `handling=required` (or with no disposition), **When** received,
   **Then** the call proceeds as in Story 1 and a warning log names the
   skipped part's content type.
2. **Given** a non-multipart, non-SDP body (e.g. `text/plain`), **When**
   received, **Then** the bridge answers `415` with the same `Accept` header.
3. **Given** a multipart body with a missing or invalid boundary, no closing
   delimiter, or nesting/part counts beyond the bridge's limits, **When**
   received, **Then** the bridge answers `400` and does not attempt to read
   SDP from it.
4. **Given** a multipart body with two session-description parts, **When**
   received, **Then** the bridge answers `400`.
5. **Given** any refusal above, **Then** a single log line records the outer
   content type, the selected or offending part type, and the reason — and
   never the contents of a location part or any subscriber identity.

---

### User Story 3 - The precondition answer is one the carrier can act on (Priority: P2)

The caller's offer carries QoS preconditions (common in IMS: the caller's
access is reserved, the bridge's side is "optional"). The bridge answers with
the full status picture RFC 3312 requires — current and desired status for
both segments — and does not ask the carrier to send another offer it would
then refuse.

**Why this priority**: Needed for T2 calls to survive past answer, but
separable from Story 1 and testable on its own against the RFC's own example.

**Independent Test**: Generate the answer for the T2 offer and for the
RFC 3312 §13.2 segmented example; compare the QoS lines with the expected
lines below.

**Acceptance Scenarios**:

1. **Given** an offer with `a=curr:qos local sendrecv`,
   `a=curr:qos remote none`, `a=des:qos mandatory local sendrecv`,
   `a=des:qos optional remote sendrecv` (the T2 offer), **When** answered,
   **Then** the answer's QoS lines are exactly
   `a=curr:qos local sendrecv`, `a=curr:qos remote sendrecv`,
   `a=des:qos mandatory remote sendrecv`, `a=des:qos optional local sendrecv`
   (order not significant) and contain no `a=conf:qos` line.
2. **Given** any offer with desired-status lines, **When** answered, **Then**
   every offered desired-status line appears in the answer with its strength
   unchanged and its status type and direction inverted (local↔remote,
   send↔recv; `e2e` and `sendrecv` unchanged).
3. **Given** an offer with no precondition lines (every Jio/Airtel/Vodafone
   offer seen so far), **When** answered, **Then** the answer is byte-for-byte
   what it is today.
4. **Given** an offer requiring a mandatory end-to-end precondition, **When**
   received, **Then** it is still declined `580`, as today.
5. **Given** an offer with `a=des:qos optional e2e sendrecv` and
   `a=curr:qos e2e send`, **When** answered, **Then** the answer reports
   `a=curr:qos e2e recv`; with `a=curr:qos e2e none` (or no e2e or local
   current-status line) it reports `a=curr:qos e2e none`.

---

### User Story 4 - Session descriptions in ACKs and 2xx responses are read the same way (Priority: P2)

Whether a session description arrives in an INVITE, in the ACK completing an
offerless call, or in the 2xx to a call the bridge placed, it is found and
validated by the same rules.

**Why this priority**: Closes the same RFC 5621 gap on the remaining paths;
no carrier has been seen to send multipart there yet.

**Independent Test**: Feed a multipart ACK (offerless call) and a multipart
2xx (outbound call) and confirm they yield the same answer as their bare SDP.

**Acceptance Scenarios**:

1. **Given** an offerless inbound call, **When** the ACK carries the answer
   inside a multipart body, **Then** the call is bridged as with a bare-SDP ACK.
2. **Given** an offerless inbound call, **When** the ACK's body is malformed
   or unsupported, **Then** the call is torn down the same way an incompatible
   ACK answer is today.
3. **Given** an outbound call, **When** the 2xx carries its answer inside a
   multipart body, **Then** media starts as with a bare-SDP 2xx; a malformed
   or unsupported body fails the call through the existing error path.

---

### User Story 5 - Parameterised SMS bodies are decoded as SMS (Priority: P3)

An SMS-over-IMS `MESSAGE` typed `application/vnd.3gpp.sms` with a parameter
is decoded as a binary 3GPP SMS, not relayed as text.

**Why this priority**: Latent inconsistency found during triage; no carrier
observed sending it.

**Independent Test**: Deliver an SMS body with `Content-Type:
application/vnd.3gpp.sms;charset=...` and confirm it decodes identically to the
unparameterised form.

**Acceptance Scenarios**:

1. **Given** a `MESSAGE` with `application/vnd.3gpp.sms` plus any parameter,
   **When** received, **Then** it is decoded and relayed exactly as the
   unparameterised form.

### Edge Cases

- **Unknown multipart subtype** (e.g. `multipart/x-vendor`): treated as
  `multipart/mixed` (RFC 5621 §4.2).
- **`multipart/alternative`**: the last part the bridge can use is chosen
  (RFC 2046 §5.1.4); the parts' own handling values are ignored (RFC 5621
  §8.3).
- **Nested multipart**: walked recursively within the depth limit; an
  optional nested container is skipped as a whole if it holds nothing usable.
- **Per-part Content-Length present and wrong**: ignored — parts are framed by
  boundaries only.
- **Preamble/epilogue text** around the parts: ignored (RFC 2046 §5.1.1).
- **Line endings**: CRLF per RFC; bare LF tolerated.
- **SDP part with a non-identity Content-Transfer-Encoding** (e.g. base64):
  refused `415` (RFC 5621 §3.2 expects binary; no decoder is provided).
- **Multipart body containing no session description at all**: refused `415`.
- **SDP part with an explicit disposition other than `session`**: not treated
  as the session description.
- **Non-empty body with no Content-Type**: still read as SDP (existing
  tolerance kept, despite RFC 3261 §20.15).
- **Offerer's own mandatory segment not yet met** in the offer: handled per
  FR-018a (ring immediately, mirror the status, info log).

## Requirements *(mandatory)*

### Functional Requirements

**Body decoding (shared by INVITE, offerless ACK, outbound 2xx)**

- **FR-001**: The bridge MUST locate the session description in a message
  body by the same rules on every path that reads one: inbound INVITE, the ACK
  of an offerless inbound call, and the 2xx of an outbound INVITE.
- **FR-002**: An empty body MUST keep today's offerless handling; a body typed
  `application/sdp` (any parameters) or with no Content-Type MUST be used
  whole, as today.
- **FR-003**: A `multipart/*` body MUST be split on its boundary per RFC 2046
  §5.1.1 — delimiter at line start, the preceding line break belonging to the
  delimiter, preamble and epilogue ignored, closing delimiter required — and
  MUST NOT be framed by per-part Content-Length.
- **FR-004**: Media types and parameters MUST be compared case-insensitively
  for type, subtype and parameter names, and parameter values MAY be quoted
  strings.
- **FR-005**: Nested multipart bodies MUST be supported up to a nesting depth
  of 4 and 16 parts per level; beyond either limit the body is malformed.
- **FR-006**: Each part's disposition MUST default per RFC 3261 §20.11:
  `session` for `application/sdp`, `render` otherwise; `handling` defaults to
  `required`.
- **FR-007**: In `multipart/mixed`, `multipart/related` and unknown multipart
  subtypes, the session description is the single `application/sdp` part with
  disposition `session`. Unknown parts MUST be skipped whatever their
  `handling`: silently-logged (info) when `optional`, with a warning naming
  the content type when `required` or defaulted to required. This deviates
  from RFC 5621 §8.4 (SHOULD) by design — see Clarifications.
- **FR-008**: In `multipart/alternative`, the last part the bridge can use
  MUST be selected, and the inner parts' handling values ignored.
- **FR-009**: The bridge MUST NOT interpret location parts, Content-ID
  references, or the Geolocation header, and MUST NOT advertise any such
  capability.

**Refusals and diagnostics**

- **FR-010**: An INVITE whose body is unsupported (unknown top-level type,
  no usable session description in a multipart body, or an SDP
  part with a non-identity transfer encoding) MUST be answered `415` with
  `Accept: application/sdp, multipart/mixed, multipart/alternative`.
- **FR-011**: An INVITE whose body is malformed (boundary missing or invalid,
  no closing delimiter, limits exceeded) or ambiguous (more than one session
  description) MUST be answered `400`.
- **FR-012**: Every refusal MUST happen before any SDP is read and before the
  PBX is contacted, and MUST be reported to call observability as a failed
  call, as today's `415` is.
- **FR-013**: An unusable body in an offerless call's ACK MUST end the call the
  same way an incompatible ACK answer does today; in an outbound 2xx it MUST
  fail the call through the existing error path.
- **FR-014**: Each decode decision on a non-trivial body MUST produce one log
  line with outer content type, selected or offending part type and outcome,
  and MUST NOT log part contents (location, coordinates) or subscriber
  identities.

**Precondition answer**

- **FR-015**: When the offer carries desired-status lines, the answer MUST
  carry one desired-status line for each, with strength unchanged and status
  type and direction inverted per RFC 3312 §5.2 Table 4.
- **FR-016**: The answer MUST NOT carry a confirmation request
  (`a=conf:qos`) for the bridge's own segment.
- **FR-017**: Segment current-status lines MUST remain as today: the
  bridge's own segment reported met for the requested direction, the
  offerer's segment mirrored from the offerer's own current status.
- **FR-017a**: An end-to-end current-status line MUST report met only in the
  directions the offer's own `a=curr:qos e2e` line shows met (falling back to
  its `a=curr:qos local` line when no e2e line is present), direction
  inverted per RFC 3312 Table 4, and `none` otherwise. It MUST NOT report
  end-to-end as met on the bridge's say-so alone.
- **FR-018**: Offers without precondition lines MUST produce an answer
  identical to today's; a mandatory end-to-end precondition MUST still be
  declined `580`.
- **FR-018a**: When the offer's own mandatory segment is not yet met, the
  bridge MUST still ring the PBX immediately (no 580, no withheld `180`),
  mirror the unmet status in the answer per FR-017, and emit one info log
  naming the unmet segment and direction.

**SMS**

- **FR-019**: A `MESSAGE` body typed `application/vnd.3gpp.sms` with
  parameters MUST be decoded as a 3GPP SMS, exactly as without parameters.

### Key Entities

- **Media type**: type, subtype and parameters (notably `boundary`) of a body
  or part.
- **Disposition**: what a part is for (`session`, `render`, other) and whether
  handling it is `required` or `optional`.
- **Body part**: one part of a multipart body — its media type, disposition,
  transfer encoding and content; may itself be multipart.
- **Decode outcome**: no body (offerless), one session description, or a
  refusal (unsupported, malformed, ambiguous) with the offending type.
- **Precondition status**: per segment (local, remote, end-to-end) and
  direction — the current, desired and confirmation status lines of an offer
  or answer.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of INVITEs shaped like the captured T2 request (optional
  location part + session description) reach the PBX in tests; zero are
  answered `415`.
- **SC-002**: Every verification-matrix body shape in Story 2 produces its
  specified response, and none of them causes a PBX call attempt.
- **SC-003**: Answers to every existing carrier offer fixture without
  preconditions are byte-identical before and after this feature.
- **SC-004**: The answer to the T2 offer contains exactly the four QoS lines
  of Story 3 scenario 1 and no confirmation request.
- **SC-005**: A live inbound call from the reporter's T2 line is bridged with
  two-way audio for at least 30 seconds with no carrier UPDATE refused during
  the call.
- **SC-006**: Inbound calls on the maintainers' Jio and Vodafone rigs connect
  with audio exactly as before.

## Assumptions

- T2's body layout in the issue is representative of its MT INVITEs; other
  carriers may attach other optional parts, which this feature skips by
  design.
- Carriers do not always mark ancillary parts `handling=optional`. Because
  the bridge only ever acts on the session description, skipping an
  unrecognised part (with a warning, FR-007/FR-014) is safer for callers than
  refusing the call; the warning makes a carrier that relies on such a part
  visible.
- Real-world SIP bodies are binary-encoded (RFC 5621 §3.2); transfer-encoding
  decoding is not provided.
- The bridge's own media segment is always ready, so it never needs the
  carrier to confirm anything about it; PRACK/UPDATE remain unimplemented.
- Live T2 verification depends on the issue reporter; the maintainers' rigs
  are Jio and Vodafone and cover the regression checks only.
- Out of scope: a failure description in `580` responses (RFC 3312 §8),
  PRACK/UPDATE/100rel support (RFC 3312 §11), location conveyance (RFC 6442),
  multipart `MESSAGE` bodies.
- This feature reverses specs/045 research Decision 10 (decline multipart)
  and corrects specs/048's confirmation/desired-status handling; both are to
  be recorded in this feature's research.
