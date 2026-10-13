# Data Model: Multipart SIP bodies

All types are in-memory, per message; nothing is persisted.

## `body::MediaType`

| Field | Type | Notes |
|---|---|---|
| `type_` | `String` | lowercased |
| `subtype` | `String` | lowercased |
| `params` | `Vec<(String, String)>` | names lowercased; values unquoted (quoted-string escapes resolved) |

- `MediaType::parse(&str) -> Option<MediaType>` — `None` when there is no
  `type/subtype`.
- `is(&self, "application/sdp") -> bool`, `param(&self, "boundary") -> Option<&str>`,
  `is_multipart() -> bool`.

## `body::Disposition`

| Field | Type | Notes |
|---|---|---|
| `kind` | `String` | lowercased; default `session` for `application/sdp`, `render` otherwise |
| `handling` | `Handling` (`Required` \| `Optional`) | default `Required`; unknown values treated as `Required` |

## `body::Part<'a>` (internal)

| Field | Type | Notes |
|---|---|---|
| `media_type` | `Option<MediaType>` | absent `Content-Type` in a part = `text/plain` (RFC 2045 §5.2); never SDP |
| `disposition` | `Disposition` | |
| `transfer_encoding` | `Option<String>` | lowercased |
| `content` | `&'a [u8]` | delimiter CRLF stripped |

## `body::Decoded`

```text
Decoded::Empty                         body blank after trim
Decoded::Sdp { text: String, info }    exactly one session description
```

`info: DecodeInfo { outer: String, selected: String, skipped: Vec<String> }` —
for the diagnostic log line; `skipped` holds content types only.

## `body::Rejection`

```text
Rejection::Unsupported { outer, offending: String }   415
Rejection::NoSession   { outer, skipped }             415
Rejection::Malformed   { outer, reason: &'static str } 400
Rejection::Ambiguous   { outer, count: usize }        400
```

`Rejection::status() -> u16` gives the SIP status from the table above.

## Entry point

`body::extract_sdp(content_type: Option<&str>, content_disposition: Option<&str>, body: &[u8]) -> Result<Decoded, Rejection>`

Validation rules (from spec FR-002…FR-011): see research.md Decisions 4–6.

## `agent::inbound::InviteBody` (decision for `handle_invite`)

```text
InviteBody::Offerless
InviteBody::Offer(String)               SDP text for sdp::parse_offer
InviteBody::Refuse { response: String, reason } full 415/400 text to send
```

## Precondition answer (changes in `ims::sdp`)

- `QosAnswerLine` loses `confirm` (no `a=conf:qos` is ever emitted).
- `PreconditionVerdict::Proceed` carries `current: Vec<QosAnswerLine>` and
  `desired: Vec<QosDesired>` (answer-relative, already inverted).
- New `pub fn offerer_unmet_mandatory(&SdpOffer) -> Vec<(QosStatusType, QosDirection)>`
  — offer `des` lines at `mandatory` strength on the offerer's own (`local`)
  segment whose `curr local` does not cover the direction.

State transitions: none — every decision is per message.
