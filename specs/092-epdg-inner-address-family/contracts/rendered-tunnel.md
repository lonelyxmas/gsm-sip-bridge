# Contract: the rendered per-line ePDG connection

The supervisor writes `/etc/swanctl/conf.d/epdg-{idx}.conf` for each line from `docker/strongswan/swanctl-epdg.conf.template`. This feature affects only two of its setting lines.

## `dual` (the default)

```conf
        vips = 0.0.0.0, ::
...
                remote_ts = 0.0.0.0/0, ::/0
```

Every non-comment line is byte-identical to the current release's (FR-003, research R2). This is checked against an `insta` snapshot recorded before the template changes.

## `ipv4`

```conf
        vips = 0.0.0.0
...
                remote_ts = 0.0.0.0/0
```

## `ipv6`

```conf
        vips = ::
...
                remote_ts = ::/0
```

## Invariants

- For `ipv4` and `ipv6`, every non-comment line other than `vips` and `remote_ts` equals the `dual` render.
- No `@VIPS@` or `@REMOTE_TS@` token is left in the output for any family.
- Each line's file reflects only that line's effective family (FR-006).

## `render swanctl-epdg` subcommand

`--address-family <dual|ipv4|ipv6>`, optional, defaults to `dual`. Without the flag the output is the same as the `dual` render above.
