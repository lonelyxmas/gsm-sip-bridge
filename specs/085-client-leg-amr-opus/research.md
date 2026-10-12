# Research: AMR and Opus on the phone-facing leg

Facts about pjproject were checked against the **2.16** sources (the version pinned by `PJSIP_VERSION` in `docker/Dockerfile`):
- `pjmedia/src/pjmedia-codec/opencore_amr.c`
- `pjmedia/src/pjmedia-codec/opus.c`
- `pjmedia/src/pjmedia/sdp_neg.c`
- `aconfigure.ac`

## R1. Getting AMR and Opus into the PJSIP build

**Decision**: In the `pjsip-builder` stage:
- install `opencore-amr-dev`, `vo-amrwbenc-dev` and `opus-dev`;
- pass `--with-opencore-amr=/usr --with-opencore-amrwbenc=/usr --with-opus=/usr` to `./configure`;
- after configure, fail the build unless `pjmedia/include/pjmedia-codec/config_auto.h` defines `PJMEDIA_HAS_OPENCORE_AMRNB_CODEC`, `PJMEDIA_HAS_OPENCORE_AMRWB_CODEC` and `PJMEDIA_HAS_OPUS_CODEC` as 1.

The runtime stage adds `opus`; `opencore-amr` and `vo-amrwbenc` are already there for Agent A.

**Rationale**: `aconfigure.ac` autodetects all three when headers are present. When they are missing, autodetection fails *silently* and produces a build without the codecs, which would only surface at runtime. Explicit `--with-*` flags plus the `config_auto.h` assertion make a missing dependency break the image build instead.

**Alternatives considered**: Relying on autodetection alone was rejected for the silent-failure reason above. Vendoring the codec libraries was rejected because Alpine packages already exist and the runtime image ships two of them.

## R2. Linking

**Decision**: Add `opencore-amrnb`, `opencore-amrwb`, `vo-amrwbenc` and `opus` to the force-dynamic `system_libs` set in `pjsua-sys/build.rs`.

**Rationale**: With `PJSUA_SYS_STATIC=1`, every library that `libpjproject.pc` lists and that isn't in `system_libs` is linked statically. These four are Alpine shared libraries already present at runtime (`amr-sys` links the opencore ones dynamically today). Linking them dynamically avoids depending on `.a` files the `-dev` packages may not ship, and avoids two copies of opencore in one binary.

**Alternatives considered**: Static linking was rejected for the reasons above, with nothing gained.

## R3. Default priorities, and why every new codec must be pinned explicitly

**Facts**:
- `pjsua_codec_set_priority` is process-global (`pjsua-safe/src/endpoint.rs`).
- Codecs registered by PJSIP start at a normal priority, well above 1.
- The live Agent B offer already lists speex/16000, speex/8000, iLBC and GSM ahead of PCMU (`ims/sdp.rs` test fixture `PJSIP_REAL_VETH_OFFER`).
- `PJMEDIA_SDP_NEG_PREFER_REMOTE_CODEC_ORDER` defaults to on (`sdp_neg.c:174`), so when answering an offer PJSIP follows the *remote* end's order among the codecs it has enabled.

**Consequence**: Simply compiling AMR and Opus in would make them outrank PCMU in every offer, both on Agent B and on the circuit-switched path, whose endpoint (`sip/mod.rs`) sets no priorities at all.

**Decision**:
- **Telephony side** (`vowifi::run_telephony_side`, shared by VoWiFi and VoLTE): when `[sip].codecs` is absent, keep today's handling of every existing codec and pin AMR-WB=4, AMR-NB=3, Opus=2. That is below every existing codec and above L16=1, which only matters internally.
- **Circuit-switched path** (`sip/mod.rs` endpoint creation): set AMR, AMR-WB and Opus to priority 0. The GSM path is then byte-for-byte unchanged (FR-009).

**Alternatives considered**: Leaving the GSM path alone was rejected because it violates FR-009.

## R4. Why PCMU cannot be removed by configuration (spec amendment)

**Facts**:
- Agent B places both of its calls (to the PBX and over the veth link) from the same endpoint, and the same account (`Account::local` or `Account::register` in `vowifi/mod.rs`), so they share one codec table.
- Agent A answers the veth offer with L16 or PCMU *by name* (`sdp::select_veth_codec`). PCMU is the only option on narrowband calls, and on any call when `[vowifi].wideband = false`.

**Decision**: PCMU is mandatory in `[sip].codecs`. A list without it is rejected at startup. The spec's FR-004 and User Story 3 were amended accordingly.

**Alternatives considered**: Per-leg SDP filtering through pjsua's `on_call_sdp_created` callback, keyed by the call's remote URI (veth versus PBX). This would let an operator drop PCMU from the PBX leg only. It was rejected for v1 for several reasons:
- it adds a new unsafe FFI callback that edits SDP media and attribute arrays;
- an outgoing call's id isn't known until `pjsua_call_make_call` returns, so the callback would have to key on the remote URI;
- dropping PCMU has no practical value: it is the universal fallback, and FR-005 depends on it.

It can be revisited if someone asks for it (Constitution V, YAGNI).

## R5. What an operator-set list does to the other codecs

**Decision**: When `[sip].codecs` is set:
- each listed codec gets a descending priority (first entry 200, then 199, …);
- every *other* codec PJSIP has (speex, iLBC, GSM, PCMA if unlisted, G.722 if unlisted, and so on) is set to 0;
- L16/16000 stays at 1 because the veth link needs it.

This is the same internal-codec exception that exists today: L16 already appears in PBX-leg offers at priority 1.

**Rationale**: FR-004 requires unlisted codecs to be neither offered nor accepted. Disabling them in the shared table is the only lever that needs no SDP surgery.

## R6. AMR negotiation parameters (FR-006, FR-007)

**Facts** (from `opencore_amr.c`):
- When the remote fmtp is parsed (`:574-661`), PJSIP adopts the remote's `octet-align`.
- For `mode-set`, it uses the local default mode if the set contains it, otherwise the highest mode in the set below it (comment at `:609`).
- The default encoder modes are 12.2 kbit/s (NB) and 12.65 kbit/s (WB), which matches FR-007.
- Our own offer advertises the default config: bandwidth-efficient, with no `octet-align` parameter.

**Decision**: Use PJSIP's AMR defaults, so no `pjmedia_codec_opencore_amr*_set_config` call is needed.

**Risk**: A phone that accepts *only* octet-aligned AMR and *answers* our bandwidth-efficient offer may reject it. In that case the call falls back to G.722 or PCMU (FR-005). The quickstart's live baresip check covers both packings.

## R7. Opus (FR-007a)

**Decision**: Use PJSIP's Opus defaults (`PJMEDIA_CODEC_OPUS_DEFAULT_*`), with no `pjmedia_codec_opus_set_default_param` call. This is the clarification answer to Q3.

PJSIP's codec id is `opus/48000/2`. The conference bridge (16 or 8 kHz) resamples, as it does for G.722 today.

## R8. Observability (FR-011)

**Facts**: `on_call_media_state_cb` (`pjsua-safe/src/endpoint.rs:675`) already logs `codec`, `sample_rate` and `channels` for every call whose media goes active, on both legs.

**Decision**:
- Reuse that log line.
- Add `phone_codec` to the per-call record Agent B keeps (`vowifi::control::CallRecord`), so `vowifi-status` and recent-call history show it without grepping logs.
- Do not add a metric label: codec as a label is bounded, but there is no consumer for it yet (YAGNI).

## R9. Where the setting lives

**Decision**: Add `[sip].codecs`, an optional array of strings. Accepted names, case-insensitive, are `G722`, `PCMU`, `PCMA`, `AMR-WB`, `AMR-NB` and `OPUS`; each maps to a PJSIP codec id (see data-model.md). The circuit-switched path ignores the setting; the docs and the key's doc comment say so.

**Rationale**:
- `[sip]` is where operators already describe the PBX/phone side.
- The telephone-side agent reads `config.sip` already.
- One key serves both VoWiFi and VoLTE, which share `run_telephony_side`.
- Strict parsing (`deny_unknown_fields`) means the new key must exist in `raw.rs`, and `test_config_docs.rs` forces it into `docs/configuration.md`.

**Alternatives considered**:
- `[vowifi]`, next to `wideband`: rejected because VoLTE reads `wideband` as a hard-coded `true` and doesn't consult `[vowifi]` for telephone-side behaviour.
- `[audio]`: rejected because that section is about sound devices and jitter, not signalling.

## R10. Size of the veth INVITE

**Facts**: Agent A reads Agent B's veth INVITE as one datagram into a 4096-byte buffer (`agent/veth.rs`).

**Decision**: Explicitly measure the offer size in the integration test of the default codec set. Two AMR rtpmap/fmtp pairs plus Opus add about 150 bytes, against a current offer of roughly 900 bytes. With an operator list, the offer shrinks.

## R11. Verification approach (FR-014)

**Decision**:
- **Automated tests** cover config parsing and validation and the priority plan, as a pure function from the config to `(codec_id, priority)` pairs that is unit- and integration-testable without PJSIP.
- **pjsip-linked tests** (in the privileged test container, per the project's existing practice) check the emitted offer order.
- **Manual baresip calls** go into `quickstart.md`.
- `siptest` gets no AMR or Opus media support.
