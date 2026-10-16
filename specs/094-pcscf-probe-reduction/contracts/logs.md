# Contract: logs

Logged once by `start_vowifi_subsystem`, at `info`, before any line starts. It is not repeated per line or on re-discovery.

Off (default):

```text
idle-tunnel keepalive off ([vowifi].idle_keepalive = false); P-CSCF reachability is probed by the line supervisor only
```

On:

```text
idle-tunnel keepalive on: TCP connect to each line's P-CSCF :5060 every <N>s ([vowifi].idle_keepalive = true)
```

Structured fields: `idle_keepalive` (bool) and, when on, `interval_sec`. No other log output changes. The keepalive loop itself stays silent, as it is today.
