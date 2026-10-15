# Contract: `tunnel_address_family` configuration key

## Keys

```toml
[vowifi]
# tunnel_address_family = "dual"     # default for every line

[[vowifi.line]]
modem_serial = "abc123def456"
tunnel_address_family = "ipv4"       # this line only
```

| Key | Type | Default | Accepted |
|---|---|---|---|
| `[vowifi].tunnel_address_family` | string | `"dual"` | `"dual"`, `"ipv4"`, `"ipv6"` |
| `[[vowifi.line]].tunnel_address_family` | string | inherits `[vowifi]` | `"dual"`, `"ipv4"`, `"ipv6"` |

## Errors (bridge refuses to start)

The exact wording may vary. Each error must name the key path, the value and the accepted values.

```text
vowifi.tunnel_address_family must be "dual", "ipv4" or "ipv6", got "ip4"
vowifi.line[1].tunnel_address_family must be "dual", "ipv4" or "ipv6", got "IPv4"
vowifi.line[0].tunnel_address_family = "ipv4" requires tunnel_engine = "strongswan" (the swu engine always negotiates both families); remove it or set "dual"
vowifi.tunnel_address_family = "ipv6" requires tunnel_engine = "strongswan" (the swu engine always negotiates both families); remove it or set "dual"
```

## Documentation obligations

- **`config.toml.example`**: document the key in `[vowifi]`, and include it in a commented `[[vowifi.line]]` example. The text must cover:
  - what each value requests;
  - the symptom: `N(INVAL_SEL)` in `CREATE_CHILD_SA` responses in the charon log, about a minute without service roughly every hour, and tunnel-missing or tunnel-stuck recoveries logged by the supervisor;
  - the remedy for that symptom (`ipv4`);
  - when to go back to `dual` (the carrier starts assigning an IPv6 inner address);
  - the risk of `ipv6` on an IPv4-only carrier;
  - that it needs the strongswan engine;
  - that it takes effect when the container is recreated.
- **`docs/configuration.md`**: a row in the `[vowifi]` table and a row in the `[[vowifi.line]]` table. `tests/test_config_docs.rs` fails if either row is missing.
