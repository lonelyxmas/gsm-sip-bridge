# Implementation Plan: AMR and Opus on the phone-facing leg

**Branch**: `085-client-leg-amr-opus` | **Date**: 2026-10-12 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/085-client-leg-amr-opus/spec.md` (issue #109, option 1)

## Summary

Build pjproject with opencore-amr, vo-amrwbenc and libopus, so the telephone-side agent's PJSIP can negotiate AMR-WB, AMR-NB and Opus with the PBX or phone. A new optional `[sip].codecs` list lets operators set the order. When the list is absent, existing codecs keep today's order and the three new codecs are pinned to the lowest non-internal priority.

PCMU is mandatory, because the internal agent link falls back to it (research R4). The circuit-switched path explicitly disables the new codecs, so it is unchanged. Audio is still decoded and re-encoded; passthrough is #111.

## Technical Context

**Language/Version**: Rust (workspace toolchain in `rust-toolchain.toml`), plus C pjproject 2.16 built in Docker
**Primary Dependencies**: pjproject 2.16 (`pjsua-sys`/`pjsua-safe`), opencore-amr, vo-amrwbenc, libopus (Alpine packages)
**Storage**: N/A (config and in-memory per-call record only)
**Testing**: `cargo test` via `make test`; pjsip-linked checks in the privileged test container; manual baresip live calls (quickstart.md)
**Target Platform**: Linux container (Alpine), amd64 and arm64 (Pi)
**Project Type**: Long-running daemon and CLI (`gsm-sip-bridge` workspace)
**Performance Goals**: SC-001, phone-leg bandwidth at most 40% of G.722's with AMR-WB; SC-004, added latency within 20 ms of G.722
**Constraints**: zero behaviour change for far ends that support neither AMR nor Opus (SC-002); GSM path byte-for-byte unchanged (FR-009); veth INVITE stays within a single 4096-byte datagram
**Scale/Scope**: one deployment-wide setting; touches about 6 source files, the Dockerfile and the docs

## Constitution Check

*GATE: must pass before Phase 0 research; re-checked after Phase 1 design.*

| Principle | Status | How |
|-----------|--------|-----|
| I. Integration-first testing | ✅ | Config parsing and validation are tested through the real loader (`tests/test_config.rs` style). The priority plan is a pure function, tested directly with no mocks. pjsip-linked offer ordering is checked in the privileged container. No mocks are introduced. |
| II. Green-on-commit | ✅ | Every task ends with `make format && make lint && make test`. |
| III. Atomic commits | ✅ | The tasks map to independent commits: build deps, config, plan function, apply on Agent B, GSM pin, observability, docs. |
| IV. Makefile-driven | ✅ | No new tooling. The Docker build already runs through existing targets. |
| V. Simplicity | ✅ | Per-leg SDP filtering was rejected in favour of a mandatory PCMU (R4). There is no Opus tuning knob (Q3) and no metric label (R8). |

Re-check after design: still passing, with no violations, so Complexity Tracking is empty.

## Project Structure

### Documentation (this feature)

```text
specs/085-client-leg-amr-opus/
├── spec.md
├── plan.md              # this file
├── research.md          # R1–R11
├── data-model.md        # PhoneCodec, CodecPriorityPlan, CallRecord.phone_codec
├── quickstart.md        # manual live verification (FR-014b)
├── contracts/config.md  # [sip].codecs contract
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks
```

### Source code touched

```text
docker/Dockerfile                         # pjsip-builder: -dev pkgs, --with-* flags, config_auto.h assert; runtime: + opus
pjsua-sys/build.rs                        # system_libs += opencore-amrnb, opencore-amrwb, vo-amrwbenc, opus
gsm-sip-bridge/src/config/raw.rs          # [sip].codecs: Option<Vec<String>>
gsm-sip-bridge/src/config/build.rs        # parse into Vec<PhoneCodec> + validation (FR-010)
gsm-sip-bridge/src/config/mod.rs          # SipConfig.codecs: Option<Vec<PhoneCodec>>, PhoneCodec enum
gsm-sip-bridge/src/vowifi/codecs.rs       # NEW: pure CodecPriorityPlan + apply + build-presence check
gsm-sip-bridge/src/vowifi/mod.rs          # replace prioritize_wideband_codecs with vowifi::codecs
gsm-sip-bridge/src/vowifi/control.rs      # CallRecord.phone_codec
gsm-sip-bridge/src/sip/mod.rs             # GSM endpoint: pin AMR/AMR-WB/opus to 0 (FR-009)
pjsua-safe/src/endpoint.rs                # expose negotiated codec per call (reuse negotiated_audio_codec)
config.toml.example, docs/configuration.md, docs/vowifi-bridge.md, CHANGELOG.md / RELEASE_NOTES.md
gsm-sip-bridge/tests/test_config.rs       # [sip].codecs parsing/validation
```

**Structure Decision**: Everything stays in the existing workspace crates. The one new module, `vowifi/codecs.rs`, holds the pure priority plan so it can be tested in isolation and called from `run_telephony_side`, which VoWiFi and VoLTE share.

## Phase 0: Research

Complete; see [research.md](research.md). Key outcomes:

- **R1/R2**: explicit `--with-*` configure flags plus a `config_auto.h` assertion, and dynamic linking of the codec libraries.
- **R3**: newly compiled codecs default above PCMU, so they must be pinned on Agent B *and* disabled on the GSM endpoint.
- **R4**: PCMU is mandatory. This amends spec FR-004 and User Story 3.
- **R6**: PJSIP's AMR codec already honours the far end's `octet-align` and `mode-set`, so no config call is needed.
- **R9**: the setting lives at `[sip].codecs`.

## Phase 1: Design

- [data-model.md](data-model.md): the `PhoneCodec` mapping, validation rules, the `CodecPriorityPlan` truth table, and `CallRecord.phone_codec`.
- [contracts/config.md](contracts/config.md): the operator-facing config contract and error messages.
- [quickstart.md](quickstart.md): the live verification script and results table.

## Risks

| Risk | Mitigation |
|------|------------|
| The priority plan doesn't reproduce today's default offer exactly | A pjsip-linked test diffs the offer's codec order with the setting unset against the pre-change order captured from `main` |
| A phone that only does octet-aligned AMR rejects our bandwidth-efficient offer | It falls back to G.722 or PCMU (FR-005); quickstart §3 checks this live |
| arm64 image build lacks `opus-dev` or `vo-amrwbenc-dev` | The `config_auto.h` assertion fails the build loudly; verify on the arm64 pipeline before release |
| A PBX that supports Opus starts negotiating it with no config change | Accepted by clarification Q1; a release note is required (FR-013) |

## Complexity Tracking

No violations.
