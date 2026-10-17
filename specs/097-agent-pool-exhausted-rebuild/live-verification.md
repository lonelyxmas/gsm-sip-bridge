# Live verification: rebuild on an exhausted P-CSCF pool (#120 item 2)

**Date**: 2026-10-17
**Branch**: `097-agent-pool-exhausted-rebuild` at `969d85c`

**Images**:

| Image | Rig | Notes |
|---|---|---|
| `gsm-sip-bridge:pool-exhausted-097-local` | Vodafone, amd64 | Built from `git archive HEAD`. |
| `gsm-sip-bridge:pool-exhausted-097-forcedprobe-local` | Vodafone, amd64 | The SC-001 throwaway: HEAD plus one detached commit in a scratch worktree, never pushed, that hard-wires `StrongswanEngine::pcscf_reachable` to `true`. |
| `ghcr.io/selvakn/gsm-sip-bridge:pool-exhausted-097-arm64` | Jio Pi | Built natively on arm64 via `arm-build-200`. |

Each container was **recreated** (`down`, then `up -d`). On the Pi, the previous compose file is kept as `docker-compose.yaml.bak-pre-pool-exhausted-097-arm64`.

P-CSCF addresses are omitted. Vodafone's Gm runs over IPv6 with one P-CSCF per negotiation. Jio's runs over IPv4, from a 4-member pool (2 IPv4, 2 IPv6).

## Method

The fault is a route that **exists** but leads nowhere: the P-CSCF goes into the line's veth, inside the line netns, while the tunnel stays up. Because the route exists, the agent attributes the failure to the P-CSCF and marks it. A `blackhole` route would read as "no route" and never mark (research R8).

- **Vodafone, one P-CSCF:**

  ```bash
  ip -n ims0 -6 route add <pcscf>/128 dev veth-ims0
  ```

- **Vodafone, every P-CSCF the carrier may hand out (SC-007):**

  ```bash
  ip -n ims0 -6 route add 2000::/3 dev veth-ims0
  ```

  This route is more specific than the default via `tun23-0`, and it lives in `ims0` only. The ePDG's IKE/ESP runs in the host namespace and is untouched.

- **Jio, the active P-CSCF only:**

  ```bash
  ip -n ims0 route add <active>/32 via 10.99.0.2 dev veth-ims0
  ```

## Results

| Criterion | Rig | Result | Evidence |
|---|---|---|---|
| SC-001 | Vodafone, forced-probe | **Pass**, slower than estimated | See *SC-001 timeline* below. |
| SC-007 | Vodafone, forced-probe | **Pass** | See *SC-007 timeline* below. |
| SC-002 | Vodafone, real build | **Pass**: the probe fires first | Black-holed at 09:12:51Z. `recovered from PcscfUnreachable` at +97s, ahead of the agent path, which took 206s to mark in SC-001. The line was `Registered` at 09:14:39Z (+108s). `tunnel_recoveries_total{reason="pcscf_unreachable"}` is 1. |
| SC-003 | Jio | **Pass** | Active P-CSCF black-holed at 09:03:06Z. At 09:03:17Z a REGISTER through it failed (`class="transport"`, connect timed out). It was marked `cause=transport remaining=3`, then came `failing over to the next P-CSCF now`, and the line was `registered` through the IPv4 sibling at 09:03:19Z, 13s in. There were zero supervisor recoveries and no new IKE_SA (see *SC-003 charon log* below). Route lifted at 09:13:13Z. |
| SC-004 | Jio | **Pass** | 55 minutes of idle on the branch build (08:08:11 to 09:03:06Z) with zero recoveries and zero `PcscfPoolExhausted`. |
| SC-004 | Vodafone | **Pass** | 30 minutes of idle on the real build (09:15:05 to 09:45:05Z) with zero recoveries. `tunnel_recoveries_total` stayed at the single SC-002 `pcscf_unreachable`, and `registered_at` stayed at 09:14:39Z, so there was no re-registration. Still `Registered`, `gm_connection: up`. |

### SC-001 timeline

| Time (Z) | Offset | Event |
|---|---|---|
| 08:57:13 | 0s | The only P-CSCF is black-holed. |
| 08:58:48 | +95s | `Gm keepalive went unanswered`. |
| 09:00:38 | +205s | The next keepalive fails to send. |
| 09:00:39 | +206s | `P-CSCF marked unavailable … cause=gm-keepalive remaining=0`. |
| 09:01:07 | +234s | `recovered from PcscfPoolExhausted`, logged with `the IMS agent marked every P-CSCF unavailable since the negotiation at 08:12:30: <pcscf> (gm-keepalive, marked 09:00:39, until 09:06:09)`. |
| 09:01:14 | +241s | The new negotiation handed out a different P-CSCF, and the agent was `registered` through it. |

`tunnel_recoveries_total{reason="pcscf_pool_exhausted"}` went from absent to 1. In #120 run C, the same fault with no probe never recovered.

### SC-007 timeline

| Time (Z) | Event |
|---|---|
| 09:02:44 | Every IPv6 P-CSCF is black-holed. |
| 09:05:17 | The new P-CSCF is marked `gm-keepalive`. |
| ~09:05:37 | Second `PcscfPoolExhausted` rebuild. A new episode opens: the 09:01:14 registration had closed the first one. |
| 09:10:37 | One `pcscf_pool_exhausted` alert sent (HTTP 204). `critical_event_active{category="pcscf_pool_exhausted"}` becomes 1. |
| 09:11:10 | Routes lifted. |
| 09:11:20 | `Registered`. |
| 09:11:22 | One all-clear sent (204). The active gauge goes to 0. |

Final counts: `critical_alerts_total{category="pcscf_pool_exhausted"}` shows `sent` = 2 (the failure and the all-clear) and `suppressed` = 2 (heartbeats during the alerted episode). `registration_loss`, `tunnel_failure` and `gm_connection_lost` alerted and all-cleared alongside it, independently (FR-016).

### SC-003 charon log

charon's `established` lines went from 3 to 5. Both new lines are the routine hourly CHILD_SA rekey (`ims0{2}`, 09:07Z) under the same IKE_SA `#1`.

## Findings

1. **SC-001 detection took 234s, against the plan's ~200s estimate.** The overrun is entirely between the first unanswered keepalive (+95s) and the next send attempt (+205s). That should have been the 10s response timeout of the confirming probe. The time went somewhere in the agent's reconnect step. The agent's log for that window was overwritten when it restarted after the rebuild, so the cause isn't confirmed.

   Whatever the cause, it applies equally whether or not the probe exists, and it doesn't change the outcome: the line recovered. It does matter for the probe-removal decision, though. The agent path would be about 2.4× slower than the probe here (234s vs 97s). It's worth a look before #120 item 3.

2. **With the probe on (SC-002), the probe wins.** `PcscfUnreachable` fired at +97s, well before the agent could have marked. So until the probe is removed, `PcscfPoolExhausted` acts mostly as a backstop on Vodafone-style lines. The recovery counter now separates the two reasons, so that can be measured in production.

3. **The episode boundary works as designed.** The SC-001 registration closed the first episode, and SC-007's alert was timed from the second rebuild, not the first. The replaced agent's `registered = true` heartbeats did not end the episode early.

## Deviations from tasks.md

- The Pi image is tagged `pool-exhausted-097-arm64`, outside the `jio-gmN` sequence, as with the 094 and 096 verification builds.
- SC-007 used a `2000::/3` veth route, so that the carrier's replacement P-CSCFs were dead too. A single `/128` let the first rebuild recover in 241s, which is too short an episode to alert.
- The Jio idle window (SC-004) is the 55 minutes before the SC-003 fault, not a separate 30-minute run.
  Jio was also clean after the SC-003 route was lifted (09:13:13Z to 09:45Z): zero recoveries.
- Both rigs were left running the branch builds: Vodafone on `pool-exhausted-097-local`, Jio on `pool-exhausted-097-arm64`.
