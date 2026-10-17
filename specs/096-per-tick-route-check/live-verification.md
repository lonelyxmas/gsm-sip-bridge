# Live verification: per-tick default-route check (#120 finding 3)

**Date**: 2026-10-17
**Branch**: `096-per-tick-route-check` at `8052ce9`

**Images**:
- `gsm-sip-bridge:route-tick-096-local` (Vodafone rig, amd64, built from `git archive HEAD`)
- `ghcr.io/selvakn/gsm-sip-bridge:route-tick-096-arm64` (Jio Pi, built natively on arm64)
- `gsm-sip-bridge:route-tick-096-forcedprobe-local` (Vodafone rig, the SC-003 throwaway: HEAD plus one detached commit, never pushed, hard-wiring `StrongswanEngine::pcscf_reachable` to `true`)

Each container was **recreated** (`down`, then `up -d`), not restarted. On the Pi, the previous compose file is kept as `docker-compose.yaml.bak-pre-route-tick-096-arm64`.

P-CSCF addresses are omitted. Vodafone's Gm runs over IPv6 and Jio's over IPv4.

## Method

The fault deletes the default route in the Gm family, inside the line's namespace, with the tunnel and P-CSCF otherwise healthy:

- **Vodafone**: `docker exec gsm-sip-bridge-test ip -n ims0 -6 route del default dev tun23-0`
- **Jio**: `docker exec gsm_gsm-sip-bridge_1 ip -n ims0 route del default dev tun23-0`

For each run I recorded:
- the time from the delete to `recovered from DefaultRouteMissing` in `docker logs`
- charon's count of `established` lines before and after
- `swanctl --list-sas`
- the new counter on `:9091/metrics`
- `vowifi-status`

## Results

| Criterion | Rig | Result | Evidence |
|---|---|---|---|
| SC-001 | Vodafone | **Pass** | Deleted at 04:24:24Z and repaired 1s later: `netns ims0 has no IPv6 default route via tun23-0 … reinstating it`, then `recovered from DefaultRouteMissing`. The IPv6 default route is back. |
| SC-001 | Jio | **Pass** | Deleted at 04:26:44Z and repaired 16s later, the same pair of log lines with `IPv4`. The route is back. |
| SC-002 | Vodafone | **Pass** | charon's `established` lines stayed at 3, and `swanctl` still lists IKE_SA `#1` and CHILD_SA `#1` `INSTALLED`. The agent restarted and was `Registered` again by 04:24:34Z (10s). |
| SC-002 | Jio | **Pass** | `established` lines stayed at 3, with IKE_SA `#1` and CHILD_SA `#1` `INSTALLED`. `Registered` again by 04:27:07Z (23s). |
| SC-003 | Vodafone | **Pass** | Forced-probe build, deployed at 04:58Z. Route deleted at 04:58:24Z and repaired 16s later with no renegotiation (`established` 3 → 3). With the probe always passing, this fault was never repaired before this change (#120 run C: 10 minutes down). |
| SC-006 | Vodafone | **Pass** | 33 minutes of idle (04:24:34–04:57:49Z) with no further `DefaultRouteMissing`, rebuild, `dead peer` or restart. The counter stayed at 1, and the line stayed `Registered`. |
| SC-006 | Jio | **Pass** | 30 minutes of idle (04:27:07–04:57:49Z), with the same result. |
| SC-007 | both | **Pass** | `gsm_sip_bridge_vowifi_tunnel_recoveries_total{reason="default_route_missing"}` went from absent (zero) to 1 on each line (`module="ec20-71"` for Vodafone, `"ec20-11"` for Jio), and no other reason's series appeared. |

### Notes

- The 1s on Vodafone and 16s on Jio are just where in the 30s tick each delete landed. In the worst case, a repair waits one full interval.
- In each run, `vowifi-ims-agent exited; restarting in 5s` follows the repair. This is the unchanged `DefaultRouteMissing` agent restart (Clarification Q1), not a regression.
- After SC-003, the Vodafone rig went back to `route-tick-096-local`, and the line was `Registered` at 04:58:57Z.

## Deviations from tasks.md

- The Pi image is tagged `route-tick-096-arm64`, outside the `jio-gmN` sequence, as with 094's verification build.
- The throwaway build was a detached commit in a scratch worktree (now removed), not a named scratch branch. Either way, it was never pushed.
