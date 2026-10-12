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
