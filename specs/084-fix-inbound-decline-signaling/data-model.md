# Data Model: Correct decline signaling on inbound IMS calls

## Telephone-side outcome (Agent B, `vowifi::PbxOutcome`)

| Variant | When | `BridgeFailed.reason` | Carrier response (Agent A) | `bridge_failed` metric |
|---|---|---|---|---|
| `Answered` | leg reached `Confirmed` | (sends `CallAnswered`) | `200 OK` | n/a |
| `Rejected { status }` | leg ended with 486, 600 or 603 | `pbx_rejected` | `486 Busy Here` | `pbx_declined` |
| `NoAnswer { status: None }` | Agent B's 45 s ring timer expired | `pbx_no_answer` | `480 Temporarily Unavailable` | `ring_timeout` |
| `NoAnswer { status: Some(480 or 408) }` | the telephone side's own no-answer timer | `pbx_no_answer` | `480` | `ring_timeout` |
| `Failed { status }` | any other final status, or `None` (unknown) | `pbx_failed` | `480` | `pbx_failed` |
| `CallerGone` | the caller cancelled (Agent A sent `CallEnded`) | `caller_cancelled` | (`487` already sent by Agent A) | `caller_cancelled` |

Classification is a pure function of `Option<u16>`:
`classify_pbx_disconnect(status) -> PbxOutcome`.

## Setup failure (Agent B, before `BridgeReady`)

| When | Reason | Carrier response | Metric |
|---|---|---|---|
| The PBX or veth leg can't be placed | `pbx_unreachable` | `480` (was `486`) | `bridge_setup_failed` (was `pbx_declined`) |

## Call record (`vowifi::control::CallRecord.outcome`)

`declined:<reason>`, followed by `:<status>` when a telephone-side status is
known. Examples: `declined:pbx_rejected:603`, `declined:pbx_no_answer`,
`declined:pbx_failed:404`. This stays a free-form string, as before.

## RecentDeclines (Agent A, `LoopState`)

| Field | Type | Rule |
|---|---|---|
| entries | `VecDeque<(String /*Call-ID*/, Instant)>` | at most 64; oldest evicted first |
| ttl | constant 32 s | expired entries pruned on insert and lookup |

- Written whenever an inbound INVITE ends without becoming the active call.
- Read only by `log_ack`. It never gates admission.

## To-tag lifetime (Agent A)

One tag per inbound INVITE, generated in `handle_inbound_invite`. It is used
by every response sent after the busy and PBX-down checks: the `180`, the
`200`, the `487`, every decline, and the `Err` fallback.
