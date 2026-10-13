# Contract: observability (logs, status)

Field names are stable: operators grep for them and field reports quote them (SC-006).

## Log: every reg-event NOTIFY (FR-016)

One line per `NOTIFY` with `Event: reg`, from `session::handle_notify`.

| Field | Example | Notes |
|---|---|---|
| `attribution` | `contact-addr` / `imei` / `sole-contact` / `none` | how the current entries were found |
| `contact_addr` | `10.0.0.1:5070` | the line's current contact address |
| `own_entries` | `2` | current-binding entries |
| `retired_entries` | `1` | IMEI-matched entries at another address |
| `contact_states` | `terminated/deactivated,active/registered` | `state/event` of the current entries |
| `applied` | `deactivated` / `unregistered` / `probation` / `rejected` / `none` | the policy input chosen (FR-014) |
| `retry_after_secs` | `600` | present only when one was found |
| `scope` | `identity-deregistered` / `binding-removed` | only when `applied != none` (FR-015) |
| `subscription_state` | `terminated;reason=noresource` | verbatim header |
| `body` | full reginfo XML | always |

Level and message:
- `applied != none` → `WARN` `"reg-event NOTIFY: our current binding was deregistered"`
- otherwise → `INFO` `"reg-event NOTIFY for our binding"` (or `"reg-event NOTIFY, nothing attributable to our
  current binding"` when `own_entries == 0`)

## Log: retired binding (FR-005)

One `INFO` line per retired entry that is `terminated`:
`"replaced binding retired"` with `retired_addr`, `state`, `event`, `contact_addr`.

## Log: policy decisions (`ims/agent/mod.rs`)

| Message | Level | Fields |
|---|---|---|
| `"re-registering now: the network deactivated our binding"` | WARN | `registered_secs_ago`, `counted` |
| `"the network dropped our binding; retrying on backoff"` | WARN | `event`, `registered_secs_ago`, `retry_in_secs` |
| `"the network rejected our registration; holding off"` | WARN | `cooldown_secs`, `retry_after_secs`, `next_attempt_at` (RFC 3339) |
| `"ending call: the network rejected our registration"` | WARN | `call_id` |
| `"rejection cooldown over; trying to register once"` | INFO | — |

## Status (FR-017)

`RegistrationStatusReply.rejection_cooldown_until: Option<u64>` (serde-default, so older peers parse).

`vowifi-status` and `volte-status`, per line, after `gm_connection`:

```text
    rejection_cooldown: none
    rejection_cooldown: until 2026-10-14T10:42:00+00:00 (next attempt in 1712s)
```

During the cooldown the line also reads `state: Failed`, `can_answer: false`,
`last_failure: <ts> the network rejected our registration (reg-event rejected); next attempt at <ts>`.

## Alerting

No new alert. `obs.set_registered(false)` on `rejected` feeds the existing registration-loss alert; with a
30-minute default cooldown the alert's `unhealthy_sec` threshold is crossed and it fires.
