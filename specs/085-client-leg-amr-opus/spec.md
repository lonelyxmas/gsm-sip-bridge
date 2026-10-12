# Feature Specification: AMR and Opus on the phone-facing leg

**Feature Branch**: `085-client-leg-amr-opus`  
**Created**: 2026-10-12  
**Status**: Draft  
**Input**: User description: "Offer AMR (AMR-WB and AMR-NB) and Opus on the PBX/phone-facing SIP leg of the VoWiFi and VoLTE bridges (issue #109, "Option 1"). Today the bridge decodes carrier AMR in Agent A, carries L16/16000 over the internal veth link, and Agent B's PJSIP re-encodes to G.722 or PCMU for the client — the PJSIP build is compiled without opencore-amr or Opus, so an AMR-capable client (baresip, ACR Phone) can never negotiate AMR. Goal: register AMR-WB, AMR-NB and Opus on the client leg and let operators control the client-leg codec preference order via config (default keeps G.722 first so existing PBX deployments behave identically). This still transcodes; the value is bandwidth for clients on mobile data. True passthrough is out of scope (spike #111). The circuit-switched GSM path is out of scope. Must keep per-call fallback to G.722/PCMU, and must work with the built-in [sip_server] registrar mode and with an external PBX."

## Background

On the VoWiFi and VoLTE paths a call's audio crosses two codec boundaries:

```
carrier ──AMR──▶ carrier-side agent ──16 kHz PCM──▶ telephone-side agent ──G.722 / PCMU──▶ PBX or phone
```

The telephone-side agent can only speak G.722 and PCMU (plus internal-only codecs) to the PBX or phone. A softphone that supports AMR or Opus, such as baresip or ACR Phone, therefore always ends up on G.722 at 64 kbit/s, even when it runs over a metered mobile connection where a ~12–24 kbit/s codec would do.

This feature lets that leg negotiate AMR-WB, AMR-NB or Opus. Audio is still decoded and re-encoded inside the bridge; carrying the carrier's AMR frames through untouched is a separate investigation (#111).

## Clarifications

### Session 2026-10-12

- Q: Are the new codecs available by default, or only when an operator turns them on? → A: On by default at the lowest priority. The bridge never offers them first, but a phone or PBX that lists them first may get them with no config change.
- Q: Is the codec preference set once for the whole deployment, or per line? → A: One list for the whole deployment; every line uses it.
- Q: What Opus settings should the bridge use on the phone-facing leg? → A: The SIP stack's Opus defaults (full-band advertisement, codec-chosen bitrate); no Opus tuning in this feature.
- Q: How should this be verified before it merges? → A: Automated tests for config and offer ordering, plus a manual live check with baresip on each new codec (inbound and outbound), recorded in the quickstart.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - AMR-capable softphone receives a cellular call in AMR (Priority: P1)

An operator runs the bridge in built-in registrar mode. Their softphone (AMR-WB capable, on mobile data) is registered as the ring target. Someone calls the SIM's number; the softphone rings, the user answers, and the call's audio between bridge and softphone is AMR-WB — using a fraction of the bandwidth G.722 would.

**Why this priority**: This is the request in #109 and the only reason the feature exists. On its own it delivers the bandwidth saving.

**Independent Test**: With the operator's preference set to put AMR-WB first, place an inbound call to a line whose ring target is an AMR-WB-capable softphone. Confirm the negotiated codec on the phone-facing leg is AMR-WB, that audio is intelligible in both directions for the full call, and that the phone-facing media bitrate is well below G.722's.

**Acceptance Scenarios**:

1. **Given** the operator prefers AMR-WB first and the softphone supports AMR-WB, **When** an inbound VoWiFi call is answered, **Then** the phone-facing leg uses AMR-WB and two-way audio works for the whole call.
2. **Given** the same setup over VoLTE, **When** an inbound VoLTE call is answered, **Then** the result is the same as over VoWiFi.
3. **Given** the softphone offers AMR-WB in the bandwidth-efficient packing only (or octet-aligned only), **When** a call is set up, **Then** the bridge uses the packing the softphone supports and audio works.
4. **Given** the softphone restricts the allowed AMR bitrates, **When** a call is set up, **Then** the bridge encodes only at a bitrate the softphone allows.

---

### User Story 2 - Existing deployments are unaffected unless the far end asks (Priority: P1)

An operator who changes nothing upgrades the bridge: same config, and an existing PBX or phone that does not support AMR or Opus (for example stock Asterisk). Every call negotiates exactly the codec it did before. The new codecs are available at the lowest priority, so only a far end that itself puts AMR or Opus first can end up on them.

**Why this priority**: The bridge's main users sit behind a PBX that has no AMR. Moving them onto a codec their PBX must transcode, or cannot handle, would be a regression on every call.

**Independent Test**: Run the existing PBX and built-in-registrar setups (no AMR or Opus on the far end) with an unchanged config, before and after the upgrade. Compare the negotiated phone-facing codec per call; they must match.

**Acceptance Scenarios**:

1. **Given** an unchanged configuration and a PBX supporting only G.722 and PCMU, **When** inbound and outbound calls are placed, **Then** the phone-facing leg uses G.722 as before.
2. **Given** an unchanged configuration and a PBX supporting only PCMU, **When** a call is placed, **Then** the phone-facing leg uses PCMU as before.
3. **Given** an unchanged configuration, **When** the bridge sends an offer on the phone-facing leg, **Then** G.722 and PCMU are listed ahead of AMR-WB, AMR-NB and Opus.
4. **Given** an unchanged configuration and a phone that offers Opus (or AMR-WB) first, **When** it dials out through the bridge, **Then** the bridge may accept that codec. This is expected, not a regression.
5. **Given** an unchanged configuration, **When** a call goes over the circuit-switched GSM path, **Then** its codec choice is exactly what it was before the upgrade.

---

### User Story 3 - Operator chooses the phone-facing codec order (Priority: P2)

An operator lists the phone-facing codecs they want, in order of preference (for example AMR-WB, Opus, G.722, PCMU). The bridge offers and accepts them in that order. A codec left off the list is never offered or accepted on that leg.

**Why this priority**: Without it, AMR or Opus only gets used when the remote end happens to prefer it. With it, an operator with a bandwidth-sensitive client can steer every call.

**Independent Test**: Set different preference orders and check, for a phone that supports all codecs, that the negotiated codec follows the first entry. Remove a codec from the list and check it is no longer offered.

**Acceptance Scenarios**:

1. **Given** a preference of Opus then G.722, **When** a phone supporting both answers a bridge-originated offer, **Then** Opus is negotiated.
2. **Given** a preference that omits G.722, **When** the bridge sends an offer, **Then** G.722 does not appear in it.
3. **Given** a preference that names a codec this build cannot provide, or an unknown name, **When** the bridge starts, **Then** it reports the problem clearly at startup rather than failing during a call.
4. **Given** a preference that omits PCMU, **When** the bridge starts, **Then** it refuses to start and explains that PCMU is required (see FR-004).

---

### User Story 4 - Phone dials out using AMR or Opus (Priority: P2)

A softphone registered to the bridge dials out via the SIM, offering AMR-WB or Opus first. The bridge accepts that codec on the phone-facing leg and the call proceeds normally.

**Why this priority**: Bandwidth matters in both directions. Outbound calls use the same leg, so the saving applies there too.

**Independent Test**: From an AMR-WB-only (or Opus-only) softphone, dial an outside number through the bridge. Confirm the call connects with two-way audio and that the phone-facing codec is the one the phone offered.

**Acceptance Scenarios**:

1. **Given** a softphone offering only AMR-WB, **When** it dials out through the bridge, **Then** the call connects and the phone-facing leg uses AMR-WB.
2. **Given** a softphone offering only Opus, **When** it dials out through the bridge, **Then** the call connects and the phone-facing leg uses Opus.

---

### Edge Cases

- **Remote supports none of the new codecs**: falls back to G.722 or PCMU (if allowed by the operator's list) with no failed call.
- **Operator's list and remote's offer share no codec**: the call is rejected the same way a codec mismatch is rejected today, and the reason is visible in logs.
- **Carrier leg is narrowband** (AMR-NB from the carrier) **while the phone-facing leg is AMR-WB or Opus**: the call works; the audio is narrowband in content, not in format.
- **Remote changes codec mid-call** (re-INVITE, for example hold/resume): the call keeps two-way audio on whichever allowed codec is renegotiated.
- **DTMF**: in-call keypad tones keep working on every new codec.
- **Comfort-noise or silence frames** from an AMR or Opus phone do not cause gaps, clicks or call teardown.
- **Internal link between the two agents**: adding new codecs must not change what that link negotiates, so the wideband path between the two agents is preserved.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: On the VoWiFi and VoLTE bridges, the phone-facing leg MUST be able to negotiate AMR-WB, AMR-NB and Opus, in addition to the codecs it supports today.
- **FR-002**: Operators MUST be able to set, in configuration, which codecs the phone-facing leg may use and in what order of preference. The setting is a single deployment-wide list that applies to every line; there are no per-line overrides.
- **FR-003**: With no new configuration, the phone-facing leg's preference order MUST be G.722, PCMU, then AMR-WB, AMR-NB and Opus at the lowest priority. The bridge MUST never list a new codec ahead of G.722 or PCMU in its own offers. A far end that supports none of the new codecs MUST negotiate exactly what it does today, with both an external PBX and the built-in registrar. A far end that lists a new codec first MAY be given it.
- **FR-004**: A codec excluded by the operator MUST NOT be offered or accepted on the phone-facing leg, with one exception: PCMU is mandatory. The list MUST include PCMU, because the internal link between the two agents falls back to it and codec settings are shared by every call in the process. A list without PCMU is invalid configuration (FR-010). *(Amended during planning, research.md R4.)*
- **FR-005**: Whenever the remote end does not support a preferred codec, the call MUST fall back to the next allowed codec both sides share, with no failed call or audio loss.
- **FR-006**: The bridge MUST accept both AMR packings (octet-aligned and bandwidth-efficient) on the phone-facing leg, using whichever the remote end requires.
- **FR-007**: When the remote end restricts the allowed AMR bitrates, the bridge MUST encode only at an allowed bitrate. With no restriction, it MUST use a standard voice bitrate: 12.65 kbit/s for AMR-WB, 12.2 kbit/s for AMR-NB.
- **FR-007a**: Opus MUST use the SIP stack's default Opus parameters (audio bandwidth advertisement and bitrate behaviour), with no bridge-specific tuning or operator setting.
- **FR-008**: This MUST work in both call directions (calls arriving from the carrier, and calls a phone places through the bridge) and with both the built-in registrar and an external PBX.
- **FR-009**: The codecs negotiated on the circuit-switched GSM path and on the internal link between the two agents MUST be unchanged by this feature.
- **FR-010**: Invalid codec configuration, such as an unknown name, a codec missing from the build, or an empty list, MUST stop the bridge at startup with an error naming the offending entry. This is consistent with the project's strict config parsing; there is no silent fallback to the default.
- **FR-011**: The codec negotiated on the phone-facing leg MUST be visible per call in the existing logs and call observability, so an operator can confirm what was used.
- **FR-012**: DTMF MUST keep working on every newly supported codec.
- **FR-013**: Documentation MUST explain the new setting, the bandwidth trade-off, that audio is still transcoded (no passthrough), and that a PBX in the path must itself support the chosen codec. The release notes MUST call out that, with no config change, a far end that prefers AMR or Opus can now negotiate it.
- **FR-014**: Before merge, verification MUST include: (a) automated tests covering config parsing and validation, and the codec order of the bridge's phone-facing offers (default and operator-set); and (b) a manual live call with baresip on AMR-WB, AMR-NB and Opus, both inbound and outbound, recorded in the feature's quickstart. Extending the automated SIP test tool to carry AMR or Opus audio is out of scope.

### Key Entities

- **Phone-facing codec preference**: one operator-set, deployment-wide ordered list of codec names that applies to the telephone-side agent's PBX/phone leg on every line. Default (setting absent): today's order for every existing codec, unchanged, with AMR-WB, AMR-NB and Opus appended at the lowest priority.
- **Negotiated phone-facing codec**: per-call, the codec actually agreed with the PBX or phone. It is recorded for observability.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With AMR-WB preferred and an AMR-WB softphone, the phone-facing media bandwidth of a call is at most 40% of the same call on G.722.
- **SC-002**: With an unchanged configuration, 100% of calls in the regression set negotiate the same codec as before the upgrade. The set covers an external PBX and the built-in registrar, inbound and outbound, with far ends that support neither AMR nor Opus, plus the GSM path.
- **SC-003**: Calls on AMR-WB, AMR-NB and Opus each sustain two-way intelligible audio for at least 5 minutes with no drop-outs attributable to the codec.
- **SC-004**: Mouth-to-ear latency added by the bridge on an AMR or Opus phone-facing leg is within 20 ms of the same call on G.722.
- **SC-005**: An operator can switch the phone-facing leg to AMR-WB by changing only the codec preference setting, without consulting anything beyond the configuration documentation.

## Assumptions

- The AMR codec libraries already shipped in the container image for the carrier-side agent can also be used by the telephone-side agent's SIP stack. Opus requires adding one library to the image; the size increase is acceptable.
- Codec patent and licensing exposure is unchanged for AMR, since the image already ships AMR. Opus is royalty-free.
- The preference setting is deployment-wide: one list applies to every line, because one telephone-side agent serves all lines. Per-line or per-target preferences are out of scope.
- Audio is decoded and re-encoded at the bridge, as today; this feature does not reduce the bridge's processing load or improve audio quality. True AMR passthrough is tracked separately in #111.
- Opus keeps the SIP stack's defaults, so its bitrate is not tuned for bandwidth. The bridge's audio is at most 16 kHz, so Opus may spend more bits than the audio needs. AMR-WB is the codec to use when bandwidth matters most, and Opus tuning can follow later if needed.
- The circuit-switched GSM path is out of scope: its audio comes from the modem as plain PCM.
- An external PBX in the path must itself support (or transcode) the chosen codec. Stock Asterisk does not support AMR, so the built-in registrar mode is the primary target for AMR. Opus is the more broadly PBX-compatible bandwidth option.
- Verification follows FR-014: automated checks for config and offer ordering, and manual live baresip calls for audio on each new codec. The live check uses the Vodafone test rig as the carrier line.
