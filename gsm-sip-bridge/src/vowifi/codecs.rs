//! Phone-facing codec priorities for the telephone-side agent (spec 085, #109).
//!
//! Priorities are endpoint-global in PJSIP, and the endpoint places both of its
//! calls (PBX/phone leg and the veth link to Agent A) from one codec table, so
//! this module only ever shapes that single table.

use crate::config::PhoneCodec;
use crate::error::{BridgeError, BridgeResult};
use pjsua_safe::Endpoint;

/// PJSIP's G.722 codec id. Wideband (16 kHz internally, whatever RFC 3551's
/// historical `G722/8000` rtpmap says), built into pjproject with no external
/// library, and understood by every mainstream PBX without an extra module —
/// which is why it, rather than Opus, is what the PBX leg reaches for by
/// default.
const G722_CODEC_ID: &str = "G722/16000/1";
/// PJSIP's 16 kHz linear-PCM codec id — uncompressed, and used only on the
/// veth link to Agent A (see `ims::sdp::NegotiatedCodec::L16`). Its low
/// priority only keeps it out of the PBX's way; Agent A picks it by name.
const L16_16K_CODEC_ID: &str = "L16/16000/1";

/// Priorities for the three codecs spec 085 adds when no list is configured.
/// Newly compiled-in codecs would otherwise default *above* PCMU, silently
/// changing every deployment's offer (research R3); these sit below every
/// pre-existing codec and above L16, so a far end that prefers them can still
/// pick them but nobody gets them by accident.
const DEFAULT_NEW_CODECS: [(&str, u8); 3] = [
    (PhoneCodec::AmrWb.pjsip_id_const(), 4),
    (PhoneCodec::AmrNb.pjsip_id_const(), 3),
    (PhoneCodec::Opus.pjsip_id_const(), 2),
];

/// Turn the configured preference into `(codec id, priority)` assignments, in
/// the order they must be applied. `registered` is every codec id the PJSIP
/// build has (only consulted when an explicit list is set).
///
/// Absent list: today's behaviour for existing codecs (G.722 at 200 when
/// wideband, L16 at 1) plus the three new codecs pinned low; PCMU is never
/// touched. Explicit list: everything registered is disabled first, L16 stays
/// at 1 for the internal link, then list entry `i` gets `200 - i`.
pub(crate) fn priority_plan(
    codecs: Option<&[PhoneCodec]>,
    wideband: bool,
    registered: &[String],
) -> Vec<(String, u8)> {
    let mut plan: Vec<(String, u8)> = Vec::new();
    match codecs {
        None => {
            if wideband {
                plan.push((G722_CODEC_ID.to_string(), 200));
                plan.push((L16_16K_CODEC_ID.to_string(), 1));
            }
            for (id, prio) in DEFAULT_NEW_CODECS {
                plan.push((id.to_string(), prio));
            }
        }
        Some(list) => {
            for id in registered {
                plan.push((id.clone(), 0));
            }
            plan.push((L16_16K_CODEC_ID.to_string(), 1));
            for (i, codec) in list.iter().enumerate() {
                plan.push((codec.pjsip_id().to_string(), 200u8.saturating_sub(i as u8)));
            }
        }
    }
    plan
}

/// Check every codec an operator listed against what this PJSIP registered.
///
/// An empty table is only acceptable in a build without `pjsip-linked`, which
/// has no codec table at all. In a linked build it means the native read failed
/// (`Endpoint::codecs()` returns an empty list on error), and passing the list
/// unchecked would let a codec the build lacks through silently.
fn validate_listed(list: &[PhoneCodec], registered: &[String], linked: bool) -> BridgeResult<()> {
    if registered.is_empty() {
        return if linked {
            Err(BridgeError::Ims(
                "could not read PJSIP's codec table, so [sip].codecs cannot be checked".to_string(),
            ))
        } else {
            Ok(())
        };
    }
    for codec in list {
        if !registered.iter().any(|id| id == codec.pjsip_id()) {
            return Err(BridgeError::Config(format!(
                "[sip].codecs names {}, which this build does not include",
                codec.config_name()
            )));
        }
    }
    Ok(())
}

/// Apply the configured preference to `endpoint` and log the resulting offer.
///
/// Without a list this is best-effort, as the wideband setup always was: a
/// build missing a codec logs a warning and carries on. With a list, naming a
/// codec this build lacks is fatal (FR-010) — an operator who asked for AMR-WB
/// must not silently get G.722.
pub(crate) fn apply(
    endpoint: &Endpoint,
    codecs: Option<&[PhoneCodec]>,
    wideband: bool,
    amr_octet_align: bool,
) -> BridgeResult<()> {
    let registered: Vec<String> = endpoint.codecs().into_iter().map(|c| c.id).collect();

    if let Some(list) = codecs {
        validate_listed(list, &registered, endpoint.is_linked())?;
    }

    if let Err(e) = endpoint.set_amr_octet_align(amr_octet_align) {
        tracing::warn!(error = %e, "could not set the AMR packing; PJSIP's default (bandwidth-efficient) stays");
    }

    for (codec_id, priority) in priority_plan(codecs, wideband, &registered) {
        if let Err(e) = endpoint.set_codec_priority(&codec_id, priority) {
            tracing::warn!(
                codec = %codec_id,
                error = %e,
                "could not set codec priority; this PJSIP build may not have the codec"
            );
        }
    }
    tracing::info!(
        codecs = ?endpoint
            .codecs()
            .iter()
            .filter(|c| c.priority > 0)
            .map(|c| (c.id.clone(), c.priority))
            .collect::<Vec<_>>(),
        "PJSIP codecs offered, in priority order"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use PhoneCodec::*;

    fn registered() -> Vec<String> {
        [
            "speex/16000/1",
            "speex/8000/1",
            "iLBC/8000/1",
            "GSM/8000/1",
            "PCMU/8000/1",
            "PCMA/8000/1",
            "G722/16000/1",
            "L16/16000/1",
            "AMR/8000/1",
            "AMR-WB/16000/1",
            "opus/48000/2",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn prio(plan: &[(String, u8)], id: &str) -> Option<u8> {
        plan.iter().rev().find(|(i, _)| i == id).map(|(_, p)| *p)
    }

    #[test]
    fn default_wideband_pins_g722_l16_and_the_new_codecs() {
        let plan = priority_plan(None, true, &registered());
        assert_eq!(prio(&plan, "G722/16000/1"), Some(200));
        assert_eq!(prio(&plan, "L16/16000/1"), Some(1));
        assert_eq!(prio(&plan, "AMR-WB/16000/1"), Some(4));
        assert_eq!(prio(&plan, "AMR/8000/1"), Some(3));
        assert_eq!(prio(&plan, "opus/48000/2"), Some(2));
        assert_eq!(plan.len(), 5);
    }

    #[test]
    fn default_narrowband_only_pins_the_new_codecs() {
        let plan = priority_plan(None, false, &registered());
        assert_eq!(prio(&plan, "G722/16000/1"), None);
        assert_eq!(prio(&plan, "L16/16000/1"), None);
        assert_eq!(plan.len(), 3);
    }

    #[test]
    fn default_plan_never_touches_pcmu() {
        for wideband in [true, false] {
            let plan = priority_plan(None, wideband, &registered());
            assert_eq!(prio(&plan, "PCMU/8000/1"), None);
        }
    }

    #[test]
    fn explicit_list_orders_descending_from_200() {
        let list = [AmrWb, Opus, G722, Pcmu];
        let plan = priority_plan(Some(&list), true, &registered());
        assert_eq!(prio(&plan, "AMR-WB/16000/1"), Some(200));
        assert_eq!(prio(&plan, "opus/48000/2"), Some(199));
        assert_eq!(prio(&plan, "G722/16000/1"), Some(198));
        assert_eq!(prio(&plan, "PCMU/8000/1"), Some(197));
    }

    #[test]
    fn explicit_list_disables_every_unlisted_codec_except_l16() {
        let list = [AmrWb, Pcmu];
        let plan = priority_plan(Some(&list), true, &registered());
        for id in registered() {
            let p = prio(&plan, &id).unwrap();
            match id.as_str() {
                "AMR-WB/16000/1" | "PCMU/8000/1" => assert!(p > 1, "{id} = {p}"),
                "L16/16000/1" => assert_eq!(p, 1),
                _ => assert_eq!(p, 0, "{id} must be disabled"),
            }
        }
    }

    #[test]
    fn explicit_list_is_the_same_for_narrowband() {
        let list = [Opus, Pcmu];
        assert_eq!(
            priority_plan(Some(&list), true, &registered()),
            priority_plan(Some(&list), false, &registered())
        );
    }

    #[test]
    fn a_listed_codec_the_build_lacks_is_a_config_error() {
        let registered = vec!["PCMU/8000/1".to_string()];
        let err = validate_listed(&[Pcmu, AmrWb], &registered, true).unwrap_err();
        assert!(matches!(err, BridgeError::Config(_)));
        assert!(err.to_string().contains("AMR-WB"), "{err}");
    }

    #[test]
    fn an_empty_codec_table_fails_a_linked_build_but_not_an_unlinked_one() {
        assert!(validate_listed(&[Pcmu], &[], true).is_err());
        assert!(validate_listed(&[Pcmu], &[], false).is_ok());
    }

    #[test]
    fn a_fully_registered_list_passes() {
        assert!(validate_listed(&[AmrWb, Pcmu], &registered(), true).is_ok());
    }

    #[test]
    fn a_listed_codec_ends_enabled_even_though_it_was_disabled_first() {
        // Every registered codec is zeroed first, then the list is applied;
        // the last assignment wins, so the listed entry must come out enabled.
        let plan = priority_plan(Some(&[Pcmu]), true, &registered());
        assert_eq!(prio(&plan, "PCMU/8000/1"), Some(200));
        assert_eq!(plan.iter().filter(|(i, _)| i == "PCMU/8000/1").count(), 2);
    }
}
