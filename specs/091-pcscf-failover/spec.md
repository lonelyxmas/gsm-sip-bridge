# Feature Specification: P-CSCF pool and failover

**Feature Branch**: `091-pcscf-failover`  
**Created**: 2026-10-14  
**Status**: Draft  
**Input**: User description: "P-CSCF pool and failover (GitHub issue #124, PR 3 of 3; builds on specs 089 and 090). Publish the full advertised P-CSCF pool per line beside the existing single selection; on a qualifying endpoint failure while the tunnel is up, mark the current P-CSCF unavailable (RFC 5626 §4.5 retry delay + 5 min, or Retry-After) and register through the next eligible one; exhausted pool → existing backoff and supervisor rebuild (rediscovery); marks survive agent restarts and are dropped with a new pool. Out of scope: initial selection rule, ePDG DNS rotation, VoLTE P-CSCF discovery, multiple registrations."

## Background

A VoWiFi line reaches the carrier's IMS through a **P-CSCF** (the carrier's SIP edge proxy). The ePDG hands the line a list of P-CSCF addresses each time the tunnel is negotiated. In #124 every negotiation advertised two addresses, drawn from a pool of four, in varying order.

Today the supervisor reduces each list to one address with a deterministic rule (IPv4 first, then the lowest address). The rule exists so that a pool that is merely reordered never restarts a healthy line, and that protection must stay. The line's IMS agent reads only that one address, and every registration, renewal and agent restart targets it. If that P-CSCF stops answering while the tunnel is fine, the line retries the same dead address until its retry budget runs out, exits, and is restarted against the same address.

TS 24.229 says what a UE should do instead:

| Clause | Trigger | Required action |
|---|---|---|
| §5.1.2A.1.6 (shall) | Timer F expiry, or a fatal transport error, on a non-INVITE request sent from the registered contact (single-registration mode) | Treat the contact as unbound; mark the current P-CSCF unavailable; register through another stored P-CSCF not marked unavailable; rediscover P-CSCFs when none remain |
| §5.1.1.2 | REGISTER Timer F expiry | Mark the P-CSCF unavailable for the RFC 5626 §4.5 retry delay plus 5 minutes; may register through another |
| §5.1.1.2 | `503` to REGISTER with `Retry-After` longer than Timer F | Mark the P-CSCF unavailable for the Retry-After period; may register through another |

Because every renewal in this bridge is already a full initial registration (new transport, new Gm security association), moving to another P-CSCF needs nothing beyond choosing a different address for the next attempt.

**Evidence limits.** The #124 trace does not show the selected P-CSCF failing. Its nine `Network is unreachable` errors are local tunnel loss: every P-CSCF sits behind the same tunnel, so failover could not have helped. Its five peer connection closures were each followed by a successful re-registration to the same P-CSCF. This spec closes a standards-conformance and resilience gap; it is not the fix for #124's re-registration loop (spec 089 is).

Spec 090 supplies one of the triggers: a SUBSCRIBE Timer F expiry confirmed by an unanswered keepalive raises a "qualifying endpoint failure".

## Clarifications

### Session 2026-10-14

- Q: After a qualifying failure, when is the attempt through the next unmarked P-CSCF made? → A: Immediately, as a fresh initial registration; the backoff ladder applies only once every candidate is marked.
- Q: Does a peer closing an established connection (FIN/RST) count as a fatal transport error? → A: No. Only connect refused/timeout or a send failure on a new connection qualifies; a peer close triggers the existing reconnect and qualifies only if that reconnect fails.
- Q: After an agent restart, does a line that failed over to a working P-CSCF return to the supervisor's selection once its mark expires? → A: No. The last successfully registered P-CSCF is persisted with the marks and goes first on restart while it is still in the pool and unmarked.
- Q: When every candidate is marked unavailable, which address does each backoff-ladder attempt target? → A: The candidate whose mark expires soonest; ties go to candidate order.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A line moves to another advertised P-CSCF when its own stops answering (Priority: P1)

An operator runs a VoWiFi line whose tunnel advertised P-CSCFs A and B; the line registered through A. A then stops answering while the tunnel stays up. The bridge marks A unavailable, registers through B at its next attempt, and the line is reachable again without operator action or a tunnel rebuild.

**Why this priority**: This is the capability the standard requires and the line lacks entirely.

**Independent Test**: With a pool of two reachable test endpoints, stop the first one answering (registered line, tunnel up). Confirm the next registration goes to the second, the first is reported unavailable with an expiry time, and the line ends up registered.

**Acceptance Scenarios**:

1. **Given** pool `[10.0.0.10, 10.0.1.10]`, registered through `10.0.0.10`, tunnel up, **When** a qualifying endpoint failure occurs on `10.0.0.10`, **Then** `10.0.0.10` is marked unavailable and the next registration attempt goes to `10.0.1.10`.
2. **Given** a REGISTER to `10.0.0.10` that gets no response within 128 s, tunnel up, **When** the attempt times out, **Then** `10.0.0.10` is marked unavailable and the next attempt goes to `10.0.1.10`.
3. **Given** a `503` to REGISTER from `10.0.0.10` with `Retry-After: 600`, **When** processed, **Then** `10.0.0.10` is unavailable for 600 s and the next attempt goes to `10.0.1.10`.
4. **Given** the line failed over to `10.0.1.10` and is registered there, **When** `10.0.0.10`'s unavailability expires, **Then** the line stays on `10.0.1.10` (it is working) until `10.0.1.10` itself fails.
5. **Given** a call is in progress when the failure occurs, **When** the failure is recorded, **Then** the call is not interrupted and the move to `10.0.1.10` happens with the first registration after the call ends.

---

### User Story 2 - Failover never fires for the wrong reasons (Priority: P1)

The bridge moves to another P-CSCF only when the P-CSCF itself is the likely fault. A tunnel that is down, a reordered pool, a network refusal, or a deregistration notice never moves a line.

**Why this priority**: The deterministic selection exists precisely because a needless move tears down a healthy registration; the failover must not reintroduce that.

**Independent Test**: Drive each non-qualifying situation below and confirm the active P-CSCF and the unavailability marks do not change.

**Acceptance Scenarios**:

1. **Given** the tunnel is down and a registration attempt fails with "network unreachable", **When** processed, **Then** no P-CSCF is marked unavailable and the existing re-attach recovery handles it.
2. **Given** a healthy line registered through `10.0.0.10`, **When** a new negotiation advertises `[10.0.1.10, 10.0.0.10]` (same set, new order), **Then** the line is neither restarted nor moved.
3. **Given** a healthy line, **When** a new negotiation advertises the same set plus a third address, **Then** the line is not moved; the new address only joins the candidates.
4. **Given** a `403` (or any 4xx/5xx/6xx without Retry-After) to REGISTER, **When** processed, **Then** no P-CSCF is marked unavailable; the existing backoff applies.
5. **Given** a reg-event NOTIFY reporting the binding `rejected` (spec 089), **When** processed, **Then** no P-CSCF is marked unavailable.

---

### User Story 3 - Running out of P-CSCFs leads to a bounded, orderly rebuild (Priority: P2)

When every advertised P-CSCF is marked unavailable (or the pool has one address, or none), the line falls back to the existing renewal backoff and, once that budget is spent, to the supervisor's existing recovery, which renegotiates the tunnel and obtains a fresh list. No rapid cycling between addresses.

**Why this priority**: Without a bound, two dead P-CSCFs would be retried alternately forever.

**Independent Test**: Make every candidate fail, with an injected clock, and count attempts per unit of time; confirm the agent eventually hands over to the supervisor and that the rebuilt tunnel's new list replaces the old one.

**Acceptance Scenarios**:

1. **Given** pool `[A, B]` with both marked unavailable and `B`'s mark expiring first, **When** the next attempt is due, **Then** it follows the existing backoff ladder and targets `B`, and the agent exits to the supervisor when the existing budget is spent.
2. **Given** a single-address pool, **When** that address fails, **Then** behaviour equals today's (backoff, then supervisor), plus the unavailability mark.
3. **Given** no pool was published (older supervisor, or VoLTE), **When** the agent starts, **Then** it uses the single published address as a one-element pool.
4. **Given** the supervisor rebuilt the tunnel and a new negotiation advertised `[C, D]`, **When** the agent starts, **Then** its candidates are `[C, D]` and marks for `A` and `B` are gone.

---

### User Story 4 - A restart does not walk back into a dead P-CSCF (Priority: P2)

If the agent restarts (for any reason) while a P-CSCF is still marked unavailable, and the tunnel's pool has not changed, the restarted agent skips that P-CSCF.

**Why this priority**: The supervisor's single selection always names the same address; without remembered marks and a remembered working P-CSCF, every restart would retry the dead one first or abandon a working one.

**Independent Test**: Mark `A` unavailable, restart the agent with the same pool, and confirm the first registration goes to `B`; let the mark expire, restart again, and confirm the line stays on `B`; then mark `B` and confirm `A` is used.

**Acceptance Scenarios**:

1. **Given** `A` is marked unavailable for 10 more minutes and the pool is unchanged, **When** the agent restarts, **Then** its first registration goes to `B`.
2. **Given** `A`'s mark has expired and the line was last registered through `B`, **When** the agent restarts, **Then** its first registration goes to `B` (the working P-CSCF is kept).
3. **Given** `A`'s mark has expired and `B` is marked unavailable or no longer in the pool, **When** the agent restarts, **Then** `A` is eligible again, in its normal order.

---

### User Story 5 - Operators can see which P-CSCF a line uses and why (Priority: P3)

Status queries show each line's active P-CSCF, the advertised pool, and which candidates are marked unavailable and until when. Every selection, and the failure that caused a move, is logged.

**Why this priority**: Diagnosability; #124's reporter had to reconstruct the pool from tunnel logs.

**Independent Test**: Trigger a failover and read status and the log.

**Acceptance Scenarios**:

1. **Given** a line that failed over from `A` to `B`, **When** status is queried, **Then** it shows `B` active, pool `[A, B]`, and `A` unavailable until a stated time.
2. **Given** any candidate selection, **When** it happens, **Then** one log line records the chosen address, the reason (startup, failover after a named failure, pool replaced) and, later, the outcome of the registration through it.

---

### Edge Cases

- The pool contains IPv6 and IPv4 addresses: IPv4 candidates are tried first, then IPv6, preserving today's preference.
- The supervisor's single selection is not in the published pool (inconsistent files): the selection is tried first, then the pool.
- A fatal transport error on connect to `A` happens at the same moment the tunnel drops: the tunnel being down takes precedence and no mark is set; the existing re-attach path decides.
- The P-CSCF closes an established connection (as in #124's five peer closures): the existing reconnect runs against the same address; only a failed reconnect marks it unavailable.
- A qualifying failure is reported by an event belonging to the previous registration (e.g. a late keepalive deadline after a renewal already moved on): ignored; it cannot mark the new endpoint.
- The pool changes while the line is registered through `B` and `B` is still advertised: the line stays on `B`.
- The pool changes and `B` (active) is no longer advertised: the supervisor's existing "P-CSCF changed" handling decides, as today; the agent's candidates become the new pool.
- Two lines share one tunnel daemon: marks and active P-CSCF are per line; one line's failover never touches another line's tunnel or registration.
- Consecutive failures of the same address grow its unavailability per RFC 5626 §4.5 (30 s base, 1800 s cap), plus 5 minutes; a successful registration through it resets that count.

## Requirements *(mandatory)*

### Functional Requirements

**Pool publication**

- **FR-001**: The supervisor MUST publish, per line, the complete P-CSCF list from the latest tunnel negotiation, in addition to the existing single selection.
- **FR-002**: The pool MUST be published at every point the single selection is published today: initial tunnel establishment, a detected P-CSCF change, and the post-recovery sync with a fresh negotiation.
- **FR-003**: Only the latest negotiation's addresses MUST be published; addresses from earlier negotiations MUST NOT be candidates.
- **FR-004**: The existing single selection and its "P-CSCF changed" behaviour MUST be unchanged; a reordered or enlarged pool containing the active address MUST NOT restart or move a healthy line.

**Candidates**

- **FR-005**: The agent MUST build an ordered candidate list: the last successfully registered P-CSCF first (when it is still in the pool and not marked unavailable), then the supervisor's single selection, then the remaining pool addresses with IPv4 before IPv6.
- **FR-006**: When no pool is published, the agent MUST treat the single selection as a one-element pool. VoLTE lines MUST use their configured P-CSCF as a one-element pool.

**Qualifying failures**

- **FR-007**: The following MUST mark the current P-CSCF unavailable, but only while the line's tunnel or attachment is up: a REGISTER that receives no response within 128 s; a fatal transport error reaching the P-CSCF (connection refused, connect timeout, or a send failure on a newly opened connection); a qualifying endpoint failure from spec 090; the existing Gm keepalive escalation (keepalive unanswered and reconnects exhausted).
- **FR-008**: A `503` to REGISTER with `Retry-After` longer than 128 s MUST mark the P-CSCF unavailable for the Retry-After period.
- **FR-009**: The following MUST NOT mark any P-CSCF unavailable: "network unreachable"/no-route errors; any failure while the tunnel or attachment is down; 4xx/5xx/6xx responses without Retry-After; any reg-event deregistration, including `rejected`; a peer close (FIN/RST) of an established connection, unless the reconnect that follows fails per FR-007.
- **FR-010**: Other than per FR-008, an unavailability mark MUST last the RFC 5626 §4.5 retry delay (base 30 s, cap 1800 s, growing with that address's consecutive failures) plus 5 minutes. A successful registration through an address MUST reset its failure count.

**Selection and recovery**

- **FR-011**: After a qualifying failure, the next registration attempt MUST go to the first candidate not marked unavailable, as a full initial registration, without waiting for a backoff step. Because each failure marks its address, a pool of N addresses yields at most N such immediate attempts before FR-013 applies.
- **FR-012**: While registrations through the active P-CSCF succeed, renewals MUST keep using it, even after an earlier candidate's mark expires.
- **FR-013**: When every candidate is marked unavailable, and only then, the agent MUST follow the existing renewal backoff ladder and its existing exit to the supervisor; it MUST NOT cycle between candidates faster than that ladder. Each such attempt MUST target the candidate whose mark expires soonest (ties broken by candidate order).
- **FR-014**: A new negotiation's pool MUST replace the candidate list and MUST discard marks, and the saved last-registered P-CSCF, for addresses no longer advertised.
- **FR-015**: Unavailability marks, and the last successfully registered P-CSCF, MUST survive an agent restart while they are valid and the pool is unchanged.
- **FR-016**: A P-CSCF switch MUST NOT happen during a call; it MUST wait for the first registration after the call ends (existing maintenance deferral).
- **FR-017**: Failures attributable to a previous registration MUST NOT mark, or move away from, the current endpoint.
- **FR-018**: Recovery on one line MUST NOT affect other lines' tunnels, P-CSCF state or registrations.

**Observability**

- **FR-019**: Every candidate selection MUST be logged with the address, the reason, and the outcome of the registration through it, without logging subscriber identifiers beyond those already logged.
- **FR-020**: Status queries MUST show the active P-CSCF, the pool, and each unavailable candidate with its expiry.

### Key Entities

- **P-CSCF pool**: the address list from a line's latest tunnel negotiation; replaced wholesale by each new negotiation.
- **Candidate list**: the pool ordered for selection (last successfully registered P-CSCF, then single selection, then IPv4 before IPv6).
- **Active P-CSCF**: the candidate the line is currently registered through, or is attempting. The last one that registered successfully is persisted with the marks and is preferred on restart (FR-005).
- **Unavailability mark**: per line and address; expiry time and consecutive-failure count; survives agent restarts; dropped when the address leaves the pool.
- **Qualifying endpoint failure**: a failure attributable to the P-CSCF itself while the tunnel is up (FR-007, FR-008).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With a two-address pool and the first address dead while the tunnel is up, a line is registered through the second within 5 minutes of the failure, without a tunnel rebuild or operator action.
- **SC-002**: Over 24 hours of pool reorders and additions that keep the active address, zero healthy lines are restarted or moved.
- **SC-003**: With every candidate dead, registration attempts never exceed the existing backoff ladder's rate, and the line reaches the supervisor's rebuild within the existing retry budget.
- **SC-004**: Tunnel-down failures (as in #124's "network unreachable" errors) cause zero P-CSCF unavailability marks.
- **SC-005**: An agent restart within an address's unavailability window never sends its first registration to that address.
- **SC-006**: An operator can tell from one status query which P-CSCF each line is using and which are unavailable until when.

## Assumptions

- Lines run in single-registration mode; RFC 5626 multiple registrations (flows) are out of scope, so §5.1.2A.1.6's single-registration branch applies.
- Specs 089 and 090 are in place; spec 090 provides the SUBSCRIBE-based qualifying endpoint failure.
- The supervisor's rule for the single selection is unchanged and stays the source of truth for "P-CSCF changed".
- "Rediscovery" for VoWiFi is the existing supervisor tunnel rebuild, which yields a fresh list; no new discovery mechanism is added.
- VoLTE lines keep a single configured P-CSCF; they gain the unavailability bookkeeping and observability but have no alternate to move to.
- Unavailability marks are local, per-line runtime state (not configuration) and need not survive a host reboot.
- The existing ePDG DNS rotation is a separate recovery layer and is unchanged.
