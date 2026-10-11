# Contract: inbound decline signaling

## 1. Carrier-facing (Gm), final responses to an inbound INVITE

| Situation | Response | To-tag |
|---|---|---|
| The telephone side declines (486, 600 or 603) | `486 Busy Here` | the `180`'s |
| No answer (the bridge's timer, or a telephone-side 480 or 408) | `480 Temporarily Unavailable` | the `180`'s |
| Other telephone-side failure, or unknown | `480 Temporarily Unavailable` | the `180`'s |
| Telephone-side leg can't be placed during setup | `480 Temporarily Unavailable` | the `180`'s |
| Unexpected internal error after `180` | `480 Temporarily Unavailable` | the `180`'s |
| The caller cancelled | `487 Request Terminated` | the `180`'s (unchanged) |
| Another call is active | `486 Busy Here` | fresh (unchanged) |
| PBX registration down | `480` | fresh (unchanged) |
| `Require` names an unsupported extension; bad body; SAVP; e2e precondition | `420` / `415` / `488` / `580` | the call's tag (value only; no `1xx` sent yet) |

## 2. Agent B → Agent A control (`ControlMessage::BridgeFailed.reason`)

The wire shape is unchanged: `{"type":"bridge_failed","call_id":…,"reason":…}`.
Reason vocabulary:

- `pbx_rejected`: only for a telephone-side 486, 600 or 603.
- `pbx_no_answer`: Agent B's ring timer, or a telephone-side 480 or 408.
- `pbx_failed` (**new**): any other end of the telephone-side leg before
  answer.
- `pbx_unreachable`: setup failure (unchanged).

An Agent A that receives an unknown reason MUST answer `480`.

## 3. Metrics (`bridge_failed{reason}`)

Adds the label value `pbx_failed`. `pbx_declined` now counts only
`pbx_rejected`. Every no-answer counts as `ring_timeout`. A setup-time
`pbx_unreachable` counts as `bridge_setup_failed`.
