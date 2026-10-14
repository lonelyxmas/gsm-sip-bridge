# Data Model: Reg-event NOTIFY binding attribution and per-event deregistration policy

All types are in-memory and crate-private unless noted. Nothing is persisted.

## `ReginfoContact` (`ims/session.rs`, extended)

One `<contact>` element of a reginfo document.

| Field | Type | Source | Notes |
|---|---|---|---|
| `state` | `String` | `state="…"` | existing |
| `event` | `String` | `event="…"` | existing; empty when absent |
| `addr` | `Option<SocketAddr>` | `<uri>` via `contact_uri_addr` | new; `None` = no checkable address |
| `retry_after` | `Option<u64>` | `retry-after="…"` attribute | new; seconds, RFC 3680 |
| `registration_state` | `Option<String>` | enclosing `<registration state="…">` | new; `None` if not nested |

## `Attribution` (existing enum, semantics narrowed)

`ContactAddr` | `Imei` | `SoleContact` | `None`. `Imei` now means only "IMEI echoed on an entry with no checkable
address" (research R1). `ContactAddr` takes precedence in the label when any current entry matched by address.

## `OwnContacts` (extended)

| Field | Type | Notes |
|---|---|---|
| `entries` | `Vec<ReginfoContact>` | the **current** binding's entries (≥ 1 per public identity) |
| `retired` | `Vec<ReginfoContact>` | new — IMEI-matched entries naming a different address (previous bindings) |
| `matched_by` | `Attribution` | how `entries` were found |

Validation: an element is in at most one of `entries` / `retired`.

## `DeregEvent` (new enum, `ims/session.rs`)

`Unregistered < Probation < Deactivated < Rejected` (derive `Ord`; FR-014 picks the max).
Parsed case-insensitively from `event` when `state` is `terminated`; `expired` and anything else → not a
deregistration (FR-013).

## `Deregistration` (new struct, returned by `handle_notify`)

| Field | Type | Notes |
|---|---|---|
| `event` | `DeregEvent` | most restrictive among current entries |
| `retry_after` | `Option<Duration>` | max of the current entries' `retry-after` and `Subscription-State`'s `retry-after` param |
| `scope` | `DeregScope` | `IdentityDeregistered` if every current entry's `registration_state` is `terminated`, else `BindingRemoved` |

`handle_notify(...) -> Option<Deregistration>` replaces today's `-> bool`.

## `LoopState` (`ims/agent/mod.rs`, extended)

| Field | Type | Notes |
|---|---|---|
| `rejection` | `Option<RejectionCooldown>` | new; `Some` while a `rejected` cooldown is running |

`RejectionCooldown { until: Instant, until_wall: SystemTime }` — `until` gates the loop, `until_wall` is what
status reports.

### State transitions (registration-loss handling)

```text
Registered ──NOTIFY current deactivated──► Failed, force_renewal
                 mature: next attempt now, uncounted
                 early, clean ladder: next attempt now, counted (+1)
                 early, dirty ladder: restored ladder, counted (+1), wait backoff
Registered ──NOTIFY current unregistered/probation──► Failed, force_renewal,
                 counted (+1), wait max(backoff, retry-after)
Registered ──NOTIFY current rejected──► Failed, call/origination ended,
                 ladder reset (5s, 0), NOT counted,
                 rejection = until now+max(cooldown, retry-after), next attempt = until
Failed(rejection) ──cooldown elapses──► Renewing (one attempt)
     ├─ success ──► Registered, rejection cleared, ladder clean
     └─ failure ──► Failed, normal ladder (counted)
Registered ──NOTIFY retired/foreign/expired only──► unchanged (logged)
```

Invariant (FR-012/SC-004): no transition triggered by `Rejected` calls `schedule_renewal_retry`, so the
consecutive-failure budget can only be spent by real REGISTER failures.

## `DispatchParams` (extended)

| Field | Type | Notes |
|---|---|---|
| `reg_rejected_cooldown` | `Duration` | resolved per line (contracts/config.md) |

## `RegistrationStatus` (`ims/mod.rs`, extended)

| Field | Type | Notes |
|---|---|---|
| `rejected_until` | `Option<SystemTime>` | new, in-memory only (not in `render_status`) |

## `ControlMessage::RegistrationStatusReply` (`vowifi/control.rs`, extended — wire type)

| Field | Type | Notes |
|---|---|---|
| `rejection_cooldown_until` | `Option<u64>` | new, `#[serde(default)]`; unix seconds |
