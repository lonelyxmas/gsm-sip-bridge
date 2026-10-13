# Implementation Plan: Multipart SIP bodies and an RFC 3312-conformant precondition answer

**Branch**: `088-multipart-sip-bodies` | **Date**: 2026-10-14 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/088-multipart-sip-bodies/spec.md` (issue #103)

## Summary

Inbound T2 calls are declined `415` because the INVITE body gate only admits
`application/sdp`; T2 sends `multipart/mixed` (optional PIDF-LO + SDP). This
is a v8.15.0 regression and an RFC 5621 §4.2 violation. Add a small shared
MIME body decoder (`ims::body`) that finds the single session description in
any SIP body and use it for the inbound INVITE, the offerless ACK and the
outbound 2xx. Unknown parts are skipped with a log; only bodies with no usable
SDP (415) or broken/ambiguous multipart (400) are refused. Fix the precondition
answer T2 will be the first carrier to exercise: emit `a=des:qos`, never
`a=conf:qos`, and report `e2e` only as met as the caller says. Side fix: a
parameterised SMS content type is decoded as SMS.

## Technical Context

**Language/Version**: Rust (workspace toolchain, `rust-toolchain.toml`)  
**Primary Dependencies**: none new; `tracing` for logs  
**Storage**: N/A  
**Testing**: `cargo test` via `make test`; in-module tests on real parsed `SipRequest`/byte fixtures  
**Target Platform**: Linux (x86_64 host, arm64 Pi), systemd or Docker  
**Project Type**: single Rust binary (`gsm-sip-bridge` crate in a workspace)  
**Performance Goals**: decoding is O(body size); bodies are a few KB; no measurable call-setup latency added  
**Constraints**: bounded work on carrier-controlled input (depth ≤ 4, ≤ 16 parts/level); no PII in logs; no real numbers in fixtures  
**Scale/Scope**: ~1 new module (~300 lines incl. tests), edits in 4 files

## Constitution Check

*GATE: checked before Phase 0 and after Phase 1.*

| Principle | Status |
|---|---|
| I. Integration-first testing | PASS — tests feed real bytes / real parsed `SipRequest`s through the real decoder, SDP parser and answer builder; no mocks. `handle_invite` has no unit seam (needs live control channel + veth, specs/047), so its body decision is extracted into a pure `invite_body_decision` and tested directly (research D7). |
| II. Green-on-commit | PASS — every task ends with `make format && make lint && make test`. |
| III. Frequent atomic commits | PASS — one commit per user story / phase (see tasks.md). |
| IV. Makefile-driven | PASS — no new targets needed. |
| V. Simplicity | PASS with justification — one new module, replacing three ad-hoc body handlings, no new crate (research D2/D3). |

Post-design re-check: unchanged, PASS.

## Project Structure

### Documentation (this feature)

```text
specs/088-multipart-sip-bodies/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/sip-body-handling.md
├── checklists/requirements.md
└── tasks.md            # /speckit-tasks
```

### Source Code

```text
gsm-sip-bridge/src/ims/
├── body.rs             # NEW: MediaType, Disposition, multipart split, extract_sdp, Rejection
├── mod.rs              # register `mod body;`
├── sdp.rs              # precondition answer: des lines, no conf, e2e current, offerer_unmet_mandatory
├── call.rs             # outbound 2xx → body::extract_sdp before parse_answer
├── sip_client.rs       # 415 builder unchanged; reuse build_uas_response for 400
└── agent/
    ├── inbound.rs      # invite_body_decision replaces invite_content_type_supported; ACK decode; unmet-mandatory log
    └── mod.rs          # OPTIONS Accept; decode_pdu_body / message_content_type_supported use MediaType
```

**Structure Decision**: everything lives in the existing `ims` module tree;
the only new file is `ims/body.rs`. Tests are in-module `#[cfg(test)]`, the
pattern every touched file already uses.

## Implementation Notes

- **`body::extract_sdp`** — research D4–D6. Takes `body_bytes`, not the
  lossy `body` string. Returns `Decoded`/`Rejection` (data-model.md). Logs
  once per multipart or refused body (D10).
- **`agent::inbound`** — new `INVITE_BODY_ACCEPT` constant
  (`"application/sdp, multipart/mixed, multipart/alternative"`), reused by the
  OPTIONS response in `agent/mod.rs:453`. `invite_body_decision` builds the
  415 (`build_415_unsupported_media`) or 400 (`build_uas_response(400, "Bad
  Request", …)`) with the call's `to_tag`. `handle_invite` keeps the
  observability report on refusal.
- **Offerless ACK** — capture `Content-Type`, `Content-Disposition` and
  `body_bytes` of the ACK, not just `body`; decode; any non-`Sdp` →
  existing `teardown("offerless_ack_incompatible")`.
- **Outbound 2xx** — `SipResponse` has only `body: String`; decode
  `resp.body.as_bytes()`; map `Rejection` to `BridgeError::Ims`.
- **`sdp`** — data-model.md "Precondition answer". `build_veth_answer` never
  sees QoS lines (PJSIP offers none) so it is unaffected; verify by test.
  Update the existing precondition tests (~`sdp.rs` 2600–2870) that assert
  `a=conf:qos`/`confirm`.
- **Unmet mandatory log** — in `handle_invite` right after
  `precondition_verdict` returns `Proceed`, one `tracing::info!` per call
  listing the unmet segment/direction.
- **SMS** — `decode_pdu_body` and `message_content_type_supported` compare
  `MediaType::parse(ct)` instead of strings.
- **Docs** — `RELEASE_NOTES.md` entry (bold summary only); remove
  `invite_content_type_supported` and its doc reference to SDP-05; replace
  the test that pins multipart as unsupported.

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| New `ims::body` module | Same decoding at three call sites (+ media-type parsing for SMS) | Inline per call site → three copies of RFC 2046 rules; scanning whole body as SDP → the misparse SDP-05 fixed |
