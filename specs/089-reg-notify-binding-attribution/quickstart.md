# Quickstart: validating 089

## Q0 — Automated (CI)

```bash
make format && make lint && make test
```

Covers: attribution tables (R1), the #124 fixture (old `:5064` retired, current `:5070`), the #117 fixtures
unchanged (SC-005), IPv6 and URI-param matching, event ordering (FR-014), retry-after extraction, and the
`LoopState` policy with an injected clock: deactivated first/repeat, unregistered/probation floors, rejected
cooldown, 24h of repeated `rejected` with zero exits (SC-004).

## Q1 — Live, the #124 pattern (blocking: SC-001)

On a carrier that retires replaced bindings (the reporter's, or Jio on the remote Pi — see
`test/` for the pinned-tag local setup):

1. Deploy the build; let the line register and run ≥ 3 h (or force renewals with a short granted expiry).
2. Count:
   ```bash
   grep -c '"replaced binding retired"' <log>
   grep -c 'our current binding was deregistered' <log>
   grep -c 'registration renewed' <log>
   ```
   Expect: retired ≈ renewals, deregistered = 0, renewals at the normal cadence only.

## Q2 — Live, Vodafone attribution check (blocking: research R1 risk)

On the local Vodafone VoWiFi rig, after the initial registration:

```bash
grep 'reg-event NOTIFY' <log> | grep -o 'attribution=[a-z-]*' | sort | uniq -c
```

Expect `attribution=contact-addr` with `own_entries=2`. If it reads `imei` or `none`, or a retired entry names the
*current* port with different formatting, stop: Vodafone rewrites `<uri>` and R1 needs revisiting before merge.

## Q3 — Status output

```bash
gsm-sip-bridge vowifi-status   # and volte-status on a VoLTE deployment
```

Expect a `rejection_cooldown: none` line per line. (The `until …` form is covered by unit tests; a live
`rejected` cannot be provoked on demand.)
