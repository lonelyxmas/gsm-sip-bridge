# Specification Quality Checklist: AMR and Opus on the phone-facing leg

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-10-12
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details)
- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded
- [x] Dependencies and assumptions identified

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria
- [x] User scenarios cover primary flows
- [x] Feature meets measurable outcomes defined in Success Criteria
- [x] No implementation details leak into specification

## Notes

- Codec names (AMR-WB, AMR-NB, Opus, G.722, PCMU) appear throughout. They are the user-facing subject of the feature (operators configure them and phones advertise them), not implementation choices, so they do not count as implementation leakage. The spec names no library, build flag, config key or module.
- FR-010 leaves "refuse to start vs. fall back to default" to the project's existing invalid-config convention, to be confirmed during planning rather than flagged as a clarification.
- The main planning risk is FR-009: SIP-stack codec priorities are process-wide, and newly compiled-in codecs may default to a higher priority than PCMU. That could change the GSM path's codec choice and the internal link between the two agents unless both are pinned explicitly.
