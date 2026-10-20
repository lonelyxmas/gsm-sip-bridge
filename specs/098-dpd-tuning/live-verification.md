# Live verification: DPD tuning (#120 replacement 3)

**Branch**: `098-dpd-tuning`. The first round ran at `15cf71f`; the #137 fix (`287d078`) was then built into the "098b" images, and every check after it ran on those.

**Images**:

| Image | Rig | Notes |
|---|---|---|
| `gsm-sip-bridge:dpd-098-forcedprobe-local` | Vodafone, amd64 | Throwaway: `15cf71f` plus one detached change, never pushed, that hard-wires the supervisor's P-CSCF probe to pass, so DPD is the only liveness check. |
| `ghcr.io/selvakn/gsm-sip-bridge:dpd-098-forcedprobe-arm64` | Jio Pi | The same throwaway, built natively via `arm-build-200`. |
| `ghcr.io/selvakn/gsm-sip-bridge:dpd-098-arm64` | Jio Pi | The real branch image at `15cf71f` (probe on). |
| `gsm-sip-bridge:dpd-098b-forcedprobe-local` | Vodafone, amd64 | The throwaway change on top of `287d078`. |
| `ghcr.io/selvakn/gsm-sip-bridge:dpd-098b-forcedprobe-arm64` | Jio Pi | The same, arm64. |
| `gsm-sip-bridge:dpd-098b-local` | Vodafone, amd64 | The real branch image at `287d078`, built from `git archive HEAD`. |
| `ghcr.io/selvakn/gsm-sip-bridge:dpd-098b-arm64` | Jio Pi | The same, arm64. |

Each container was recreated, not restarted. Times are UTC unless marked IST (the Pi's charon log is in IST). Addresses are redacted to roles: *ePDG*, *P-CSCF*, *Pi*, *PPPoE client*.

Captures: outer UDP 500/4500 on the host (`tcpdump -i any`), charon's log copied out every 60s, the supervisor's log, and for the #137 runs the IMS agent's log streamed out of the container so it survives a restart.

## Summary

| Criterion | Vodafone | Jio |
|---|---|---|
| SC-001: 12h at 120s, probe forced off, zero false dead-peer teardowns, ≥1 REGISTER refresh, CHILD_SA and IKE_SA rekey | **Pass**: 12h, 0 teardowns, 0 DPD retransmits; 13 REGISTER refreshes, 12 CHILD_SA and 3 IKE_SA rekeys | **Pass**: 26.5h, 1 teardown, caused by a host uplink outage, not a false dead peer (below) |
| SC-002: idle-hour DPD requests ≤ ⅓ of the 30s baseline | **Pass**: 6–20/h, 13/h on average, against ~109/h at 30s | **Pass**: ≤ 28/h at 120s against ~110/h at 30s |
| SC-003: black-holed ePDG detected and rebuild begun within 315s | **Pass**: 254s, by the IMS agent's path, 14s before DPD would have given up | **Pass**: 198s by DPD |
| SC-004: real image, idle hour, no DPD, no new recoveries | **Pass** on `dpd-098b-local`: 0 DPD, 0 retransmits, 0 recoveries | **Pass** on `dpd-098b-arm64`: 0 DPD, 0 retransmits, 0 recoveries (after a nightly uplink outage, below) |
| SC-005: per-carrier verdict, handset default | See `research.md` R1 and R7 Findings | See `research.md` R7 Findings |
| SC-006: every dead-peer loss classified, matching the capture | **Pass**: none occurred (the black-hole was caught by the agent first) | **Pass**: 3 of 3 (two `false`, one `real`) |
| SC-007: #137 gap accounted for to within 5s | **Pass**: 225s = 95 + 110 + 1 + 19, cause fixed in `287d078`; the fixed image matched its model to the second (171s) | n/a (Vodafone only); the fix re-checked on Jio's failover path |

**SC-003's bound is DPD's worst case.** It was 300s until this run, and was widened to 315s once the worst case was worked out: detection takes the remaining DPD delay, plus charon's 165s of retransmits, plus up to 30s until the supervisor's next poll. A fault that lands just after an inbound packet gives 120 + 165 + 30 = 315s. Both runs here landed late in the idle phase (198s and 254s). With the probe removed, the IMS agent's keepalive path (120s interval, three 10s retries since `287d078`) bounds it independently at about 120 + 30 + 30 = 180s for a dead path that also kills the Gm connection, which a black-holed ePDG does.

## Jio

### 120s soak (SC-001, SC-002)

Probe-forced image, `dpd_delay_sec` unset (default 120s). charon's log runs from 16:02Z on 2026-10-18 to 18:30Z on 2026-10-19, about 26.5h.

| Measure | Value |
|---|---|
| `giving up after 5 retransmits` | 1 (21:33:20Z, 2026-10-18) |
| IKE_SA rekeys | 6, plus the one re-establishment after the teardown |
| CHILD_SA rekeys | 27 |
| REGISTER refreshes | At least one, inferred: the agent registered at 21:34:02Z with a 3600s expiry. `vowifi-status` at 18:33Z the next day showed `registered_at` 17:50:10Z and Registered, with no restart in between. |
| DPD requests per hour | 2–13 in most hours. 28 in the two fully idle hours, against a ceiling of 30 at 120s and ~110/h at 30s (R7) |

The DPD rate falls with traffic because any inbound packet restarts charon's idle timer.

#### The teardown, and three near-misses: the Pi's uplink, not the ePDG

Three DPD exchanges were answered only after retransmits:

- 23:43:26 IST, 2026-10-18: answered on retransmit 5, 75s before charon would have given up.
- 10:48:15 IST, 2026-10-19: answered on retransmit 2.
- 11:14:20 IST, 2026-10-19: answered on retransmit 4.

The capture shows the same thing in all three windows and in the teardown window:

- **Nothing arrives from the ePDG** on `eth0` while the request is outstanding: no ESP, no IKE and no NAT-T keepalive.
- **The Pi's outbound packets to the ePDG loop.** Each one leaves `eth0`, comes back in on the Pi's PPPoE server interface from the PPPoE client, and is NATed out of `eth0` again with a new source port. That happens 96–384 times per window.

The PPPoE client is a router that uses the Pi's LTE link as a backup WAN, and it is also the Pi's own upstream. When that router's primary WAN flaps, it fails over to the Pi, and the Pi's default route sends the traffic straight back to it. This is a fault in the test site's network, not in the bridge or the carrier.

The teardown timeline:

| Time (Z) | Event | Source |
|---|---|---|
| 21:28:06 | Last NAT-T keepalive from the ePDG (it sends one about every 2 minutes) | capture |
| 21:28:35 | Last inbound ESP. Nothing more arrives from the ePDG until 21:32:36 | capture |
| 21:30:35 | 120s of silence: DPD request 23. In-tunnel TCP retransmits also go out unanswered | charon, capture |
| 21:30:39 – 21:32:05 | Retransmits 1–5, all on the wire, and looped back via the PPPoE client | charon, capture |
| 21:30:36, 21:32:57 | The watcher's ssh to the Pi, over the same uplink, fails | watcher, journal |
| 21:32:36–38 | A burst of 17 inbound ESP packets: the path is back, 31s after the last retransmit | capture |
| 21:33:20 | `giving up after 5 retransmits` | charon |
| 21:33:27 | Supervisor: `dead peer verdict: false (inbound ESP 48s before the loss, 59 packets, snapshot 30s old)` | supervisor |
| 21:33:28 | New IKE_SA established | charon |
| 21:34:02 | Agent registered, after three `AT+CSIM failure` restarts that coincided with a ModemManager SIM-card event on the Pi | supervisor, journal |

**This is not a false dead-peer teardown in SC-001's sense.** The ePDG path was dead for 241s, longer than charon's 165s give-up window. The `false` verdict is still correct by its definition: inbound ESP arrived inside the window, because the path came back after charon's last retransmit at +90s and before it gave up at +165s (R7 Findings). As R7 noted, the verdict alone cannot tell this case from an ePDG that ignores IKE while ESP flows. The capture separates them, and the R7 event at 10:52:57Z remains the only genuine ePDG ignore observed.

The same outage recurred the next night in the same window (below), so it is most likely a scheduled reconnect on the router's primary WAN at about 03:00 IST.

### Black-holed ePDG (SC-003, SC-006)

Probe-forced image at 120s. The Pi has no packet filter, so the fault is a blackhole route for the ePDG's /32 in the host's network namespace (the container uses host networking). That drops our outbound packets only; inbound from the ePDG is untouched.

| Time (Z) | Event |
|---|---|
| 18:34:13 | Last inbound ESP |
| 18:35:56 | Fault: `ip route add blackhole <ePDG>/32` |
| 18:36:13 | First DPD request, 120s after the last inbound. charon's socket writes fail with `Invalid argument` |
| 18:36:17 – 18:37:42 | Retransmits 1–5 at +4, +11, +24, +47 and +90s |
| 18:38:58 | `giving up after 5 retransmits`, 165s after the first request |
| 18:39:14 | Supervisor: `dead peer verdict: real (inbound ESP 301s before the loss, 12 packets, snapshot 30s old)`; re-initiating |
| 18:39:35 | Route removed |
| 18:39:40 | New IKE_SA |
| 18:39:44 | Agent registered, 9s after the route was removed |

The loss was detected **198s** after the fault, within the 315s bound. The verdict was `real`, matching the capture. Here the line had already been idle for 103s when the fault began; see the Summary for the worst case.

### P-CSCF failover with the #137 fix

`dpd-098b-forcedprobe-arm64`. Jio hands out a pool of four P-CSCFs. Black-holing the active one (a /32 route to the veth peer inside the IMS namespace) at 20:57:52Z:

| Time (Z) | Since fault | Event |
|---|---|---|
| 20:59:32.6 | +100s | Keepalive unanswered; the reconnect times out after 5s. Attempt 1 |
| 20:59:47.6 | +115s | Attempt 2 |
| 21:00:02.6 | +130s | Attempt 3: P-CSCF marked, 3 remaining; failover to a sibling |
| 21:00:03.6 | +131s | Registered through the sibling |

No tunnel rebuild and no supervisor recovery. On Jio a dead connect times out after 5s rather than failing at once, so the attempts land 15s apart instead of 10s.

### Real image, probe on (SC-004)

First on `dpd-098-arm64` (`15cf71f`), registered at 18:46:40Z and checked at 19:47:27Z: 0 DPD requests, 0 retransmits, 0 recoveries, 112 NAT-T keepalives, the only INFORMATIONAL a rekey's delete, still Registered. Because `287d078` changed the agent, the hour was repeated on `dpd-098b-arm64`.

`dpd-098b-arm64` registered at 21:01:10Z. At 03:00 IST (21:30Z) the Pi's uplink failed again, with the same signature as the teardown above:

| Time (Z) | Event | Source |
|---|---|---|
| 21:30:08 | Last inbound ESP | capture |
| 21:30:38 | In-tunnel retransmits start going out unanswered | capture |
| 21:31:01 – 21:32:32 | Outbound to the ePDG loops via the PPPoE client | capture |
| 21:31:43 | The agent has marked the last of the four P-CSCFs | supervisor |
| 21:32:07 | Supervisor: recovered from `PcscfPoolExhausted`; the ePDG did not acknowledge the DELETE, forced | supervisor |
| 21:32:55 | First inbound again: the IKE_SA_INIT response, after 4 retransmits | capture, charon |

charon sent no DPD request: the outage was caught by the agent's path, which is the probe-on design. Not counting this real outage, the idle hour was taken from the recovery onward, 21:33:48Z to 22:35:07Z: **0 DPD requests, 0 retransmits, 0 recoveries**, 121 NAT-T keepalives, the only INFORMATIONAL a CHILD_SA rekey's delete, still Registered with the Gm connection up.

## Vodafone

The Vodafone modem fell off the USB bus when the workstation lost power on 2026-10-19, after about 11h of a clean first soak; it was re-plugged and the checks below were all run afterwards.

### Black-holed ePDG (SC-003, SC-006)

`dpd-098-forcedprobe-local` at 120s. Fault: a blackhole route for the ePDG's /32 in the host namespace.

| Time (Z) | Event |
|---|---|
| 20:27:49 | IKE_SA established (fresh container); last inbound about 20:27:54 |
| 20:28:11 | Fault |
| 20:29:54 | First DPD request, 120s after the last inbound; retransmits at +4, 11, 24, 47 and 89s |
| 20:31:55 | The IMS agent marks the only P-CSCF unavailable (Gm keepalive) |
| 20:32:25 | Supervisor: recovered from `PcscfPoolExhausted`; DELETE not acknowledged, forced. **Rebuild begun 254s after the fault** |
| 20:32:39 | (Where charon would have given up) |
| 20:33 – 20:36 | IKE_SA_INIT cannot leave: `ChildSaMissing` / `ChildSaStuck`, then `tunnel flapping (3 rebuilds within 300s)` |
| 20:36:10 | Route removed |
| 20:36:37 | Registered through a new P-CSCF, 27s later |

The agent's path beat DPD by about 14s, so no dead-peer verdict was produced and there is nothing to classify.

### The #137 gap (SC-007)

`dpd-098-forcedprobe-local`. Fault: Vodafone's only P-CSCF routed to the veth peer inside the IMS namespace, with the agent's log streamed out.

| Time (Z) | Since fault | Event |
|---|---|---|
| 20:37:14 | 0 | Fault |
| 20:38:39 | +85s | Keepalive sent on its 120s interval |
| 20:38:49.5 | +95s | Unanswered after 10s. Attempt 1: the reconnect fails at once, and no confirming probe is sent |
| | | **110s with nothing in flight**: the next probe falls due 120s after the dead one was *sent* |
| 20:40:39.6 | +205s | Send fails: attempt 2 |
| 20:40:40.6 | +206s | Attempt 3: escalate, P-CSCF marked |
| 20:40:59.6 | +225s | Supervisor: recovered from `PcscfPoolExhausted` (19s poll wait) |

225s = 85 + 10 + 110 + 1 + 19, matching 097's 234s run, which had the same shape and a 29s poll wait. The 110s is `PING_INTERVAL − PING_RESPONSE_TIMEOUT`. Fixed in `287d078`: after a failed repair the next probe is due 10s later (`research.md` R8 Findings).

Vodafone gives a single P-CSCF, and the new negotiation handed back the same one, so the agent then waited 120s before retrying. That wait is separate from the gap.

Re-run on `dpd-098b-forcedprobe-local`:

| Time (Z) | Since fault | Event |
|---|---|---|
| 20:55:11 | 0 | Fault |
| 20:57:16.3 | +125s | Keepalive (sent at +115s) unanswered. Attempt 1 |
| 20:57:26.3 | +135s | Attempt 2, 10s later |
| 20:57:36.3 | +145s | Attempt 3: P-CSCF marked |
| 20:58:02.2 | +171s | Supervisor: recovered from `PcscfPoolExhausted` (26s poll wait) |
| 20:58:17 | +186s | Registered through a different P-CSCF |

From the first dead keepalive to the mark took 20s, down from 111s. The model (interval phase + 10 + 20 + poll wait = 115 + 10 + 20 + 26) gives 171s, exactly what was measured.

### 120s soak (SC-001, SC-002)

`dpd-098b-forcedprobe-local`, from 20:59:02Z on 2026-10-19 to 08:59:23Z on 2026-10-20.

| Measure | Value |
|---|---|
| `giving up after 5 retransmits` | 0 |
| IKE retransmits | 0: every DPD request was answered first time |
| Supervisor recoveries | 0 |
| IKE_SA rekeys | 3 |
| CHILD_SA rekeys | 12, all answered |
| REGISTER refreshes | 13 (`registration renewed`, 3600s, about every 55 minutes) |
| DPD requests | 155, 6–20 per clock hour, 13/h on average, against ~109/h at 30s (1873 over about 17.2h) |

### Real image, probe on (SC-004)

`dpd-098b-local`, registered at 09:06:12Z on 2026-10-20 and checked at 10:09:19Z: **0 DPD requests, 0 retransmits, 0 recoveries**, 115 NAT-T keepalives, the only INFORMATIONAL a CHILD_SA rekey's delete, still Registered with the Gm connection up. The startup log reads `strongSwan DPD: dpd_delay = 120s ([vowifi].dpd_delay_sec)`.

## The dead-peer verdict after the review fix (`b0dab07`)

The PR #143 review found that `real` rested only on the last healthy tick's snapshot, up to 30s old, so a path that recovered after it was misclassified. `b0dab07` also reads the tunnel interface's RX counter on the snapshot's tick and on the loss tick (`research.md` R4). Images: `dpd-098c-local` and `dpd-098c-arm64`, with throwaway forced-probe variants built the same way as before. Each rig was recreated on each image.

| Rig | Fault (forced-probe image) | Verdict line |
|---|---|---|
| Vodafone | Outbound IKE to the ePDG dropped (`u32` match on the zero non-ESP marker); ESP and NAT-T keepalives pass. DPD at 11:46:26Z, give-up at 11:49:11Z | `false (inbound ESP 54s before the loss, 22 packets, snapshot 30s old, 0 received on the tunnel since; …)`. Re-registered 14s after the rule was removed |
| Jio | ePDG black-holed at 12:00:38Z, seven minutes after registration | `real (inbound ESP 294s before the loss, 42 packets, snapshot 30s old, 0 received on the tunnel since; …)`, 224s after the fault. Re-registered 9s after the route was removed |

The Vodafone run reproduces #120 finding 2 on demand: the ePDG never sees the DPD, ESP keeps flowing, and charon tears down a working tunnel.

An earlier Jio black-hole, started 9s after registration, read `false`: inbound ESP 56s before the loss, and 1 packet on the tunnel after the snapshot. A blackhole route drops outbound only, and the network was still sending into the tunnel then. That is `false` by definition. As with the PPPoE-loop outage, the verdict cannot tell a one-way path loss from an ePDG that ignores IKE; the capture can.

Real images, 30 minutes idle each:

| Rig | DPD requests | Retransmits | Recoveries | Line |
|---|---|---|---|---|
| Vodafone, from 11:54:14Z | 0 | 0 | 0 | Registered, Gm up |
| Jio, from 12:05:18Z | 0 | 0 | 0 | Registered, Gm up |

The counter is read on both architectures: both verdict lines carry a count, not `tunnel counter unread`. Switching the check to `ip -s link show` caused no false reports of a missing tunnel interface.
