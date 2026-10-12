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

## Automated / container checks

Run on the Vodafone rig (host network) against `gsm-sip-bridge:amr-opus-local`, 2026-10-12, with a Debian baresip (amr, opus, g722 modules) in Docker as the phone:

- Default config, startup log: `G722=200, speex 130/129, iLBC/GSM/PCMU/PCMA/speex32k=128, AMR-WB=4, AMR=3, opus=2, L16=1`. The existing codecs keep today's order; the new ones sit below them.
- `codecs = ["AMR-WB","G722","PCMU"]`, startup log: `AMR-WB=200, G722=199, PCMU=198, L16=1`; nothing else enabled.
- Captured inbound INVITE to the phone with that setting: `m=audio ... 96 9 0 97 120 121` = AMR-WB, G722, PCMU, L16 (internal), telephone-event. No speex, iLBC, GSM or opus. The AMR-WB line carries **no fmtp** (bandwidth-efficient).

## Findings

- **Pre-fix result:** inbound AMR to an octet-align-only phone fell back to G722, because the bridge offered bandwidth-efficient AMR (the no-`fmtp` capture above is from before this fix). Outbound AMR worked because PJSIP adopts the phone's `octet-align=1`. The `[sip].amr_octet_align` default (`true`) fixes the inbound case; the Results table below records successful inbound and outbound calls.

## Results

| Check | VoWiFi | VoLTE | Notes |
|-------|--------|-------|-------|
| 1.2 default → G722 | PASS (phone_codec=G722) | not run | siptest as phone |
| 1.3 phone prefers AMR-WB → AMR-WB | | | |
| 2.1–2.3 AMR-WB in/out | PASS (in + out) | not run | octet-aligned offer (`[sip].amr_octet_align` default); inbound with `codecs=[AMR-WB,G722,PCMU]` |
| 2.4 bitrate | PASS | not run | bridge→phone RTP 46 B/20 ms (34 B AMR-WB octet-aligned) = 13.6 kbit/s payload vs 64 for G.722 (21%) |
| 2.5 DTMF | sent, receipt unverified | not run | baresip sent digits 1-5; bridge logs nothing for DTMF and the carrier side is not observable from the rig |
| 2.6 hold/resume | PARTIAL | not run | baresip stats: audio 0 bit/s on hold, full rate after resume; baresip returned 'Invalid argument' to the resume command |
| 3.1–3.3 packing / mode-set | 3.1 PASS | not run | octet-align=1 negotiated with baresip; bandwidth-efficient and mode-set not tested (baresip cannot) |
| 4.1 AMR-NB | PASS (in + out) | not run | `codecs=[AMR-NB,PCMU]`, phone leg `AMR` 8 kHz |
| 4.2 Opus | PASS (in + out) | not run | `codecs=[OPUS,PCMU]`, phone leg `opus` 48 kHz stereo-flagged |
| 5.1–5.3 negatives | 5.3 PASS | not run | siptest (no AMR) + codecs=[AMR-WB,G722,PCMU] connected on G722; 5.1/5.2 covered by unit tests |
| 6 GSM unchanged | not run | n/a | `[cs]` is disabled on this rig; only unit-tested |
