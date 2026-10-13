# Research: Reg-event NOTIFY binding attribution and per-event deregistration policy

All decisions below are grounded in the code as of `a1240e6` (v8.20.0 + spec commit). The spec has no open
NEEDS CLARIFICATION; this file records the design choices the spec leaves to planning.

## R1 — Attribution: address first, IMEI only as a tiebreaker for address-less entries

**Decision**: `find_own_contacts` (`ims/session.rs`) classifies every `<contact>` element into one of three buckets:

| Entry's `<uri>` | Mentions our IMEI? | Bucket |
|---|---|---|
| parses to `contact_addr` | either | **current** (ours, `Attribution::ContactAddr`) |
| parses to a different address | yes | **retired** (a previous binding of ours — logged, never acted on) |
| parses to a different address | no | foreign (another device) |
| absent / unparseable | yes | **current** (`Attribution::Imei`) |
| absent / unparseable | no | sole-contact fallback only (unchanged, `Attribution::SoleContact`) |

**Rationale**: FR-001/FR-002 forbid IMEI-only attribution when an address *proves* the entry is another binding;
the #124 loop is exactly that case (`:5064` retired while current is `:5070`). An entry that carries our IMEI but
no checkable address cannot be proven to be a previous binding, and dropping it would make the line deaf to a real
deregistration on a carrier that echoes the IMEI but rewrites or omits `<uri>`. Keeping it preserves today's
behaviour where today's behaviour was not the bug.

**Alternatives considered**:
- *Address only, drop IMEI entirely* — simplest, but turns a carrier that omits `<uri>` from "reacts correctly"
  into "never notices it was dropped". Rejected.
- *IMEI first, address as tiebreaker* (today) — the bug.
- *Compare against a history of our previous contact addresses* — needs the agent to remember old sessions; the
  different-address test already identifies them without state. Rejected (Principle V).

**Risk carried to the live gate**: Vodafone echoes the IMEI under both AORs (`attribution=imei own_entries=2`
in every live log so far), so it has never shown whether its `<uri>` preserves our `host:port`. If it rewrote the
port, the new code would classify our *current* binding as *retired* and ignore a real deregistration. The
quickstart's Vodafone check (Q2) is therefore blocking: the post-change log must read `attribution=contact-addr`
on the initial registration's NOTIFY. Diagnostic for the failure mode: FR-016's log line carries `contact_addr`
and the body, and an IMEI-matched entry with no current-address match is logged as `retired` with its address,
so a rewrite would be visible on the first renewal.

## R2 — Address comparison

**Decision**: keep `contact_uri_addr` (literal `ip:port` / `[ipv6]:port`, URI params stripped) and compare
`SocketAddr` values. Already satisfies the IPv6 and URI-parameter edge cases; add tests for them.

## R3 — Per-event policy and its selection

**Decision**: a new `DeregEvent` enum, ordered by restrictiveness:
`Rejected > Deactivated > Probation > Unregistered` (FR-014). `handle_notify` returns
`Option<Deregistration { event, retry_after: Option<Duration>, scope }>` — the most restrictive event across the
*current* entries, the largest retry-after found (the `<contact retry-after>` attribute on those entries, and the
`retry-after` parameter of `Subscription-State`), and whether the identity as a whole was deregistered (R6).
`expired` and any non-terminated state yield `None` (FR-013, unchanged).

The decision itself lives in a pure `classify_reg_notify(body, subscription_state, imei, contact_addr) ->
NotifyVerdict { own: OwnContacts, deregistration: Option<Deregistration> }`; `handle_notify` sends the `200 OK`,
calls it, logs, and returns `verdict.deregistration`.

**Rationale**: `handle_notify` already owns parsing and logging; returning a value instead of `bool`, computed by
a pure function, keeps the decision testable on real bodies without a socket.

## R4 — How each event drives `LoopState`

Today `on_network_deregistration` does: `force_renewal = true`; if the binding is ≥ `REGISTRATION_SETTLE` (60s)
old, re-register immediately, uncounted; otherwise (early loss, issue #117) restore the ladder the last success
cleared, count one failure, schedule on that backoff.

**Decision** (one method, branching on `DeregEvent`):

- **`Deactivated`** (FR-006, clarification Q2): today's path, with one change — an early loss whose restored
  ladder was clean (`consecutive_renewal_failures == 0` before counting) clears `next_renewal_attempt`, so the
  *first* attempt goes out on the next loop pass. It is still counted. A repeat within the settle window then
  restores a ladder of `(10s, 1)` and waits; the six-failure exit still applies. A mature binding: immediate,
  uncounted (unchanged).
- **`Unregistered` / `Probation`** (FR-007/FR-008): always on the ladder. Early loss → today's early-loss path.
  Mature binding → `schedule_renewal_retry()` from the current (clean) ladder: first retry after 5s, counted as
  one failure, reset on success. Then `next_renewal_attempt = max(scheduled, now + retry_after)`.
- **`Rejected`** (FR-009..FR-012, clarification Q1): not counted, ladder not restored. Reset the ladder to
  `(RETRY_INITIAL_BACKOFF, 0)` so a failure of the post-cooldown attempt starts the normal ladder clean;
  `rejection = Some(RejectionCooldown { until: now + max(cooldown, retry_after) })`;
  `next_renewal_attempt = Some(until)`; `force_renewal = true` so the attempt fires when the gate opens even
  though expiry is far off. `early_loss_counted = true` so a duplicate NOTIFY about the same loss is a no-op
  for the ladder; a duplicate `rejected` simply recomputes the same cooldown (idempotent, and a *new* rejection
  after a successful post-cooldown registration restarts it — FR-011, scenario 7).
  `on_renewal_success` clears `rejection`.

**Rationale**: everything reuses `next_renewal_attempt` (the existing retry gate in `on_idle_tick`) and
`force_renewal` (the existing "re-register even though not due" flag). No new timer, no new loop branch: the
cooldown is just a far-off `next_renewal_attempt` that `schedule_renewal_retry` never touched, so the exit budget
is untouched by construction (FR-012, SC-004).

**Alternatives considered**: a separate cooldown gate in `on_idle_tick` — redundant with `next_renewal_attempt`.
A persisted cooldown — rejected by the spec's assumption (agent restart re-registers).

## R5 — Ending the call on `rejected`

**Decision**: `handle_reg_notify` takes `&mut RegisteredSession` and, for `Rejected` only:
- an `active_call` → the same sequence as `handle_attachment_loss` (lifecycle end, stop media,
  `report_answered_call_ended`, `call::end_call_best_effort(.., reason::REGISTRATION_REJECTED)`,
  `maintenance.release()`, reset the attachment watch);
- a pending `origination` → a new `origination::fail_origination(p, session, reason)`: `p.fail(reason)` (the
  existing best-effort `CallFailed` to Agent B — `abandon_origination` deliberately sends none, because there
  Agent B initiated) followed by `abandon_origination`'s per-step teardown (CANCEL a pending INVITE and keep the
  attempt in `AwaitingCancel` so a racing answer is still cleaned up; BYE an answered leg).

A new `reason::REGISTRATION_REJECTED` (`"registration_rejected"`) in `vowifi::control::reason`, so Agent B's call
record says why.

**Rationale**: §5.4.1.5 — the network has already released the sessions bound to that contact; the BYE is
best-effort, exactly like attachment loss. `Deactivated`/`Probation`/`Unregistered` keep today's deferral (the
maintenance policy holds the re-registration until the call ends).

## R6 — Identity-level vs binding-level classification (FR-015)

**Decision**: `contact_blocks` is extended to yield, for each `<contact>`, the `state` attribute of its enclosing
`<registration>` element (found by scanning back to the nearest preceding `<registration` open tag; `None` when
there is none, e.g. the minimal fixtures). `Deregistration::scope` is `IdentityDeregistered` when every current
entry's enclosing registration is `terminated`, else `BindingRemoved`. Log only; the policy uses the contact event
in both cases (spec Story 3).

## R7 — Rejection cooldown configuration

**Decision**: `reg_rejected_cooldown_sec`, default `1800`, accepted range `60..=86400`:
- `[vowifi].reg_rejected_cooldown_sec` (u64, default 1800) and `[[vowifi.line]].reg_rejected_cooldown_sec` (override);
- `[volte].reg_rejected_cooldown_sec` (`Option`, absent inherits `[vowifi]`'s value — the same rule as
  `sms_delivery_report`) and `[[volte.line]].reg_rejected_cooldown_sec` (override).

Resolved into `VowifiConfig::reg_rejected_cooldown` / the VoLTE runtime line, passed to
`DispatchParams::reg_rejected_cooldown: Duration`.

**Rationale**: FR-010 requires per-line; a section default keeps a single-line config to one key. `_sec` matches
the existing `keepalive_interval_sec` naming. The lower bound stops an operator from configuring the loop back in.

## R8 — Status reporting (FR-017)

**Decision**: `RegistrationStatus` gains an in-memory `rejected_until: Option<SystemTime>` (not persisted by
`render_status`, like `gm_connection`). `ControlMessage::RegistrationStatusReply` gains
`#[serde(default)] rejection_cooldown_until: Option<u64>`. Both status printers (`vowifi::mod`'s `vowifi-status`
and `volte::bridge::print_line_live_status`) print
`rejection_cooldown: until <ts> (next attempt in <N>s)` when set, `none` otherwise. `blocked_reason` already
reports "not registered" from `state == Failed`.

## R9 — Logging (FR-005, FR-016)

See `contracts/observability.md`. One structured line per reg-event NOTIFY carrying `attribution`,
`contact_addr`, `own_entries`, `retired_entries`, `contact_states`, `applied` (event or `none`), `scope`, and
`body`. Level: `warn` when an event is applied, `info` otherwise. A retired binding produces an additional
`info` line "replaced binding retired" with its address and event.

## R10 — Gm keepalive during the cooldown

**Decision**: leave `probe_gm_connection` running. Its escalation sets `force_renewal`, which the cooldown's
`next_renewal_attempt` already gates, so it cannot shorten the cooldown. Skipping it would add a branch for a
cosmetic gain (Principle V).
