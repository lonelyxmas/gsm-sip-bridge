# Data Model: Per-line inner address family

## TunnelAddressFamily (new enum, `config` module)

| Variant | TOML spelling | `@VIPS@` | `@REMOTE_TS@` | `admits(addr)` |
|---|---|---|---|---|
| `Dual` (default) | `dual` | `0.0.0.0, ::` | `0.0.0.0/0, ::/0` | every address |
| `Ipv4` | `ipv4` | `0.0.0.0` | `0.0.0.0/0` | IPv4 addresses only |
| `Ipv6` | `ipv6` | `::` | `::/0` | IPv6 addresses only |

- Derives `Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize`. Serde uses the lowercase spelling.
- `FromStr` accepts exactly the three lowercase spellings. Anything else is an error that lists them.
- `Display` prints the lowercase spelling, used in logs and error messages.
- `admits(addr: &str)` classifies by `addr.parse::<IpAddr>()`. An unparseable address is admitted only by `Dual`, which keeps today's behaviour.

## Where it lives

| Struct | Field | Type | Notes |
|---|---|---|---|
| `config::raw::RawVowifi` | `tunnel_address_family` | `String` | Defaults to `"dual"`. Validated in `config/build.rs`. |
| `config::raw::RawVowifiLine` | `tunnel_address_family` | `Option<String>` | `None` means inherit. |
| `config::VowifiConfig` | `tunnel_address_family` | `TunnelAddressFamily` | `#[serde(default)]`, so an older line-resolution file reads as `Dual`. After `resolve_one_line`, this is the line's **effective** value. |
| `config::VowifiLineOverride` | `tunnel_address_family` | `Option<TunnelAddressFamily>` | `#[serde(default)]`. |
| `supervise::render::SwanctlEpdgParams` | `address_family` | `TunnelAddressFamily` | Supplies `@VIPS@` and `@REMOTE_TS@`. |
| `supervise::engines::StrongswanEngine` | `address_family` | `TunnelAddressFamily` | Filters P-CSCFs (FR-013) and gates the rekey warning (FR-009a). |

## Resolving the effective value

The effective value is the first of these that is set:

1. `[[vowifi.line]].tunnel_address_family`
2. `[vowifi].tunnel_address_family`
3. `dual`

`config/build.rs` validates both levels. `vowifi::discovery::resolve_one_line` writes the result into the line's `VowifiConfig`.

## Validation rules

| Rule | Where | Error names |
|---|---|---|
| Value is not one of `dual`, `ipv4`, `ipv6` | `config/build.rs`, at each level | `vowifi.tunnel_address_family` or `vowifi.line[{i}].tunnel_address_family`, the value, the accepted values |
| A value other than `dual` while `tunnel_engine = "swu"` | `config/build.rs`, after both levels are parsed | where the value came from, and that the setting needs `tunnel_engine = "strongswan"` |

## Supervisor state for the rekey warning

| State | Owner | Purpose |
|---|---|---|
| `warned_inval_sel_session: Option<String>` | the line's steady-state loop in `orchestrate.rs` | The IKE_SA session tag (`ims0[5]`) already warned about, so there is one warning per tunnel session. |

The tag changes when a new IKE_SA is established, so a rebuilt session that is rejected again warns again. Nothing is persisted: after a supervisor restart, the current session can warn once more.
