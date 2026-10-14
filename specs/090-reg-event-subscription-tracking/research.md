# Research: Reg-event SUBSCRIBE transaction and dialog tracking

All decisions below are grounded in the code as of `f23b8af` (spec 089 merged, plus the 090 spec commit). The
spec has no open NEEDS CLARIFICATION: five clarifications are recorded in it (session 2026-10-14). This file
records the design choices the spec leaves to planning.

Code as it stands:

- `session::subscribe_reg_event` builds the SUBSCRIBE with a fresh random Call-ID and From tag, sends it, logs, and
  returns `()`. Nothing keeps the identifiers.
- It is called from three places: `run_inner` after the first registration (`agent/mod.rs:882`), `on_idle_tick`
  after each successful renewal (`agent/mod.rs:3158`), and the one-shot `probe` CLI (`agent/probe.rs:59`).
- Its response lands in `handle_carrier_response`. That matches outbound INVITEs, then session-refresh UPDATEs,
  then the Gm keepalive by CSeq number alone, and logs everything else as `"received response outside an active
  transaction"`.
- Every NOTIFY goes to `handle_reg_notify` → `session::handle_notify`, which answers `200 OK` before looking at
  anything.

## R1: A pure subscription state machine in its own module

**Decision**: add `ims/agent/reg_subscription.rs`, holding a `RegSubscription` that `LoopState` owns, following
the `ping.rs` precedent (`PingState`). It is a pure state machine with no socket and no clock of its own:

- Every entry point takes `now: Instant`.
- I/O requests come back as a `SubAction` value. `LoopState` performs the sends, and feeds back what happened
  (sent, with these identifiers; or the send failed).

**Rationale**: the spec's acceptance scenarios all depend on time (128 s, 300 s retry-after, the 600 s backoff
cap). `PingState` shows the pattern already works here: its tests never sleep and never open a socket.
`agent/mod.rs` is already 5,179 lines, and this is a self-contained protocol state machine.

**Alternatives considered**:
- *Fields directly on `LoopState`*: about a dozen new fields beside the 089 ones, and the tests would need a
  whole `LoopState` for every timer case. Rejected.
- *A thread per subscription with real timers*: the dispatch loop is deliberately single-threaded over
  `inbound.rx`, and a second actor would need locking around `session`. Rejected (Principle V).

## R2: The initial SUBSCRIBE moves into the dispatch loop

**Decision**:
- `subscribe_reg_event` becomes `send_reg_subscribe(session, &SubscribeRequest) -> BridgeResult<SentSubscribe>`.
  `SentSubscribe` carries the Call-ID, local tag, CSeq and requested expiry; a send error comes back as `Err`.
- `build_subscribe`'s `SubscribeParts` gains an optional `dialog`: remote tag, remote target and route set. Without
  it the request is the initial out-of-dialog SUBSCRIBE (today's exact bytes). With it, the request is an
  in-dialog refresh: the Request-URI is the remote target, the Route headers are the dialog route set, and `To`
  carries the remote tag.
- `run_inner` stops sending the SUBSCRIBE. `dispatch_loop` sends it as its first action, through `LoopState`, so
  the very first subscription is tracked. `start_inbound` still runs first: listeners are up before the SUBSCRIBE
  leaves, as the existing comment requires.
- `probe.rs` keeps a fire-and-forget call. It is a one-shot diagnostic that never runs the dispatch loop, so it
  discards the `SentSubscribe`.

**Rationale**: FR-001 needs the identifiers. FR-011 needs to know when the send failed. A subscription sent before
`LoopState` exists would have to be smuggled into it; sending it from the loop removes that seam.

## R3: Correlating SUBSCRIBE responses

**Decision**: `handle_carrier_response` checks for a SUBSCRIBE response first, by `cseq_method(CSeq) ==
"SUBSCRIBE"`. Every such response goes to `RegSubscription::on_response` and is consumed, and the method returns
one of:

- `Current`: the Call-ID and CSeq match the outstanding transaction of the current subscription.
- `Superseded`: the Call-ID matches a remembered superseded subscription, or the current one with an older CSeq.
  It is logged at INFO and has no effect (FR-004, US1 scenario 5).
- `Unknown`: no match. It is logged at INFO with its Call-ID and CSeq, and has no effect.

1xx responses are logged at DEBUG and do not stop Timer F: for a non-INVITE transaction, Timer F runs until a
final response (RFC 3261 §17.1.2.2).

**Rationale**: SC-005 forbids any SUBSCRIBE response reaching `"outside an active transaction"`.

Checking SUBSCRIBE before the keepalive also closes a latent mis-match. `PingState::on_response` matches by CSeq
number only. A late SUBSCRIBE response from a session that has since been replaced can carry the same CSeq number
as the new session's in-flight `OPTIONS`, and would then be scored as the keepalive's answer. Routing by method
first makes that impossible without touching `ping.rs`.

The confirming probe (R5) is an `OPTIONS` too. It is matched before `PingState`, by exact CSeq.

## R4: Timer F and Timer N

**Decision**: `TIMER_F = TIMER_N = 64 × T1 = 128 s` (IMS T1 = 2 s, TS 24.229 table 7.7.1), as constants in
`reg_subscription.rs`.

- Both are measured from the moment the SUBSCRIBE was sent.
- Timer F applies to every SUBSCRIBE, initial or refresh (FR-010). Timer N applies only to the initial SUBSCRIBE
  of each subscription: RFC 6665 §4.1.2.4 defines it for dialog creation, and a refresh already belongs to a
  confirmed dialog.
- A NOTIFY for the subscription satisfies **both** timers. It proves the network received the SUBSCRIBE, so a
  missing final response is no longer evidence of a dead path. This is consistent with FR-012's "a NOTIFY before
  the 2xx satisfies it", and avoids raising a qualifying endpoint failure on a subscription that is working.
- When both expire on the same tick, Timer F wins and Timer N is cancelled. One unanswered SUBSCRIBE produces one
  decision, not two retries.

**Evaluation point**: `RegSubscription::poll(now)` runs on every `on_idle_tick`, right after
`take_owed_budget_exit` and **before** the `busy()` gate. The confirming probe must go out mid-call
(clarification Q3), and refreshes are not maintenance. The idle tick runs at 1 s resolution when idle and 100 ms
during a call, so SC-001's 140 s (128 s + 10 s probe deadline + tick) holds. A message flood can starve
`on_idle_tick`, but every existing timer (keepalive, renewal) already accepts that risk.

## R5: The confirming probe and the qualifying endpoint failure

**Decision**:

- **When Timer F fires, or the SUBSCRIBE send returns `Err`:**
  - If `force_renewal` is already set, the tracker records a subscription failure, and `LoopState` logs that an
    escalation is already pending. Nothing is stacked (edge case).
  - Otherwise the tracker enters `Probing` and asks for `SubAction::Probe`. `LoopState` calls
    `session.send_gm_ping()` directly, *not* through `PingState`, and hands the returned CSeq back with
    `RegSubscription::on_probe_sent(cseq, now)`. This is an exception to the keepalive's usual silence during
    calls (clarification Q3).
- **The probe is answered** (any final response with that CSeq, as for the keepalive, specs/028 R1): the outcome
  is a subscription failure only. It is logged, a re-subscribe is scheduled on the backoff (R9), and registration
  state is untouched (FR-011b).
- **The probe is not answered within `PING_RESPONSE_TIMEOUT` (10 s), or `send_gm_ping` returns `Err`:**
  `RegSubscription` asks for `SubAction::EndpointFailure { cause }`, and
  `LoopState::on_qualifying_endpoint_failure` does the following (FR-011a):
  - Sets `force_renewal = true`. This bypasses the transport-rebuild ladder (clarification Q1):
    `probe_gm_connection` already returns early while `force_renewal` is set, so any ladder in progress is
    abandoned (edge case).
  - Calls `ping.reset()` and sets `reconnect_attempts = 0`.
  - Sets the status to `Failed`, with `last_failure` set to `"reg-event SUBSCRIBE unanswered and the Gm keepalive
    probe failed; re-registering"`, and calls `obs.set_registered(false)`.
  - Sets `gm_conn` to `Failed { since }` (episode start per `gm_episode_since`) and calls
    `obs.set_gm_connection_up(false)`. This uses the existing meaning of `Failed`, "repair has been escalated to
    re-registration", and a successful renewal resets it to `Up` as it does today.
  - Logs a WARN line named in contracts/observability.md, with `cause = subscribe-timeout | subscribe-send-failed`.
    That log line and the `on_qualifying_endpoint_failure` method are the "distinct signal" spec 091 hooks into. No
    further abstraction is built now (Principle V).
- **During a call:** the renewal that `force_renewal` asks for is deferred by `MaintenancePolicy` exactly as today
  (FR-022). Status reads `Failed` straight away.
- **If the forced renewal fails:** it goes through `schedule_renewal_retry` like any other renewal, and is counted
  against the existing budget.

`PING_RESPONSE_TIMEOUT` becomes `pub(super)` so the tracker uses the same deadline as the keepalive, as the spec
requires.

**Alternatives considered**:
- *Feed the probe through `PingState`*: its verdict suppresses everything during a call, and a `Dead` result there
  starts the rebuild ladder. Clarifications Q1 and Q3 both rule that out. Rejected.

## R6: Matching NOTIFYs to dialogs

**Decision**: the dialog key is `(Call-ID, local tag)`, where the local tag is our SUBSCRIBE's `From` tag and the
NOTIFY's `To` tag (FR-013). `handle_reg_notify` asks `RegSubscription::on_notify(req, now)` **before** sending
anything. The method returns one of:

- `Current { state }`: the NOTIFY is answered `200 OK`, then classified by 089's `classify_reg_notify` and policy
  exactly as today. After that the Subscription-State is applied (R7).
- `Superseded`: answered `200 OK`, plus an INFO line with the Call-ID, `Subscription-State`, the 089 attribution
  summary and the body. It is never passed to the deregistration policy (FR-015).
- `Unknown`: answered `481 Call/Transaction Does Not Exist` via `build_uas_response`, plus an INFO line with both
  identifier sets, the NOTIFY's and the current subscription's (the "identifier mismatch" edge case). It has no
  other effect (FR-016).

This means `session::handle_notify` no longer sends the `200` itself. It is split into
`respond_notify(sink, req, status)` and the existing classification and logging. The `200`/`481` choice belongs to
the matcher.

**The remote tag and route set** come from whichever arrives first:
- **The 2xx:** `To` tag, then `uac_route_set` (Record-Route reversed, falling back to Service-Route) and
  `uac_remote_target` (Contact), reusing `agent/call.rs`.
- **The first NOTIFY** (RFC 6665 §4.1.2.4: the NOTIFY creates the dialog): its `From` tag, its Record-Route in
  received order (`route_set_in_order`), and its Contact.

Once the remote tag is set it is never replaced. A later 2xx with a different tag is logged and its tag ignored
(edge case). If there is no usable Contact, refreshes fall back to the IMPU as Request-URI and to the
Service-Route.

**A NOTIFY with no `Event: reg`** is still matched by dialog. We only ever subscribe to `reg`, so any other package
is `Unknown` → `481`. That replaces today's "log it and `200`". RFC 6665 §4.4.1 allows this, and nothing in the
tree subscribes to another package.

**The `respond_on_client` path** needs no change. `dispatch_loop` has already swapped the sink before
`handle_reg_notify` runs.

**Superseded memory** (clarification Q4): a `VecDeque<SupersededSub { call_id, local_tag, forget_at }>`.
- `forget_at` is the granted expiry (the requested expiry if none was granted) plus 128 s.
- At most 32 are kept, and the oldest is dropped first.
- Pruned at each `on_notify` and `poll`.

**Risk carried to the live gate**: RFC 6665 §4.1.3 says a `481` to a NOTIFY terminates that subscription at the
notifier. If a carrier sent a current-subscription NOTIFY with identifiers that differ from what we recorded, this
change would make the network drop our subscription. Nothing in the 089 logs suggests it: every NOTIFY carried the
SUBSCRIBE's Call-ID. The quickstart's live check (Q2) is blocking: zero `481`s on Jio and Vodafone over a 3-hour
run, every `Current` NOTIFY confirming the subscription, and Jio exercised with `respond_on_client` on. Per the
Jio memory notes, response details there have bitten before.

## R7: Subscription-State handling

**Decision**: a pure `parse_subscription_state(&str) -> SubState { state, expires, reason, retry_after }`. It
reuses 089's `subscription_retry_after` parameter scan, generalised to read `expires` and `reason` too.

- **Header missing:** treated as `active` with no `expires`, plus a WARN `"NOTIFY without Subscription-State"`
  (edge case).
- **`active` / `pending` with `expires`:** the subscription's expiry is updated, and the refresh is rescheduled
  from it (FR-017).
- **`terminated`:** the current subscription is moved to superseded, and the next action follows FR-018:

| Reason | Next SUBSCRIBE |
|---|---|
| `deactivated`, `timeout` | immediately |
| `probation`, `giveup` | at `max(now + retry-after, backoff deadline)`; backoff alone if no `retry-after` |
| `rejected`, `noresource` | none until the next registration (`stopped`) |
| absent, or unrecognised | on backoff |

**Order** for a terminated current NOTIFY: 089's classification and policy run first (its body may report the
binding deregistered, which forces a renewal), then the Subscription-State action. A renewal that follows makes a
fresh subscription and discards the scheduled re-subscribe (R12), so the two cannot double up.

## R8: Refresh within the dialog

**Decision**: an active subscription is refreshed at `expires_at − renewal_headroom_for(granted, 300 s)`. This
reuses `ims::renewal_headroom_for`, the same rule registration uses, so a short grant cannot cause a refresh
storm (FR-019). The refresh is an in-dialog SUBSCRIBE: same Call-ID and tags, CSeq taken from `session.cseq`,
which only increases within the session that owns the dialog, and the requested Expires (3600, or a raised
`Min-Expires`).

| Refresh outcome | Action |
|---|---|
| 2xx | new expiry from its `Expires`; reschedule |
| `423` + `Min-Expires` | resend at once with the larger value |
| `481`, `408`, or Timer F with the probe answered | the subscription is gone; start a new initial SUBSCRIBE (481 at once, the others on backoff) |
| Timer F with the probe unanswered | qualifying endpoint failure (R5) |
| `489` | stop until the next registration |
| `503` + `Retry-After`, other 4xx/5xx/6xx | the subscription stays valid until its expiry; retry the refresh on backoff, with `Retry-After` as a floor |
| expiry reached without a successful refresh | treated as `terminated;reason=timeout`: a new initial SUBSCRIBE at once |

Refreshes run regardless of `busy()`. A refresh is ordinary signalling, not maintenance, and it does not touch
the transport.

## R9: Subscription backoff

**Decision**: `RegSubscription` has its own `backoff: Duration`: `SUB_INITIAL_BACKOFF = 5 s`, doubled with the
existing `session::next_backoff`, capped at `SUB_MAX_BACKOFF = 600 s` (clarification Q2, FR-009a).

- It is reset when a subscription is confirmed (first NOTIFY) and in `on_new_registration`.
- A network `Retry-After`, from the header on 503 or the parameter on `terminated`, is a floor:
  `next = max(now + backoff, now + retry_after)`.
- The schedule is a single `retry_at: Option<Instant>`, so a failure can never queue more than one pending
  SUBSCRIBE (SC-006).
- It never touches `LoopState.backoff` or `consecutive_renewal_failures`: subscription failures do not spend the
  renewal budget.

## R10: Error responses to the initial SUBSCRIBE

| Status | Action | FR |
|---|---|---|
| 2xx | accepted; expiry from the response's `Expires` (requested value if absent) | FR-005 |
| `423` with `Min-Expires` | resend at once requesting `max(requested, Min-Expires)`; remembered for the rest of this registration | FR-006 |
| `423` without `Min-Expires` | as "other" | |
| `489` | `stopped` until the next registration; WARN | FR-008 |
| `503` with `Retry-After` | retry on backoff, never before `Retry-After` | FR-007 |
| any other 3xx–6xx (including `401`/`407`, per the spec's assumption) | WARN; retry on backoff | FR-009 |

Each retry is a **new** initial SUBSCRIBE: a fresh Call-ID and tag, and the old ones are moved to superseded.
RFC 6665 §4.1.2.1 has no dialog after a failure response, so there is nothing to reuse.

## R11: Status reporting

**Decision**:

- `ims::RegistrationStatus` gains `reg_subscription: SubscriptionHealth` and
  `reg_subscription_last_failure: Option<(SystemTime, String)>`. They are in-memory only, like `gm_connection`,
  and synced from `LoopState` in `sync_shared_status`.
- `RegistrationStatusReply` gains `reg_subscription: String` and
  `reg_subscription_last_failure: Option<(u64, String)>`, both `#[serde(default)]`, so older peers still parse.
- `vowifi-status` and `volte-status` print two new lines after `gm_connection`, as specified in
  contracts/control.md.

`SubscriptionHealth` is **not** an input to `ServiceHealth` and does not change `can_answer`. The spec's
subscription failure explicitly leaves registration untouched (FR-011b, FR-012), and the three facts are reported
side by side, none of them presented as proof of reachability (FR-020).

Registration (`state`) and transport (`gm_connection`) are already reported, so FR-020 adds only the subscription
fact.

## R12: Lifecycle across renewals

**Decision**: `on_renewal_success` calls `RegSubscription::on_new_registration(now)`. That method:

- moves the current subscription (if any) to superseded;
- clears `retry_at`, `stopped`, any probe in flight and the `Min-Expires` floor;
- resets the backoff;
- returns `SubAction::SendInitial`, which `on_idle_tick` performs right where `subscribe_reg_event` is called today.

Everything still pending belonged to the old subscription record, which no longer exists, so no old timer can
fire against the new one (FR-003, US2 scenario 8).

A **failed** renewal leaves the old session in place, so its subscription and timers carry on unchanged.

## R13: Testing

- **`reg_subscription.rs`**: pure tests with injected `Instant`s, one per acceptance scenario of US1, US2 and US4,
  and the edge cases. No socket.
- **`session.rs`**: `build_subscribe` in-dialog formatting; `parse_subscription_state`; `respond_notify`
  producing a `481` with the request's own headers.
- **`agent/mod.rs`**: `LoopState`-level tests over the existing `udp_sink_pair` helper (a real UDP socket pair, not
  a mock). They check that a superseded NOTIFY gets `200` and leaves `status.state` alone, that an unknown one gets
  `481`, that a qualifying endpoint failure sets `force_renewal` and `Failed` but does not end a call, and that
  `on_renewal_success` produces a fresh `SendInitial`.
- **Status**: `SubscriptionHealth::render`, the reply round trip with and without the new fields, and the CLI
  formatter.

No new mocks (constitution I). The live gate (quickstart Q1 and Q2) covers carrier behaviour CI cannot reach.
