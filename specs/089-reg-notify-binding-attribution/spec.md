# Feature Specification: Reg-event NOTIFY binding attribution and per-event deregistration policy

**Feature Branch**: `089-reg-notify-binding-attribution`  
**Created**: 2026-10-14  
**Status**: Draft  
**Input**: User description: "Reg-event NOTIFY binding attribution and per-event deregistration policy (GitHub issue #124, PR 1 of 3). Attribute reg-event `<contact>` entries to our current binding by contact address, never by IMEI alone; ignore the network retiring a binding a renewal replaced; apply TS 24.229 §5.1.1.7 per event (`deactivated` → re-register now, `unregistered`/`probation` → backoff ladder, `rejected` → release dialogs, alert, long cooldown then one attempt). Out of scope: SUBSCRIBE transaction tracking (spec 090), P-CSCF failover (spec 091), refreshing the existing binding instead of re-registering on a new contact."

## Background

An IMS line keeps one **registration binding**: the carrier's registrar (S-CSCF) maps the line's public identity to one **contact address** (inner IP + protected server port). The line also subscribes to its own **registration-state event package** (`reg`), and the network sends a NOTIFY carrying an XML document (RFC 3680 `reginfo`) whenever any binding for the identity changes. Each `<contact>` element in it has a `<uri>` (the contact address), a `state` (`active`/`terminated`) and an `event` (`registered`, `created`, `refreshed`, `expired`, `deactivated`, `probation`, `unregistered`, `rejected`, …).

Every registration renewal in this bridge is a fresh initial registration on a **new** contact address (new protected ports, new Gm IPsec SA). TS 24.229 §5.1.1.4.1 permits this, and §5.4.2.1.2 NOTE 2 states the consequence: without multiple registrations, "if the UE registers new contact address then all registrations… using an old contact address are deregistered", reported as a `<contact>` with `state="terminated"` and event `unregistered`, `deactivated` or `rejected`.

The bridge decides which `<contact>` entries are its own by matching **either** its IMEI (the `+sip.instance` value carriers echo back) **or** its current contact address. The replaced binding came from the same device, so it carries the same IMEI and is claimed as the line's own. The bridge then believes its current binding was dropped, forces a re-registration, which replaces the binding again, which produces another such NOTIFY: a self-sustaining loop.

Issue #124 (reporter on v8.19.2, one carrier, window of ~3 h 11 min):

| Observation | Value |
|---|---|
| Successful renewals | 484 (+ the initial registration) |
| Forced re-registrations after an "own contact terminated/rejected" NOTIFY | 478 |
| Median time from renewal success to that NOTIFY | ~1.09 s |
| `Subscription-State` on those NOTIFYs | `terminated;reason=noresource` |

Triage reproduced the misattribution on `main` (v8.20.0): a NOTIFY reporting the old `:5064` contact `terminated/rejected` while the line's current contact is `:5070` is attributed to the line by IMEI and reported as a deregistration. Issue #117's early-loss backoff (v8.20.0) only slows the loop and, after six consecutive "failures", exits the agent, so the line restarts and loops again.

TS 24.229 §5.1.1.4.1 NOTE 10 identifies a registration "by contact address" when multiple registrations are not used, which is this bridge's mode.

A second, smaller deviation: the bridge treats every deregistration event the same and re-registers. TS 24.229 §5.1.1.7 says only `deactivated` means "start an initial registration"; for `rejected` the UE releases its dialogs, and §5.4.1.5 says the S-CSCF uses `rejected` precisely when it "does not expect the UE to reregister". Re-registering straight away after a genuine `rejected` turns a network refusal into a loop of its own.

## Clarifications

### Session 2026-10-14

- Q: When the network reports the line's *current* binding `terminated` with event `rejected`, how should the bridge behave? → A: Long cooldown, then retry. Do not re-register immediately; mark the line failed so the existing registration-loss alert fires; retry once after a long cooldown (default 30 minutes, or the network's retry-after if it gave one), and keep using the cooldown if `rejected` repeats. Not "never retry" (an unattended line must self-heal), not the immediate ladder (that hammers a carrier that said no).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A renewal no longer causes a re-registration loop (Priority: P1)

An operator runs an IMS line (VoWiFi or VoLTE). Its registration renews; the network then retires the binding the renewal replaced and sends a NOTIFY saying so. The bridge recognises that entry as a **previous** binding of its own, logs it as retired, and stays registered. No re-registration follows, and the line renews only on its normal schedule.

**Why this priority**: This is the loop in #124: hundreds of needless re-registrations an hour, a binding that is constantly being replaced, and plausibly the reporter's missing inbound calls.

**Independent Test**: Feed the dispatch path a NOTIFY whose only terminated `<contact>` names the line's previous contact address (with the line's IMEI echoed) while the session's current contact is a different port. Confirm registration state stays `Registered`, no renewal is scheduled early, and an info line records the retired binding.

**Acceptance Scenarios**:

1. **Given** the line's current contact is `10.0.0.1:5070`, **When** a NOTIFY reports `<contact>` `sip:+919000000000@10.0.0.1:5064` as `terminated/rejected` with IMEI `860000000000000` echoed, **Then** the entry is not attributed to the current binding, registration state is unchanged, and the log records a retired previous binding.
2. **Given** the same current contact, **When** a full-state NOTIFY lists the old `:5064` contact `terminated/unregistered` **and** the new `:5070` contact `active/registered`, **Then** the line is reported as still registered.
3. **Given** the same current contact, **When** a NOTIFY reports `:5070` `terminated/deactivated`, **Then** the line is treated as deregistered (current binding, attributed by contact address).
4. **Given** a carrier that lists the line's binding under both a SIP and a `tel:` identity (issue #117), **When** both entries name the current contact address, **Then** both are recognised as the line's own, exactly as today.

---

### User Story 2 - Each deregistration event gets the response the standard calls for (Priority: P1)

When the network reports the line's **current** binding terminated, the bridge reacts according to the event:

- `deactivated`: re-register at once (the network expects it).
- `unregistered` or `probation`: re-register on the existing renewal backoff ladder, waiting at least any retry-after the network gave.
- `rejected`: end any call in progress, mark the line failed (so the existing registration-loss alert fires), and stay unregistered until a long cooldown elapses; then try once. A repeated `rejected` restarts the cooldown.
- `expired`: not a network deregistration (the line's own lifetime ran out); unchanged behaviour.

**Why this priority**: Once Story 1 stops the false positives, the remaining NOTIFYs are real, and `rejected` is the one where today's "re-register immediately" both contradicts TS 24.229 and recreates a loop.

**Independent Test**: Drive the registration state machine with a NOTIFY for each event on the current binding, using an injected clock. Observe when (or whether) a renewal attempt is scheduled, the reported registration state, and whether the agent's failure budget is charged.

**Acceptance Scenarios**:

1. **Given** a registered line, **When** its current binding is reported `terminated/deactivated`, **Then** a re-registration is attempted on the next loop pass.
2. **Given** a registered line, **When** its current binding is reported `terminated/unregistered`, **Then** a re-registration is scheduled on the backoff ladder and counts as one failure towards it, as for any other early loss.
3. **Given** a registered line, **When** its current binding is reported `terminated/probation` with `retry-after="600"`, **Then** no re-registration is attempted for 600 seconds.
4. **Given** a registered idle line, **When** its current binding is reported `terminated/rejected`, **Then** the line reports `Failed` and not registered, no re-registration is attempted for the cooldown period, exactly one attempt follows when the cooldown ends, and the agent does not exit.
5. **Given** a call in progress, **When** the current binding is reported `terminated/rejected`, **Then** the call is ended and the same cooldown applies.
6. **Given** `rejected` arrives again after the post-cooldown attempt registered, **When** it is processed, **Then** a new cooldown starts; however many times this repeats, the agent never exits because of it.
7. **Given** a per-line cooldown of 10 minutes is configured, **When** `rejected` arrives with no retry-after, **Then** the next attempt is 10 minutes later.

---

### User Story 3 - A NOTIFY that removes the whole identity is understood (Priority: P2)

A NOTIFY can say a single binding is gone while the identity stays registered (`<registration state="active">` with one `<contact state="terminated">`), or that the identity itself is deregistered (`<registration state="terminated">`). The bridge distinguishes the two in its log and applies Story 2's policy using the event on the line's own `<contact>` entries in either case.

**Why this priority**: Correctness of the log and of the policy input. It does not change the main fix, but it is how operators read what the network did.

**Independent Test**: Feed one NOTIFY of each shape and compare the logged classification and the resulting policy.

**Acceptance Scenarios**:

1. **Given** `<registration state="terminated">` whose `<contact>` for the current address is `terminated/deactivated`, **When** processed, **Then** the log says the identity was deregistered and the line re-registers at once.
2. **Given** `<registration state="active">` with the current contact `terminated/rejected` and another device's contact `active`, **When** processed, **Then** the log says the line's binding was removed and the `rejected` policy applies.

---

### Edge Cases

- A NOTIFY whose only `<contact>` has no `<uri>` (or an unparseable one): keep today's sole-contact fallback and treat it as the line's own.
- A NOTIFY whose only `<contact>` names a different address and does not echo the IMEI (another device, e.g. a paired handset): not the line's own, as today.
- The line's contact address is IPv6 (`[addr]:port`): attribution compares addresses, not strings, so formatting differences do not matter.
- A carrier appends URI parameters to `<uri>` (`;transport=tcp`, `;Hpt=…`): ignored for matching, as today.
- A NOTIFY arrives for the old binding after the renewal swapped sessions but before the new session's NOTIFY: it names the old address, so it is ignored.
- Several `<contact>` entries for the current address with different events (e.g. `rejected` under one identity, `unregistered` under another): the most restrictive applies (`rejected` > `deactivated` > `probation` > `unregistered`), so a `rejected` is never downgraded into an immediate retry.
- `rejected` arrives while a renewal attempt is already scheduled: the cooldown replaces the earlier schedule.
- The agent restarts during a cooldown (e.g. supervisor action): the restarted agent registers normally; the cooldown is not persisted across restarts.
- `retry-after` larger than the configured cooldown: the larger value wins.

## Requirements *(mandatory)*

### Functional Requirements

**Attribution**

- **FR-001**: The system MUST attribute a reg-event `<contact>` entry to the line's current binding only when its `<uri>` names the line's current contact address (IP and port).
- **FR-002**: The system MUST NOT attribute an entry naming a different address to the current binding, even when it carries the line's IMEI.
- **FR-003**: The system MUST keep the existing fallback that treats a document's sole `<contact>` as the line's own when that contact carries no parseable address.
- **FR-004**: The system MUST recognise every entry naming the current contact address, including the same binding listed under several public identities (issue #117).
- **FR-005**: The system MUST log, at info level and without changing registration state, a terminated entry that names a different address but carries the line's IMEI ("replaced binding retired").

**Per-event policy for the current binding** (TS 24.229 §5.1.1.7, §5.4.1.5; RFC 3680 §5.2)

- **FR-006**: On `deactivated`, the system MUST attempt a new registration immediately.
- **FR-007**: On `unregistered`, the system MUST re-register on the existing renewal backoff ladder, with the existing early-loss accounting (issue #117).
- **FR-008**: On `probation`, the system MUST re-register on the backoff ladder and MUST NOT attempt before the entry's `retry-after` value, if present, has elapsed.
- **FR-009**: On `rejected`, the system MUST end any call or dialog in progress on the line, report the registration as failed and not registered (so the existing registration-loss alert can fire), and MUST NOT attempt a new registration until the rejection cooldown has elapsed.
- **FR-010**: The rejection cooldown MUST default to 30 minutes, MUST be configurable per line, and MUST be extended to any larger retry-after the network supplied.
- **FR-011**: When the rejection cooldown elapses, the system MUST make one registration attempt. A failure of that attempt continues on the normal renewal backoff ladder. A further `rejected` starts a new cooldown.
- **FR-012**: Neither a `rejected` event nor its cooldown MUST count towards the consecutive-renewal-failure budget that makes the agent exit, so repeated `rejected` cannot cause an agent exit/restart loop.
- **FR-013**: `expired` and any non-terminated state MUST NOT be treated as a network deregistration (unchanged).
- **FR-014**: When several of the line's own entries in one NOTIFY report different events, the system MUST apply the most restrictive one, in the order `rejected`, `deactivated`, `probation`, `unregistered`.

**Classification and observability**

- **FR-015**: The system MUST distinguish, in its log, an identity deregistered as a whole (`<registration state="terminated">`) from a single binding removed under an active registration.
- **FR-016**: Every reg-event NOTIFY MUST be logged with its attribution method, the line's current contact address, the event applied (if any) and the full body.
- **FR-017**: Registration status queries (`vowifi-status`/`volte-status`) MUST show when a line is in a rejection cooldown and when its next attempt is due.

**Scope**

- **FR-018**: The behaviour MUST be identical for VoWiFi and VoLTE lines.

### Key Entities

- **Current binding**: the line's contact address from its most recent successful registration; the only identity the line answers to in a NOTIFY.
- **Previous binding**: a contact address from an earlier registration of the same line, now replaced; carries the same IMEI.
- **Deregistration event**: `deactivated`, `unregistered`, `probation` or `rejected` on a terminated entry for the current binding, with an optional retry-after.
- **Rejection cooldown**: a per-line hold-off period after `rejected`, with a configured default and a network-supplied override.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On a line whose network retires replaced bindings (the #124 pattern), forced re-registrations caused by reg-event NOTIFYs drop from roughly one per renewal to zero over a 3-hour observation.
- **SC-002**: The NOTIFY retiring a replaced binding produces zero renewal attempts within the following 60 seconds in 100% of cases in the test suite.
- **SC-003**: After a `rejected` on the current binding, no registration attempt occurs before the cooldown ends, and exactly one occurs within one loop interval after it ends.
- **SC-004**: 24 hours of repeated `rejected` (simulated) produce zero agent exits caused by the rejection.
- **SC-005**: All existing reg-event attribution tests, including the issue #117 cases, pass unchanged.
- **SC-006**: A field report from a line on this version can tell, from the log alone, which `<contact>` entry triggered (or did not trigger) a re-registration and why.

## Assumptions

- Lines run in single-registration mode (no `reg-id`, no RFC 5626 outbound); the contact address alone identifies a binding.
- Renewals continue to register on a new contact address; changing that is a separate decision outside this spec.
- The cooldown lives in the agent's memory and does not survive an agent restart; a restart is an operator or supervisor decision and re-registering then is acceptable.
- The existing registration-loss alert threshold applies unchanged; no new alert category is needed.
- `retry-after` is read from the `<contact>` element's `retry-after` attribute (RFC 3680) and, for a terminated subscription, from `Subscription-State`'s `retry-after` parameter; the larger applies.
- Ending a call on `rejected` is acceptable: the network has already released the sessions bound to that contact (§5.4.1.5), so the call is dead either way.
- SUBSCRIBE transaction and dialog tracking (spec 090) and P-CSCF failover (spec 091) build on this spec and are not required for it.
