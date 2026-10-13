# Quickstart: verifying specs/086 (and the SC-008 merge gate)

## 1. Automated

```bash
make format
make lint      # whole workspace incl. test targets, -D warnings
make test
```

Key tests:

| What | Where |
|---|---|
| CHILD_SA parser vs live 5.9.3 output shapes | `gsm-sip-bridge/src/supervise/engines.rs` tests |
| Bounded `--force` terminate argv | `engines.rs` tests (mock runner) |
| Miss → re-initiate, 2nd miss → rebuild, reset rules, route-repair precedence | `gsm-sip-bridge/src/supervise/line_supervisor.rs` tests |
| `child_sa_up` ingest, stale zeroing, alert de-duplication | `gsm-sip-bridge/src/metrics/ingest.rs`, `metrics/server.rs` tests |
| Supervisor report over the real control socket → `/metrics` | `gsm-sip-bridge/tests/test_observability_ingest.rs`, `tests/test_metrics_endpoint.rs` |

## 2. Live gate on the local Vodafone line (blocking — SC-008)

Build a local image from the branch, point `test/docker-compose.yaml`'s pinned `image:` at it,
and **recreate** (not restart) the container. Stale container `/tmp` state reads as a carrier
fault otherwise.

```bash
docker build -f docker/Dockerfile -t gsm-sip-bridge:child-sa-recovery-local .
# edit test/docker-compose.yaml: image: gsm-sip-bridge:child-sa-recovery-local
(cd test && docker compose up -d --force-recreate)
docker logs -f gsm-sip-bridge-test 2>&1 | grep -E '\[supervise\]'
```

### 2a. Gauge reads down during establishment, up once established

```bash
curl -s localhost:9091/metrics | grep -E 'vowifi_child_sa_up|agent_up\{agent="supervisor"'
```

Expect `vowifi_child_sa_up{module=…} 0` in the first seconds after start, then `1` after
`[supervise] line 0: tunnel UP`.

### 2b. Natural rekey: no false recovery (≥ 1 rekey, ~55–60 min)

```bash
docker exec gsm-sip-bridge-test sh -c 'grep -c "closing CHILD_SA" /tmp/charon.log'   # before / after
docker logs gsm-sip-bridge-test 2>&1 | grep -c 'recovered from ChildSa'              # must stay 0
```

Pass: the `closing CHILD_SA` count went up by at least 1, there are zero `ChildSaMissing`/`ChildSaStuck` recoveries,
and the gauge stayed at 1 (sample it every 30s across the rekey).

### 2c. Induced CHILD_SA loss, IKE_SA kept up

```bash
docker exec gsm-sip-bridge-test sh -c \
  'STRONGSWAN_CONF=/etc/strongswan-shared.conf swanctl --terminate --child ims0'
docker exec gsm-sip-bridge-test sh -c \
  'STRONGSWAN_CONF=/etc/strongswan-shared.conf swanctl --list-sas --ike ims0'   # IKE_SA still ESTABLISHED, no child
```

Pass:
- Within 30s: `[supervise] line 0: ims0 CHILD_SA missing; re-initiating` and `recovered from ChildSaMissing`.
- The gauge reads 0 at the next scrape after detection, and 1 again once the CHILD_SA is back.
- The line re-registers without operator action. Place one inbound test call to confirm it rings.
- The escalation (`ChildSaStuck`) is **not** expected here, because Vodafone answers the re-initiate. It is covered by unit tests.

### 2d. Record results

Append a "Live gate results" section to `specs/086-epdg-child-sa-recovery/quickstart.md`.
Include timestamps, rekey count, recovery log lines and gauge samples. Use **no** IMSI/MSISDN, and
synthesise any identity that appears in `swanctl` output.

## 3. Live gate results (SC-008), 2026-10-13

Build: `gsm-sip-bridge:child-sa-recovery-local` from branch `086-epdg-child-sa-recovery`
(head `fix(alerts): hold the phase until a late all-clear has been sent`), run in `test/` on the
local Vodafone line, strongSwan engine, one line. Container recreated at 01:57:45Z.

| Check | Result |
|---|---|
| 2a gauge during establishment | **Pass.** `vowifi_child_sa_up` read 0 at 01:57:50Z (tunnel still establishing), 1 at 01:58:05Z once `tunnel UP`. `agent_up{agent="supervisor"}` appeared and read 1. |
| 2b natural rekey | **Pass.** The CHILD_SA rekeyed on its own at 02:53:07Z (`ims0{2}` replaced by `ims0{3}`; `closing CHILD_SA` count went 1 → 2). The gauge was sampled every 30s from 01:59:05Z to 03:13:38Z (150 samples): every one read 1, including across 02:53:07Z. No `recovered from ChildSa*` line after the induced one in 2c. |
| 2c induced CHILD_SA loss | **Pass.** See the timeline below. |
| 2c inbound call | **Pass.** An inbound carrier call rang the registered test phone, was answered and bridged, ran 20s and ended with a clean BYE; about 20s of audio was recorded each way. |

### 2c timeline (UTC)

```
01:58:13  swanctl --terminate --child ims0          # IKE_SA left up
01:58:22  [supervise] line 0: ims0 CHILD_SA missing; re-initiating
01:58:22  [supervise] line 0: recovered from ChildSaMissing        # 9s after the loss
01:58:22  gauge 1 -> 0
01:58:27  CHILD_SA ims0{2} established (new virtual IP, new P-CSCF pool)
01:58:52  [supervise] line 0: P-CSCF changed (...); refreshing     # next 30s tick
01:58:52  gauge 0 -> 1
          vowifi-ims-agent restarted, "registered after restart loop"
```

Gauge samples around the event, every 3s: `1` (01:58:13 to 01:58:19), `0` from 01:58:22 to 01:58:49, `1` from 01:58:52.

Notes:
- `ChildSaStuck` did not occur, as expected: Vodafone answered the re-initiate. The escalation is covered by unit tests.
- The re-initiate negotiated a different P-CSCF pool than the one read at recovery time, so the change was only noticed on the following tick, 30s later. Recovery still completed well inside the one-minute target.
