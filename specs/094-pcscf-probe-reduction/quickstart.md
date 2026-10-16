# Quickstart: verifying one P-CSCF probe per line

## Local (no hardware)

```bash
make format && make lint && make test
```

The tests that matter most:

- `config`: `idle_keepalive` defaults to `false`; `keepalive_interval_sec` defaults to `30`; a non-boolean `idle_keepalive` is rejected, naming the key; the key is rejected under `[[vowifi.line]]`.
- `orchestrate`: `idle_keepalive_interval` returns `None` by default and `Some(N s)` when on; `idle_keepalive_once` makes exactly one in-namespace connect when a P-CSCF is present and none when the file is blank.
- `healthcheck`: one test per row of [contracts/healthcheck.md](contracts/healthcheck.md), each asserting `tcp_connect_attempts()` stays empty.
- `vowifi::query_status_bounded` against a loopback listener: reply, silent peer (returns within about the read timeout), refused port.

## Live: counting P-CSCF connects

Count TCP SYNs to the P-CSCF's SIP port on the line's tunnel interface, inside the line's namespace. With strongSwan the tunnel interface is `ipsec-<N>` and the namespace is the line's `netns` from `vowifi-status` or the lines file:

```bash
ip netns exec <netns> tcpdump -ni ipsec-<N> -w /tmp/p5060.pcap \
  'tcp dst port 5060 and tcp[tcpflags] & (tcp-syn|tcp-ack) == tcp-syn'
# … wait 10 min, then
tcpdump -r /tmp/p5060.pcap | wc -l
```

Run this where `tcpdump` and `CAP_NET_ADMIN` are available (the privileged test container on the local rig). The agent's own unprotected initial REGISTER also opens a connection to `:5060`, so allow a handful of extra SYNs around registrations.

| Config | Expected SYNs / 10 min / line |
|---|---|
| default (`idle_keepalive` absent) | about 20, from the supervisor only (SC-001) |
| `idle_keepalive = true`, `keepalive_interval_sec = 30` | about 40 (SC-005) |

The HEALTHCHECK runs every 30s inside the container. To isolate it (SC-002), capture while running `gsm-sip-bridge --config /etc/gsm-sip-bridge/config.toml healthcheck` by hand three times in a row, and check that no SYN lines up with those runs.

Recreate the container after any config change; do not just restart it (stale `/tmp` state otherwise reads as a carrier fault).

## Live: required before merge (FR-016)

1. **Vodafone rig (local host)**, default config. Soak until at least one REGISTER refresh and one CHILD_SA rekey have happened (about 1h10m from tunnel up; longer covers an IKE_SA rekey too). Pass: registered throughout (`vowifi-status`), no `dead peer` / `IkeSaLost` in the logs, about 120 SYNs/h (SC-001, SC-004).
2. **Jio Pi**, default config, about 1h covering at least one REGISTER refresh. Same pass criteria (SC-004a).
3. **HEALTHCHECK** on either rig: three manual runs, zero SYNs, exit 0 (SC-002). Then stop the line's agent (kill the `vowifi-ims-agent` for that line) and run it again: it exits non-zero with `line N: IMS agent at …:5071 not responding: …` (SC-003). The supervisor restarts the agent, so run the check straight after the kill.
4. With `idle_keepalive = true` on either rig, 10 minutes: about 40 SYNs and the "keepalive on" startup line (SC-005).

Record the results in `specs/094-pcscf-probe-reduction/live-verification.md`.
