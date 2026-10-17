# Data model

## Pool state file (`/tmp/pcscf-N.state`, written by the agent, read by both)

Format `version: 1`, unchanged. One field is added to each mark ([contracts/files.md](contracts/files.md)).

| Field | Type | Notes |
|---|---|---|
| `version` | u32 | Always 1. Any other value means the agent starts clean and the supervisor sees "not exhausted". |
| `last_registered` | IP? | Unchanged. |
| `marks[].addr` | IP | Unchanged. |
| `marks[].until_unix` | u64 | Unchanged. When the mark ends. |
| `marks[].failures` | u32 | Unchanged. |
| `marks[].cause` | string | Unchanged: `gm-keepalive`, `register-timeout`, `transport`, `endpoint-failure:<c>` or `retry-after`. |
| `marks[].marked_at_unix` | u64, default 0 | **New.** The unix second at which the mark was last set or renewed. 0 means unknown, and such a mark never counts. |

Rust: `PoolState` and `PoolStateMark` in `ims::pcscf_files` (moved from `ims::agent::pcscf_pool`'s private `PersistedPool`/`PersistedMark`), plus `read_pool_state(path) -> Option<PoolState>`. The reader returns `None` for a missing, unreadable, unparseable or wrong-version file. The agent's in-memory `Mark` gains `marked_at: SystemTime`.

## `SteadyTracking` (supervisor, per line, in memory)

| Field | Type | Notes |
|---|---|---|
| `negotiated_at` | `Option<SystemTime>` | **New.** The baseline of research R3. `None` disables the check. |
| `pool_exhausted` | `Vec<ExhaustedMember>` | **New.** Filled by the tick that returns `Recovered{PcscfPoolExhausted}`, empty otherwise. The orchestrator reads it for the log line and the event. |
| `pool_exhausted_deferral_logged` | `bool` | **New.** The FR-007 once-per-episode deferral log. Cleared when the pool is no longer exhausted. |

`ExhaustedMember { addr: String, cause: String, until_unix: u64, marked_at_unix: u64 }`

## `exhausted_pool(pool, current, state, baseline, now) -> Option<Vec<ExhaustedMember>>`

The members are `pool` when it is non-empty, otherwise `[current]` (or none when `current` is empty too). The result is `Some(members)` exactly when the member list is non-empty and every member has a mark with all three of:
- `until_unix > now`;
- `marked_at_unix >= baseline` (with `baseline` as unix seconds, rounded down);
- `cause != "retry-after"`.

Otherwise the result is `None`. Members are compared as parsed `IpAddr`, so `2001:db8::1` and `2001:0db8::1` are the same member.

## Recovery reason

`DegradeReason::PcscfPoolExhausted` ↔ `TunnelRecoveryReason::PcscfPoolExhausted`, label `pcscf_pool_exhausted`. Its flags:
- `recovery_restarts_agent`: true
- `recovery_renegotiates`: true

## Wire event (supervisor → daemon)

`ObservedEvent::TunnelRecovered { reason, pcscfs, rebuilt_at_unix }`. Both new fields are filled only for `PcscfPoolExhausted`; every other reason keeps its existing wire shape.
- `pcscfs: Vec<String>` is `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
- `rebuilt_at_unix: Option<u64>` is `#[serde(default, skip_serializing_if = "Option::is_none")]`.

## Alert episode (daemon, on the supervisor's `AgentRecord`)

| Field | Type | Notes |
|---|---|---|
| `pool_exhausted_since` | `Option<Instant>` | Set by the first `PcscfPoolExhausted` event; `None` once the episode ends. |
| `pool_exhausted_alert_phase` | `AlertPhase` | As for every other category. |
| `pool_exhausted_rebuilds` | u32 | Incremented by each event in the episode. |
| `pool_exhausted_rebuilt_at_unix` | `Option<u64>` | When the latest rebuild was issued, from the event. |
| `pool_exhausted_generation` / `_ended` | `Option<Instant>` / bool | The `Failure` in flight, and whether its episode has already ended. |

On every record, `epoch_first_seen_unix` records when the daemon first saw the record's current epoch.

Side map `module_id → Vec<String>`: the latest event's `pcscfs`, used for the alert text.

State transitions:

```text
Idle (since=None)
  -- TunnelRecovered{PcscfPoolExhausted, rebuilt_at} --> Open(since=now, rebuilds=1, rebuilt_at)
Open
  -- another such event --> Open(rebuilds+1, rebuilt_at)
  -- IMS report registered=true from an agent first seen after rebuilt_at --> Idle (since=None)
     (if a Failure is still Pending: ended=true, and its callback sends the all-clear)
decide_transition(since, phase, unhealthy_sec) on every supervisor report:
  since older than threshold, Idle phase -> Failure alert (Pending -> Alerted)
  since=None, Alerted -> Recovered alert
```
