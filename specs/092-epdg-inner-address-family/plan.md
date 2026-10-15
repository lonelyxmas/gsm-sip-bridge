# Implementation Plan: Per-line inner address family for the ePDG tunnel

**Branch**: `092-epdg-inner-address-family` | **Date**: 2026-10-14 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/092-epdg-inner-address-family/spec.md` (GitHub #126, fix for #107)

## Summary

This adds `tunnel_address_family = "dual" | "ipv4" | "ipv6"` to `[vowifi]`, overridable per `[[vowifi.line]]`. It sets which inner address families a line's ePDG tunnel requests (`vips`) and which remote traffic it carries (`remote_ts`). The values reach charon through two new placeholders in `docker/strongswan/swanctl-epdg.conf.template`.

`dual` is the default, and with it every setting line renders exactly as today. `ipv4` is the documented remedy for an ePDG that rejects CHILD_SA rekeys with `INVALID_SELECTORS`.

Three smaller pieces come with it:

- The supervisor logs each line's effective family.
- A `dual` line warns once per tunnel session when its rekey is rejected with `INVALID_SELECTORS`.
- On `ipv4` and `ipv6` lines, P-CSCFs of the excluded family are filtered out of the supervisor's selection and spec 091's pool.

The swu engine refuses a value other than `dual` at config load. No IKE daemon change.

## Technical Context

**Language/Version**: Rust (workspace toolchain pinned in `rust-toolchain.toml`)
**Primary Dependencies**: existing only: serde/toml (config), insta (snapshots), clap (CLI). Runtime: the Osmocom strongswan-epdg fork, pinned at `022f3e5` and unchanged.
**Storage**: none. Config is TOML; the line-resolution file in `/tmp` carries `VowifiConfig`, which gets a new `#[serde(default)]` field.
**Testing**: `make test` (cargo nextest), `make lint` (clippy `-D warnings` over all targets, plus deny/shellcheck).
**Target Platform**: Linux containers, amd64 and arm64 (Raspberry Pi).
**Project Type**: a single Rust binary (`gsm-sip-bridge`) with supervisor, agents and CLI subcommands.
**Performance Goals**: n/a. The steady-state tick reads the charon log as it already does; the detector adds one linear pass over this line's lines.
**Constraints**:
- Per line, on one shared charon (FR-006).
- `dual` renders the same setting lines as today (FR-003, research R2).
- No `@…@` token may be left in the output.
- No real subscriber identifiers in tests or docs (CLAUDE.md).
**Scale/Scope**: about 1 to 4 VoWiFi lines per container. Around 8 source files touched, plus the template, docs and release notes.

No `NEEDS CLARIFICATION` remains. research.md R1–R10 resolve every design question.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | Status | How |
|---|---|---|
| I. Integration-First Testing | PASS | Tests render the **real** template (`include_str!`) and parse real charon-log text and real TOML through the real config builder. No mocks are introduced (R10). |
| II. Green-on-Commit | PASS | Each task in tasks.md ends green. The snapshot is committed *before* the template changes, so the "unchanged" claim is proven by a commit that passes both before and after. |
| III. Frequent Atomic Commits | PASS | tasks.md groups the work into commit-sized units: snapshot, config, render, swu check, filter, detector, docs. |
| IV. Makefile-Driven Build | PASS | Only `make format`, `make lint` and `make test` are needed. No new targets. |
| V. Simplicity & Refactorability | PASS | One enum and two placeholders, following existing patterns (per-line override as `reg_rejected_cooldown_sec`, log scanning as `ike_sa_loss_cause`). The pure `extract_*` functions keep their signatures; filtering is added in the engine only. No new abstraction layers. |

**Post-design re-check**: PASS. Phase 1 added no layers. Contracts are documentation only.

## Project Structure

### Documentation (this feature)

```text
specs/092-epdg-inner-address-family/
├── spec.md
├── plan.md              # this file
├── research.md          # Phase 0
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1
├── contracts/
│   ├── config.md        # the TOML key, errors, doc obligations
│   ├── rendered-tunnel.md
│   └── logs.md          # the three supervisor log lines
├── checklists/requirements.md
└── tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
docker/strongswan/
└── swanctl-epdg.conf.template     # vips = @VIPS@, remote_ts = @REMOTE_TS@; header + vips comment updated

gsm-sip-bridge/src/
├── config/
│   ├── mod.rs                     # TunnelAddressFamily enum; VowifiConfig + VowifiLineOverride fields
│   ├── raw.rs                     # RawVowifi / RawVowifiLine keys (section! macro -> key lists)
│   └── build.rs                   # parse + validate both levels; swu refusal (R4, R5)
├── vowifi/discovery.rs            # resolve_one_line applies the line override
├── supervise/
│   ├── render.rs                  # SwanctlEpdgParams.address_family; substitute @VIPS@/@REMOTE_TS@; snapshot tests
│   ├── engines.rs                 # StrongswanEngine.address_family; filtered pcscf methods; ignored_pcscfs;
│   │                              #   child_rekey_invalid_selectors_session (+ engine wrapper)
│   └── orchestrate.rs             # pass family to render + engine; log effective family; log ignored P-CSCFs
│                                  #   at publication points; per-session INVAL_SEL warning in steady state;
│                                  #   priming engine passes Dual
├── cli.rs                         # render swanctl-epdg --address-family (default dual)
└── commands/discover.rs           # pass --address-family through

gsm-sip-bridge/tests/
└── test_config_docs.rs            # unchanged; enforces the new key is documented

config.toml.example                # [vowifi] key + [[vowifi.line]] example, symptom text
docs/configuration.md              # rows in [vowifi] and [[vowifi.line]] tables
RELEASE_NOTES.md                   # ## Unreleased entry (#126, #107)
```

**Structure Decision**: this is a change to the existing single-crate layout; no new modules. Unit tests go in the `#[cfg(test)]` modules next to the code they cover, as in `render.rs` and `engines.rs` today. Config parsing tests go in `config/build.rs`'s tests or `tests/test_config.rs`, wherever existing `[vowifi]` validation tests live.

## Design notes

1. **Snapshot first (SC-001).** Record an insta snapshot of the real template, rendered with fixed synthetic parameters and comment lines removed. Commit it while the template is unchanged. Every later commit must leave it unchanged.
2. **Render.** `render_swanctl_epdg` substitutes `@VIPS@` and `@REMOTE_TS@` from `params.address_family` (R1). The template's header documents both placeholders, and the comment above `vips` describes the setting instead of "both".
3. **Config.** The `TunnelAddressFamily` enum lives in `config/mod.rs`. Validation in `build.rs` follows the existing error style. After `tunnel_engine` is parsed, it checks both levels against swu (R5).
4. **Engine filter (FR-013).** `StrongswanEngine.address_family` filters each extracted batch. The single selection becomes `pick_stable_pcscf(filtered pool)`, which gives the same result for `Dual`. The direct pool extraction in `orchestrate.rs` (around line 1771) moves to the engine method. `ignored_pcscfs` is logged at the four publication points (R6).
5. **Detector (FR-009a).** A pure function over the charon log, scoped to the current IKE_SA session, returns the session tag. The steady-state `StillUp` branch warns when the tag is new, for `Dual` lines only (R7).
6. **Interaction with spec 091.** The agent's `pcscf_pool.rs` is untouched. It only ever sees the filtered pool the supervisor publishes.

## Risks

| Risk | Mitigation |
|---|---|
| The fork's `p-cscf` plugin requests P-CSCF6 even for an `ipv4`-only `vips` | The FR-013 filter makes it harmless. Quickstart step 3 checks the CPRQ live. |
| A template comment edit accidentally changes a setting line | The comment-stripped snapshot from design note 1 catches it. |
| Detector log text differs in the fork | The match uses `CREATE_CHILD_SA response` and `N(INVAL_SEL)`, which appear verbatim in #107's capture from this fork. The reporter confirms the warning fires (quickstart 4.1). |
| An older line-resolution file in `/tmp` lacks the field | `#[serde(default)]` reads it as `Dual`. The usual recreate-not-restart advice applies anyway. |

## Complexity Tracking

No constitution violations to justify.
