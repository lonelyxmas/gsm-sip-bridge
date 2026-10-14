# Data model: Reg-event SUBSCRIBE transaction and dialog tracking

Everything here is in-memory and lives for one agent process. Nothing is persisted (spec assumption). The types sit
in `gsm-sip-bridge/src/ims/agent/reg_subscription.rs` unless another path is given.

## `RegSubscription` (owned by `LoopState`, one per line)

| Field | Type | Meaning |
|---|---|---|
| `current` | `Option<Subscription>` | The live subscription for this registration. `None` before the first send, after a terminal stop, or while waiting to retry. |
| `superseded` | `VecDeque<SupersededSub>` | Earlier subscriptions of this line, kept only to recognise their late NOTIFYs and responses. At most 32 entries. |
| `retry_at` | `Option<Instant>` | When the next initial SUBSCRIBE goes out. There is a single slot, so at most one is ever pending (SC-006). |
| `backoff` | `Duration` | The next retry delay: 5 s doubling to 600 s. Reset on confirmation and on a new registration (FR-009a). |
| `min_expires` | `u32` | The Expires to request: `DEFAULT_EXPIRES` (3600), raised by a `423`'s `Min-Expires` for the rest of this registration. |
| `stopped` | `Option<String>` | Set on `489`, `terminated;reason=rejected` or `terminated;reason=noresource`. While set, no SUBSCRIBE is sent until `on_new_registration`. |
| `last_failure` | `Option<(SystemTime, String)>` | The most recent subscription failure, for status (FR-020). Survives renewals. |

### Entry points (all take `now: Instant`)

| Method | Input | Returns |
|---|---|---|
| `on_new_registration(now)` | A registration (initial or renewal) succeeded. | `SubAction::SendInitial { expires }` |
| `on_sent(sent: SentSubscribe, now)` | `LoopState` sent the requested SUBSCRIBE. | — |
| `on_send_failed(now, force_renewal_pending)` | The send returned `Err`. | `SubAction::Probe` or `None` |
| `on_response(resp, now)` | Any response whose CSeq method is `SUBSCRIBE`. | `ResponseMatch` plus an optional `SubAction` |
| `on_notify(req, now)` | Any NOTIFY. | `NotifyMatch` |
| `on_probe_sent(cseq, now)` / `on_probe_send_failed(now)` | The result of a `Probe` action. | — / `EndpointFailure` |
| `on_probe_response(cseq)` | Any response, before `PingState` sees it. | `bool` (consumed) |
| `poll(now, force_renewal_pending)` | Every `on_idle_tick`. | `Option<SubAction>` |
| `health(now_wall)` | `sync_shared_status`. | `SubscriptionHealth` |

## `Subscription`

| Field | Type | Meaning |
|---|---|---|
| `call_id` | `String` | Our Call-ID (FR-001). |
| `local_tag` | `String` | Our `From` tag, which the NOTIFY's `To` tag must match (FR-013). |
| `remote_tag` | `Option<String>` | The network's tag, from the first 2xx or NOTIFY, whichever came first. Never replaced. |
| `route_set` | `Vec<String>` | `Route:` lines for in-dialog requests (research R6). |
| `remote_target` | `Option<String>` | The Request-URI for refreshes. Falls back to the IMPU. |
| `requested_expires` | `u32` | The Expires on the last SUBSCRIBE sent. |
| `granted` | `Option<Duration>` | From the 2xx `Expires` or the `Subscription-State` `expires`. |
| `expires_at` | `Option<Instant>` | `granted` applied at the time it was learned. |
| `confirmed` | `bool` | A NOTIFY has arrived in this dialog (stops Timer N). |
| `txn` | `Txn` | The SUBSCRIBE transaction in flight. |

### `Txn` (the SUBSCRIBE transaction in flight)

```text
Pending { cseq, sent_at, kind: Initial | Refresh }   Timer F (and Timer N if Initial) runs from sent_at
Probing { cseq, kind, probe: Option<(u32, Instant)>, cause }
                                                     Timer F fired or the send failed; awaiting the keepalive probe
Idle                                                 no transaction outstanding
```

### State transitions

```text
                 on_new_registration / retry_at reached
                               │ SendInitial
                               ▼
       ┌──────────── Pending(Initial) ──────────────┐
  2xx  │                    │ NOTIFY                 │ 3xx–6xx (R10)
       ▼                    ▼                        ▼
 Accepted (unconfirmed)   Confirmed               current → superseded;
       │ NOTIFY              (Timers F and N         retry_at = backoff / floor;
       ▼                      satisfied)             489 → stopped
   Confirmed ◄───────────────┘
       │ expires_at − headroom
       ▼
 Pending(Refresh) ── 2xx ──► Confirmed (new expiry)
       │ 481/408 · expiry reached · terminated(reason) → current → superseded; next per R7/R8

 Timer F (Pending) or send Err ──► Probing ── probe answered ──► subscription failure, retry on backoff
                                            └─ probe unanswered/unsendable ──► EndpointFailure (force_renewal)
 Timer N (Pending(Initial), accepted, not confirmed) ──► subscription failure, retry on backoff
```

## `SupersededSub`

| Field | Type | Meaning |
|---|---|---|
| `call_id` | `String` | |
| `local_tag` | `String` | |
| `forget_at` | `Instant` | Granted expiry (or the requested expiry if none was granted) + 128 s (clarification Q4). |

## `SentSubscribe` (`ims/session.rs`)

Returned by `send_reg_subscribe`: `call_id`, `local_tag`, `cseq`, `expires`. In-dialog refreshes reuse
`call_id` and `local_tag`, so only `cseq` and `expires` change between sends.

## `SubAction`

| Variant | `LoopState` does |
|---|---|
| `SendInitial { expires }` | `send_reg_subscribe` with no dialog → `on_sent` / `on_send_failed` |
| `SendRefresh { expires }` | `send_reg_subscribe` with the current dialog → `on_sent` / `on_send_failed` |
| `Probe` | `session.send_gm_ping()` → `on_probe_sent` / `on_probe_send_failed` |
| `EndpointFailure { cause }` | `on_qualifying_endpoint_failure(cause)` (research R5) |

## `ResponseMatch` / `NotifyMatch`

| `ResponseMatch` | Meaning |
|---|---|
| `Current` | answers the outstanding transaction of the current subscription |
| `Superseded` | a remembered older subscription, or an older CSeq of the current one |
| `Unknown` | nothing we remember |

| `NotifyMatch` | Response | Then |
|---|---|---|
| `Current { state: SubState }` | `200 OK` | 089 classification and policy, then R7 |
| `Superseded` | `200 OK` | log only |
| `Unknown` | `481` | log only |

## `SubState` (`ims/session.rs`)

`state: SubStateKind (Active | Pending | Terminated | Missing)`, `expires: Option<u64>`, `reason: Option<String>`
(lower-cased), `retry_after: Option<u64>`.

## `EndpointFailureCause`

`SubscribeTimeout` (Timer F) | `SubscribeSendFailed`. Rendered `subscribe-timeout` / `subscribe-send-failed`.

## `SubscriptionHealth` (`ims/mod.rs`)

| Variant | Rendered |
|---|---|
| `None` | `none` (before the first SUBSCRIBE) |
| `Pending { since }` | `pending since <rfc3339>` |
| `Confirmed { expires_at }` | `confirmed, expires <rfc3339>` |
| `Unconfirmed { since, retry_at: Option<SystemTime> }` | `unconfirmed since <rfc3339>; retry at <rfc3339>` (`; retry at` omitted when `None`) |
| `Stopped { reason }` | `stopped until next registration (<reason>)` |

"Accepted but not yet confirmed" renders as `pending`, because only a NOTIFY confirms a subscription (US2).

## Changes to existing types

| Type | Change |
|---|---|
| `ims::RegistrationStatus` | `+ reg_subscription: SubscriptionHealth` (default `None`), `+ reg_subscription_last_failure: Option<(SystemTime, String)>` |
| `ControlMessage::RegistrationStatusReply` | `+ reg_subscription: String`, `+ reg_subscription_last_failure: Option<(u64, String)>`, both `#[serde(default)]` |
| `LoopState` | `+ reg_sub: RegSubscription` |
| `SubscribeParts` | `+ dialog: Option<SubscribeDialog { remote_tag, request_uri, route_set }>` |
| `ping::PING_RESPONSE_TIMEOUT` | made `pub(super)` |
