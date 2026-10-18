# Tasks: Tune strongSwan DPD (098)

**Input**: `specs/098-dpd-tuning/` (spec.md, plan.md, research.md, data-model.md, contracts/, quickstart.md)

**Tests**: required. The constitution defaults to TDD: write each test before the code it covers, and watch it fail first.

Paths are relative to `gsm-sip-bridge/src/` unless stated otherwise.

Before every commit:
- run `make format && make lint && make test` with `CARGO_TARGET_DIR=$PWD/target-098` (shared-target-dir memory);
- end the message with the Co-Authored-By trailer.

Fixtures use synthetic addresses only (`10.0.0.x`, `192.0.2.x`, `2001:db8::x`).

**Order note**: the live research (US1) needs the setting (US2) and the classification (US3) in the image, so the code phases run first even though US1 is P1. The live phases then run US1 → US4 → US5 → the SC-001 soak.

## Phase 1: Setup

- [X] T001 Confirm the branch is `098-dpd-tuning` on top of `origin/main` (f793f89) and that `make test` is green before any change. Commit the spec artifacts as `docs(098): spec, plan and tasks for DPD tuning (#120)`.

## Phase 2: Foundational

None: the setting and the classification are independent.

## Phase 3: User Story 2 — configurable, phone-like DPD delay (P1, code)

**Goal**: [contracts/config.md](contracts/config.md). **Independent test**: config and render unit tests; the rendered connection differs only in `dpd_delay`.

- [X] T002 [US2] Tests first in `config/mod.rs` `mod tests`: default 120; explicit `300`; `0` accepted; `9`, `3601`, `-1`, `"120"` rejected with `vowifi.dpd_delay_sec` in the error; not a per-line key.
- [X] T003 [US2] Add `dpd_delay_sec: u64` to `RawVowifi` (`config/raw.rs`, default 120) and `VowifiConfig` (`config/mod.rs`, doc comment per contract), validate in `build_vowifi` (`config/build.rs`): `0` or `10..=3600`, error `vowifi.dpd_delay_sec must be 0 or between 10 and 3600 (got N)`.
- [X] T004 [P] [US2] Export `DPD_DELAY_SEC` in `commands/config.rs` beside `IDLE_KEEPALIVE`; extend its test if the export list is asserted.
- [X] T005 [US2] Tests first in `supervise/render.rs`: `@DPD_DELAY@` renders `120s` and `0s`; then add `dpd_delay_sec: u64` to `SwanctlEpdgParams` and the substitution. Replace `dpd_delay = 30s` with `dpd_delay = @DPD_DELAY@` in `docker/strongswan/swanctl-epdg.conf.template`, updating its comment (#120 finding 2, R1/R2). Update every `SwanctlEpdgParams` construction (`supervise/orchestrate.rs`, `commands/discover.rs`, render tests) and accept the snapshot change (`dpd_delay = 120s`) after checking it is the only diff.
- [X] T006 [US2] Startup log in `supervise/orchestrate.rs` (strongSwan and swu wording per contract), once per deployment, next to the idle-keepalive line. Test via the existing startup-log test seam if one exists; otherwise a small pure `dpd_startup_line(engine, secs) -> String` with a unit test.
- [X] T007 [US2] `make format lint test`; commit `feat(vowifi): [vowifi].dpd_delay_sec, default 120s like a handset (#120)`.

## Phase 4: User Story 3 — classify false vs real dead peers (P2, code)

**Goal**: [contracts/dead-peer.md](contracts/dead-peer.md). **Independent test**: parser and classifier unit tests; event → socket → registry integration test.

- [X] T008 [P] [US3] Tests first in `supervise/engines.rs`: `inbound_esp(sas, "ims0")` over real `--list-sas` text — one SA with `4s ago`; two SAs mid-rekey (youngest age, summed packets); an SA with no `ago` (age `None`); another connection's SA ignored; no CHILD_SA → `None`. Then implement it, returning `line_supervisor::InboundEsp`.
- [X] T009 [US3] In `supervise/line_supervisor.rs`: `InboundEsp`, `DeadPeerVerdict`, `DeadPeerClassification`, `DPD_GIVE_UP_WINDOW` (165s, derivation in the doc comment), and pure `classify_dead_peer(snapshot: Option<(Instant, InboundEsp)>, now: Instant) -> DeadPeerClassification`. Tests first: the full table in the contract, including boundaries (`L == T − G` → unknown/real per rule, `L == S − G` → real).
- [X] T010 [US3] Add `fn inbound_esp(&self) -> Option<InboundEsp>` to `TunnelEngine` (default `None`); `StrongswanEngine` caches what `steady_state_health` parsed (a `Cell`/`RefCell`, cleared when the listing fails). `FakeEngine` gets a scripted value.
- [X] T011 [US3] Tick wiring, tests first: `Ok` health stores `tracking.inbound = Some((now, esp))`; `ProcessDied`/`ViciBroken`/`TunVanished` clear it; `IkeSaMissing` with `DeadPeer` sets `tracking.dead_peer`; `CarrierDelete`/`Unknown` and every other outcome leave it `None`; it is cleared at the start of each tick. Use an injected `now` only if the tick already takes one; otherwise test `classify_dead_peer` for timing and the tick for wiring.
- [X] T012 [P] [US3] `control/protocol.rs`: `DeadPeerVerdict` wire enum (`false`/`real`/`unknown`, `as_str`), `TunnelRecovered.dead_peer: Option<DeadPeerVerdict>` with `serde(default, skip_serializing_if)`. Tests: existing wire shapes unchanged; with `dead_peer`; an old payload without it parses. Update every `TunnelRecovered` construction.
- [X] T013 [US3] `metrics/mod.rs`: `VOWIFI_DEAD_PEER_LOSSES_TOTAL` (`module`, `verdict`); `metrics/ingest.rs` counts it from `TunnelRecovered{dead_peer: Some(v)}`. Unit test per verdict, and none for an event without it.
- [X] T014 [US3] `supervise/orchestrate.rs`: `tunnel_recovered_event` takes the verdict; the strongSwan `Recovered` branch logs `dead peer verdict: …` with evidence per contract, and sends it on the event.
- [X] T015 [US3] Extend `gsm-sip-bridge/tests/test_vowifi_tunnel_recovery_metrics.rs`: a real reporter sends `TunnelRecovered{IkeSaLost, dead_peer: false}` then `real`; the registry shows one each, and the recovery counter still counts both.
- [X] T016 [US3] `make format lint test`; commit `feat(supervise): classify dead-peer IKE_SA losses by inbound ESP, and count them (#120)`.

## Phase 5: Polish (docs)

- [X] T017 [P] `docs/configuration.md` `[vowifi]` row for `dpd_delay_sec` (default, `0`, range, recreate-to-apply, no effect while the probe runs); `config.toml.example` commented entry; `docs/observability.md` the new counter; `docs/operations.md` how to read a `false` verdict; `CHANGELOG.md` under Unreleased (bold summary only). `tests/test_config_docs.rs` must pass.
- [X] T018 `make format lint test`; commit `docs: dpd_delay_sec and the dead-peer loss counter (#120)`.

## Phase 6: User Story 1 — reproduce and explain the ignored DPD (P1, live)

**Independent test**: research note with evidence and a per-carrier verdict.

- [X] T019 [US1] Build the probe-forced throwaway image (quickstart) for amd64 (Vodafone, local) and arm64 (Jio, via `arm-build-200`). Never push the commit.
- [X] T020 [US1] Deploy on both rigs with `dpd_delay_sec = 30`; start the captures and the charon-log copy loop (quickstart §1). Recreate, confirm `Registered`.
- [X] T021 [US1] Run up to 12h. For each `giving up after 5 retransmits`, extract the timeline (R7) from charon log + outer capture + supervisor log, and the live `dead peer verdict`.
- [X] T022 [US1] Write the findings into `specs/098-dpd-tuning/research.md` (R7 Findings): per-carrier verdict, DPD exchanges answered vs ignored. If *rekey/collision-triggered*, stop and amend the spec (US1 scenario 3) before T023.

## Phase 7: User Story 4 — a dead ePDG is still caught (P2, live)

- [ ] T023 [US4] On the probe-forced build at the default 120s, black-hole the ePDG on each rig (quickstart §3). Record fault → first DPD → give-up → rebuild → re-registration after removal; expect verdict `real` and a rebuild within 300s.

## Phase 8: User Story 5 — the #137 gap (P3, live)

- [ ] T024 [US5] Vodafone, probe-forced, P-CSCF black-holed with the agent log streamed out (quickstart §5). Account for every interval within 5s in `research.md` (R8 Findings).
- [ ] T025 [US5] If a fix qualifies under FR-011a, implement it test-first in `ims/agent/`, re-verify on both rigs, commit separately; otherwise file a GitHub issue and link it from R8.

## Phase 9: Verification soak and sign-off

- [ ] T026 12h idle soak per carrier on the probe-forced build at the default (quickstart §2): zero `giving up after`, ≥1 REGISTER refresh, CHILD_SA rekey and IKE_SA rekey; DPD requests/hour vs T021's baseline (SC-002).
- [ ] T027 Real branch image (probe on), 1h idle per rig: no DPD in the outer capture, no new recoveries (SC-004).
- [ ] T028 Write `specs/098-dpd-tuning/live-verification.md` (SC-001…SC-007, addresses redacted to roles); commit `docs(098): research findings and live verification (#120)`.

## Dependencies

- T001 → everything.
- US2 (T002–T007) and US3 (T008–T016) are independent; within US3, T008/T012 can run in parallel, T009 → T010 → T011 → T014 → T015.
- T017 after US2 and US3.
- Live: T019 needs US2+US3 merged into the branch. T020 → T021 → T022 → T023 (gate on the verdict) → T026. T024 can run on Vodafone after T022, in parallel with Jio's T023. T027 any time after T019.

## Parallel examples

- US2: T004 alongside T005.
- US3: T008 (engines.rs) alongside T012 (protocol.rs).
- Live: Jio's 12h soak runs while Vodafone does T024.

## Implementation strategy

MVP is US2: the setting alone, shipped with the default, changes nothing in production and unblocks every live run. US3 adds the evidence. The live phases decide whether the default stands; probe removal is a later feature gated on SC-001/SC-003 and its own ≥48h soak.
