# Phase 0 Research: Multipart SIP bodies and an RFC 3312-conformant precondition answer

Inputs: issue #103 (T2 Russia capture), the triage comment on it, and the
RFC text fetched directly (RFC 5621, RFC 2046 §5.1, RFC 3261 §§7.3.3, 8.2.3,
20.11, 20.15, 21.4.13, RFC 3312 §§4–8, 11, 13.2). Every claim below was checked
against the RFC text or the code, not recalled.

## Decision 1: Reverse specs/045 Decision 10 — parse multipart, don't decline it

**Decision**: Replace the INVITE `Content-Type` gate
(`invite_content_type_supported`, `agent/inbound.rs:124`) with a real MIME
body decoder that finds the session description inside multipart bodies.

**Rationale**: RFC 5621 §4.2: "SIP UAs MUST support parsing 'multipart' MIME
bodies, including nested body parts. Additionally, UAs MUST support the
'multipart/mixed' and 'multipart/alternative' MIME types." Its own example of
why is "the inclusion of location information in an INVITE request" — exactly
T2's body. specs/045 declined multipart on "no evidence any carrier this bridge
talks to sends multipart bodies"; #103 is that evidence. Verified offline: the
captured T2 SDP wrapped in the same multipart layout parses, through today's
`sdp::parse_offer` line scanner, to an offer identical to the bare SDP — so
these calls worked before v8.15.0 and the 415 is a regression.

**Alternatives considered**:
- *Revert SDP-05 (scan the whole body as SDP again)* — rejected: it restores
  T2 by accident and brings back the misparse SDP-05 fixed (a sibling part's
  lines read as SDP).
- *Accept `multipart/mixed` and pass the whole body to the scanner* — same
  objection.

## Decision 2: One shared decoder for every SDP consumer, in a new `ims::body` module

**Decision**: New module `gsm-sip-bridge/src/ims/body.rs`, used by the inbound
INVITE, the offerless-INVITE ACK (`agent/inbound.rs` ≈877–957), and the
outbound INVITE's 2xx (`ims/call.rs:287`). `agent/veth.rs:87` is not changed:
its peer is the bridge's own PJSIP, which always sends bare SDP.

**Rationale**: RFC 5621 §4.2: "SIP extensions can also include 'multipart'
MIME bodies in responses. That is why both UACs and UASs need to support
'multipart' bodies." Three call sites with the same rules is the case for one
function (constitution V's complexity justification: it removes a third ad-hoc
copy rather than adding a layer). `sdp.rs` stays an SDP parser; MIME is a
different grammar.

**Alternatives considered**: putting splitting inside `sdp::parse_offer` —
rejected, it would need the headers too and mixes two grammars.

## Decision 3: Hand-written MIME splitter, no new crate

**Decision**: Implement RFC 2046 §5.1.1 splitting directly (~200 lines with
tests), no dependency.

**Rationale**: The required subset is small (boundary split, part headers,
two Content-* headers, one parameter). Mail-oriented crates bring
Content-Transfer-Encoding/charset machinery this feature excludes, don't know
SIP's disposition/handling defaults (RFC 3261 §20.11), and every new crate
goes through `deny.toml`. Fewer moving parts (constitution V).

**Alternatives considered**: `mailparse`, `mime` crates — rejected for the
reasons above; `mime` alone only parses the media type, which is ~30 lines.

## Decision 4: Framing rules

**Decision**:
- Work on `body_bytes` (raw bytes); convert only the selected SDP part to text.
- Delimiter = `--` + boundary at the start of a line; the CRLF (or bare LF)
  before it belongs to the delimiter and is stripped from the preceding part
  (RFC 2046 §5.1.1). Trailing whitespace after the boundary on the delimiter
  line is allowed.
- First delimiter may be at offset 0. Text before it (preamble) and after the
  close delimiter (epilogue) is ignored.
- Close delimiter `--boundary--` is required; its absence is `Malformed`.
- Boundary must be 1–70 characters (RFC 2046 BNF); otherwise `Malformed`.
- Part headers end at the first blank line; names case-insensitive; folded
  continuation lines joined. SIP compact forms do **not** apply inside MIME
  parts (RFC 3261 §7.3.3 governs SIP message headers only).
- Per-part `Content-Length` is ignored (it is not a MIME header; T2 sends it).
- Limits: nesting depth 4, 16 parts per multipart level → `Malformed` beyond.

**Rationale**: RFC 2046 grammar; limits bound work on a body the carrier
controls. Real IMS bodies are 2 parts, depth 1.

## Decision 5: Part selection and the lenient-unknown-part rule (Clarification Q1)

**Decision**:
- Disposition defaults (RFC 3261 §20.11): `session` for `application/sdp`,
  `render` otherwise; `handling` defaults to `required`.
- `multipart/mixed`, `multipart/related`, unknown `multipart/*` (RFC 5621 §4.2
  note: treated as mixed): candidates are `application/sdp` parts with
  disposition `session`, found recursively. Exactly one → use it; more than
  one → `Ambiguous`; none → `NoSession`.
- `multipart/alternative`: walk parts last-to-first and take the first one that
  yields a session description (RFC 2046 §5.1.4: last is the preferred
  representation); inner `handling` ignored (RFC 5621 §8.3).
- Unknown parts are skipped whatever their `handling`; `optional` is logged at
  info, `required`/defaulted at warn with the content type. **Deviation** from
  RFC 5621 §8.4 (SHOULD 415) by clarification: carriers attach 3GPP XML parts
  and a strict rule repeats the #103 outage for any missing
  `handling=optional`.
- SDP part with `Content-Transfer-Encoding` other than `7bit`/`8bit`/`binary`
  (case-insensitive) → `Unsupported` (RFC 5621 §3.2: SIP SHOULD use binary).
- No Content-ID, Geolocation or PIDF interpretation (RFC 5621 §9.3 permits
  ignoring an optional part it doesn't understand; RFC 6442 out of scope).

## Decision 6: Outcomes and responses

**Decision**:

| Outcome | INVITE response | ACK (offerless) | Outbound 2xx |
|---|---|---|---|
| `Empty` | offerless path (unchanged) | existing "no answer" teardown | existing parse error |
| `Sdp(text)` | `parse_offer(text)` | `parse_answer(text)` | `parse_answer(text)` |
| `Unsupported{..}` / `NoSession` | `415` + `Accept: application/sdp, multipart/mixed, multipart/alternative` | `teardown("offerless_ack_incompatible")` | `BridgeError::Ims` |
| `Malformed{..}` / `Ambiguous` | `400 Bad Request` | same teardown | `BridgeError::Ims` |

`400` for malformed bodies follows RFC 3261 §21.4.1 ("malformed syntax");
`415` + `Accept` follows RFC 3261 §21.4.13 / RFC 5621 §8.4. A body with no
`Content-Type` is still treated as SDP (existing tolerance, kept deliberately
despite RFC 3261 §20.15).

The same `Accept` value is used in the `OPTIONS` 200
(`agent/mod.rs:453`, today `application/sdp`), since RFC 3261 §11.2 says the
OPTIONS answer lists what the UAS accepts. Widening a capability header on
Jio is checked live (memory: Jio enforces capability headers it never
exercises) — see quickstart.

**Alternatives considered**: `488` for ambiguity — rejected; 488 is about
session parameters, not message syntax.

## Decision 7: Test seam

`handle_invite` has no unit-test seam (it needs a live control channel and
veth listener; specs/047 quickstart). So the body decision is a pure function
`invite_body_decision(&SipRequest, to_tag) -> InviteBody` in
`agent/inbound.rs` returning either the SDP to parse, the offerless marker, or
the complete response text to send — and `handle_invite` only acts on it.
Tests drive it with real parsed `SipRequest`s (no mocks, constitution I).

## Decision 8: Precondition answer (RFC 3312)

**Decision** (changes to `sdp::precondition_verdict`/`build_answer_for`):
1. **Desired status**: for each offer `a=des:qos <strength> <type> <dir>` emit
   `a=des:qos <strength> <inv type> <inv dir>` (Table 4: local↔remote,
   send↔recv; `e2e`/`sendrecv`/`none` unchanged). Strength never downgraded
   (§5.2); unknown strength lines are already dropped by the parser.
2. **No `a=conf:qos`** for the bridge's own segment. §6: conf is needed only
   by a UAS "not capable of unilaterally meeting all of the mandatory
   preconditions"; the bridge's segment is always met. §7: receiving conf
   obliges the carrier to send a new offer (UPDATE) once met — it already is,
   so the carrier would send one at once and get `405` from
   `handle_carrier_update` (`agent/mod.rs:2255`).
3. **Segment current status** unchanged: own segment = met in the requested
   direction (inverted); offerer's = its own `curr local`, mirrored and
   inverted.
4. **e2e current status** (Clarification Q3): from the offer's
   `a=curr:qos e2e` (fallback: `a=curr:qos local`), inverted; `none` when
   neither is present. Replaces today's unconditional "met".
5. **Offerer's mandatory segment unmet** (Clarification Q2): proceed; one info
   log from `handle_invite` naming segment and direction, via a new pure
   `sdp::offerer_unmet_mandatory(&SdpOffer)`.

Expected T2 answer (matches RFC 3312 §13.2 SDP2's shape):
`a=curr:qos local sendrecv`, `a=curr:qos remote sendrecv`,
`a=des:qos mandatory remote sendrecv`, `a=des:qos optional local sendrecv`.

Emission order: `curr` lines then `des` lines, in offer order — matches the
RFC's examples; order is not significant to receivers.

**Correction recorded**: specs/048 research Decision 4 quotes "RFC 3312 §6.2
… MUST reject … with 580". RFC 3312 has no §6.2; the 580 text is §8 and is a
SHOULD. No behaviour change follows from this.

## Decision 9: MESSAGE media type

`decode_pdu_body` (`agent/mod.rs:1071`) compares the whole header with
`application/vnd.3gpp.sms`, while `message_content_type_supported` strips
parameters. Both switch to `body::MediaType::parse` so a parameterised SMS
type is decoded as binary. No multipart for MESSAGE (out of scope).

## Decision 10: Diagnostics

One `tracing` line per non-trivial decode (multipart or refusal), fields:
`outer`, `selected` (part type or none), `skipped` (content types of skipped
parts), `outcome`/`reason`. Never part contents, Content-ID values,
coordinates, or identities. No new metric (clarify: low impact; the existing
failed-call report covers counting refusals).
