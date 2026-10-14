---
description: "Task list for specs/092-epdg-inner-address-family"
---

# Tasks: Per-line inner address family for the ePDG tunnel

**Input**: `specs/092-epdg-inner-address-family/`: plan.md, spec.md (2026-10-14 clarifications), research.md (R1–R10), data-model.md, contracts/config.md, contracts/rendered-tunnel.md, contracts/logs.md, quickstart.md
**Tests**: Included. The constitution makes TDD the default. Write each test first and watch it fail before you implement. The one exception is T001, the baseline snapshot: it must pass on the **unchanged** code.

**Every commit** (CLAUDE.md, constitution II): run `make format && make lint && make test` first, with a private `CARGO_TARGET_DIR=$PWD/target` (the shared target dir is clobbered by other worktrees). Commit only if all three pass.

**Never commit real MSISDNs, IMSIs or carrier addresses.** Use:
- IMSI `001010000000001`, MCC/MNC `001`/`001`;
- ePDG `192.0.2.1`;
- P-CSCFs `10.0.0.10`, `10.0.1.10`, `2402:8100::10`, `2402:8100::11`;
- connection names `ims0`, `ims1`.

All paths are relative to the repo root:

| Shorthand | Path |
|---|---|
| `template` | `docker/strongswan/swanctl-epdg.conf.template` |
| `config/mod.rs` | `gsm-sip-bridge/src/config/mod.rs` (tests: `mod tests`, `MINIMAL_TOML` fixture) |
| `raw.rs` | `gsm-sip-bridge/src/config/raw.rs` |
| `build.rs` | `gsm-sip-bridge/src/config/build.rs` |
| `discovery.rs` | `gsm-sip-bridge/src/vowifi/discovery.rs` |
| `render.rs` | `gsm-sip-bridge/src/supervise/render.rs` |
| `engines.rs` | `gsm-sip-bridge/src/supervise/engines.rs` |
| `orchestrate.rs` | `gsm-sip-bridge/src/supervise/orchestrate.rs` |
| `cli.rs` | `gsm-sip-bridge/src/cli.rs` |
| `discover.rs` | `gsm-sip-bridge/src/commands/discover.rs` |

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- **[Story]**: US1 = stop the hourly outage (P1); US2 = existing deployments unaffected (P1); US3 = one line changed, others untouched (P2); US4 = a typo fails loudly (P2)

---

## Phase 1: Setup (baseline before anything changes)

**Purpose**: record what the current release renders, so that "unchanged for `dual`" (FR-003, SC-001) is checked against a fixed reference rather than claimed.

- [ ] T001 In `render.rs` `mod tests`, add `fn strip_comments(s: &str) -> String`. It drops every line whose first non-whitespace character is `#` and keeps every other line and its newline exactly (research R2). Then add the test `real_template_dual_render_matches_the_pre_092_baseline`:
  - load the real template with `include_str!("../../../docker/strongswan/swanctl-epdg.conf.template")`;
  - render it with fixed synthetic `SwanctlEpdgParams`: `conn_name "ims0"`, `imsi "001010000000001"`, `mcc "001"`, `mnc "001"`, `epdg_ip "192.0.2.1"`, `if_id "23"`, `updown_script "/etc/strongswan.d/ims-updown-0.sh"`, `src_addr None`;
  - `insta::assert_snapshot!(strip_comments(&rendered))`.

  Run it on the **unchanged** template and code, accept the snapshot (`cargo insta accept`), and confirm the `.snap` contains `vips = 0.0.0.0, ::` and `remote_ts = 0.0.0.0/0, ::/0`. Commit it on its own: `test(092): snapshot the dual ePDG connection before adding the family setting`. **No later task may update this snapshot.** If a later task changes it, that task is wrong.

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: the enum, the config plumbing and the render parameter that every story uses. When this phase ends, everything still behaves as `dual`, and T001's snapshot is untouched.

- [ ] T002 Add `pub enum TunnelAddressFamily { #[default] Dual, Ipv4, Ipv6 }` to `config/mod.rs`, with:
  - derives `Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize` and `#[serde(rename_all = "lowercase")]`;
  - `FromStr`, accepting exactly `dual`, `ipv4`, `ipv6` (case-sensitive), with `Err(String)` listing the accepted values;
  - `Display`, printing the lowercase spelling;
  - `pub fn vips(self) -> &'static str` and `pub fn remote_ts(self) -> &'static str`, returning the values in data-model.md;
  - `pub fn admits(self, addr: &str) -> bool`: `Dual` admits everything; `Ipv4` and `Ipv6` admit only addresses that parse (`IpAddr`) as their own family.

  Write its tests first in `config/mod.rs` `mod tests`:
  - each spelling round-trips through `FromStr` and `Display`;
  - `"IPv4"`, `"ip4"`, `""` and `"both"` are errors whose message lists all three values;
  - `vips` and `remote_ts` match data-model.md exactly for each variant;
  - the `admits` table: `10.0.0.10` and `2402:8100::10` under each family, and an unparseable string admitted by `Dual` only.
- [ ] T003 Add the TOML keys in `raw.rs`:
  - `RawVowifi.tunnel_address_family: String`, defaulting to `"dual"` (next to `tunnel_engine`, with a doc comment pointing at `VowifiConfig::tunnel_address_family`);
  - `RawVowifiLine.tunnel_address_family: Option<String>`.

  Both keys must come out of the `section!` macro's generated key lists, so the unknown-key check accepts them. Add the matching fields:
  - `VowifiConfig.tunnel_address_family: TunnelAddressFamily` with `#[serde(default)]`;
  - `VowifiLineOverride.tunnel_address_family: Option<TunnelAddressFamily>` with `#[serde(default)]`.

  Both go in `config/mod.rs`, with doc comments referencing specs/092.
- [ ] T004 In `build.rs`, parse both levels with `TunnelAddressFamily::from_str`:
  - `[vowifi]` goes into `VowifiConfig.tunnel_address_family`;
  - each `[[vowifi.line]]` goes into `VowifiLineOverride.tunnel_address_family`.

  For now, a parse failure becomes `BridgeError::Config(format!("vowifi.tunnel_address_family must be \"dual\", \"ipv4\" or \"ipv6\", got {v:?}"))`, or `vowifi.line[{i}].tunnel_address_family ...` for a line. The exact wording is in contracts/config.md; US4 adds the tests. Test first in `config/mod.rs` `mod tests`: a config with no key gives `Dual`, and `[vowifi] tunnel_address_family = "ipv4"` gives `Ipv4`.
- [ ] T005 In `discovery.rs` `resolve_one_line`, apply the line override next to the `reg_rejected_cooldown_sec` override (around line 429): `if let Some(f) = over.and_then(|o| o.tunnel_address_family) { config.tunnel_address_family = f; }`. Test first in `discovery.rs` `mod tests`: a base of `Ipv4` with no override gives `Ipv4`, and a base of `Dual` with a line override of `Ipv6` gives `Ipv6`.
- [ ] T006 In `render.rs`:
  - add `pub address_family: TunnelAddressFamily` to `SwanctlEpdgParams`;
  - in `render_swanctl_epdg`, add `.replace("@VIPS@", params.address_family.vips())` and `.replace("@REMOTE_TS@", params.address_family.remote_ts())` to the substitution chain;
  - in the template, change `vips = 0.0.0.0, ::` to `vips = @VIPS@` and `remote_ts = 0.0.0.0/0, ::/0` to `remote_ts = @REMOTE_TS@`;
  - in the template header's placeholder list, document `@VIPS@` and `@REMOTE_TS@` (from `[vowifi].tunnel_address_family`, per line, specs/092). Replace the comment above `vips`, which currently says it requests both families, with one that says the requested families come from `tunnel_address_family` (default both) and names the #107 symptom `ipv4` exists for.

  Pass `address_family: TunnelAddressFamily::Dual` at **every** existing construction site for now: `orchestrate.rs` (around line 1524), `discover.rs` (around line 208), and the existing `render.rs` tests. **T001's snapshot must still pass unchanged.** Add `dual_render_leaves_no_placeholder` (real template, `Dual`: the output contains no `@VIPS@` and no `@REMOTE_TS@`).
- [ ] T007 In `engines.rs`, add `pub address_family: TunnelAddressFamily` to `StrongswanEngine`. Set `Dual` at every construction site: `orchestrate.rs` (around lines 1622 and 1801) and the `strongswan_engine()` test helper. No behaviour change yet.

**Checkpoint**: `make test` is green and T001's snapshot is unchanged. Commit: `feat(092): tunnel_address_family config key and @VIPS@/@REMOTE_TS@ placeholders (dual only)`.

---

## Phase 3: User Story 1: stop the hourly outage on an affected ePDG (P1) 🎯 MVP

**Goal**: setting `ipv4` on a line makes that line's tunnel request only IPv4. The line logs its effective family, ignores P-CSCFs of the other family, and a `dual` line points the operator at the setting when a rekey is rejected with `INVALID_SELECTORS`.

**Independent test**: render with `ipv4` and check the `vips` and `remote_ts` lines. Feed the engine a mixed-family charon log and check the `ipv4` selection and pool. Feed the detector a log containing `N(INVAL_SEL)` and check for exactly one tag per session.

### Tests for User Story 1 (write first, watch them fail)

- [ ] T008 [P] [US1] In `render.rs` `mod tests`, add `ipv4_and_ipv6_change_only_vips_and_remote_ts`. Render the real template with the T001 parameters under each family. Assert:
  - `ipv4` contains `        vips = 0.0.0.0\n` and `                remote_ts = 0.0.0.0/0\n`;
  - `ipv6` contains `        vips = ::\n` and `                remote_ts = ::/0\n`;
  - after `strip_comments`, each render equals the `Dual` render line by line except the `vips` and `remote_ts` lines (contracts/rendered-tunnel.md invariants);
  - no `@` placeholder token is left.
- [ ] T009 [P] [US1] In `engines.rs` `mod tests`, add `pcscf_selection_and_pool_follow_the_address_family`. Use a log for `ims0` whose one closed batch is `2402:8100::10, 10.0.1.10, 2402:8100::11, 10.0.0.10`, then `CHILD_SA ims0{1} established`. Using the engine methods through the existing fake runner:
  - `Ipv4`: `latest_pcscf` is `Some("10.0.0.10")`, `latest_pcscf_pool` is `["10.0.1.10", "10.0.0.10"]`, and `ignored_pcscfs` is `["2402:8100::10", "2402:8100::11"]`;
  - `Ipv6`: `latest_pcscf` is `Some("2402:8100::10")`, the pool is the two IPv6 addresses, and `ignored_pcscfs` is the two IPv4 addresses;
  - an `Ipv6` engine over an IPv4-only batch gives `latest_pcscf` `None` and an empty pool;
  - `fresh_pcscf_since` and `fresh_pcscf_pool_since` after a mark apply the same filter.
- [ ] T010 [P] [US1] In `engines.rs` `mod tests`, add the table test `child_rekey_invalid_selectors_session_table` for the new pure function `child_rekey_invalid_selectors_session(charon_log, conn_name) -> Option<String>` (research R7):
  - no rejection gives `None`;
  - `<ims0|3> IKE_SA ims0[3] established ...`, then `<ims0|3> parsed CREATE_CHILD_SA response 12 [ N(INVAL_SEL) ]` gives `Some("ims0[3]")`;
  - a rejection in session `[3]` followed by `IKE_SA ims0[4] established` with no rejection after it gives `None`;
  - an `<ims1|7>` rejection is ignored when asking about `ims0`;
  - rejections in `[3]` and then in `[4]` give `Some("ims0[4]")`;
  - a `CREATE_CHILD_SA response` line **without** `N(INVAL_SEL)` gives `None`.
- [ ] T011 [P] [US1] In `orchestrate.rs` `mod tests`, add a test for a small pure helper that `orchestrate.rs` will own: `fn inval_sel_warning(idx, family, session, last_warned: &mut Option<String>) -> Option<String>`. It returns the contracts/logs.md warning text only when `family == Dual`, `session` is `Some`, and `session != *last_warned`, and then it updates `last_warned`. Check:
  - the same session twice warns once;
  - a new session warns again;
  - `Ipv4` never warns;
  - `None` never warns.

### Implementation for User Story 1

- [ ] T012 [US1] In `orchestrate.rs`, where `SwanctlEpdgParams` is built (around line 1524), pass `address_family: line.config.tunnel_address_family`. Before rendering, print `[supervise] line {idx}: tunnel address family: {family}` (contracts/logs.md, FR-009). At the persistent-line `StrongswanEngine` construction (around line 1801), set `address_family: line.config.tunnel_address_family`. The VoLTE priming engine (around line 1622) stays `Dual`; add a comment saying it captures a VoLTE P-CSCF and has no per-line VoWiFi family. Makes T008 meaningful end to end.
- [ ] T013 [US1] In `engines.rs`, implement the family filter (research R6). Add `fn filtered(&self, batch: Vec<String>) -> Vec<String>`, which keeps the addresses `self.address_family.admits`. Then:
  - `latest_pcscf_pool` returns `filtered(extract_latest_pcscf_pool(..))`;
  - `latest_pcscf` returns `pick_stable_pcscf(&self.latest_pcscf_pool(runner))`. Make `pick_stable_pcscf` `pub(crate)` if needed; it takes a minimum, so deduplicating doesn't change the `Dual` result;
  - `fresh_pcscf_pool_since` filters and returns `None` when the result is empty;
  - `fresh_pcscf_since` returns `pick_stable_pcscf` over `fresh_pcscf_pool_since`;
  - new `pub fn ignored_pcscfs(&self, runner) -> Vec<String>` returns the latest pool's addresses that the family rejects.

  Leave the pure `extract_*` functions and their tests untouched. Makes T009 pass.
- [ ] T014 [US1] In `orchestrate.rs`, replace the direct `super::engines::extract_latest_pcscf_pool(&c, &vowifi_conn_name(idx))` read after `tunnel UP` (around line 1771) with `engine.latest_pcscf_pool(runner.as_ref())`. Move the engine construction above it if needed, or reuse the engine already built for that line. At the four P-CSCF publication points (initial establish, `SteadyOutcome::PcscfChanged`, the post-recovery fresh sync, and `republish_pool_after_set_change` when it republishes), print `[supervise] line {idx}: ignoring P-CSCF {addr} (tunnel_address_family = {family})` for each address in `engine.ignored_pcscfs(runner)`. These points run once per negotiation or pool change, not every tick (FR-013).
- [ ] T015 [US1] In `engines.rs`:
  - implement `child_rekey_invalid_selectors_session`. Walk `lines_for_conn`. On a line containing `IKE_SA {conn}[` followed by `] established`, set the current session to the `{conn}[N]` text and clear the "rejected" flag. On a line containing both `CREATE_CHILD_SA response` and `N(INVAL_SEL)`, set the flag. Return the current session if the flag is set. Doc comment: specs/092 FR-009a, #107, same session scoping as `ike_sa_loss_cause`;
  - add the engine wrapper `pub fn rekey_invalid_selectors_session(&self, runner) -> Option<String>`, which reads `self.shared.charon_log`.

  Makes T010 pass.
- [ ] T016 [US1] In `orchestrate.rs`, implement `inval_sel_warning` (T011). In the persistent line's steady-state loop, keep `let mut warned_inval_sel: Option<String> = None;`. In the `SteadyOutcome::StillUp` branch, call `engine.rekey_invalid_selectors_session(runner.as_ref())` only when `engine.address_family == TunnelAddressFamily::Dual`, and `println!` the helper's message when it returns `Some`. Makes T011 pass. Commit: `feat(092): ipv4/ipv6 tunnel families, P-CSCF family filter, INVALID_SELECTORS hint (US1)`.
- [ ] T017 [US1] In `cli.rs`, add `#[arg(long, default_value = "dual")] address_family: TunnelAddressFamily` to `RenderArgs`' `SwanctlEpdg` variant. Implement `clap::ValueEnum` on the enum, or parse through `FromStr`, whichever matches how `cli.rs` handles other enum args. Pass it through the `RenderAsset::SwanctlEpdg` match in `discover.rs` into `SwanctlEpdgParams`. Test in `gsm-sip-bridge/tests/test_cli.rs`, following its existing render tests if any, else in `discover.rs` tests:
  - `render swanctl-epdg` with no `--address-family` produces the same output as `--address-family dual`;
  - `--address-family ipv4` produces `vips = 0.0.0.0`.

**Checkpoint**: US1 works on its own: an `ipv4` line requests IPv4 only, filters IPv6 P-CSCFs and logs its family; a `dual` line warns on `INVALID_SELECTORS`.

---

## Phase 4: User Story 2: existing deployments are unaffected (P1)

**Goal**: with the key absent or `dual`, every setting line renders, and every P-CSCF selection is made, exactly as before.

**Independent test**: T001's snapshot, plus the parity tests below.

- [ ] T018 [P] [US2] In `config/mod.rs` `mod tests`, add `absent_and_explicit_dual_resolve_identically`: `MINIMAL_TOML` with no key and with `[vowifi] tunnel_address_family = "dual"` produce equal `VowifiConfig.tunnel_address_family`, and a `[[vowifi.line]]` with no key has `tunnel_address_family == None`.
- [ ] T019 [P] [US2] In `discovery.rs` `mod tests`, add `older_resolution_entry_without_the_field_reads_dual`. Take a serialized `LineResolutionEntry` JSON, built by serializing a current entry and deleting the `tunnel_address_family` key from its `config` object, and check that it deserializes with `config.tunnel_address_family == Dual` (`#[serde(default)]`, plan Risks).
- [ ] T020 [P] [US2] In `engines.rs` `mod tests`, add `dual_engine_selection_and_pool_match_the_pure_extractors`. Over the T009 log, a `Dual` engine's `latest_pcscf` equals `extract_latest_pcscf`, `latest_pcscf_pool` equals `extract_latest_pcscf_pool`, and `ignored_pcscfs` is empty. Do the same for the `fresh_*` pair against `extract_pcscf_after` and `extract_pcscf_pool_after`.
- [ ] T021 [US2] Run `make test` and confirm T001's `.snap` file is unmodified (`git diff --exit-code` on the `snapshots/` file). Note the result in the PR description.

**Checkpoint**: the default path is proven unchanged. Commit: `test(092): dual parity for config, resolution file and P-CSCF selection (US2)`.

---

## Phase 5: User Story 3: one line changed, others untouched (P2)

**Goal**: the precedence is line, then `[vowifi]`, then `dual`, and each line renders only its own value.

**Independent test**: two lines with different effective families produce two different rendered connections.

- [ ] T022 [P] [US3] In `config/mod.rs` `mod tests`, add `line_override_beats_the_section_default_both_ways`:
  - `[vowifi]` absent with line 0 `"ipv4"` and line 1 absent gives overrides `[Some(Ipv4), None]` and a section value of `Dual`;
  - `[vowifi] "ipv4"` with line 0 `"dual"` gives a section value of `Ipv4` and overrides `[Some(Dual), None]`.
- [ ] T023 [P] [US3] In `discovery.rs` `mod tests`, add `each_line_resolves_its_own_family`. Resolve two probed modems against a base of `Dual` where only the second has a `[[vowifi.line]]` override of `Ipv4` (matched by `modem_serial`). Resolved line 0 should be `Dual` and line 1 `Ipv4`. Then flip it: base `Ipv4`, override `Dual`.
- [ ] T024 [US3] In `render.rs` `mod tests`, add `two_lines_render_independently`. Render the real template for `conn_name "ims0"` with `Dual` and for `"ims1"` with `Ipv4`. `ims0`'s output contains `vips = 0.0.0.0, ::` and `ims1`'s contains `vips = 0.0.0.0\n`, with each connection name appearing only in its own output (FR-006).

**Checkpoint**: commit `test(092): per-line precedence and independent renders (US3)`.

---

## Phase 6: User Story 4: a typo fails loudly (P2)

**Goal**: an unknown value, or a value other than `dual` under swu, stops the bridge at config load with an error naming the key path, the value and the accepted values (FR-007, FR-008, contracts/config.md).

**Independent test**: each bad config is rejected with the expected message fragments.

### Tests for User Story 4 (write first)

- [ ] T025 [P] [US4] In `config/mod.rs` `mod tests`, add `unknown_tunnel_address_family_is_a_config_error`. Loop over the values `"ip4"`, `"IPv4"` and `"both"`, at `[vowifi]` and on `[[vowifi.line]]` index 1 (two line entries). Each must give an error containing:
  - the key path, `vowifi.tunnel_address_family` or `vowifi.line[1].tunnel_address_family`;
  - the offending value, quoted;
  - `"dual"`, `"ipv4"` and `"ipv6"`.

  Same style as the existing `reg_rejected_cooldown_sec` loop (around `config/mod.rs:3124`).
- [ ] T026 [P] [US4] In `config/mod.rs` `mod tests`, add `swu_refuses_a_non_dual_family`. With `[vowifi] tunnel_engine = "swu"`:
  - `[vowifi] tunnel_address_family = "ipv4"` is an error containing `vowifi.tunnel_address_family` and `strongswan`;
  - a line 0 value of `"ipv6"` is an error containing `vowifi.line[0]` and `strongswan`;
  - a `"dual"` value at either level, or no value, loads fine.

### Implementation for User Story 4

- [ ] T027 [US4] In `build.rs`, finish the error text to match contracts/config.md. Then, after `tunnel_engine` is validated (around line 794), add the swu check. If `tunnel_engine == "swu"`, a section value other than `Dual` is an error. The first `line_overrides[i]` with `Some(f)` where `f != Dual` is an error with `vowifi.line[{i}].tunnel_address_family = "{f}" requires tunnel_engine = "strongswan" (the swu engine always negotiates both families); remove it or set "dual"`. The section error has the same text with the `vowifi.tunnel_address_family` key path. Makes T025 and T026 pass. Commit: `feat(092): reject unknown or swu-incompatible tunnel_address_family (US4)`.

**Checkpoint**: all four stories are covered and `make test` is green.

---

## Phase 7: Polish and cross-cutting

- [ ] T028 [P] In `config.toml.example`, document `tunnel_address_family` in `[vowifi]`, near `tunnel_engine`, commented out with its default, `# tunnel_address_family = "dual"`. Cover every point in contracts/config.md's documentation obligations:
  - what each value requests;
  - the symptom: charon log `CREATE_CHILD_SA` responses carrying `N(INVAL_SEL)`, about a minute without service roughly every hour, and supervisor `ChildSaMissing`/`ChildSaStuck` recoveries;
  - that the bridge also warns about it on a `dual` line;
  - the remedy, `"ipv4"`;
  - going back to `dual` once the carrier assigns IPv6;
  - the risk of `"ipv6"` on an IPv4-only carrier;
  - strongswan engine only;
  - takes effect on container recreate.

  Add `# tunnel_address_family = "ipv4"   # this line only; see [vowifi]` to the first commented `[[vowifi.line]]` example. Reference #107/#126. Use no real identifiers.
- [ ] T029 [P] In `docs/configuration.md`, add a `tunnel_address_family` row (enum, default `dual`) to the `[vowifi]` table with the same substance as T028, and a row to the `[[vowifi.line]]` table: "inherits `[vowifi].tunnel_address_family`". `tests/test_config_docs.rs` must pass.
- [ ] T030 [P] In `RELEASE_NOTES.md`, add an entry under `## Unreleased`. Only the summary is bold (memory: release-note style). For example: `- **Per-line tunnel address family for ePDGs that reject rekeys** -- ...`, then plain text saying:
  - the new `tunnel_address_family` key (`dual` default, `ipv4`, `ipv6`) under `[vowifi]` and `[[vowifi.line]]`;
  - opt-in, and the default negotiates exactly as before;
  - `ipv4` fixes the hourly CHILD_SA rekey rejection with `INVALID_SELECTORS` reported in #107;
  - a `dual` line now logs a hint when that happens;
  - P-CSCFs of an excluded family are ignored;
  - swu refuses non-`dual` values;
  - ends with `(#126; #107)`.

  Add a matching `CHANGELOG.md` entry if that file's `Unreleased` section mirrors release notes.
- [ ] T031 Run quickstart.md §1 locally: all three `render swanctl-epdg` invocations print the expected `vips` and `remote_ts`. Run `make format && make lint && make test` with a private `CARGO_TARGET_DIR`. Commit the docs: `docs(092): document tunnel_address_family and the INVALID_SELECTORS symptom`.
- [ ] T032 Live gate, local Vodafone rig (quickstart.md §2–3), using the `test/` deployment with a freshly built image, and recreating the container on every config change:
  - (a) key unset: the line logs `tunnel address family: dual` and registers, and the setting lines of `epdg-0.conf` match the previous image's;
  - (b) `ipv4`: the IKE_AUTH `CPRQ` has no `ADDR6` or `P-CSCF6` (confirms research R3 on the pinned fork), all P-CSCFs are IPv4, `vowifi-status` shows an IPv4-only pool, and inbound and outbound calls connect;
  - (c) wait through one natural CHILD_SA rekey with no `ChildSaMissing`;
  - (d) back to `dual`.

  Record the results under a `## Live gate` heading at the end of this file, with no real identifiers.
- [ ] T033 Push the branch and open the PR (title `feat: per-line tunnel address family for the ePDG tunnel (#126)`, body linking #126 and #107, with the T021 snapshot note and the T032 results). Ask the #107 reporter on the issue to run quickstart.md §4: confirm the `dual` warning fires, then set `ipv4` and observe at least 2 consecutive natural rekeys with no `N(INVAL_SEL)` and no `ChildSaMissing`/`ChildSaStuck` (SC-003, SC-004). Record their result here when it arrives. Do not reply to Greptile bot comments; fix the code instead (memory).

---

## Dependencies & execution order

- **Phase 1 (T001)** comes before everything. The snapshot must exist before the template changes.
- **Phase 2 (T002 → T003 → T004 → T005; T006 and T007 after T002)** blocks every story.
- **US1 (Phase 3)** depends on Phase 2. Within it, tests T008–T011 come first; then T012 (after T006, T007), T013 (after T009), T014 (after T013), T015 (after T010), T016 (after T011, T015), and T017 (after T006).
- **US2 (Phase 4)** depends on Phase 2. T020 needs T013, the engine methods.
- **US3 (Phase 5)** depends on Phase 2 only.
- **US4 (Phase 6)** depends on T004.
- **Polish (Phase 7)** comes after all stories. T032 needs an image built from the finished branch, and T033 comes after T032.

### Parallel opportunities

- T008, T009, T010 and T011 touch different test modules and can be written together.
- T018, T019 and T020, US2's tests, are in three different files.
- T022 and T023 are in `config/mod.rs` and `discovery.rs`.
- T025 and T026 are both in `config/mod.rs`, so write them sequentially even though they're marked [P] relative to other stories.
- T028, T029 and T030 are three different docs.
- After Phase 2, US3 and US4 can proceed alongside US1.

### Parallel example: User Story 1

```text
T008 render.rs tests      | T009 engines.rs filter tests
T010 engines.rs detector  | T011 orchestrate.rs warning helper test
```

T009 and T010 are both in `engines.rs` `mod tests`, so if two people work on them, add them in separate hunks.

## Implementation strategy

1. **MVP = Phases 1–3**: the snapshot, the config, and US1. That alone fixes #107 for an operator who sets `ipv4`.
2. Then US2: parity proof for the default path. It's cheap, and it's the regression shield for Vodafone, Jio and Airtel.
3. Then US3 and US4, the precedence and validation guarantees.
4. Polish: docs, release note, the live gate on the Vodafone rig, then the PR and the reporter's validation on the affected ePDG.

Suggested commits, in order:
1. T001
2. Phase 2
3. US1 (T012–T016), then T017
4. US2
5. US3
6. US4
7. docs
8. live-gate notes
