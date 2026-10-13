# Contract: SIP body handling and precondition answer (wire behaviour)

The bridge's external interface here is SIP towards the carrier. This is the
observable contract the tests pin.

## Inbound INVITE

| Body | Response / effect |
|---|---|
| empty | offerless call (unchanged) |
| no `Content-Type`, non-empty | read as SDP (unchanged) |
| `application/sdp` (any params) | read as SDP (unchanged) |
| `multipart/*` with exactly one session SDP part (optional or unknown other parts) | read that part as SDP; call proceeds |
| `multipart/alternative` | last usable alternative |
| unknown top-level type | `415`, `Accept: application/sdp, multipart/mixed, multipart/alternative` |
| multipart, no session SDP part | `415`, same `Accept` |
| SDP part with non-identity `Content-Transfer-Encoding` | `415`, same `Accept` |
| multipart, bad/missing boundary, no close delimiter, depth > 4, > 16 parts at a level | `400 Bad Request` |
| multipart, more than one session SDP part | `400 Bad Request` |

Every refusal: sent after `100 Trying`, before any PBX contact, reported as a
failed call (`BridgeSetupFailed`), same To-tag as other responses.

## OPTIONS

`200 OK` carries `Accept: application/sdp, multipart/mixed, multipart/alternative`
(was `application/sdp`).

## Offerless ACK / outbound 2xx

Same decoding. Any refusal → offerless: `BYE` with reason
`offerless_ack_incompatible`; outbound: call fails through the existing error.

## MESSAGE

`application/vnd.3gpp.sms` with any parameters is decoded as binary SMS.
Multipart MESSAGE bodies are unchanged (415 as today).

## SDP answer QoS lines

For an offer with precondition lines:

- one `a=curr:qos local <inv dir>` per offer `des … remote <dir>` (own segment, met)
- `a=curr:qos remote <inv>` mirroring the offer's `a=curr:qos local`
- `a=curr:qos e2e <inv>` from the offer's `curr e2e` (fallback `curr local`), else `none`, for each offer `des … e2e`
- one `a=des:qos <strength> <inv type> <inv dir>` per offer `des` line
- never `a=conf:qos`

Golden (T2 offer → answer):

```text
offer:  a=curr:qos local sendrecv
        a=curr:qos remote none
        a=des:qos mandatory local sendrecv
        a=des:qos optional remote sendrecv
answer: a=curr:qos local sendrecv
        a=curr:qos remote sendrecv
        a=des:qos mandatory remote sendrecv
        a=des:qos optional local sendrecv
```

Offers with no precondition lines: answer byte-identical to v8.20.0.
`a=des:qos mandatory e2e …`: `580` (unchanged).

## Log line (per multipart or refused body)

`target=gsm_sip_bridge::ims::body` fields: `outer`, `selected`, `skipped`,
`outcome`. Never: part contents, Content-ID values, coordinates, numbers.
