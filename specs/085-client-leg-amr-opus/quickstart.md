# Quickstart: verifying AMR and Opus on the phone-facing leg

This is the manual live check FR-014(b) requires. Record the results in the table at the bottom before merging.

## Setup

- **Image**: a local build of this branch, run from `test/` (pinned tag, own config and volume; see the project's local docker test convention).
- **Carrier**: the Vodafone test rig, on VoWiFi and on VoLTE.
- **Phone**: baresip with the `amr` and `opus` modules loaded, registered to the bridge's built-in registrar (`[sip_server]` mode, `ring_aor` = the baresip AOR). Comment out `[sip].server`.
- **Placeholders only**: use `+919000000000` and similar in any notes you commit.

## 1. Default config (no `[sip].codecs`)

1. Start the bridge. In the telephone-side agent's startup log, `PJSIP codecs offered, in priority order` lists G722 first, PCMU above AMR-WB, AMR, opus, and L16 last.
2. Configure baresip to prefer PCMU/G722 (amr/opus loaded but ordered last). Call in. The phone leg must be on G722 (`SIP call media active — negotiated audio codec`).
3. Reorder baresip so `AMR-WB` comes first, then dial out from baresip to an outside number. The phone leg must be on AMR-WB. This is expected under clarification Q1.

## 2. `codecs = ["AMR-WB", "G722", "PCMU"]`

For each step, check two-way audio for at least 5 minutes, check the phone-leg codec in the log, and confirm `phone_codec` appears in `vowifi-status` recent calls.

1. Inbound VoWiFi call: the phone leg is on AMR-WB.
2. Inbound VoLTE call: the phone leg is on AMR-WB.
3. Outbound call from baresip: the phone leg is on AMR-WB.
4. Watch the phone-leg RTP rate (`tcpdump` in the privileged test container, or baresip's stats). It should be about 12.65 kbit/s of payload, against 64 kbit/s with G.722 (SC-001 requires at most 40% of G.722's bandwidth).
5. Press keypad digits during the call. The far end must receive DTMF.
6. Put the call on hold from baresip and resume it. Audio must return.

## 3. AMR packing variants

1. Set baresip's AMR to octet-aligned (`octet-align=1`). Repeat step 2.1.
2. Set baresip's AMR to bandwidth-efficient. Repeat step 2.1.
3. Restrict baresip's `mode-set` (for example to `0,1,2`). Confirm the bridge's encoded frames stay within it; the AMR frame type is visible in the capture.

## 4. Other codecs

1. Set `codecs = ["AMR-NB", "PCMU"]`, then call in and dial out. The phone leg is on AMR (NB).
2. Set `codecs = ["OPUS", "G722", "PCMU"]`, then call in and dial out. The phone leg is on opus.

## 5. Negative cases

1. Set `codecs = ["G722"]`. The bridge refuses to start, and the error mentions PCMU.
2. Set `codecs = ["G729", "PCMU"]`. The bridge refuses to start, and the error names `G729`.
3. Remove `amr` from baresip, set `codecs = ["AMR-WB", "G722", "PCMU"]`, and call in. The call connects on G722 (fallback, FR-005).

## 6. Regression: circuit-switched path

Place one GSM call with an unchanged config. The negotiated codec is the same as on the release before this branch.

## Results

| Check | VoWiFi | VoLTE | Notes |
|-------|--------|-------|-------|
| 1.2 default → G722 | | | |
| 1.3 phone prefers AMR-WB → AMR-WB | | | |
| 2.1–2.3 AMR-WB in/out | | | |
| 2.4 bitrate | | | |
| 2.5 DTMF | | | |
| 2.6 hold/resume | | | |
| 3.1–3.3 packing / mode-set | | | |
| 4.1 AMR-NB | | | |
| 4.2 Opus | | | |
| 5.1–5.3 negatives | | | |
| 6 GSM unchanged | | n/a | |
