# Quickstart: verifying the per-tick route check

## Local (no hardware)

```bash
make format && make lint && make test
```

The tests that matter most:

- `line_supervisor` tests, one per row of [contracts/steady-tick.md](contracts/steady-tick.md). The key one is `a_missing_route_is_repaired_even_while_the_pcscf_answers` (finding 3).
- `control::protocol`: `tunnel_recovered` round-trips with every reason label.
- `tests/test_vowifi_tunnel_recovery_metrics.rs`: a supervisor reporter's `TunnelRecovered` events reach the registry counter over a real control socket.

## Live: both rigs (spec Clarification Q2)

Deploy the branch build:

- **Vodafone rig**: the local host, `test/` compose (pin the local image tag).
- **Jio Pi**: `pi@192.168.100.2`, `/apps/gsm`, docker-compose v1. Build arm64 natively via `arm-build-200` and `docker save | ssh pi docker load`. Back up `docker-compose.yaml` as `docker-compose.yaml.bak-pre-<tag>`.

**Recreate** the container (`down`, then `up -d`). Wait for `vowifi-status` to show `Registered` (about 3 minutes).

### 1. Route delete (SC-001, SC-002, SC-007)

Record the counter, then delete the default route in the Gm family:

```bash
curl -s localhost:9091/metrics | grep vowifi_tunnel_recoveries_total     # before
# Vodafone (IPv6 Gm), inside the container or via nsenter into the line netns:
ip -n ims0 -6 route del default dev tun23-0
# Jio (IPv4 Gm), the Pi has no /var/run/netns:
sudo nsenter -t $(pgrep -f "vowifi-ims-agent --line 0" | head -1) -n ip route del default dev tun23-0
```

Pass:

- `recovered from DefaultRouteMissing` appears in `docker logs` within ≤30s.
- The `reinstating it` log line is present.
- charon's log has no new `IKE_SA ... established` or `CHILD_SA ... established` around it.
- `vowifi-status` is back to `Registered` after the agent restart.
- The counter for `reason="default_route_missing"` has gone up by exactly 1, and no other reason has moved.

### 2. Healthy idle (SC-006)

Leave the line idle for about 30 minutes after step 1 settles. Pass: there are zero further `DefaultRouteMissing` log lines, and the counter doesn't change.

### 3. Forced-probe throwaway build, Vodafone only (SC-003)

On a scratch branch (never pushed), make `StrongswanEngine::pcscf_reachable` return `true` unconditionally. Build a local image, deploy it to `test/`, recreate the container, and repeat step 1. Pass: the same `DefaultRouteMissing` repair within ≤30s.

If the throwaway build were missing the per-tick check, the probe would "pass" and nothing would ever repair the route. That was the failure in #120 run C.

Afterwards, redeploy the real branch image.

Record the results in `specs/096-per-tick-route-check/live-verification.md`, with P-CSCF addresses redacted to role names.
