# Contract: status reply and status CLI (FR-020)

## `ControlMessage::RegistrationStatusReply`: new fields

| Field | Type | Default (older peer) | Meaning |
|---|---|---|---|
| `reg_subscription` | `String` | `""` | `SubscriptionHealth::render()`: `none`, `pending since <ts>`, `confirmed, expires <ts>`, `unconfirmed since <ts>[; retry at <ts>]`, `stopped until next registration (<reason>)` |
| `reg_subscription_last_failure` | `Option<(u64, String)>` | `None` | Unix seconds and reason of the most recent subscription failure. It survives a later success, so an operator can see the last thing that went wrong. |

Both fields are `#[serde(default)]`. A reply from an older agent still parses. A newer CLI prints `unknown` for an
empty `reg_subscription`, the same treatment `gm_connection` gets.

Timestamps inside `reg_subscription` are RFC 3339 UTC, as in `gm_connection`.

## `vowifi-status` / `volte-status`

Two lines follow `gm_connection` in each line's registration block:

```text
    state: Registered
    registered_at: …
    expires_at: …
    gm_connection: up
    reg_subscription: unconfirmed since 2026-10-14T10:02:11+00:00; retry at 2026-10-14T10:04:19+00:00
    reg_subscription_last_failure: 2026-10-14T10:02:11+00:00 accepted but no NOTIFY within 128s
    rejection_cooldown: none
    last_failure: none
    can_answer: true
```

When there has been no failure, the second line reads `reg_subscription_last_failure: none`.

`can_answer` and `blocked_reason` are unchanged. Subscription health is reported beside registration and transport
health and is not an input to them (research R11).

## Failure reasons (`reg_subscription_last_failure` text)

| Cause | Text |
|---|---|
| Timer N | `accepted but no NOTIFY within 128s` |
| Timer F, probe answered | `SUBSCRIBE unanswered within 128s (Gm path alive)` |
| Send failed, probe answered | `SUBSCRIBE could not be sent (Gm path alive)` |
| Error response | `SUBSCRIBE refused: <status> <reason>` |
| Terminated | `terminated by the network: <reason or "no reason">` |
| Refresh expiry | `expired without a successful refresh` |

A qualifying endpoint failure is reported through `state: Failed`, `last_failure` and `gm_connection: failed since
…`, not here: it is a registration fact.
