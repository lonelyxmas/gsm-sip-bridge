# Implementation Plan: Tune strongSwan DPD so one ignored request can't tear down a working tunnel

**Branch**: `098-dpd-tuning` | **Date**: 2026-10-18 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/098-dpd-tuning/spec.md`

## Summary

The change has three code parts and a live part.

**1. Setting.** `[vowifi].dpd_delay_sec` (default 120, `0` or `10..=3600`) is parsed in `config/{raw,build,mod}.rs`, exported to the shell as `DPD_DELAY_SEC`, and rendered into every strongSwan line's connection as `dpd_delay = <N>s` through a new `@DPD_DELAY@` placeholder in `docker/strongswan/swanctl-epdg.conf.template`. `SwanctlEpdgParams` gains `dpd_delay_sec`. Startup logs the effective value once (or that it is ignored on swu). The 120s default is the Android IWLAN default, confirmed in AOSP (research R1).

**2. Classification.** The strongSwan engine already lists SAs every tick. A new pure parser, `inbound_esp`, reads each of the connection's CHILD_SAs' `in … N packets, Ms ago` line and returns the youngest last-use age and the summed packets. The engine caches what its last `steady_state_health` saw; the tick snapshots it on every structurally healthy tick into `SteadyTracking.inbound`. When the tick returns `Recovered{IkeSaLost}` with cause `DeadPeer`, a pure `classify_dead_peer` turns the snapshot, the tick time and `DPD_GIVE_UP_WINDOW` (165s, charon's retransmit schedule) into `false`/`real`/`unknown` (research R4), stored in `tracking.dead_peer`.

**3. Reporting.** The strongSwan loop logs the verdict with its evidence and puts it on the existing `TunnelRecovered` event as an optional `dead_peer` field. Ingest counts it in a new `gsm_sip_bridge_vowifi_dead_peer_losses_total{module,verdict}`. No alert (Clarification Q2).

**4. Live.** Reproduce run C at 30s with the probe forced off (US1), then soak at 120s for 12h per carrier (SC-001), black-hole the ePDG (US4), and chase #137's gap (US5). All recorded in `live-verification.md` and `research.md`.

## Technical Context

**Language/Version**: Rust (workspace toolchain, `rust-toolchain.toml`)
**Primary Dependencies**: none new
**Storage**: none (rendered `swanctl.conf` is written at startup as today)
**Testing**: `cargo test` via `make test`:
- **Config:** default, explicit, `0`, `9`/`3601`/negative/non-integer rejected, not a per-line key; `tests/test_config_docs.rs` enforces the docs row.
- **Render:** the snapshot test for the real template updates (`dpd_delay = 120s`); a render test for `0s`.
- **Parser:** `inbound_esp` over real `--list-sas` text (two CHILD_SAs mid-rekey, no `ago`, other connection's SA ignored).
- **Classification:** `classify_dead_peer` truth table with injected instants.
- **Tick:** `FakeEngine` gains a scripted inbound snapshot; IkeSaLost+DeadPeer sets `tracking.dead_peer`; other causes and other reasons leave it `None`.
- **Wire + ingest:** event shape with and without `dead_peer`; counter per verdict; the integration test `tests/test_vowifi_tunnel_recovery_metrics.rs` extended through a real control socket.

**Target Platform**: Linux container (amd64/arm64)
**Project Type**: single Rust workspace (`gsm-sip-bridge` crate)
**Performance Goals**: no new commands or traffic per tick; one extra parse of output already fetched.
**Constraints**:
- With the probe on (default build), behaviour and outer traffic are unchanged (FR-007): DPD still never fires.
- Rendered connection changes only in `dpd_delay` (FR-005).
- swu: setting accepted and ignored; no classification (no reporter, no SA listing).

**Scale/Scope**: 1–8 lines per host. Production changes: ~40 lines config, ~10 render/template, ~60 engines, ~70 line_supervisor, ~25 orchestrate, ~30 protocol/metrics/ingest. Plus tests, docs, changelog.

## Constitution Check

| Principle | Status |
|---|---|
| I. Integration-first testing | **Pass.** Parser and classifier tested on real `swanctl` output text; the event goes through a real control socket into the real registry. The tick uses the existing `FakeEngine`, whose justification (charon, a netns, root) is documented at its definition. |
| II. Green-on-commit | **Pass.** `make format && make lint && make test` before every commit, with `CARGO_TARGET_DIR=$PWD/target-098`. |
| III. Atomic commits | **Pass.** (1) spec artifacts; (2) the setting and render; (3) parser + classification in the tick; (4) event, metric, log; (5) docs and changelog; (6) research and live verification. |
| IV. Makefile-driven | **Pass.** No new targets. |
| V. Simplicity | **Pass.** No new process, socket or alert. Reuses the per-tick SA listing, the `SteadyTracking` side-channel pattern from 097, and the existing event. |

Re-checked after Phase 1: unchanged.

## Project Structure

### Documentation (this feature)

```text
specs/098-dpd-tuning/
├── spec.md
├── plan.md               # this file
├── research.md           # R1–R8; R7/R8 findings appended after the rig runs
├── data-model.md
├── quickstart.md         # live recipe
├── contracts/
│   ├── config.md         # [vowifi].dpd_delay_sec and its rendering
│   └── dead-peer.md      # classification rule, event field, metric, log line
├── checklists/requirements.md
├── tasks.md
└── live-verification.md  # written after the rig runs
```

### Source Code (repository root)

```text
docker/strongswan/swanctl-epdg.conf.template   # dpd_delay = @DPD_DELAY@
gsm-sip-bridge/src/
├── config/{raw,build,mod}.rs     # dpd_delay_sec
├── commands/{config,discover}.rs # DPD_DELAY_SEC export; discover renders with it
├── supervise/
│   ├── render.rs                 # SwanctlEpdgParams.dpd_delay_sec, @DPD_DELAY@
│   ├── engines.rs                # inbound_esp(); StrongswanEngine caches the last snapshot
│   ├── line_supervisor.rs        # InboundEsp, DeadPeerVerdict, classify_dead_peer, tick wiring
│   └── orchestrate.rs            # params, startup log, verdict log, event field
├── control/protocol.rs           # DeadPeerVerdict wire enum; TunnelRecovered.dead_peer
└── metrics/{mod,ingest}.rs       # VOWIFI_DEAD_PEER_LOSSES_TOTAL
gsm-sip-bridge/tests/test_vowifi_tunnel_recovery_metrics.rs
docs/{configuration,observability}.md, config.toml.example, CHANGELOG.md
```

**Structure Decision**: single crate; each change in the module that owns its concern.

## Complexity Tracking

No violations.
