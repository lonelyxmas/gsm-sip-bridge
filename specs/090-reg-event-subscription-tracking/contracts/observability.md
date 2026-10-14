# Contract: observability (logs)

Field names and messages are stable: operators grep for them, and SC-005 is measured with them. Every SUBSCRIBE
response and every NOTIFY produces exactly one of the lines below that names its match (`current`, `superseded`
or `unknown`).

## SUBSCRIBE sent

`INFO` `"sent reg-event SUBSCRIBE"`

| Field | Example |
|---|---|
| `impu` | `sip:+919000000000@ims.mnc000.mcc000.3gppnetwork.org` |
| `call_id` | `3fa9c2e1b07d4e12` |
| `cseq` | `7` |
| `kind` | `initial` / `refresh` |
| `expires` | `3600` |

A send failure is `WARN` `"failed to send reg-event SUBSCRIBE"` with `error`, `call_id` and `kind`.

## SUBSCRIBE response

| Match | Level | Message | Fields |
|---|---|---|---|
| current, 2xx | INFO | `"reg-event SUBSCRIBE accepted"` | `call_id`, `cseq`, `status`, `granted_expires`, `kind` |
| current, 423 | INFO | `"reg-event SUBSCRIBE interval too brief; resending"` | `call_id`, `min_expires` |
| current, 489 | WARN | `"the network does not support the reg event package; not subscribing again for this registration"` | `call_id` |
| current, other 3xx–6xx | WARN | `"reg-event SUBSCRIBE refused; retrying on backoff"` | `call_id`, `status`, `reason`, `retry_in_secs`, `kind` |
| superseded | INFO | `"response to a superseded reg-event SUBSCRIBE; ignored"` | `call_id`, `cseq`, `status` |
| unknown | INFO | `"response to an unknown reg-event SUBSCRIBE; ignored"` | `call_id`, `cseq`, `status` |

## Timers and the confirming probe

| Message | Level | Fields |
|---|---|---|
| `"reg-event SUBSCRIBE unanswered; probing the Gm path"` | WARN | `call_id`, `cause` (`subscribe-timeout` / `subscribe-send-failed`), `in_call` |
| `"Gm path answered; reg-event subscription failed, retrying on backoff"` | WARN | `call_id`, `retry_in_secs` |
| `"qualifying endpoint failure: re-registering"` | WARN | `cause`, `call_id`, `in_call` (when `true`, the re-registration waits for the call) |
| `"qualifying endpoint failure while a re-registration is already pending"` | INFO | `cause`, `call_id` |
| `"reg-event subscription accepted but never confirmed; retrying on backoff"` (Timer N) | WARN | `call_id`, `retry_in_secs` |

The `"qualifying endpoint failure: re-registering"` line is the signal spec 091 builds P-CSCF failover on.

## NOTIFY

| Match | Response | Level | Message | Fields |
|---|---|---|---|---|
| current | `200` | as spec 089 (`"reg-event NOTIFY …"`) | unchanged | 089's fields, plus `subscription=current` |
| superseded | `200` | INFO | `"NOTIFY for a superseded reg-event subscription; acknowledged, not applied"` | `call_id`, `subscription_state`, `attribution`, `own_entries`, `contact_states`, `body` |
| unknown | `481` | INFO | `"NOTIFY for no subscription of ours; refused 481"` | `call_id`, `to_tag`, `event`, `subscription_state`, `current_call_id`, `current_local_tag` |
| current, no `Subscription-State` | `200` | WARN | `"NOTIFY without Subscription-State; treating as active"` | `call_id` |

## Subscription-State outcomes (current subscription)

| Message | Level | Fields |
|---|---|---|
| `"reg-event subscription confirmed"` | INFO | `call_id`, `expires_in_secs` |
| `"reg-event subscription terminated by the network"` | WARN | `call_id`, `reason`, `retry_after_secs`, `next` (`now` / `backoff` / `after-retry-after` / `next-registration`), `retry_in_secs` |
| `"reg-event subscription expired without a successful refresh; resubscribing"` | WARN | `call_id` |

## Removed

`"received response outside an active transaction"` is no longer reached by any SUBSCRIBE response. It stays for
any other unmatched response.
