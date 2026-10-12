---
description: "Task list for AMR and Opus on the phone-facing leg"
---

# Tasks: AMR and Opus on the phone-facing leg

**Input**: Design documents from `specs/085-client-leg-amr-opus/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/config.md, quickstart.md

**Tests**: These are included. FR-014(a) requires automated tests, and the constitution makes TDD the default: write each test first and watch it fail.

**Commit rule (every task)**: run `make format && make lint && make test` and get them green before committing (CLAUDE.md). Use one commit per task or per tightly coupled pair. Never put real phone numbers in tests or docs; use `+919000000000` placeholders.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: US1–US4 from spec.md

---

## Phase 1: Setup (build the codecs into PJSIP)

**Purpose**: make AMR-WB, AMR-NB and Opus exist in the image's PJSIP (research R1, R2). Nothing in Rust changes yet.

- [X] T001 In `docker/Dockerfile`, stage `pjsip-builder`: add `opencore-amr-dev vo-amrwbenc-dev opus-dev` to the `apk add` line. Add `--with-opencore-amr=/usr --with-opencore-amrwbenc=/usr --with-opus=/usr` to `./configure`. Immediately after configure, add a check that fails the build unless `pjmedia/include/pjmedia-codec/config_auto.h` defines `PJMEDIA_HAS_OPENCORE_AMRNB_CODEC`, `PJMEDIA_HAS_OPENCORE_AMRWB_CODEC` and `PJMEDIA_HAS_OPUS_CODEC` as `1`; use `grep -Eq '#define PJMEDIA_HAS_..._CODEC[[:space:]]+1'` per macro. Update the stage's comment block to say why (issue #109; spec 085).
- [X] T002 In `docker/Dockerfile`: add `opus` to the runtime stage's `apk add` list next to `opencore-amr vo-amrwbenc`, and extend the comment above it to say the codec libraries now also serve PJSIP's phone-facing leg. Add `opus-dev` to the `rust-builder` stage's `apk add`, which is needed when linking against the pjproject `.pc`.
- [X] T003 [P] In `pjsua-sys/build.rs`, add `"opencore-amrnb"`, `"opencore-amrwb"`, `"vo-amrwbenc"` and `"opus"` to the `system_libs` set (around line 71) so they link dynamically under `PJSUA_SYS_STATIC=1`. Extend the doc comment on `warn_if_config_site_incomplete`'s neighbour, or the `system_libs` comment, to explain why (research R2).
- [ ] T004 Build the image locally (`make docker-build` or the project's existing image target) and run the telephone-side agent once. Confirm that the startup log line `PJSIP codecs offered, in priority order` now includes `AMR/8000/1`, `AMR-WB/16000/1` and `opus/48000/2`. Save that line, plus the **current `main` image's** same line, to `specs/085-client-leg-amr-opus/baseline-codec-order.txt`; T013 needs it. Then run the arm64 build (`arm-build-200` skill) and confirm the `config_auto.h` check passes there too.

**Checkpoint**: the image has the codecs. In this state they outrank PCMU (research R3), so **do not release from this checkpoint**; Phases 2 and 4 must land in the same release.

---

## Phase 2: Foundational (config + pure priority plan)

**Purpose**: the `[sip].codecs` setting and the pure function that turns it into PJSIP priorities. Every user story depends on this.

- [X] T005 [P] In `gsm-sip-bridge/tests/test_config.rs`, write failing tests for `[sip].codecs` per `contracts/config.md`:
  - absent → `None`;
  - `["amr-wb","Opus","G722","PCMU"]` parses case-insensitively, in order;
  - `[]` → error;
  - `["G722"]` → error whose message contains `[sip].codecs` and `PCMU`;
  - `["G729","PCMU"]` → error naming `G729` and listing the accepted names;
  - `["PCMU","pcmu"]` → duplicate error.
- [X] T006 In `gsm-sip-bridge/src/config/mod.rs`, add `pub enum PhoneCodec { G722, Pcmu, Pcma, AmrWb, AmrNb, Opus }` with `fn pjsip_id(self) -> &'static str` per the data-model.md table, and add `pub codecs: Option<Vec<PhoneCodec>>` to `SipConfig`. Its doc comment must say the setting applies to VoWiFi/VoLTE only and that PCMU is required (research R4).
- [X] T007 In `gsm-sip-bridge/src/config/raw.rs`, add `codecs: Option<Vec<String>>` to the raw `[sip]` struct (the struct already uses `deny_unknown_fields`). In `gsm-sip-bridge/src/config/build.rs`, parse it into `Vec<PhoneCodec>` with the validation from data-model.md: unknown, empty, duplicate and missing-PCMU are all errors, formatted `configuration error: [sip].codecs: <reason>`. T005 should now pass.
- [X] T008 [P] Create `gsm-sip-bridge/src/vowifi/codecs.rs` with a pure `pub(crate) fn priority_plan(codecs: Option<&[PhoneCodec]>, wideband: bool, registered: &[String]) -> Vec<(String, u8)>` implementing the data-model.md `CodecPriorityPlan` table:
  - `None` + wideband → G722=200, L16=1, AMR-WB=4, AMR=3, opus=2;
  - `None` + narrowband → only the three new codecs pinned;
  - `Some` → every id in `registered` set to 0, then `L16/16000/1`=1, then `list[i]` = 200−i.

  Add `#[cfg(test)]` unit tests covering every row, including that `Some(list)` never leaves an unlisted registered codec non-zero, and that PCMU's priority in the default plan is untouched (absent from the output). Register the module in `gsm-sip-bridge/src/vowifi/mod.rs` (`mod codecs;`).
- [X] T009 In `gsm-sip-bridge/src/vowifi/codecs.rs`, add `pub(crate) fn apply(endpoint: &pjsua_safe::Endpoint, codecs: Option<&[PhoneCodec]>, wideband: bool) -> BridgeResult<()>`. It does four things:
  1. reads `endpoint.codecs()`;
  2. when `codecs` is `Some`, returns `BridgeError` if any listed codec's `pjsip_id()` is not registered, with the message ``[sip].codecs names <X>, which this build does not include`` (FR-010);
  3. applies `priority_plan` through `set_codec_priority`, keeping today's warn-and-continue behaviour for the `None` path;
  4. logs the resulting `PJSIP codecs offered, in priority order` line (moved from `prioritize_wideband_codecs`).

**Checkpoint**: config parses and validates; the plan is unit-tested. Nothing is wired into a running endpoint yet.

---

## Phase 3: User Story 1 — AMR-capable softphone receives a call in AMR (P1) 🎯 MVP

**Goal**: with `codecs = ["AMR-WB","G722","PCMU"]`, an inbound VoWiFi or VoLTE call reaches baresip on AMR-WB.

**Independent test**: quickstart.md §2.1–2.4.

- [X] T010 [US1] In `gsm-sip-bridge/src/vowifi/mod.rs`, replace the `if wideband { prioritize_wideband_codecs(&endpoint); }` call in `run_telephony_side` (around line 301) with `codecs::apply(&endpoint, config.sip.codecs.as_deref(), wideband)?`, called unconditionally because the narrowband path must pin the new codecs too. Delete `prioritize_wideband_codecs` and move its doc comment's reasoning (why G.722 at 200 and L16 at 1) into `codecs.rs`. Keep the `G722_CODEC_ID` and `L16_16K_CODEC_ID` constants where `codecs.rs` can use them.
- [X] T011 [P] [US1] In `pjsua-safe/src/endpoint.rs`, add `pub fn negotiated_codec(&self, call_id: i32) -> Option<String>`. It wraps the existing `unsafe fn negotiated_audio_codec` (line ~639) and returns `None` without `pjsip-linked`, following the file's existing `#[cfg]` pattern with a `// SAFETY:` comment.
- [X] T012 [US1] In `gsm-sip-bridge/src/vowifi/control.rs`, add `#[serde(default)] pub phone_codec: Option<String>` to `CallRecord`; `serde(default)` keeps older agents wire-compatible. In `gsm-sip-bridge/src/vowifi/mod.rs`, fill it from `endpoint.negotiated_codec(pbx_call.call_id())` once the PBX/phone leg's media is active and before `push_recent_call`. Show it in the `vowifi-status` recent-calls output, in whichever function renders `CallRecord` there. Add a serde round-trip test in `control.rs` proving that a record without the field still deserializes (FR-011).
- [X] T013 [US1] pjsip-linked check, run in the privileged test container (see the "verifying PJSIP outbound INVITE headers" practice): with `codecs = ["AMR-WB","G722","PCMU"]`, capture the telephone-side agent's INVITE to a `siptest` PBX. Assert that the `m=audio` payload order starts AMR-WB, G722, PCMU, and that no speex, iLBC, GSM or opus appears. Record the result in `specs/085-client-leg-amr-opus/quickstart.md` under a new "Automated / container checks" heading.
- [ ] T014 [US1] Live: run quickstart.md §2.1–2.6 and §3 (AMR-WB inbound on VoWiFi and VoLTE, bitrate, DTMF, hold/resume, both AMR packings, mode-set) against the Vodafone rig with baresip, and fill in the Results table.

**Checkpoint**: MVP. Bandwidth saving demonstrated for inbound calls.

---

## Phase 4: User Story 2 — Existing deployments unaffected unless the far end asks (P1)

**Goal**: an unchanged config gives the same codec choice as before for far ends with no AMR or Opus, and the GSM path is unchanged.

**Independent test**: quickstart.md §1 and §6, plus T016.

- [X] T015 [US2] In `gsm-sip-bridge/src/sip/mod.rs`, after each `Endpoint::create(self.endpoint_config())` (lines ~210 and ~312), set priority 0 for the prefixes `"AMR"` (which matches AMR and AMR-WB) and `"opus"`, logging a warning and continuing on error, like `prioritize_wideband_codecs` did. Add a short comment explaining that the circuit-switched path must stay byte-for-byte unchanged (FR-009, research R3). If both creation sites share a helper, put it there once.
- [ ] T016 [US2] pjsip-linked check in the privileged container, with no `[sip].codecs`: capture the telephone-side agent's PBX-leg INVITE and diff its `m=audio` codec order against the `main` baseline from T004. The only allowed difference is AMR-WB, AMR and opus appearing after every pre-existing codec and before L16. Repeat for the GSM path's PBX INVITE: no difference at all. Also confirm the veth INVITE stays under 4096 bytes (research R10). Record the results in quickstart.md.
- [X] T017 [P] [US2] Unit test in `gsm-sip-bridge/src/ims/sdp.rs`'s test module: add a veth-offer fixture like `PJSIP_REAL_VETH_OFFER` that also contains `AMR-WB/16000`, `AMR/8000` and `opus/48000/2` (with fmtp), listed *before* L16 and PCMU. Assert that `select_veth_codec` still picks L16 when wideband and PCMU when narrowband, proving Agent A ignores the new codecs (FR-009).
- [ ] T018 [US2] Live: run quickstart.md §1 (default config: G722 to a G722/PCMU-only phone, and AMR-WB when baresip prefers it) and §6 (a GSM call is unchanged). Fill in the Results.

**Checkpoint**: it is now safe to release Phase 1 together with Phase 2.

---

## Phase 5: User Story 3 — Operator chooses the phone-facing codec order (P2)

**Goal**: operators can reorder and restrict codecs, and invalid lists are refused at startup.

**Independent test**: quickstart.md §4 and §5.1–5.2.

- [X] T019 [P] [US3] Integration test in `gsm-sip-bridge/tests/test_config.rs`: load a full config file with `[sip].codecs = ["OPUS","G722","PCMU"]` through the same entry point the daemon uses, and assert `SipConfig.codecs`. Also add a test that an `[sip].codecs` typo key (for example `codec`) fails with the strict-parsing unknown-key error.
- [X] T020 [US3] Make the "codec not in this build" error from T009 fatal at telephone-side agent startup. It should surface as a failed start with that message, not a retry loop. In `gsm-sip-bridge/src/volte/bridge.rs`, the `loop { run_telephony_side(...) }` retry must not spin forever on this config error: classify it as fatal and exit, matching how other config errors stop the process. Add a test for the classification function if one is introduced.
- [X] T021 [US3] Live: run quickstart.md §4.1 (AMR-NB), §4.2 (Opus), §5.1 and §5.2. Fill in the Results.

---

## Phase 6: User Story 4 — Phone dials out using AMR or Opus (P2)

**Goal**: outbound calls from an AMR-only or Opus-only softphone connect, with that codec on the phone leg.

**Independent test**: quickstart.md §2.3 and §4 (outbound halves).

- [X] T022 [US4] Live: from baresip configured AMR-WB-only, then Opus-only, dial an outside number through the bridge in `[sip_server]` mode, once with `codecs` unset and once with it listing the codec. Confirm the call connects with two-way audio and that `phone_codec` and the log show the phone's codec. Fill in the Results. No code is expected; if this fails, open a task for the outbound path in `gsm-sip-bridge/src/vowifi/mod.rs`, where `Account::local` handles incoming calls.

---

## Phase 7: Polish & cross-cutting

- [X] T023 [P] `config.toml.example`: add the commented `codecs` block from `contracts/config.md` under `[sip]`.
- [X] T024 [P] `docs/configuration.md`: add a `[sip].codecs` row (type, default, accepted names, PCMU required, VoWiFi/VoLTE only) so `tests/test_config_docs.rs` passes. In `docs/vowifi-bridge.md`, add a short "Phone-facing codecs" section covering:
  - the bandwidth figures (AMR-WB about 12.65 kbit/s against 64 kbit/s for G.722);
  - that the bridge still transcodes, linking #111 for passthrough;
  - that a PBX in the path must support the codec itself (stock Asterisk has no AMR);
  - that the built-in registrar is the primary AMR target.
- [X] T025 [P] `CHANGELOG.md` / `RELEASE_NOTES.md`: add an entry in the project's release-note style (bold only the feature name). It must state that, with no config change, a phone or PBX that prefers AMR or Opus can now negotiate it (FR-013, clarification Q1).
- [ ] T026 Run `make format && make lint && make test` on the full branch, then run `/speckit-analyze` to confirm that spec.md, plan.md and tasks.md are consistent. Make sure the quickstart Results table is fully filled in before opening the PR. The PR closes #109's option 1 and references #111.

---

## Dependencies & execution order

- **Phase 1 (T001–T004)**: T003 can run in parallel with T001/T002. T004 needs T001–T003.
- **Phase 2 (T005–T009)**: needs only the repository, not the image. T005 and T008 can run in parallel. T007 needs T006. T009 needs T008.
- **US1 (T010–T014)**: needs Phase 2. T013 and T014 also need Phase 1. T011 can run in parallel with T010.
- **US2 (T015–T018)**: needs Phase 2. T015 and T017 are independent of US1. T016 needs Phase 1 and T010.
- **US3 (T019–T021)**: needs Phase 2 and T010.
- **US4 (T022)**: needs US1 (T010, T012).
- **Polish (T023–T026)**: T023–T025 can run in parallel at any time after Phase 2. T026 runs last.

**Release gate**: Phase 1 must never ship without T010 and T015 (research R3).

## Parallel examples

```text
# Phase 1/2 kickoff:
T003 (pjsua-sys/build.rs)  ‖  T005 (tests/test_config.rs)  ‖  T008 (vowifi/codecs.rs)

# After Phase 2:
T010 (vowifi/mod.rs)  ‖  T011 (pjsua-safe/endpoint.rs)  ‖  T015 (sip/mod.rs)  ‖  T017 (ims/sdp.rs tests)

# Docs, any time after Phase 2:
T023  ‖  T024  ‖  T025
```

## Implementation strategy

1. **MVP**: Phases 1 and 2, then US1 (T010–T013) and the safety half of US2 (T015–T017). These ship together, because Phase 1 alone changes default codec order.
2. **Live verification**: T014 and T018 on the Vodafone rig.
3. **Increment**: US3 (fatal-config handling, Opus/AMR-NB live checks), then US4 (outbound live check).
4. **Polish**: T023–T026, then the PR.
