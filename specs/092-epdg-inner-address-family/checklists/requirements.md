# Specification Quality Checklist: Per-line inner address family for the ePDG tunnel

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-10-14
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

- The audience is operators of a SIP/IMS bridge, so protocol terms (inner address, traffic selector, `INVALID_SELECTORS`, `N(INVAL_SEL)`) are domain vocabulary, not implementation detail. FR-010 quotes the log text on purpose: recognising that text is the operator's whole trigger for using the setting.
- No source files, functions or template variable names appear in the spec. Where the value is rendered is left to `/speckit-plan`.
- Settled in the clarify session on 2026-10-14:
  - the config key is `tunnel_address_family`;
  - under `swu`, a non-`dual` value stops the bridge from starting;
  - a `dual` line logs one warning per tunnel session when its rekey is rejected with `INVALID_SELECTORS` (FR-009a).
- Still informed guesses: the effective value is logged when the tunnel starts, and values are lowercase only.
- Rebased onto main after PR #130 (spec 091, P-CSCF pool and failover) was merged. Its interactions are recorded in Edge Cases, FR-013 and Assumptions.
- SC-003 and SC-004 depend on an external tester (the #107 reporter). The maintainers cannot reach the affected ePDG.
