# Quickstart: verifying specs/087 (and the SC-007 merge gate)

## 1. Automated

```bash
make format
make lint      # whole workspace incl. test targets, -D warnings
make test
```

Key tests:

| What | Where |
|---|---|
| IKE_SA presence parser, loss-cause parser, closed-batch P-CSCF after a mark | `gsm-sip-bridge/src/supervise/engines.rs` tests |
| Graceful → presence → forced teardown argv and branches | `engines.rs` tests (mock runner) |
| `IkeSaMissing` tick branch, two-strike path for `CONNECTING`, `recovery_restarts_agent`, `RebuildWindow` | `gsm-sip-bridge/src/supervise/line_supervisor.rs` tests |
| P-CSCF wait (synced / timeout / shutdown / rotated log), rebuild gate, Gm flush before spawn (strongswan only) | `gsm-sip-bridge/src/supervise/orchestrate.rs` tests |
| Wire round-trip of `tunnel_flapping` / `ike_sa_loss_cause` | `gsm-sip-bridge/src/control/protocol.rs` tests |
| Flapping alert (3 vs 2 rebuilds), single alert with overlapping outage, all-clear, cause text (SC-006a) | `gsm-sip-bridge/src/metrics/ingest.rs` tests |

## 2. Live gate on the local Vodafone line (blocking — SC-007)

Build a local image from the branch, point `test/docker-compose.yaml`'s pinned `image:` at it, and **recreate** (not restart) the container.

```bash
docker build -f docker/Dockerfile -t gsm-sip-bridge:ike-sa-loss-local .
# edit test/docker-compose.yaml: image: gsm-sip-bridge:ike-sa-loss-local
(cd test && docker compose up -d --force-recreate)
docker logs -f gsm-sip-bridge-test 2>&1 | grep -E '\[supervise\]'
```

Shorthand used below:

```bash
SW='STRONGSWAN_CONF=/etc/strongswan-shared.conf swanctl'
```

### 2a. Probes for R1 and R3

```bash
# While a tunnel is coming up (right after recreate), the IKE_SA should list as CONNECTING:
docker exec gsm-sip-bridge-test sh -c "$SW --list-sas --ike ims0"
# Against a live SA, a graceful terminate should complete and leave nothing listed:
docker exec gsm-sip-bridge-test sh -c "$SW --terminate --ike ims0 --timeout 5; echo rc=\$?; $SW --list-sas --ike ims0"
```

Record: whether `CONNECTING` was ever listed, the graceful terminate's exit code, and whether the list was empty afterwards.

### 2b. Induced IKE_SA loss (SC-001, SC-002)

```bash
docker exec gsm-sip-bridge-test sh -c "$SW --terminate --ike ims0"
docker logs --since 2m gsm-sip-bridge-test 2>&1 | grep -E 'IkeSaLost|P-CSCF synced|no new negotiation|P-CSCF changed|vowifi-ims-agent'
```

Pass (repeat 3 times, a few minutes apart):

- Within one tick: `recovered from IkeSaLost (cause: unknown)`.
- Then `P-CSCF synced from new negotiation: …`.
- Exactly one agent restart, followed by a registration.
- **No** `P-CSCF changed` on the next tick.
- `vowifi_child_sa_up` goes 1 → 0 → 1.
- An inbound call to the line rings, is answered and is bridged.
- The time from the terminate to the registration is under 75s.

### 2c. Tunnel-interface loss: acknowledged teardown (SC-003)

```bash
docker exec gsm-sip-bridge-test ip netns exec ims0 ip link del tun23-0
```

Pass:

- `recovered from TunVanished`.
- `IKE_SA deleted (acknowledged)`, and no `forced` line.
- One agent restart.
- No `P-CSCF changed` on the next tick.

### 2d. Gm state does not leak (SC-005)

```bash
count() { docker exec gsm-sip-bridge-test sh -c 'ip netns exec ims0 ip xfrm state | grep -c ^src; ip netns exec ims0 ip xfrm policy | grep -c ^src'; }
count
for i in 1 2 3; do docker exec gsm-sip-bridge-test pkill -f 'vowifi-ims-agent --line 0$'; sleep 40; done
count
```

Pass: the counts after equal one registration's worth, the same as before. When a previous agent left state behind, the log shows `cleared K stale Gm SAs / P policies before agent start`.

### 2e. Flapping alert (SC-006a, live)

Set `[alerts.tunnel_failure] enabled = true, unhealthy_sec = 300` with a test webhook, then recreate. Run 2b's terminate three times within 5 minutes.

Pass:

- `tunnel flapping (3 rebuilds within 300s)`.
- Exactly one alert, reading `rebuilt repeatedly … last IKE_SA loss: unknown`.
- After 5 quiet minutes, `tunnel no longer flapping` and one all-clear.

### 2f. No regression from #118 (SC-006)

- `docker exec gsm-sip-bridge-test sh -c "$SW --terminate --child ims0"` gives `recovered from ChildSaMissing`, with **no** agent restart and no rebuild counted.
- Leave the line up across one natural CHILD_SA rekey (~55–60 min). There are zero recoveries, and the gauge stays at 1.

### 2g. Opportunistic: a real carrier DELETE

If one occurs during the soak, `grep 'received DELETE for IKE_SA' /tmp/charon.log` should line up with a `recovered from IkeSaLost (cause: carrier DELETE)`.

## 3. Results (fill in before merge)

Redact everything: no IMSI, MSISDN, real inner or P-CSCF addresses. Use `198.51.100.10` / `2001:db8::1`.

| Step | Date (UTC) | Result | Notes |
|---|---|---|---|
| 2a | | | |
| 2b ×3 | | | |
| 2c | | | |
| 2d | | | |
| 2e | | | |
| 2f | | | |
