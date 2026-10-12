# Contract: `[sip].codecs`

```toml
[sip]
# Optional. Which codecs the bridge may use toward the PBX or phone on
# VoWiFi/VoLTE calls, most preferred first. Circuit-switched (GSM) calls
# ignore this setting.
#
# Leave unset to keep the existing order (G.722 first, then PCMU), with
# AMR-WB, AMR-NB and Opus available at the lowest priority: a phone or PBX
# that prefers them can still pick them.
#
# Accepted: G722, PCMU, PCMA, AMR-WB, AMR-NB, OPUS (case-insensitive).
# PCMU is required: the bridge's internal link falls back to it.
# Audio is still decoded and re-encoded by the bridge; this saves bandwidth
# toward the phone, it does not pass carrier audio through untouched.
# codecs = ["AMR-WB", "OPUS", "G722", "PCMU"]
```

| Condition | Behaviour |
|-----------|-----------|
| key absent | default plan (data-model.md, `None` rows) |
| valid list | listed codecs offered and accepted in this order; all others disabled except the internal L16 |
| unknown name / empty / duplicate / no PCMU | startup fails: `configuration error: [sip].codecs: <reason>` |
| codec not in this PJSIP build | telephone-side agent fails to start: `[sip].codecs names <X>, which this build does not include` |

Error messages name the key and the offending entry. That is a contract: tests assert on the substring.
