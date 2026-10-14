# Data Model: P-CSCF pool and failover

All new types live in `gsm-sip-bridge/src/ims/agent/pcscf_pool.rs` unless stated. Times that cross processes are
`SystemTime` / Unix seconds; nothing here uses `Instant`.

## PcscfPool (per line, owned by the agent)

| Field | Type | Meaning |
|---|---|---|
| `pool` | `Vec<IpAddr>` | Latest negotiation's addresses, deduplicated, in publication order (FR-001, FR-003) |
| `selection` | `IpAddr` | The supervisor's single selection (`/tmp/pcscf-N`), unchanged semantics (FR-004) |
| `active` | `Option<IpAddr>` | The candidate the line is registered through, or is attempting |
| `last_registered` | `Option<IpAddr>` | The last address a REGISTER succeeded through; persisted (Q3) |
| `marks` | `BTreeMap<IpAddr, Mark>` | Failure bookkeeping per address; persisted |
| `state_path` | `PathBuf` | `/tmp/pcscf-N.state` (VoWiFi) or `/tmp/volte-pcscf-<card_id>.state` (VoLTE) |

**Derived: `candidates()`**. The addresses in this order, with duplicates removed:
1. `last_registered`, if it is in `pool` and unmarked;
2. `selection`;
3. the IPv4 addresses in `pool`, in pool order;
4. the IPv6 addresses in `pool`, in pool order.

With no pool published, `pool = [selection]` (FR-006).

### Mark

| Field | Type | Meaning |
|---|---|---|
| `until` | `SystemTime` | Unavailable until this instant; `≤ now` means expired |
| `failures` | `u32` | Consecutive qualifying failures of this address; reset to 0 by `on_registered` |
| `cause` | `FailureCause` | The most recent cause, for status and logs |

An address is **marked** when `marks[addr].until > now`. An expired `Mark` stays in the map so its `failures` count
keeps growing the delay. It is removed by `on_registered(addr)` and by `replace_pool` when the address leaves the
pool.

**Mark duration**:
- `FailureCause::RetryAfter(d)` (a `503` with `d > TIMER_F` = 128 s): `d`.
- Anything else: `min(1800 s, 30 s · 2^(failures−1)) + 300 s`, where `failures` already includes this failure.

### FailureCause (qualifying only)

| Variant | Source | FR |
|---|---|---|
| `RegisterTimeout` | `classify_register_failure` → `Timeout` | FR-007 |
| `Transport` | `classify_register_failure` → `Transport` (refused, connect timeout, send failure on a new connection) | FR-007 |
| `EndpointFailure(EndpointFailureCause)` | spec 090 `on_qualifying_endpoint_failure` | FR-007 |
| `GmKeepalive` | `probe_gm_connection` escalation (keepalive unanswered, reconnects exhausted) | FR-007, Q2 |
| `RetryAfter(Duration)` | `503` to REGISTER with `Retry-After > 128 s` | FR-008 |

`Display` gives stable kebab-case strings: `register-timeout`, `transport`, `endpoint-failure:<090 cause>`,
`gm-keepalive`, `retry-after`.

### FailureClass (in `ims/mod.rs`, from `classify_register_failure`)

`Timeout | Transport | NoRoute | PeerClosed | NotPcscf`. Only `Timeout` and `Transport` become a `FailureCause`.
`NoRoute`, `PeerClosed` and `NotPcscf` never mark (FR-009).

### Selection

| Field | Type |
|---|---|
| `addr` | `IpAddr` |
| `reason` | `SelectReason` |

**SelectReason** values: `startup`, `sticky` (the active P-CSCF is kept), `failover` (the previous one was just
marked), `pool-replaced` (the active one left the pool), and `all-unavailable` (the soonest-expiring mark was
chosen).

## Operations and state transitions

```text
                 on_failure(addr==active, cause, now)
   ┌──────────┐ ─────────────────────────────────────▶ ┌───────────┐
   │ eligible │                                        │  marked   │
   │(no mark /│ ◀───────────────────────────────────── │(until>now)│
   │ expired) │   time passes (until ≤ now)            └───────────┘
   └──────────┘ ◀─── on_registered(addr): failures=0, mark removed ──┘
         │
         └── replace_pool without addr → forgotten (mark + last_registered dropped)
```

| Operation | Effect | Persists |
|---|---|---|
| `load(state_path, selection, pool, now)` | Read the state file. Drop marks and `last_registered` for addresses not in `pool`. A missing or corrupt file is empty state (one `WARN`). | – |
| `select(now) → Selection` | Keep `active` if it is unmarked. Else take the first unmarked candidate. Else take the soonest-expiring mark (ties go to candidate order). Sets `active`. | – |
| `on_failure(addr, cause, now) → Option<Mark>` | `None` (logged at `INFO`) if `addr != active` (FR-017). Else increment `failures`, compute `until`, store. | yes |
| `on_registered(addr)` | `active = last_registered = addr`; remove `marks[addr]` | yes |
| `replace_pool(pool, selection)` | Swap the pool. Drop marks and `last_registered` for departed addresses. `active` keeps its value only if it is still in the pool. | yes, if anything changed |
| `has_unmarked(now) → bool` | Whether any candidate is eligible; decides failover-now versus the ladder (R7) | – |
| `snapshot(now) → PcscfSnapshot` | For status | – |

## PcscfSnapshot (in `ims/mod.rs`, on `RegistrationStatus.pcscf`)

| Field | Type | Meaning |
|---|---|---|
| `active` | `Option<IpAddr>` | |
| `pool` | `Vec<IpAddr>` | candidate order |
| `unavailable` | `Vec<(IpAddr, SystemTime)>` | only marks with `until > now` |

It is kept in memory only, like `reg_subscription`, and is not an input to `RegistrationStatus::health`
(`can_answer` is unchanged).

## Agent-side changes (`ims/agent/mod.rs`)

| Item | Change |
|---|---|
| `InboundParams.pcscf_pool` | `PcscfPool`, built by the caller: VoWiFi from `EpdgTransport`, VoLTE from `carrier_agent` |
| `LoopState.pool` | Moved in from `serve_inbound` after the startup registration |
| `on_idle_tick` renewal | `select` → `reg_cfg.with_pcscf` → `attempt_renewal`. On a qualifying failure: mark, count, then fail over now or take the ladder (R7). |
| `on_qualifying_endpoint_failure` | Also `pool.on_failure(session P-CSCF, EndpointFailure(cause))`, after the route check |
| `probe_gm_connection` | Returns `true` when it raised the escalation; the caller marks with `GmKeepalive` |

## Supervisor-side state (`supervise/orchestrate.rs`)

| Item | Type | Meaning |
|---|---|---|
| `current_pool` | `Vec<String>` | Last published pool, beside `current_pcscf`; compared as a set each tick |

## Files

See [contracts/files.md](contracts/files.md).
