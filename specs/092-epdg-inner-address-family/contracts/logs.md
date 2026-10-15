# Contract: supervisor log lines

All lines go to the supervisor's stdout with the existing `[supervise] line {idx}:` prefix. Tests and operators can match the quoted text.

## Effective family (FR-009)

Printed once each time the line renders its tunnel connection:

```text
[supervise] line 0: tunnel address family: ipv4
```

## Rekey rejected with `INVALID_SELECTORS` (FR-009a)

Printed only for lines whose effective family is `dual`. It appears at most once per IKE_SA session: the first steady-state tick after the charon log shows `CREATE_CHILD_SA response ... N(INVAL_SEL)` for this line's connection, inside the current IKE_SA session.

```text
[supervise] line 0: ePDG rejected the CHILD_SA rekey's traffic selectors (INVALID_SELECTORS); if this repeats, set tunnel_address_family = "ipv4" for this line (see config.toml.example)
```

The bridge never changes the setting itself.

## Ignored P-CSCF of the other family (FR-013)

Printed for lines on `ipv4` or `ipv6`, at each P-CSCF publication point, when the negotiation advertised addresses that the family excludes:

```text
[supervise] line 0: ignoring P-CSCF 2402:8100::10 (tunnel_address_family = ipv4)
```

There is one line per ignored address. Publication points happen once per negotiation or pool change, so this does not repeat every tick.
