---
description: "Task list for specs/090-reg-event-subscription-tracking"
---

# Tasks: Reg-event SUBSCRIBE transaction and dialog tracking

**Input**: `specs/090-reg-event-subscription-tracking/`: plan.md, spec.md, research.md (R1–R13), data-model.md, contracts/observability.md, contracts/control.md, quickstart.md
**Tests**: Included. The constitution makes TDD the default, and every acceptance scenario is driven by a clock. Write each test first and watch it fail.

**Every commit** (CLAUDE.md, constitution II): run `make format && make lint && make test` first, and commit only if all three pass. Never commit real MSISDNs, IMEIs or IPs. Use:
- IMPU `sip:+919000000000@ims.mnc000.mcc000.3gppnetwork.org`
- Call-IDs `sub-a` (current) and `sub-old` (superseded)
- tags `ltag-a` / `ltag-old` (ours) and `rtag-1` (network)
- contact `10.0.0.1:5070`

All paths are relative to the repo root:

| Shorthand | Path |
|---|---|
| `session.rs` | `gsm-sip-bridge/src/ims/session.rs` |
| `ims/mod.rs` | `gsm-sip-bridge/src/ims/mod.rs` |
| `agent/mod.rs` | `gsm-sip-bridge/src/ims/agent/mod.rs` |
| `reg_subscription.rs` | `gsm-sip-bridge/src/ims/agent/reg_subscription.rs` (new) |

## Format: `[ID] [P?] [Story] Description`

- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- Stories:
  - **[US1]**: Timer F, confirming probe, qualifying endpoint failure (FR-004, FR-010, FR-011, FR-011a/b, FR-022).
  - **[US2]**: confirmation (Timer N), refresh, Subscription-State, renewal lifecycle (FR-003, FR-005, FR-012, FR-017–FR-019).
  - **[US3]**: NOTIFY dialog matching, `200`/`481`, superseded memory (FR-002, FR-013–FR-016).
  - **[US4]**: SUBSCRIBE error responses (FR-006–FR-009a).
  - **[US5]**: status (FR-020).
- FR-021 (VoWiFi = VoLTE) holds by construction: both run `dispatch_loop`. No separate task.

---

## Phase 1: Setup

- [X] T001 Confirm the baseline on branch `090-reg-event-subscription-tracking` is green: run `make lint && make test` from the repo root. Stop if anything fails before any code is touched. If another worktree shares the target dir, use a private `CARGO_TARGET_DIR`

---

## Phase 2: Foundational (blocks every story)

**Purpose**: a tracked sender, an in-dialog SUBSCRIBE builder, the `Subscription-State` parser, and the empty state machine wired into `LoopState`. Behaviour is unchanged at the end of this phase: the same SUBSCRIBE bytes go out at the same moments.

- [X] T002 [P] Write failing tests in `session.rs` for `build_subscribe` with `dialog: Some(SubscribeDialog { remote_tag: "rtag-1", request_uri: "sip:scscf@10.0.0.9:5060", route_set: ["Route: <sip:pcscf@10.0.0.8;lr>"] })`. Check that:
  - the request line uses `request_uri`;
  - the `Route` headers are exactly the route set (not Service-Route);
  - `To` carries `;tag=rtag-1`;
  - `From`, `Call-ID` and `Event: reg` are unchanged.

  Also check that `dialog: None` produces byte-for-byte today's request (existing tests keep passing).
- [X] T003 Implement `SubscribeDialog` and `SubscribeParts.dialog` in `session.rs`. Replace `subscribe_reg_event` with `send_reg_subscribe(session, &SubscribeRequest { access_network_info, expires, dialog: Option<(&str call_id, &str local_tag, SubscribeDialog)> }) -> BridgeResult<SentSubscribe>`:
  - A new subscription takes a fresh Call-ID and tag; a refresh reuses the given ones.
  - The CSeq always comes from `session.cseq`.
  - It logs `"sent reg-event SUBSCRIBE"` with `call_id`, `cseq`, `kind` and `expires` (contracts/observability.md).
  - It returns `Err` on a send failure, after logging WARN.
- [X] T004 [P] Write failing tests in `session.rs` for `parse_subscription_state(&str) -> SubState` (data-model.md):
  - `active;expires=600` → Active, 600.
  - `pending` → Pending.
  - `terminated;reason=probation;retry-after=300` → Terminated, probation, 300.
  - `Terminated; Reason=NoResource` → case-insensitive, reason lower-cased.
  - `terminated` → no reason.
  - an empty or absent header → `Missing`.
  - garbage numbers → `None`.

  Fold 089's `subscription_retry_after` into it, and keep 089's `classify_reg_notify` test for retry-after passing.
- [X] T005 Implement `SubState`, `SubStateKind` and `parse_subscription_state` in `session.rs`
- [X] T006 Create `reg_subscription.rs` with the types from data-model.md:
  - `RegSubscription`, `Subscription`, `Txn`, `SupersededSub`, `SubAction`, `ResponseMatch`, `NotifyMatch`, `EndpointFailureCause`.
  - The constants `TIMER_F`, `TIMER_N` (128 s), `SUB_INITIAL_BACKOFF` (5 s), `SUB_MAX_BACKOFF` (600 s), `SUPERSEDED_GRACE` (128 s) and `SUPERSEDED_CAP` (32).
  - Implement only `new`, `on_new_registration`, `on_sent` and `on_send_failed` (which returns `None` for now).
  - Write a failing-then-passing test: `on_new_registration` returns `SendInitial { expires: 3600 }`, and `on_sent` records the Call-ID, tag and CSeq as `current` with `Txn::Pending(Initial)`.
  - Register `mod reg_subscription;` in `agent/mod.rs`.
- [X] T007 Wire it in `agent/mod.rs`:
  - Add `LoopState.reg_sub`.
  - Add `LoopState::perform_sub_action(session, p, action)`, which sends via `send_reg_subscribe` and feeds `on_sent` / `on_send_failed`.
  - Remove the SUBSCRIBE from `run_inner` (keep `start_inbound` before the loop). `dispatch_loop` performs `reg_sub.on_new_registration(now)` as its first act.
  - In `on_idle_tick`, replace the post-renewal `subscribe_reg_event` with the same call.
  - Adapt `probe.rs` to `send_reg_subscribe(...)`, ignoring the result.
  - Run the full check and commit: `refactor(ims): track the reg-event SUBSCRIBE the dispatch loop sends`.

**Checkpoint**: same wire behaviour; every SUBSCRIBE's identifiers are now known to `LoopState`.

---

## Phase 3: User Story 1 — dead path detected through the unanswered SUBSCRIBE (P1) 🎯 MVP

**Goal**: SUBSCRIBE responses are correlated. Timer F leads to a confirming probe. An unanswered probe is a qualifying endpoint failure and forces a re-registration. An answered probe is a subscription failure on backoff.

**Independent Test**: `reg_subscription.rs` tests with an injected clock, plus `LoopState` tests over `udp_sink_pair`.

- [X] T008 [P] [US1] Write failing tests in `reg_subscription.rs` for `on_response`:
  - A `200` with Call-ID `sub-a` and CSeq `7 SUBSCRIBE` → `ResponseMatch::Current`; `txn` becomes Idle and the subscription is accepted with its granted `Expires`.
  - A response for `sub-old` (superseded) → `Superseded`, no state change (US1 scenario 5).
  - An unmatched Call-ID → `Unknown`.
  - A `100 Trying` → `Current`, but Timer F keeps running.
  - The current Call-ID with an older CSeq → `Superseded`.
- [X] T009 [US1] Implement `on_response` for 1xx/2xx/superseded/unknown in `reg_subscription.rs`. Error statuses only need to clear the transaction for now (US4 fills them in)
- [X] T010 [US1] In `agent/mod.rs` `handle_carrier_response`, route every response whose `cseq_method` is `SUBSCRIBE` to `reg_sub.on_response` **before** the keepalive match (research R3), log per contracts/observability.md, and return. Add a test that a late SUBSCRIBE response whose CSeq number equals a pending keepalive's is not scored as the keepalive's answer. Commit: `fix(ims): correlate reg-event SUBSCRIBE responses instead of logging them as stray`
- [X] T011 [P] [US1] Write failing tests in `reg_subscription.rs` for Timer F and the probe:
  - **Timer F timing:** a SUBSCRIBE sent at t0 with no response → `poll(t0+127s)` is `None`; `poll(t0+128s)` is `Probe`.
  - **Probe answered:** after `on_probe_sent(cseq, t)`, `on_probe_response(cseq)` is consumed. The next `poll` schedules a retry on the backoff at `t+5s`, records `last_failure`, and raises no `EndpointFailure` (US1 scenario 3).
  - **Probe unanswered:** `poll(t+10s)` → `EndpointFailure { SubscribeTimeout }` (scenario 1).
  - **Probe send fails:** `on_probe_send_failed` → `EndpointFailure` at once.
  - **Send failure:** `on_send_failed(now, false)` → `Probe`, and if that probe is unanswered → `EndpointFailure { SubscribeSendFailed }` (scenario 2).
  - **Late final response:** a final response at t0+100s → `poll(t0+128s)` is `None` (scenario 4).
  - **Escalation already pending:** with `force_renewal_pending = true`, Timer F records a subscription failure and returns no `Probe` (edge case: no stacking).
- [X] T012 [US1] Implement Timer F, `Txn::Probing`, `on_probe_sent`, `on_probe_send_failed`, `on_probe_response` and `poll` in `reg_subscription.rs`. Make `ping::PING_RESPONSE_TIMEOUT` `pub(super)` and use it as the probe deadline
- [X] T013 [P] [US1] Write failing `LoopState` tests in `agent/mod.rs`:
  - **Unanswered probe:** `on_qualifying_endpoint_failure(SubscribeTimeout)` sets `force_renewal`, clears `reconnect_attempts` and `ping`, sets `status.state = Failed` with the contract's reason text, and sets `gm_conn = Failed`.
  - **Mid-call:** with an `active_call`, the same happens, the call is **not** ended, and `maintenance.decide(Renewal, busy)` defers (FR-022).
  - **Ladder in progress:** with `reconnect_attempts = 2`, the qualifying failure ends the ladder: `probe_gm_connection` now returns early (clarification Q1 edge case).
  - **Mid-call probe:** a probe requested mid-call is sent via `send_gm_ping`, not gated on `busy()` (clarification Q3).
- [X] T014 [US1] Implement in `agent/mod.rs`:
  - `on_idle_tick` calls `reg_sub.poll(now, self.force_renewal)` right after `take_owed_budget_exit` and **before** the `busy()` gate, then performs the returned action.
  - `perform_sub_action` handles `Probe` (`session.send_gm_ping()`) and `EndpointFailure` (`on_qualifying_endpoint_failure`).
  - `handle_carrier_response` offers every response to `reg_sub.on_probe_response` before `PingState`.
  - Log per contract.
  - Commit: `feat(ims): detect a dead Gm path through an unanswered reg-event SUBSCRIBE (Timer F)`.

**Checkpoint**: US1 acceptance scenarios 1–5 pass in CI.

---

## Phase 4: User Story 2 — subscription confirmed, kept alive, re-established (P1)

**Goal**: Timer N, NOTIFY-before-2xx, refresh in the dialog, `Subscription-State` reasons, and a clean restart on each renewal.

**Independent Test**: pure `reg_subscription.rs` tests through every scenario with an injected clock.

- [X] T015 [P] [US2] Write failing tests in `reg_subscription.rs` for `on_notify` matching the **current** dialog (Call-ID `sub-a` and To tag `ltag-a`):
  - It returns `Current { state }`, sets `confirmed`, and learns `remote_tag` from the NOTIFY's `From`.
  - **NOTIFY before 2xx:** route set from Record-Route in received order, remote target from Contact. A later `200` with a different `To` tag keeps `rtag-1` (edge case), and no Timer N or Timer F failure follows (US2 scenario 2).
  - **2xx first:** route set via `uac_route_set` and remote target from its Contact.
  - Anything that is not current → a non-`Current` match. The `200`/`481` distinction is US3.
- [X] T016 [US2] Implement current-dialog matching and dialog learning in `reg_subscription.rs`. Reuse `uac_route_set`, `uac_remote_target` and `route_set_in_order` from `agent/call.rs`; they are already `pub(super)`, which a sibling module under `agent` can see
- [X] T017 [P] [US2] Write failing tests for Timer N:
  - A `200` at t0+1s and no NOTIFY → `poll(t0+128s)` is a subscription failure: `last_failure` reads `accepted but no NOTIFY within 128s`, `retry_at = +5s`, and no `EndpointFailure` (US2 scenario 1).
  - No response and no NOTIFY → Timer F wins, and only one action results (research R4).
  - A NOTIFY at t0+60s → nothing fires at 128s.
- [X] T018 [US2] Implement Timer N and retry scheduling on the subscription backoff in `reg_subscription.rs`. Use `session::next_backoff`, capped at 600 s and reset on confirmation (FR-009a)
- [X] T019 [P] [US2] Write failing tests for the Subscription-State actions (FR-017, FR-018):
  - `active;expires=600` updates `expires_at`.
  - **Terminated reasons:**
    - `terminated;reason=deactivated` and `timeout` → `SendInitial` on the next `poll` (scenario 4).
    - `probation;retry-after=300` → nothing before +300 s (scenario 5).
    - `giveup` with no retry-after → backoff.
    - `noresource` and `rejected` → `stopped`, with no SUBSCRIBE even after an hour (scenario 6).
    - no reason → backoff (scenario 7).
  - In every terminated case the old subscription moves to `superseded`.
- [X] T020 [US2] Implement the Subscription-State handling in `reg_subscription.rs`. In `agent/mod.rs` `handle_reg_notify`, run 089's classification and policy first, then hand `SubState` to the tracker (research R7)
- [X] T021 [P] [US2] Write failing tests for refresh (FR-019, research R8):
  - **Refresh timing:** granted 600 s confirmed at t0 → `poll(t0+299s)` is `None`; `poll(t0+300s)` is `SendRefresh` (headroom `min(300, 600/2)`).
  - **Refresh outcomes:**
    - `200` with `Expires: 600` → rescheduled.
    - A refresh `481` → `SendInitial` at once.
    - A refresh `408`, or Timer F with the probe answered → `SendInitial` on backoff.
    - A refresh `500` → the subscription stays until expiry and the refresh is retried on backoff.
  - **Expiry without refresh:** reaching `expires_at` without a successful refresh → `SendInitial` and the contract's "expired" reason.
  - **Granted 3600 s:** the refresh time falls after a 55 min registration renewal, so a renewal supersedes it first.
- [X] T022 [US2] Implement refresh in `reg_subscription.rs` (`SendRefresh` carrying the dialog). In `agent/mod.rs`, `perform_sub_action` builds the in-dialog request through `send_reg_subscribe` with `dialog: Some(..)`
- [X] T023 [P] [US2] Write failing tests for the renewal lifecycle (FR-003, US2 scenario 8):
  - Set up a pending Timer F, a scheduled retry, an in-flight probe and a raised `min_expires`. Then `on_new_registration` moves current to superseded, clears all of them, resets the backoff, and returns `SendInitial { expires: 3600 }`.
  - A later `poll` at the old Timer F deadline yields nothing.
- [X] T024 [US2] Implement `on_new_registration`'s reset and call it from `LoopState::on_renewal_success`'s caller in `on_idle_tick`. Commit: `feat(ims): confirm, refresh and re-establish the reg-event subscription`

**Checkpoint**: US2 scenarios 1–8 pass; US1 is still green.

---

## Phase 5: User Story 3 — NOTIFYs applied only to their own subscription (P2)

**Goal**: `200` + evaluate for current; `200` + log for superseded; `481` for unknown. Superseded subscriptions are remembered until expiry + 128 s, at most 32.

**Independent Test**: `LoopState` tests over `udp_sink_pair` reading the actual response sent.

- [X] T025 [P] [US3] Write failing tests in `reg_subscription.rs` for the superseded memory (clarification Q4):
  - A subscription granted 600 s and superseded at t0 is recognised at t0+727s and forgotten at t0+729s.
  - With no grant, the requested expiry is used.
  - The 33rd superseded entry evicts the oldest.
  - A NOTIFY for a forgotten entry → `Unknown`.
- [X] T026 [US3] Implement `SupersededSub` retention and pruning in `reg_subscription.rs`, run on `on_notify` and `poll`
- [X] T027 [P] [US3] Write failing tests in `session.rs` for `respond_notify(sink, req, status)`:
  - `481` → `481 Call/Transaction Does Not Exist` echoing the request's Via, From, To, Call-ID and CSeq.
  - `200` → today's response.
- [X] T028 [US3] Split `session::handle_notify`: `respond_notify` sends the response, and the existing classification and logging stays a separate step that no longer sends anything (research R6)
- [X] T029 [P] [US3] Write failing `LoopState` tests in `agent/mod.rs` (US3 scenarios 1–3):
  - **Current:** a current NOTIFY reporting our binding `terminated/deactivated` → the peer receives `200`, and status goes `Failed` (089 policy applied).
  - **Superseded:** a `sub-old` NOTIFY reporting our binding `terminated/rejected` → the peer receives `200`, status stays `Registered`, `rejection` stays `None`, and no call is ended.
  - **Unknown:** a NOTIFY with Call-ID `stray` → the peer receives `481`, with no other effect.
  - **Non-`reg` event:** a NOTIFY with `Event: presence` → `481`.
  - **No Subscription-State:** a current NOTIFY without the header → `200`, the WARN line, and treated as active.
- [X] T030 [US3] Implement matching-before-answering in `agent/mod.rs` `handle_reg_notify`. Superseded and unknown log lines follow contracts/observability.md (the unknown line carries both identifier sets). Commit: `fix(ims): answer reg-event NOTIFYs per subscription dialog (RFC 6665 §4.4.1)`

**Checkpoint**: SC-003 holds in CI; US1 and US2 are still green.

---

## Phase 6: User Story 4 — SUBSCRIBE error responses handled (P2)

**Goal**: 423, 489, 503 + Retry-After, and others on the subscription backoff, never more than one SUBSCRIBE per interval.

**Independent Test**: pure `reg_subscription.rs` tests.

- [X] T031 [P] [US4] Write failing tests in `reg_subscription.rs` (research R10):
  - **423:** `423` + `Min-Expires: 7200` (above the 3600 requested) → `SendInitial { expires: 7200 }` once `SUB_INITIAL_BACKOFF` has passed since the last initial SUBSCRIBE (scenario 1, SC-006), and later initial SUBSCRIBEs in this registration also ask ≥ 7200. A `423` without `Min-Expires` is treated as "other".
  - **503:** `503` + `Retry-After: 120` → nothing before +120 s, then `SendInitial` (scenario 2).
  - **489:** `489` → `stopped`, and no SUBSCRIBE for the rest of the registration even across 24 h of polls (scenario 3 and the "never supports reg" edge case).
  - **Other errors:** `403`, `500` and `603` → retries at +5, +10, +20 … capped at +600 s (scenario 4).
  - **401:** treated like any other error.
  - **SC-006:** across any mix of errors, there are never two `SendInitial`s within one backoff interval.
  - **Fresh identifiers:** each retry has fresh identifiers, and the refused one is superseded.
- [X] T032 [US4] Implement the error-response table in `reg_subscription.rs` `on_response`, including `Retry-After` header parsing (seconds; an HTTP-date is ignored → backoff), with log lines per contract. Commit: `feat(ims): act on reg-event SUBSCRIBE error responses`

---

## Phase 7: User Story 5 — status separates registration, subscription and transport (P3)

**Goal**: `reg_subscription` and `reg_subscription_last_failure` appear in the reply and in both status CLIs.

**Independent Test**: render, serde round trip, CLI formatting.

- [X] T033 [P] [US5] Write failing tests in `ims/mod.rs` for `SubscriptionHealth::render` (every variant, per data-model.md). Write a test that `RegistrationStatus::default()` has `reg_subscription = None`, and that `health()` (so `can_answer`) is unaffected by any `SubscriptionHealth` value (research R11)
- [X] T034 [US5] Implement `SubscriptionHealth` and the two `RegistrationStatus` fields in `ims/mod.rs`, and `RegSubscription::health(now_wall)` in `reg_subscription.rs`. Sync both fields in `agent/mod.rs` `sync_shared_status`
- [X] T035 [P] [US5] Write failing tests in `gsm-sip-bridge/src/vowifi/control.rs`:
  - The reply round-trips with both new fields.
  - A JSON reply without them parses, giving `""` and `None`.
- [X] T036 [US5] Add the two `#[serde(default)]` fields to `RegistrationStatusReply` in `vowifi/control.rs`. Fill them in `agent/mod.rs` `run_status_listener`. Print the two lines after `gm_connection` in `vowifi/mod.rs` and `volte/bridge.rs` (contracts/control.md), with `unknown` for an empty string and `none` for no failure. Add a shared formatter beside `format_rejection_cooldown` with its own unit test. Commit: `feat(ims): report reg-event subscription health in status`

---

## Phase 8: Polish & cross-cutting

- [X] T037 [P] Update operator docs: add the two status lines and the new log lines to `docs/observability.md`, which already documents `gm_connection` and `rejection_cooldown`. `make test` includes `test_config_docs`/doc-contract tests, so keep them green
- [X] T038 [P] Add `RELEASE_NOTES.md` and `CHANGELOG.md` entries. Bold only the summary phrase (memory: release-notes bold scope), and cite #124
- [X] T039 Audit the diff for SC-005: `grep -n "outside an active transaction"` must remain only on the non-SUBSCRIBE fallthrough, and every new log message must match contracts/observability.md word for word
- [X] T040 Run quickstart Q0 (`make format && make lint && make test`) and commit the docs: `docs(090): status lines, log contract, release notes`
- [X] T041 Live gate, quickstart Q1 (**blocking before merge**): a 3-hour run on the Jio Pi (`respond_on_client` on) and on the Vodafone rig. Record the counts in the PR description. Any current-session `481` stops the merge (research R6)
- [X] T042 [P] Live, quickstart Q2 (best effort) and Q3 (status lines) on the Vodafone rig; note the results in the PR (Q3 done on both rigs; Q2 not run)

---

## Dependencies & Execution Order

- **Setup (T001)** → **Foundational (T002–T007)** → the stories.
- **US1 (T008–T014)** first: the MVP, and every later story relies on its response routing (T010).
- **US2 (T015–T024)** depends on US1's `on_response` and `poll`.
- **US3 (T025–T030)** depends on US2's `on_notify` (T016).
- **US4 (T031–T032)** depends on US1's `on_response` (T009) and US2's backoff (T018). It can run in parallel with US3: both touch `reg_subscription.rs`, so run them sequentially when one person works alone.
- **US5 (T033–T036)** depends only on the Foundational phase plus T018/T020 for meaningful states. Its `vowifi/` and `volte/` edits can proceed in parallel with US3/US4.
- **Polish (T037–T042)** comes last. T041 gates the merge.

Within each story: tests → implementation → full check → commit.

## Parallel opportunities

- T002 ∥ T004 (different functions in `session.rs`; both test-only).
- In each story, the `[P]` test task can be drafted while the previous implementation task is finished.
- T035/T036's CLI work (`vowifi/`, `volte/`) ∥ US3/US4 (`reg_subscription.rs`, `agent/mod.rs`).
- T037 ∥ T038.

## Implementation strategy

- **MVP = Phases 1–3 (US1).** This alone closes #124's silent failure (`Registered` with a dead path) and gives spec 091 its trigger. It can be shipped on its own if needed.
- **Then US2.** Without it a line still loses notifications silently on a short grant or a `terminated`.
- **US3 and US4** are hardening. **US5** is diagnosability.
- One PR for the whole spec is the plan (PR 2 of 3 for #124), with one commit per phase as listed.
