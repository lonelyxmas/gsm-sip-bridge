# Quickstart: verifying the pool-exhaustion rebuild

## Local (no hardware)

```bash
CARGO_TARGET_DIR=$PWD/target-097 make format lint test
```

The key tests:

- `pcscf_files`: format round-trip, and old-file compatibility in both directions.
- `line_supervisor`: `exhausted_pool` truth table; tick rows 7 and 7a of [contracts/steady-tick.md](contracts/steady-tick.md).
- `metrics::ingest`: episode start, continue and end; one failure; suppression; all-clear; and the stale-epoch rule.
- `tests/test_vowifi_pool_exhausted_alert.rs`: reporter → socket → ingest, covering both the counter and the episode.

## Live (all runs required before merge, Clarifications Q3)

Use the same deployments as spec 096:
- **Vodafone**: the local `test/` compose.
- **Jio**: the Pi (`pi@192.168.100.2`, docker-compose v1, arm64 via `arm-build-200`).

**Recreate** the container, and wait for `vowifi-status` to show `Registered`.

Point `[alerts]` at a test webhook, and set `[alerts.pcscf_pool_exhausted]` to the default 300s.

### 1. Vodafone, probe forced to pass (SC-001, SC-007)

1. On a throwaway detached commit, make `StrongswanEngine::pcscf_reachable` return `true`. Build the image, deploy it and recreate the container.
2. Black-hole the only P-CSCF (IPv6 Gm):
   ```bash
   docker exec gsm-sip-bridge-test ip -n ims0 -6 route add <pcscf>/128 dev veth-ims0
   ```
3. First check (research R8): within about 3 minutes, `/tmp/ims-agent-0.out` shows `P-CSCF marked unavailable` for the P-CSCF.
4. Expect `recovered from PcscfPoolExhausted` in `docker logs` within 200s of the black-hole. Then:
   - the line re-registers if the carrier hands out a different P-CSCF;
   - if it hands out the same one, the loop repeats, each cycle spaced by the agent's retry.
5. Leave the route in place for at least 6 minutes. Exactly one `pcscf_pool_exhausted` Discord alert arrives.
6. Delete the route:
   ```bash
   docker exec gsm-sip-bridge-test ip -n ims0 -6 route del <pcscf>/128 dev veth-ims0
   ```
   Once the line is `Registered`, exactly one all-clear arrives.
7. Check the counter: `curl -s localhost:9091/metrics | grep 'reason="pcscf_pool_exhausted"'`.
8. Redeploy the real branch image.

### 2. Vodafone, real build (SC-002)

Repeat step 1's black-hole on the real branch image. Record which of `PcscfUnreachable` (the probe, ~90s) and `PcscfPoolExhausted` fired first, and how long the line took to recover. Either result passes.

### 3. Jio, one P-CSCF of the pool (SC-003)

```bash
P=$(pgrep -f "vowifi-ims-agent --line 0" | head -1)
sudo nsenter -t $P -n ip route add <active-pcscf>/32 via 10.99.0.2 dev veth-ims0
```

Expect failover to a sibling: `failing over to the next P-CSCF now`, then `Registered`. There must be no new IKE_SA and no `PcscfPoolExhausted`. Remove the route afterwards.

### 4. Idle (SC-004)

About 30 minutes on each rig with no faults. Expect zero `PcscfPoolExhausted`.

Record the results in `live-verification.md`, with P-CSCF addresses redacted to role names.
