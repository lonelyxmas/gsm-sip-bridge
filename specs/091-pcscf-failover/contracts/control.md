# Contract: status reply and status CLI (FR-020, SC-006)

## `ControlMessage::RegistrationStatusReply`: new field

| Field | Type | Default (older peer) | Meaning |
|---|---|---|---|
| `pcscf` | `Option<Box<PcscfStatusReply>>` | `None` | The line's P-CSCF choice. It is boxed, because three inline lists would make this the one oversized variant (`clippy::large_enum_variant`). |

`PcscfStatusReply` (JSON object):

| Field | Type | Meaning |
|---|---|---|
| `active` | `String` | The P-CSCF the line is registered through, or is attempting |
| `pool` | `Vec<String>` | Candidates in selection order (FR-005) |
| `unavailable` | `Vec<(String, u64)>` | Each address currently marked, with its expiry in Unix seconds |

`pcscf` is `#[serde(default)]`. A reply from an older agent still parses, and the CLI then prints `pcscf: unknown`
and `pcscf_unavailable: none`.

## `vowifi-status` / `volte-status`

Two lines follow `reg_subscription_last_failure` in each line's registration block. They are rendered by one shared
`vowifi::format_pcscf_lines`, which `volte/bridge.rs` reuses, as it does for `format_reg_subscription`:

```text
    gm_connection: up
    reg_subscription: confirmed, expires 2026-10-14T11:02:11+00:00
    reg_subscription_last_failure: none
    pcscf: 10.0.1.10 (pool: 10.0.1.10, 10.0.0.10)
    pcscf_unavailable: 10.0.0.10 until 2026-10-14T10:08:41+00:00
    rejection_cooldown: none
```

- **Nothing marked**: `pcscf_unavailable: none`.
- **Several marks**: comma-separated, in candidate order (`a until t1, b until t2`).
- **Older agent**: `pcscf: unknown` and `pcscf_unavailable: none`.
- **One-element pool** (VoLTE, or no `.pool` published): `pcscf: 10.0.0.10 (pool: 10.0.0.10)`.

Timestamps are RFC 3339 UTC, as in `gm_connection` and `reg_subscription`.

`can_answer` and `blocked_reason` are unchanged. A line that is mid-failover reports `state: Renewing` / `Failed` and
`last_failure` exactly as today.
