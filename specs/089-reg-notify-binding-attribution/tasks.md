---
description: "Task list for specs/089-reg-notify-binding-attribution"
---

# Tasks: Reg-event NOTIFY binding attribution and per-event deregistration policy

**Input**: `specs/089-reg-notify-binding-attribution/`: plan.md, spec.md, research.md (R1–R10), data-model.md, contracts/observability.md, contracts/config.md, quickstart.md
**Tests**: Included. The constitution makes TDD the default, and SC-002 to SC-005 are test-suite criteria. Write each test first and watch it fail.

**Every commit** (CLAUDE.md, constitution II): run `make format && make lint && make test` first, and commit only if all three pass. Never commit real MSISDNs, IMEIs or IPs. Use `+919000000000`, IMEI `860000000000000`, contacts `10.0.0.1:5064` (old) and `10.0.0.1:5070` (current), and IPv6 `[2001:db8::1]:5070`.

All paths are relative to the repo root. `session.rs` means `gsm-sip-bridge/src/ims/session.rs`, and `agent/mod.rs` means `gsm-sip-bridge/src/ims/agent/mod.rs`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- Stories:
  - **[US1]**: a renewal no longer loops (FR-001 to FR-005).
  - **[US2]**: per-event policy (FR-006 to FR-014, FR-017).
  - **[US3]**: identity vs binding classification (FR-015).
  - FR-016 (the NOTIFY log line) spans all three; it is first built in US1 and extended in US2 and US3.

---

## Phase 1: Setup

- [X] T001 Confirm the baseline on branch `089-reg-notify-binding-attribution` is green: run `make lint && make test` from the repo root. Stop if anything fails before any code is touched

---

## Phase 2: Foundational (blocks every story)

**Purpose**: richer `<contact>` parsing, the event type, and a pure classification function. Behaviour is unchanged at the end of this phase.

- [X] T002 Write failing tests in `session.rs`'s test module for the extended `ReginfoContact` (data-model.md):
  - `addr` is `Some(10.0.0.1:5070)` for `<uri>sip:+919000000000@10.0.0.1:5070;transport=tcp</uri>`.
  - `addr` is `Some([2001:db8::1]:5070)` for `<uri>sip:+919000000000@[2001:db8::1]:5070</uri>`.
  - `addr` is `None` when `<uri>` is missing or names a hostname.
  - `retry_after` is `Some(600)` from `retry-after="600"`.
  - `registration_state` is the enclosing `<registration state="…">`, and `None` for a bare `<contact>`.
  - A contact in the second `<registration>` reads that registration's state, not the first's.
- [X] T003 Implement those fields in `session.rs`:
  - `parse_contact_attrs` fills `addr` (via `contact_uri_addr`) and `retry_after`.
  - `contact_blocks` also returns each block's enclosing `<registration ...>` open tag: the nearest preceding `<registration` open tag that has no `</registration>` between it and the contact.
  - Doc comments cite research R2 and R6.
- [X] T004 Write failing tests for a new `DeregEvent` (`Unregistered < Probation < Deactivated < Rejected`, derive `Ord`):
  - `DeregEvent::from_contact` parses case-insensitively and only when `state="terminated"`.
  - `expired`, `registered` and an `active` state return `None`.
  - `max()` over `[Unregistered, Rejected, Deactivated]` is `Rejected` (FR-014).
- [X] T005 Implement `DeregEvent` in `session.rs`. `contact_reports_deregistration` becomes `DeregEvent::from_contact(c).is_some()`, so the existing RFC 3680 test still passes
- [X] T006 Write failing tests for a pure `classify_reg_notify(body, subscription_state, imei, contact_addr) -> NotifyVerdict` (research R3). With today's attribution rules still in place:
  - It returns the same `own` as `find_own_contacts` for every existing fixture.
  - `deregistration.event` is the max event among `own.entries`.
  - `retry_after` is the max of the entries' `retry-after` and `Subscription-State: terminated;reason=probation;retry-after=900`.
  - There is no deregistration when every own entry is `active` or `expired`.
- [X] T007 Implement `NotifyVerdict`, `Deregistration { event, retry_after, scope }` and `DeregScope` in `session.rs`. Set `scope = BindingRemoved` for now; US3 fills it in. Then:
  - `handle_notify` sends the `200 OK`, calls `classify_reg_notify`, keeps today's log lines, and returns `Option<Deregistration>`.
  - In `agent/mod.rs`, `handle_reg_notify` treats `Some(_)` exactly as it treated `true`.
  - Run the full check and commit: `refactor(ims): classify reg-event NOTIFYs into a typed deregistration`.

**Checkpoint**: all green, no behaviour change.

---

## Phase 3: User Story 1: a renewal no longer loops (P1) 🎯 MVP

**Goal**: a NOTIFY that retires a replaced binding is recognised and ignored (FR-001 to FR-005, SC-001, SC-002, SC-005).

**Independent test**: the #124 fixture produces no deregistration and one retired entry, and every #117 fixture is unchanged.

- [ ] T008 [US1] Write failing tests in `session.rs`, one per row of the research R1 table:
  - **Scenario 1 (#124 fixture):** current `10.0.0.1:5070`, body with `<contact>` `sip:+919000000000@10.0.0.1:5064` `terminated/rejected` that echoes IMEI `860000000000000`. Expect `own.entries` empty, `own.retired.len() == 1`, `deregistration == None`, `matched_by == Attribution::None`.
  - **Scenario 2:** a full-state body with old `:5064` `terminated/unregistered` and new `:5070` `active/registered`, both IMEI-echoing. Expect one current entry (`active`), one retired, no deregistration, `matched_by == ContactAddr`.
  - **Scenario 3:** `:5070` `terminated/deactivated` (IMEI echoed). Expect `Deactivated` with `matched_by == ContactAddr`.
  - **Scenario 4 (#117):** both AOR entries name `:5070`. Expect both current.
  - An IMEI-echoing entry with no `<uri>`, alongside a foreign contact, is current with `matched_by == Imei`.
  - A foreign address without the IMEI is neither current nor retired.
  - IPv6 current address match; a `;Hpt=…` suffix is ignored.
- [ ] T009 [US1] Rewrite `find_own_contacts` in `session.rs` to the research R1 rules:
  - Add `OwnContacts.retired`.
  - The label prefers `ContactAddr` when any current entry matched by address.
  - Keep the sole-contact fallback exactly as it is (FR-003).
  - Rewrite the doc comment: the old "either identifier is enough" reasoning is now the #124 bug, so cite TS 24.229 §5.1.1.4.1 NOTE 10 and §5.4.2.1.2 NOTE 2.
  - Review every existing `find_own_contacts` test. Any test that asserted IMEI attribution of an entry *at a different address* encoded the bug: change its expectation and say why in the test's comment. Every #117 case must pass unchanged (SC-005).
- [ ] T010 [US1] Update the logging in `handle_notify` in `session.rs` to match `contracts/observability.md`:
  - The single per-NOTIFY line gets `attribution`, `contact_addr`, `own_entries`, `retired_entries`, `contact_states`, `applied`, `subscription_state` and `body`.
  - Add one `INFO` "replaced binding retired" line per terminated retired entry, with `retired_addr`, `state`, `event` and `contact_addr` (FR-005).
  - Remove the old "terminated state, but not attributably" `WARN` for the retired case, which is now explained.
- [ ] T011 [US1] Add a test in `agent/mod.rs` for SC-002. Feed the #124 verdict (no deregistration) through the path `handle_reg_notify` takes, at `last_registered + 1s`. Assert `force_renewal == false`, `next_renewal_attempt == None`, and `consecutive_renewal_failures == 0`. If `handle_reg_notify` is hard to drive directly, extract the post-classification body into a `LoopState` method that takes `Option<Deregistration>` and test that. Then run the full check and commit: `fix(ims): attribute reg-event contacts by address, ignore retired bindings (#124)`.

**Checkpoint**: the #124 loop is closed. This is shippable on its own as the MVP.

---

## Phase 4: User Story 2: each event gets the standard's response (P1)

**Goal**: the TS 24.229 §5.1.1.7 policy for each event, plus the rejection cooldown (FR-006 to FR-014, FR-017, SC-003, SC-004).

**Independent test**: drive `LoopState` with each event using an injected clock.

### Policy core

- [ ] T012 [US2] Write failing tests in `agent/mod.rs`'s test module, next to `a_binding_dropped_right_after_registering_retries_on_backoff`. The deregistration handler now takes `(now, busy, &Deregistration, cooldown)`.
  - **Deactivated (scenarios 1 and 2):**
    - On a mature binding, `next_renewal_attempt == None` and the attempt is uncounted.
    - On an early loss with a clean ladder, `next_renewal_attempt == None` but `consecutive_renewal_failures == 1`.
    - After `on_renewal_success`, a second early deactivated waits 10s and the count reaches 2.
    - Six in a row return `Err` (the existing exit budget still applies).
  - **Unregistered (scenario 3):** on a mature binding, a retry is scheduled at +5s and counted once.
  - **Probation with retry-after 600 (scenario 4):** `next_renewal_attempt >= now + 600s`.
  - **Rejected (scenario 5, SC-003):**
    - `rejection.until == now + cooldown` and `next_renewal_attempt == Some(until)`.
    - `consecutive_renewal_failures` is unchanged and the ladder is reset to `(5s, 0)`.
    - `force_renewal == true`.
  - **Rejected with retry-after 3600 > 1800 cooldown:** the larger value wins.
  - **Rejected arriving while a ladder retry is pending:** the cooldown replaces it.
  - **SC-004:** loop 48 × (rejected → cooldown elapses → `on_renewal_success` → rejected), about 24h with a 30-minute cooldown. It never returns `Err` and `consecutive_renewal_failures` stays 0.
  - **FR-011:** a failure of the post-cooldown attempt (`schedule_renewal_retry`) lands on a 5s backoff with count 1.
  - `on_renewal_success` clears `rejection`.
- [ ] T013 [US2] Implement the policy in `agent/mod.rs` per research R4:
  - Add `RejectionCooldown { until: Instant, until_wall: SystemTime }` and `LoopState.rejection`.
  - Branch `on_network_deregistration` on `DeregEvent`.
  - Apply the retry-after floor for Unregistered and Probation.
  - `on_renewal_success` clears `rejection`.
  - Rewrite the method's doc comment to describe the per-event table and to cite TS 24.229 §5.1.1.7 and §5.4.1.5. Also remove `handle_notify`'s old "always retries … rejected handled the same" paragraph in `session.rs`.
  - Keep the never-`Err`-while-busy rule for every counted path.
- [ ] T014 [US2] In `handle_reg_notify` in `agent/mod.rs`:
  - Set `last_failure` to a reason that names the event. For rejected, use "the network rejected our registration (reg-event rejected); next attempt at <RFC 3339>".
  - Write `status.rejected_until` (T020 adds the field; do T020 first if working strictly in order).
  - Log the policy decisions from `contracts/observability.md`.
  - When the cooldown gate opens in `on_idle_tick` and `rejection` is set, log "rejection cooldown over; trying to register once" once before the attempt.
  - Run the full check and commit: `feat(ims): apply TS 24.229 §5.1.1.7 per reg-event deregistration event`.

### Ending calls on `rejected` (FR-009, scenario 6)

- [ ] T015 [P] [US2] Add `pub const REGISTRATION_REJECTED: &str = "registration_rejected";` to the `reason` module in `gsm-sip-bridge/src/vowifi/control.rs`, with a doc comment
- [ ] T016 [US2] In `gsm-sip-bridge/src/ims/agent/origination.rs`, add `pub(super) fn fail_origination(p: &mut PendingOrigination, session: &mut RegisteredSession, reason: &str)`. It runs `p.fail(reason)` followed by `abandon_origination`'s per-step teardown (research R5). Test it in the existing origination test module:
  - An `AwaitingCarrier` attempt sends `CallFailed` with that reason and moves to `AwaitingCancel`.
  - An `AwaitingVeth` attempt sends `CallFailed` and a BYE.
- [ ] T017 [US2] Change `handle_reg_notify` in `agent/mod.rs` to take `&mut RegisteredSession` and update its call site (around line 2048). On `Rejected`:
  - With an active call, end it using the `handle_attachment_loss` sequence with `reason::REGISTRATION_REJECTED`, and add an `EndedBy` variant if the lifecycle needs a distinct cause.
  - With a pending origination, call `fail_origination`.
  - Then apply the cooldown with `busy == false`.
  - Test: a `LoopState` with `test_active_call(..)` receives a Rejected verdict; afterwards `active_call` is `None` and `rejection` is set.
  - Commit: `feat(ims): end the call when the network rejects our registration`.

### Configuration (FR-010)

- [ ] T018 [P] [US2] Write failing config tests in the existing config build test module (`gsm-sip-bridge/src/config/`):
  - The default is 1800.
  - `[vowifi].reg_rejected_cooldown_sec = 600` is used, and a `[[vowifi.line]]` override wins.
  - `[volte]` absent inherits `[vowifi]`, and a `[[volte.line]]` override wins.
  - 59 and 86401 are rejected with an error naming the key.
  - An unknown-key typo is still rejected (strict parsing).
- [ ] T019 [US2] Implement the key per `contracts/config.md`:
  - Raw fields in `gsm-sip-bridge/src/config/raw.rs` (`RawVowifi`, `RawVowifiLine`, `RawVolte` as `Option<u64>`, `RawVolteLine`).
  - Validation and inheritance in `gsm-sip-bridge/src/config/build.rs`, and resolved fields on `VowifiConfig`, `VolteConfig` and `VolteLineOverride` in `gsm-sip-bridge/src/config/mod.rs`.
  - Thread it to `DispatchParams.reg_rejected_cooldown`: through `InboundParams` in `agent/mod.rs` from the VoWiFi per-line config, and from `gsm-sip-bridge/src/volte/carrier_agent.rs` using the line override or `[volte]`.
  - Commit: `feat(config): reg_rejected_cooldown_sec for the reg-event rejected cooldown`.

### Status (FR-017)

- [ ] T020 [P] [US2] Add `rejected_until: Option<SystemTime>` to `RegistrationStatus` in `gsm-sip-bridge/src/ims/mod.rs`. It is in-memory only, like `gm_connection`; update the field docs and `Default`. Add `#[serde(default)] rejection_cooldown_until: Option<u64>` to `RegistrationStatusReply` in `gsm-sip-bridge/src/vowifi/control.rs`. Tests:
  - It round-trips.
  - An older reply without it parses as `None`; extend `registration_status_reply_from_an_older_peer_omitting_gm_connection_parses` or add a sibling test.
- [ ] T021 [US2] Fill the reply in the `StatusQuery` arm of `agent/mod.rs` (around line 1004). Print `rejection_cooldown: none | until <rfc3339> (next attempt in <N>s)` in both:
  - `vowifi-status` (`gsm-sip-bridge/src/vowifi/mod.rs`, after `gm_connection`);
  - `volte-status` (`print_line_live_status` in `gsm-sip-bridge/src/volte/bridge.rs`).

  Factor the formatting into one helper next to `format_expires_in` and unit-test it. Commit: `feat(status): show the reg-event rejection cooldown`.

**Checkpoint**: Story 2's acceptance scenarios 1 to 8 are each covered by a named test.

---

## Phase 5: User Story 3: identity vs binding deregistration (P2)

**Goal**: FR-015. The log distinguishes the two shapes, and the policy is the same for both.

- [ ] T022 [US3] Write failing tests in `session.rs`:
  - `<registration state="terminated">` whose current contact is `terminated/deactivated` gives `scope == IdentityDeregistered` and `event == Deactivated` (Story 3, scenario 1).
  - `<registration state="active">` with the current contact `terminated/rejected` alongside another device's `active` contact gives `scope == BindingRemoved` and `event == Rejected` (scenario 2).
  - The #117 two-AOR body (both registrations terminated) gives `IdentityDeregistered`.
  - The bare fixtures without `<registration>` give `BindingRemoved`.
- [ ] T023 [US3] Compute `scope` in `classify_reg_notify` per research R6 and add `scope` to the per-NOTIFY log line when an event is applied. Commit: `feat(ims): tell identity deregistration from a removed binding in the reg-event log`

---

## Phase 6: Polish and cross-cutting

- [ ] T024 [P] Document `reg_rejected_cooldown_sec` (commented out at its default) under `[vowifi]`, `[[vowifi.line]]`, `[volte]` and `[[volte.line]]` in `config.toml.example` and `docs/configuration.md`
- [ ] T025 [P] Add a short reg-event NOTIFY section to `docs/observability.md` (it has none today) with the new fields and messages from `contracts/observability.md`
- [ ] T026 Add the unreleased entries:
  - **RELEASE_NOTES.md:** bold only the summary phrase, with a plain description after it. One entry for the #124 loop fix and one for the per-event policy and cooldown key.
  - **CHANGELOG.md:** following the existing format.
  - Commit: `docs: release notes and config docs for reg-event attribution (#124)`.
- [ ] T027 Final gate: `make format && make lint && make test` (quickstart Q0). Grep the diff for anything that looks like a real MSISDN, IMEI or IP
- [ ] T028 Live gate Q2 (blocking): on the local Vodafone VoWiFi rig, confirm `attribution=contact-addr own_entries=2` on the initial registration's NOTIFY and after one renewal. If it does not match, stop and revisit research R1 before merging
- [ ] T029 Live gate Q1 (blocking for SC-001): on a carrier that retires replaced bindings (Jio Pi, or the #124 reporter's build), run ≥ 3 h. The `replaced binding retired` count should be close to the renewal count, with zero `our current binding was deregistered` lines and renewals at the normal cadence only
- [ ] T030 Live check Q3: `vowifi-status` / `volte-status` print `rejection_cooldown: none`

---

## Dependencies and execution order

- Setup (T001) → Foundational (T002 to T007) → US1 (T008 to T011) → US2 → US3 → Polish.
- US1 needs only the foundational phase, and it is the MVP that fixes #124 by itself.
- Within US2:
  - T012 → T013 → T014 (T014 also needs T020).
  - T015 → T016 → T017.
  - T018 → T019.
  - T020 → T021.
  - T015, T018 and T020 are in different files and can start in parallel once T013 lands.
- US3 needs only the foundational phase, so it could run alongside US2, but it touches `session.rs` like US1. Run it after US1 to avoid conflicts.
- T028 and T029 need a deployed build of the whole branch.

## Implementation strategy

1. Ship order inside the PR: the foundational phase, then US1, gives a reviewable commit that closes the loop on its own.
2. US2 follows in focused commits: policy, call teardown, config, status.
3. Keep `MAX_CONSECUTIVE_RENEWAL_FAILURES`, `REGISTRATION_SETTLE` and the backoff constants unchanged.
4. Do not start spec 090 (SUBSCRIBE tracking) or 091 (P-CSCF failover) work here. Both branches are stacked on this one and need rebasing after this branch changes.
