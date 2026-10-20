# Quickstart: verifying the DPD tuning (098)

## Local (no hardware)

```bash
CARGO_TARGET_DIR=$PWD/target-098 make format lint test
```

Key tests: config range, template render (`dpd_delay = 120s` / `0s`), `inbound_esp` parser, `classify_dead_peer` table, the tick's IkeSaLost/DeadPeer wiring, event wire shape, and `tests/test_vowifi_tunnel_recovery_metrics.rs` for the counter.

## Live

Rigs as in 096/097: **Vodafone** on the local `test/` compose; **Jio** on the Pi (`pi@192.168.100.2`, docker-compose v1, arm64 via `arm-build-200`). **Recreate** the container after every image or config change; wait for `vowifi-status` → `Registered`.

### Probe-forced build

On a throwaway detached commit on top of this branch, make `StrongswanEngine::pcscf_reachable` return `true` without connecting. Never push it.

### 1. Reproduce run C at 30s (US1, R7)

1. Config: `dpd_delay_sec = 30`. Deploy the probe-forced build.
2. Start captures (rotate hourly):
   ```bash
   docker exec -d <c> tcpdump -i any -G 3600 -w /tmp/cap/outer-%H.pcap 'udp port 500 or udp port 4500'
   docker exec -d <c> ip netns exec ims0 tcpdump -i tun23-0 -G 3600 -w /tmp/cap/inner-%H.pcap
   ```
   and copy `/tmp/charon.log` out every 10 minutes.
3. Run until each carrier shows `giving up after 5 retransmits`, or 12h.
4. For each event, record the timeline per research R7, and the supervisor's `dead peer verdict` line.

### 2. Soak at the default (SC-001, SC-002, SC-006)

Remove `dpd_delay_sec` (default 120). Probe-forced build. 12h idle per carrier. Expect zero `giving up after`, at least one REGISTER refresh, one CHILD_SA rekey, one IKE_SA rekey. Count outer DPD requests (empty INFORMATIONAL from us) in one idle hour and compare with step 1.

### 3. Black-hole the ePDG (US4, SC-003)

On the probe-forced build at the default, drop the outer path to the ePDG:

```bash
docker exec <c> ip route add blackhole <epdg-ip>/32   # default netns of the container
```

Record: fault time, first unanswered DPD, `giving up`, `recovered from …`, the verdict (expect `real`), and re-registration after `ip route del`. Bound: rebuild begun within 315s.

### 4. Default build sanity (SC-004)

Real branch image (probe on), 1h idle per rig: no DPD exchanges in the outer capture, no new recoveries.

### 5. #137 gap (US5, R8)

Vodafone, probe-forced, P-CSCF black-holed with a /32 to the veth peer as in 097 quickstart step 1, with the agent log streamed out of the container. Account for every interval.

Record everything in `live-verification.md`, with addresses redacted to role names.
