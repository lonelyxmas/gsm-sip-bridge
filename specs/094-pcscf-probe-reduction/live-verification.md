# Live verification: one P-CSCF probe per line (#120 item 1)

**Date**: 2026-10-16
**Branch**: `094-pcscf-probe-reduction` at `95b8189`
**Images**: `gsm-sip-bridge:probe-094-local` (Vodafone rig, amd64),
`ghcr.io/selvakn/gsm-sip-bridge:probe-094-arm64` (Jio Pi, built natively on arm64)

Both rigs ran the **default** configuration: `test/config.toml` and the Pi's
`config.toml` set neither `idle_keepalive` nor `keepalive_interval_sec`. Each
container was **recreated** (`down` then `up -d`), not restarted. On startup both
logged:

```
idle-tunnel keepalive off ([vowifi].idle_keepalive = false); P-CSCF reachability is probed by the line supervisor only
```

P-CSCF addresses are redacted to role names: **Vodafone P-CSCF** (IPv6) and
**Jio P-CSCF** (IPv4).

## Method

Each capture ran inside the line's IMS namespace on the tunnel interface
(`tun23-0`), filtered to `tcp dst port 5060`. Only SYNs were counted, so each
count is one TCP connect to the P-CSCF.

- **Vodafone rig**: a privileged sidecar ran `ip netns exec ims0 tcpdump ...`.
- **Jio Pi**: the Pi has no `/var/run/netns`, so the capture used
  `nsenter -t <ims-agent pid> -n tcpdump ...`.

Intervals between consecutive SYNs were checked against a 25–35s band.

## Results

| Criterion | Rig | Result | Evidence |
|---|---|---|---|
| SC-001 | Vodafone | **Pass** | 24 SYNs in the first 10 min (11:39:22–11:50:54Z), every 30–31s. 135 SYNs over 67 min (≈121/h), down from ≈360/h before the change. |
| SC-002 | Vodafone | **Pass** | 3 HEALTHCHECK runs at 11:39:48, 11:39:50 and 11:39:52Z, all exit 0. No SYN fell outside the 30s supervisor cadence; the 11:39:52 SYN is the supervisor's scheduled one. |
| SC-003 | Vodafone | **Pass** | Killed the line's `vowifi-ims-agent` (SIGKILL), then ran `healthcheck` at once: exit 1, stderr `line 0: IMS agent at 10.99.0.1:5071 not responding: IMS error: connect to 10.99.0.1:5071 failed: Connection refused (os error 111)`. |
| SC-004 | Vodafone | **Pass** | Registered without a break from 11:31Z to 12:46Z. The REGISTER refresh succeeded at 12:26:26Z, and the CHILD_SA rekeyed at about 12:28Z (IKE SA still `ESTABLISHED`). No `dead peer`, `IkeSaLost`, rebuild or restart in the logs. Docker health: `healthy`. |
| SC-004a | Jio | **Pass** | 132 SYNs over 66 min (≈120/h). Registered without a break from 11:40Z to 12:46Z. The REGISTER refresh succeeded at 12:35:42Z, and the CHILD_SA rekeyed at about 12:38Z. No `dead peer`, `IkeSaLost`, rebuild or restart. Docker health: `healthy`. |
| SC-005 | Vodafone | **Pass** | `idle_keepalive = true`, container recreated. Startup logged `idle-tunnel keepalive on: TCP connect to each line's P-CSCF :5060 every 30s`. 40 SYNs in 10 min (12:58:24–13:07:56Z, ≈240/h): one supervisor and one keepalive connect every 30s, 0–1s apart. |

### Off-cadence SYNs

Each soak had exactly one SYN outside the 25–35s band, and it lands on the
REGISTER refresh time (`registered_at` from `vowifi-status`):

- Vodafone: an extra SYN at 12:26:25Z; `registered_at` is 12:26:26Z.
- Jio: an extra SYN at 12:35:42Z (Pi local time 18:05:42); `registered_at` is 12:35:42Z.

That is the IMS agent opening a fresh Gm connection for the refresh. It is
neither a probe nor a keepalive, and it is not counted against SC-001.

### SC-003 notes

SC-003 used a one-off `healthcheck` run straight after the kill. The Docker
HEALTHCHECK interval and retries are unchanged (30s, 3 retries), so a line
whose agent stays down turns the container `unhealthy` within the same window
as before. A single killed agent is restarted by the supervisor and needs no
HEALTHCHECK intervention.

## Deviations from tasks.md

- T019 asks for the next `jio-gmN` tag. The Pi image was tagged
  `probe-094-arm64` instead, so this verification build stays out of the
  `jio-gmN` release sequence. The previous compose file is kept as
  `docker-compose.yaml.bak-pre-probe-094-arm64`.
