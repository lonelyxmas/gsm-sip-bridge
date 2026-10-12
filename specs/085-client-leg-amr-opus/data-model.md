# Data Model: AMR and Opus on the phone-facing leg

There is no persistent storage. The entities below are configuration and per-call values held in memory.

## PhoneCodec (enum)

These are the codecs an operator may name in `[sip].codecs`. Names are parsed case-insensitively, and each variant maps to exactly one PJSIP codec id.

| Config name | Variant  | PJSIP codec id   | Notes |
|-------------|----------|------------------|-------|
| `G722`      | `G722`   | `G722/16000/1`   | today's wideband default |
| `PCMU`      | `Pcmu`   | `PCMU/8000/1`    | **mandatory** (research R4) |
| `PCMA`      | `Pcma`   | `PCMA/8000/1`    | |
| `AMR-WB`    | `AmrWb`  | `AMR-WB/16000/1` | new; needs opencore-amrwb + vo-amrwbenc in the PJSIP build |
| `AMR-NB`    | `AmrNb`  | `AMR/8000/1`     | new; PJSIP names AMR-NB plain `AMR` |
| `OPUS`      | `Opus`   | `opus/48000/2`   | new; needs libopus in the PJSIP build |

## PhoneCodecPreference (config value)

`[sip].codecs: Option<Vec<PhoneCodec>>`, deployment-wide (clarification Q2).

Validation happens at config load and is fatal (FR-010):
- An unknown name is rejected with an error naming the entry and listing the accepted names.
- An empty list is rejected.
- A duplicate entry is rejected.
- A list without `PCMU` is rejected, with the message saying why (the internal agent link depends on it).

One check runs at endpoint start, after PJSIP is up, also fatal:
- A listed codec whose PJSIP id is absent from `Endpoint::codecs()` (a build without it) is rejected with an error naming the codec.

## CodecPriorityPlan (derived, pure)

This is a function from `(Option<&[PhoneCodec]>, wideband: bool)` to an ordered list of `(codec_id_or_prefix, priority)` assignments, applied with `Endpoint::set_codec_priority`.

| Input | Assignments |
|-------|-------------|
| `None`, wideband | `G722/16000/1`=200, `L16/16000/1`=1, `AMR-WB/16000/1`=4, `AMR/8000/1`=3, `opus/48000/2`=2 (today's two, plus the three new codecs pinned low) |
| `None`, narrowband | `AMR-WB/16000/1`=4, `AMR/8000/1`=3, `opus/48000/2`=2 (existing codecs untouched, as today) |
| `Some(list)`, either | every registered codec=0 first, then `L16/16000/1`=1, then list[i]=200−i |

The circuit-switched path uses a separate fixed plan: `AMR`=0, `AMR-WB`=0, `opus`=0 (by prefix), with nothing else touched.

Keeping this as a pure function means it can be tested exhaustively without PJSIP. Only the thin "apply" step needs the `pjsip-linked` feature.

## CallRecord.phone_codec (per call)

`Option<String>` holds the negotiated codec name of the PBX/phone leg, as reported by the media-active callback (for example `"AMR-WB"`). It is `None` until media goes active, or for a call that never reached it. It is surfaced in `vowifi-status` recent calls (FR-011).
