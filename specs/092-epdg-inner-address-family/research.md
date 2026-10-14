# Research: Per-line inner address family for the ePDG tunnel

Feature: `specs/092-epdg-inner-address-family/spec.md`. Every unknown in the plan's Technical Context is resolved below.

## R1. How the family reaches charon: two template placeholders

**Decision**: Two new placeholders in `docker/strongswan/swanctl-epdg.conf.template`:

- `vips = @VIPS@`
- `remote_ts = @REMOTE_TS@`

`render_swanctl_epdg` substitutes them from a new `SwanctlEpdgParams::address_family` field:

| Family | `@VIPS@` | `@REMOTE_TS@` |
|---|---|---|
| `dual` | `0.0.0.0, ::` | `0.0.0.0/0, ::/0` |
| `ipv4` | `0.0.0.0` | `0.0.0.0/0` |
| `ipv6` | `::` | `::/0` |

**Rationale**: placeholders are how every other per-line value reaches the template (`@CONN_NAME@`, `@IF_ID@`, `@SRC_ADDR@`). The template stays the single source of the connection's shape.

**Alternatives considered**:
- *Leave the template alone and string-replace the literal `vips = 0.0.0.0, ::` line in the renderer.* This would keep the template byte-identical, but it couples the renderer to a value written in the template. If someone edits that line, the replace silently does nothing and the setting stops working. Rejected.
- *A second template per family.* Three copies of a connection block that has to stay in sync. Rejected.

## R2. What "unchanged for `dual`" means: identical settings, not identical comments

**Finding**: The template's comments are copied into the rendered file; only placeholders are substituted. Two comment changes are unavoidable:

- the header has to document the two new placeholders, as it documents every other one;
- the comment above `vips` says "Request both an IPv4 and an IPv6 inner address", which is wrong for `ipv4` and `ipv6`.

So even for `dual`, the rendered file cannot be byte-identical to the current release.

**Decision**: "Unchanged" means every non-comment line of the rendered file is byte-identical to the current release's. Comment lines may change. A comment line is one whose first non-whitespace character is `#`. charon parses only the setting lines, so negotiation is unaffected. Spec FR-003 and SC-001 are reworded to match. The snapshot test (R7) compares the rendered output with comment lines removed.

**Rationale**: the guarantee the issue cares about is that Vodafone, Jio and Airtel negotiate exactly as before, and that depends only on the setting lines. Keeping comments frozen would mean leaving a misleading comment in place.

**Alternatives considered**: keeping every comment frozen so the file is fully byte-identical. That leaves the `vips` comment wrong and the new placeholders undocumented. Rejected.

## R3. charon requests P-CSCFs only for the families it asks an address for

**Finding**: In upstream strongSwan 5.9.3, `src/libcharon/plugins/p_cscf/p_cscf_handler.c` (lines 147–148) sets:

- `request_ipv4 = has_host_family(vips, AF_INET)`
- `request_ipv6 = has_host_family(vips, AF_INET6)`

So the P-CSCF4 and P-CSCF6 attributes in the CPRQ follow the `vips` families. The pinned fork, Osmocom `strongswan-epdg` at `022f3e5`, is based on 5.9.3. Its gitea server answers scripted fetches with a bot challenge, so its source was not read directly.

**Decision**: Plan on `ipv4` lines receiving only IPv4 P-CSCFs, but do not depend on it: FR-013's filter (R5) guards the case where they don't. Quickstart step 3 confirms the CPRQ contents from a container charon log on an `ipv4` line.

**Rationale**: the filter costs a few lines, and it keeps correctness from depending on fork behaviour nobody has read.

## R4. Config shape and validation

**Decision**:
- `RawVowifi.tunnel_address_family: String`, defaulting to `"dual"`.
- `RawVowifiLine.tunnel_address_family: Option<String>`.
- `config/build.rs` parses both into a `TunnelAddressFamily { Dual, Ipv4, Ipv6 }` enum, with `FromStr` and `Display` (`dual` / `ipv4` / `ipv6`).
- An unknown value returns `BridgeError::Config`. The message names `vowifi.tunnel_address_family` or `vowifi.line[{i}].tunnel_address_family`, the value, and the accepted values. This follows the pattern of `register_request_uri` and `originating_headers`.
- `VowifiConfig.tunnel_address_family` has `#[serde(default)]`, so a line-resolution file written by an older build still deserializes as `Dual`.
- `VowifiLineOverride.tunnel_address_family: Option<TunnelAddressFamily>`.
- `vowifi::discovery::resolve_one_line` applies the override exactly as it applies `reg_rejected_cooldown_sec` (`discovery.rs:429`). Each `LineResolutionEntry.config` therefore carries the effective value.

**Rationale**: this is the existing per-line override pattern (spec Assumption). The value travels in `config: VowifiConfig`, which the supervisor already reads per line, so no new field is threaded through the supervisor.

## R5. The swu check runs in `config/build.rs`

**Decision**: After both levels and `tunnel_engine` are parsed in `config/build.rs`, a value other than `Dual` in `[vowifi]` or in any line is a `BridgeError::Config` when `tunnel_engine == "swu"`. The error names where the value came from: `vowifi.tunnel_address_family` or `vowifi.line[{i}]`.

**Rationale**: FR-008 requires the error to say whether the value came from `[vowifi]` or a specific line. That distinction exists only before `resolve_one_line` merges the levels. Checking at config load also fails before any process is spawned, the same intent as `check_pcsc_engine_compatibility`.

**Alternatives considered**: a supervisor-level check next to `check_pcsc_engine_compatibility`. By that point the levels are merged, so the error could not name its source. Rejected.

## R6. Keeping P-CSCFs of the other family out of selection and the pool (FR-013)

**Decision**:
- `StrongswanEngine` gains `address_family: TunnelAddressFamily`. The VoLTE priming engine (`orchestrate.rs` near line 1622) passes `Dual`.
- The engine filters each extracted batch through `TunnelAddressFamily::admits(addr)` in `latest_pcscf`, `latest_pcscf_pool`, `fresh_pcscf_since` and `fresh_pcscf_pool_since`.
- The single selection is `pick_stable_pcscf` over the filtered, deduplicated pool. `pick_stable_pcscf` takes a minimum, so deduplicating doesn't change its result, and for `Dual` the result is unchanged.
- The direct `extract_latest_pcscf_pool` call in `orchestrate.rs` (near line 1771) moves to the engine method.
- A new engine method, `ignored_pcscfs`, returns the advertised addresses the filter dropped. It is logged at the existing publication points: establish, `PcscfChanged`, post-recovery sync, and a set-change republish. Those happen once per negotiation or pool change, which gives FR-013's "logged once per negotiation".
- The pure `extract_*` functions keep their signatures and `dual` semantics, so their existing tests don't change.

**Rationale**: filtering at the source is the only point that covers both the supervisor's single selection and spec 091's published pool. The agent and `pcscf_pool.rs` stay untouched.

**Empty result**: an `ipv6` line on an ePDG that sends only IPv4 P-CSCFs ends up with none. The establish loop then behaves as it already does for a tunnel with no P-CSCF. The spec's Edge Cases accept this: the operator chose `ipv6`, and the config docs warn about it.

## R7. Detecting `INVALID_SELECTORS` (FR-009a)

**Decision**: A new pure function, `child_rekey_invalid_selectors_session(charon_log, conn_name) -> Option<String>`, in `supervise/engines.rs`:

- It walks `lines_for_conn` and tracks the current IKE_SA session from the most recent `IKE_SA <conn>[N] established`, the same scoping as `ike_sa_loss_cause`.
- It returns that session's tag (`"<conn>[N]"`) if any later line contains both `CREATE_CHILD_SA response` and `N(INVAL_SEL)`.
- The engine wraps it as `rekey_invalid_selectors_session`.
- The steady-state loop calls it on each `StillUp` tick, for lines whose family is `Dual` only. It warns when the tag differs from the last one it warned about.

So there is one warning per tunnel session, and a rebuilt session that is rejected again warns again (FR-009a).

**Log evidence**: #107's captures show `parsed CREATE_CHILD_SA response <n> [ N(INVAL_SEL) ]` under the connection's `<ims0|N>` prefix. charon logs the parsed message summary at level 1, which our `filelog default = 1` includes (the same assumption as `ike_sa_loss_cause`, research R2 of spec 086).

**Warning text** (contract in `contracts/logs.md`):

`[supervise] line {idx}: ePDG rejected the CHILD_SA rekey's traffic selectors (INVALID_SELECTORS); if this repeats, set tunnel_address_family = "ipv4" for this line (see config.toml.example)`

**Alternatives considered**: matching `received INVALID_SELECTORS notify`. Its wording varies with the version and the notify type, and the parsed-message summary is what #107 actually captured. Rejected.

## R8. Logging the effective family (FR-009)

**Decision**: one line when each strongswan-engine line renders its connection (`orchestrate.rs`, where `SwanctlEpdgParams` is built):

`[supervise] line {idx}: tunnel address family: {family}`

## R9. The `render swanctl-epdg` CLI and `discover`

**Decision**: `cli.rs`'s `SwanctlEpdg` render asset gains `--address-family <dual|ipv4|ipv6>`, defaulting to `dual`, and passes it through `commands/discover.rs`. Nothing in `docker/` calls it today (checked: `entrypoint.sh` no longer renders), but it is a public subcommand and has to keep producing the same output without the flag.

## R10. Tests

**Decision**:
1. **Snapshot first.** Before the template changes, add an `insta` snapshot of the real template (`include_str!("../../../docker/strongswan/swanctl-epdg.conf.template")`) rendered with fixed parameters and comment lines removed. Commit it. Changing the template and adding `Dual` must leave that snapshot unchanged. This is SC-001.
2. `ipv4` and `ipv6` assert the exact `vips` and `remote_ts` lines, and that every other non-comment line equals the `dual` render.
3. Config tests: the three values at each level, inheritance in both directions (User Story 3), unknown values at each level, the swu refusal at each level, and `dual` accepted under swu.
4. Engine tests: family filtering of the selection and pool in a mixed-family log, `dual` unchanged, `ignored_pcscfs`, and an empty result for `ipv6` on an IPv4-only batch.
5. Detector tests: no rejection, a rejection in the current session, a rejection only in an earlier session (returns `None`), another connection's rejection (ignored), and two sessions giving two tags.
6. `test_config_docs.rs` already enforces that the new key appears in `config.toml.example` and `docs/configuration.md`.

These are integration-style tests over real inputs: the real template file and real charon-log text. No mocks are needed (constitution, Principle I).
