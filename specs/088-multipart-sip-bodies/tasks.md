---
description: "Task list for multipart SIP bodies and the RFC 3312 precondition answer"
---

# Tasks: Multipart SIP bodies and an RFC 3312-conformant precondition answer

**Input**: Design documents from `specs/088-multipart-sip-bodies/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/sip-body-handling.md, quickstart.md

**Tests**: Included — the constitution makes TDD the default (write the
failing test first). All tests are in-module `#[cfg(test)]` on real bytes /
real parsed `SipRequest`s; no mocks. Fixtures are synthetic only
(`+919000000000`, RFC 5737 addresses, `example.invalid`).

**Commit rule**: every task group that ends in a commit runs
`make format && make lint && make test` first (CLAUDE.md).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: US1–US5 from spec.md

---

## Phase 1: Setup

- [X] T001 Create empty module `gsm-sip-bridge/src/ims/body.rs` (module doc citing RFC 5621 §4.2, RFC 2046 §5.1, RFC 3261 §20.11) and register `mod body;` in `gsm-sip-bridge/src/ims/mod.rs`; confirm `make build` passes

---

## Phase 2: Foundational (blocks every story)

**Purpose**: the decoder itself — `MediaType`, `Disposition`, multipart split, `extract_sdp`, `Rejection` (data-model.md; research D4–D6).

### Tests first

- [X] T002 Write failing `MediaType::parse` tests in `gsm-sip-bridge/src/ims/body.rs`: case-insensitive type/subtype/param names, quoted `boundary="a b;c"` with `\"` escape, whitespace around `;`/`=`, missing subtype → `None`
- [X] T003 Write failing `Disposition` tests in `gsm-sip-bridge/src/ims/body.rs`: defaults (`session` for `application/sdp`, `render` otherwise, `handling=required`), explicit `render;handling=optional`, unknown handling value → `Required`
- [X] T004 Write failing splitter tests in `gsm-sip-bridge/src/ims/body.rs`: two parts with CRLF; bare-LF body; preamble and epilogue ignored; delimiter CRLF stripped from the preceding part; trailing whitespace on a delimiter line; per-part `Content-Length` with a wrong value ignored; folded part header joined; missing close delimiter → `Malformed`; boundary empty or > 70 chars → `Malformed`; 17 parts → `Malformed`; depth 5 → `Malformed`
- [X] T005 Write failing `extract_sdp` selection tests in `gsm-sip-bridge/src/ims/body.rs`: empty/whitespace body → `Empty`; no Content-Type + SDP text → `Sdp`; `application/sdp;charset=x` → `Sdp`; T2-shaped `multipart/mixed` fixture (PIDF optional first, SDP second, per quickstart.md) → `Sdp` byte-equal to the SDP part with `skipped=[application/pidf+xml]`; same parts reversed; nested `multipart/mixed` in `multipart/mixed`; `multipart/x-vendor` treated as mixed; `multipart/alternative` picks the last usable part; unknown `handling=required` part + SDP → `Sdp` (lenient, Clarification Q1); two SDP session parts → `Ambiguous`; only unknown parts → `NoSession`; SDP part with `Content-Disposition: render` → not selected; SDP part with `Content-Transfer-Encoding: base64` → `Unsupported`; `text/plain` top level → `Unsupported`; `Rejection::status()` is 415/400 per contract

### Implementation

- [X] T006 Implement `MediaType` and `Disposition` in `gsm-sip-bridge/src/ims/body.rs` until T002–T003 pass
- [X] T007 Implement the RFC 2046 §5.1.1 splitter with depth/part limits in `gsm-sip-bridge/src/ims/body.rs` until T004 passes (work on `&[u8]`; never use part `Content-Length`)
- [X] T008 Implement `extract_sdp`, `Decoded`, `DecodeInfo`, `Rejection` in `gsm-sip-bridge/src/ims/body.rs` until T005 passes; emit one `tracing` line per multipart or refused body with `outer`, `selected`, `skipped`, `outcome` — warn for skipped required parts, info otherwise; never log part contents
- [X] T009 Commit "feat(ims): MIME body decoder for SIP (RFC 5621/2046)" after `make format && make lint && make test`

**Checkpoint**: decoder complete and tested; no call path uses it yet.

---

## Phase 3: User Story 1 + 2 — multipart INVITEs reach the PBX; unusable bodies are refused clearly (P1) 🎯 MVP

**Goal**: replace the INVITE content-type gate with the decoder (FR-001–FR-012, FR-014).

**Independent Test**: `invite_body_decision` on a T2-shaped INVITE returns `Offer(sdp)` equal to the bare SDP; on `text/plain` / no-SDP multipart returns a `415` with the new `Accept`; on malformed or two-SDP multipart returns `400`; plain SDP and offerless unchanged.

### Tests first

- [X] T010 [US1] Write failing `invite_body_decision` tests in `gsm-sip-bridge/src/ims/agent/inbound.rs` using the existing `invite_with_content_type`/`invite_with_body` helpers plus a new `invite_with(content_type, body)` helper: T2-shaped multipart → `Offer` equal to its SDP part; reversed order; quoted boundary; plain SDP → `Offer`; no body → `Offerless`; `Content-Length: 0` with Content-Type → `Offerless`
- [X] T011 [US2] Write failing refusal tests in `gsm-sip-bridge/src/ims/agent/inbound.rs`: `text/plain` → response starts `SIP/2.0 415` and contains `Accept: application/sdp, multipart/mixed, multipart/alternative`; multipart with only a PIDF part → 415 same `Accept`; no close delimiter → `SIP/2.0 400 Bad Request`; two SDP parts → 400; every refusal carries the given `to_tag`
- [X] T012 [US1] Write a test in `gsm-sip-bridge/src/ims/sdp.rs` that `parse_offer` on the T2 SDP part (from the shared fixture) yields remote `203.0.113.30:21892`, `RTP/AVP`, AMR-WB PTs 104/110, AMR PTs 102/108, DTMF `(96,16000),(97,8000)` — guards channel-count `/1` parsing (spec verification matrix)

### Implementation

- [X] T013 [US1] Add `pub(crate) const INVITE_BODY_ACCEPT: &str = "application/sdp, multipart/mixed, multipart/alternative";` and `enum InviteBody { Offerless, Offer(String), Refuse { response: String, reason: String } }` plus `fn invite_body_decision(req: &SipRequest, to_tag: &str) -> InviteBody` in `gsm-sip-bridge/src/ims/agent/inbound.rs`, using `body::extract_sdp(req.header("Content-Type"), req.header("Content-Disposition"), &req.body_bytes)`; 415 via `build_415_unsupported_media(req, to_tag, INVITE_BODY_ACCEPT)`, 400 via `build_uas_response(400, "Bad Request", req, Some(to_tag), None, None)` (`gsm-sip-bridge/src/ims/sip_client.rs:365`)
- [X] T014 [US1] In `handle_invite` (`gsm-sip-bridge/src/ims/agent/inbound.rs` ≈ lines 208–254) replace the `invite_content_type_supported` gate and the `req.body.trim().is_empty()` branch with a match on `invite_body_decision`: `Offerless` → `handle_offerless_invite` (unchanged args), `Offer(sdp)` → `sdp::parse_offer(&sdp)?`, `Refuse` → log `declining: INVITE body` with reason, send response, `obs.report_call_not_answered(CallStatus::Failed, BridgeFailureReason::BridgeSetupFailed, …)`, `return Ok(None)`; keep the `100 Trying` before it
- [X] T015 [US2] Delete `invite_content_type_supported`, its doc comment and the SDP-05 comment block in `gsm-sip-bridge/src/ims/agent/inbound.rs`; replace `an_unrecognised_invite_content_type_is_not_supported` and `supported_invite_content_types_are_accepted` with the T010/T011 tests; update the doc comments of the offerless tests that reference the old predicate
- [X] T016 [US2] Use `INVITE_BODY_ACCEPT` for the `Accept` header in `options_response` (`gsm-sip-bridge/src/ims/agent/mod.rs:453`) and update/add its test asserting the new value
- [X] T017 [US1] Commit "fix(ims): accept multipart INVITE bodies instead of declining 415 (#103)" after `make format && make lint && make test`

**Checkpoint**: T2-shaped INVITEs reach the PBX in tests; MVP shippable.

---

## Phase 4: User Story 3 — RFC 3312-conformant precondition answer (P2)

**Goal**: FR-015–FR-018a; contract "SDP answer QoS lines".

**Independent Test**: golden T2 offer → answer has exactly the four QoS lines of spec Story 3 scenario 1, no `a=conf:qos`; e2e scenario 5; no-precondition fixtures byte-identical.

### Tests first

- [X] T018 [P] [US3] Write failing golden test in `gsm-sip-bridge/src/ims/sdp.rs`: T2 offer → `build_answer` (wideband, `Cellular` and `Legacy`) answer QoS lines are exactly `a=curr:qos local sendrecv`, `a=curr:qos remote sendrecv`, `a=des:qos mandatory remote sendrecv`, `a=des:qos optional local sendrecv`, with no `a=conf:qos`; codec PT 110, DTMF PT 96
- [X] T019 [P] [US3] Write failing tests in `gsm-sip-bridge/src/ims/sdp.rs`: RFC 3312 §13.2 SDP1 offer → answer matching SDP2's QoS lines; `des optional local send` inverts to `des optional remote recv`; `des optional e2e sendrecv` + `curr e2e send` → `curr e2e recv`; + `curr e2e none` → `curr e2e none`; no e2e/local curr → `curr e2e none`; e2e falls back to `curr local` when no `curr e2e`; `des mandatory e2e` still `Decline`
- [X] T020 [P] [US3] Write failing tests for `offerer_unmet_mandatory` in `gsm-sip-bridge/src/ims/sdp.rs`: T2 offer → empty; `curr local none` + `des mandatory local sendrecv` → `[(Local, SendRecv)]`; `curr local send` + `des mandatory local sendrecv` → reports the missing `recv` half (or `SendRecv`, as documented in the function)
- [X] T021 [US3] Write a regression test in `gsm-sip-bridge/src/ims/sdp.rs` that answers to `JIO_MT_OFFER` and `AIRTEL_LIKE_OFFER` contain no `qos` line, and that `build_veth_answer` output is unchanged for a PJSIP-style offer (capture today's output as the expected string before changing code)

### Implementation

- [X] T022 [US3] In `gsm-sip-bridge/src/ims/sdp.rs` remove `QosAnswerLine::confirm` and the `a=conf:qos` emission in `build_answer_for` (~line 1199–1216); change `PreconditionVerdict::Proceed` to carry `current: Vec<QosAnswerLine>` and `desired: Vec<QosDesired>` (answer-relative via `invert_qos_direction` and a new `invert_qos_status_type`); emit `curr` lines then `des` lines in offer order
- [X] T023 [US3] In `precondition_verdict` (`gsm-sip-bridge/src/ims/sdp.rs:813`) derive the e2e current status from `offer.offerer_curr` (`E2e`, else `Local`), inverted, else `QosDirection::None`; update the doc comment and the existing tests (~lines 2600–2870) that assert `confirm`/`a=conf:qos`
- [X] T024 [US3] Add `pub fn offerer_unmet_mandatory(offer: &SdpOffer) -> Vec<(QosStatusType, QosDirection)>` in `gsm-sip-bridge/src/ims/sdp.rs` until T020 passes
- [X] T025 [US3] In `handle_invite` (`gsm-sip-bridge/src/ims/agent/inbound.rs`, after the `precondition_verdict` decline check) emit one `tracing::info!` with `call_id` and the unmet segment/direction list when `offerer_unmet_mandatory` is non-empty; the call proceeds (FR-018a)
- [X] T026 [US3] Commit "fix(ims): RFC 3312 answer — desired status, no self-confirmation, honest e2e" after `make format && make lint && make test`

**Checkpoint**: Stories 1–3 complete; T2 calls should survive past answer.

---

## Phase 5: User Story 4 — ACK and outbound 2xx bodies decoded the same way (P2)

**Goal**: FR-001, FR-013 on the remaining SDP consumers.

**Independent Test**: multipart ACK/2xx bodies yield the same `SdpAnswer` as their bare SDP; malformed ones take the existing failure path.

- [X] T027 [P] [US4] Write failing tests for a pure helper `fn ack_answer(ack: &SipRequest) -> Option<String>` in `gsm-sip-bridge/src/ims/agent/inbound.rs`: multipart ACK → SDP part; bare SDP → whole body; malformed / no SDP / empty → `None`
- [X] T028 [US4] Implement `ack_answer` and change the offerless wait loop (`gsm-sip-bridge/src/ims/agent/inbound.rs` ≈ 877–957) to store `ack_answer(&candidate)` instead of `candidate.body.clone()`; distinguish "ACK arrived but unusable" (→ `teardown("offerless_ack_incompatible")`) from "no ACK" (→ existing `offerless_ack_timeout`)
- [X] T029 [P] [US4] Write failing test + implement a pure `fn answer_sdp(resp: &SipResponse) -> BridgeResult<String>` in `gsm-sip-bridge/src/ims/call.rs` (decode `resp.body.as_bytes()` with `Content-Type`/`Content-Disposition`; `Empty`/`Rejection` → `BridgeError::Ims` naming the outcome) and use it before `sdp::parse_answer` at `gsm-sip-bridge/src/ims/call.rs:287`
- [X] T030 [US4] Commit "fix(ims): decode multipart bodies in offerless ACK and outbound 2xx" after `make format && make lint && make test`

---

## Phase 6: User Story 5 — parameterised SMS content type (P3)

**Goal**: FR-019.

**Independent Test**: `application/vnd.3gpp.sms;charset=x` decodes identically to the unparameterised form.

- [X] T031 [US5] Write failing test in `gsm-sip-bridge/src/ims/agent/mod.rs` (next to `decode_pdu_body_decodes_a_3gpp_sms_message`, ~line 3666) using a parameterised Content-Type
- [X] T032 [US5] Change `decode_pdu_body` (`gsm-sip-bridge/src/ims/agent/mod.rs:1071`) and `message_content_type_supported` (`:1059`) to compare via `body::MediaType::parse`; make `MediaType` `pub(crate)` if needed
- [X] T033 [US5] Commit "fix(ims): decode parameterised application/vnd.3gpp.sms as SMS" after `make format && make lint && make test`

---

## Phase 7: Polish & cross-cutting

- [ ] T034 [P] Add a `RELEASE_NOTES.md` entry under a new unreleased heading: bold one-line summary only (memory: bold only the summary), then plain text covering multipart INVITEs (#103, regression since v8.15.0), the precondition answer, ACK/2xx decoding, OPTIONS `Accept`, SMS media type; credit the #103 reporter
- [ ] T035 [P] Scan the diff for real numbers / identifiers (`git diff main | grep -E '\+[0-9]{10,}'`) — fixtures must use `+919000000000` and RFC 5737 addresses only (CLAUDE.md)
- [ ] T036 Run `cargo test -p gsm-sip-bridge ims::` and the full `make format && make lint && make test`; confirm zero warnings across all targets
- [ ] T037 Live regression on Vodafone (local) and Jio (Pi) per `specs/088-multipart-sip-bodies/quickstart.md` §Regression — inbound call with audio, registration stable across OPTIONS keepalives, no `cause=503`
- [ ] T038 Post a comment on #103 asking the reporter for the live T2 retest in `quickstart.md` §Live T2 (after the release is cut)

---

## Dependencies & Execution Order

- **Setup (T001)** → **Foundational (T002–T009)** → all stories.
- **US1+US2 (T010–T017)**: depends on Foundational. MVP.
- **US3 (T018–T026)**: independent of US1/US2 code (sdp.rs only, plus one log line in inbound.rs at T025 which touches the same function as T014 — do T025 after T017).
- **US4 (T027–T030)**: depends on Foundational only.
- **US5 (T031–T033)**: depends on T006 (`MediaType`) only.
- **Polish**: after all stories.

## Parallel Opportunities

- T002–T005 are all in `body.rs` (same file) — write sequentially, but they are independent of each other.
- T018, T019, T020 [P] (sdp.rs tests) can be written together.
- US3 (sdp.rs), US4 (call.rs/inbound.rs ACK loop) and US5 (agent/mod.rs) can proceed in parallel after Phase 2, except T025 and T028 both edit `inbound.rs` (different functions; merge carefully).
- T034, T035 [P].

## Implementation Strategy

1. **MVP**: Phases 1–3 (decoder + INVITE path). This alone fixes the reported 415.
2. **Do not release MVP alone to T2 users**: ship Phase 4 in the same release — T2 is the first carrier to send preconditions and the current answer invites an UPDATE that gets `405` (research D8).
3. Phases 5–6 are hardening; include in the same release if ready.
4. Live: Vodafone + Jio regression before release; T2 by the reporter after.
