# Research: DPD tuning (098)

## R1. What a handset uses (FR-002)

- **Decision**: 120s is the handset default, confirmed from the primary source.
- **Evidence**: AOSP `frameworks/base`, `telephony/java/android/telephony/CarrierConfigManager.java`, `Iwlan` defaults block (main branch, fetched 2026-10-18):
  - `KEY_DPD_TIMER_SEC_INT` → `120`
  - `KEY_RETRANSMIT_TIMER_MSEC_INT_ARRAY` → `{500, 1000, 2000, 4000, 8000}`
  - `KEY_NATT_KEEP_ALIVE_TIMER_SEC_INT` → `20`
- **What it tells us**: an Android handset sends a DPD request after 120s without inbound traffic, and gives up on it after ~15.5s, far sooner than charon's ~165s. A handset whose DPD request is ignored therefore tears its tunnel down too, and faster. The ePDGs evidently tolerate that from handsets, which is consistent with "rare, random loss" rather than a bridge-specific fault; whether it is random or rekey-triggered is still for R7 to establish.
- Our NAT-T keepalive (charon default 20s) already matches the handset.

## R2. How charon decides to send DPD, and what `0` means

- charon (IKEv2) schedules a liveness check every `dpd_delay`. When it fires, it sends an empty INFORMATIONAL only if **nothing** has been received from the peer for `dpd_delay`, where "received" includes inbound ESP on any of the IKE_SA's CHILD_SAs (the kernel's last-use time). Any inbound packet resets the clock. This is why the 30s P-CSCF probe (a TCP handshake whose SYN-ACK arrives inbound every 30s) kept DPD from ever firing.
- `dpd_delay = 0` (swanctl.conf(5): "0 to disable") turns charon-initiated DPD off. charon still answers the peer's own INFORMATIONAL requests regardless of the setting.
- **Decision**: render `dpd_delay = <N>s` for every value, `0s` included. No separate "disabled" branch in the template.

## R3. charon's give-up window

- charon defaults: `retransmit_timeout = 4.0`, `retransmit_base = 1.8`, `retransmit_tries = 5`. The request is sent once and retransmitted 5 times; charon gives up after `Σ_{k=0..5} 4·1.8^k ≈ 165.1s`. The project does not override them (`charon-extra.conf`), and the spec keeps them (Assumptions).
- **Decision**: a named constant `DPD_GIVE_UP_WINDOW = 165s` in the supervisor, derived in its doc comment. Used only by the classification (R4). If someone later tunes the retransmit settings, the comment says which constant must follow.

## R4. Classifying a dead-peer loss (FR-008)

- **Input**: `swanctl --list-sas` already runs on every 30s tick. Each CHILD_SA prints an inbound line:
  `    in  cea6576a (-|0x00000017),  21600 bytes,   242 packets,     4s ago`
  The trailing `Ns ago` is the kernel's last-use time for that inbound SA; it is absent when the SA has received nothing.
- **Decision**: use the last-use time, not counter deltas.
  - A counter delta needs a snapshot from before the window starts, and the CHILD_SA can be rekeyed in between, resetting the counters. The last-use time needs only the latest snapshot.
  - Over **all** of the connection's listed CHILD_SAs (the minimum age), so a rekey's old and new SAs are both looked at (spec edge case).
- **Snapshot**: on every structurally healthy tick, the supervisor stores `(S = now, L = S − min age)`, with `L = None` when no SA has received anything. Every other tick drops it. Packets (summed) are kept for the log line.
- **Rule**: let `T` be the tick that sees the IKE_SA gone with cause `DeadPeer`, and `G = DPD_GIVE_UP_WINDOW`. charon gave up somewhere in `(S, T]`, so the first unanswered request went out at `t0 ∈ (S − G, T − G]`.
  - **false** if `L > T − G`: inbound ESP arrived after the latest possible `t0`, i.e. while charon was retransmitting.
  - **real** if `L ≤ S − G`: nothing arrived after the earliest possible `t0`.
  - `L = None` (no listed SA has received anything) is **unknown**, not real: a CHILD_SA rekeyed moments before the snapshot shows the same, and a really dead peer cannot have completed that rekey, so its SA would still carry a last-use age. (Changed from the first draft during implementation.)
  - **unknown** otherwise (the band in between), or with no snapshot at all (supervisor restarted, structural fault in the preceding ticks).
- **Why it separates cleanly at the default**: charon only sends DPD after `dpd_delay` of silence, so for a really dead peer `L ≤ t0 − dpd_delay ≤ T − G − dpd_delay`, well under `S − G` when `dpd_delay` (120s) exceeds the 30s tick. For run C's false teardown the agent's `OPTIONS` is answered every 120s, so at least one inbound packet lands inside the ≥135s of the window that precedes `S`.
- **Alternatives**:
  - Parse the charon log for the first `retransmit 1 of request` timestamp. Rejected: needs wall-clock parsing of a local-time log format and is no more accurate than the tick bound.
  - Counter deltas. Rejected above.

## R5. Where the verdict travels

- **Decision**: one `DeadPeerVerdict` enum, defined in `control::protocol` (it is a wire and metric label) and re-exported by `line_supervisor`. A `dead_peer: Option<DeadPeerClassification>` field on `SteadyTracking`, set by the tick only when it returns `Recovered{IkeSaLost}` with cause `DeadPeer` and cleared at the start of every tick, the same pattern as `pool_exhausted` (specs/097). The orchestrator logs it and puts it on the existing `TunnelRecovered` event as an optional `dead_peer` field, skipped on the wire when `None`, so every other event keeps the shape specs 096/097 shipped.
- **Metric**: a new counter `gsm_sip_bridge_vowifi_dead_peer_losses_total{module, verdict}`, verdict ∈ `false`, `real`, `unknown`, counted in ingest from the event. An event, not a total, so a supervisor restart cannot rewind it (FR-009), as for `tunnel_recoveries_total`.
- **Alternatives**: a new field on `SteadyOutcome::Recovered`. Rejected: ~40 test constructions change for a value only one branch sets.
- No alert (Clarification Q2).

## R6. The setting

- **Decision**: `[vowifi].dpd_delay_sec`, integer seconds, default 120, accepted `0` or `10..=3600`. Deployment-wide, not per line (like `idle_keepalive`).
  - 10s floor: below that, DPD on a 30s tick makes the R4 bands meaningless and every quiet NAT-T gap triggers an exchange.
  - 3600s ceiling: at that point it is effectively off; use 0.
- Rendered as `@DPD_DELAY@` in `swanctl-epdg.conf.template`; `render_swanctl_epdg` takes it through `SwanctlEpdgParams`. The `discover` command renders the same template and passes the configured value.
- Exported to the shell config as `DPD_DELAY_SEC`, like `IDLE_KEEPALIVE`.
- One startup log line; on swu it says the setting is ignored (FR-006).

## R7. Reproducing run C (US1)

- **Build**: a throwaway detached commit on this branch where `StrongswanEngine::pcscf_reachable` returns `true` without connecting, and `dpd_delay_sec = 30` in the rig's config. Never pushed. The real code from this feature is in the image, so the classification is exercised live too.
- **Capture** on each rig, for up to 12h:
  - charon log (`/tmp/charon.log`), copied out every 10 minutes (it is truncated on charon restart);
  - outer: `tcpdump -i any -w outer.pcap udp port 500 or udp port 4500` in the container's default netns, rotated hourly;
  - inner: `tcpdump -i tun23-0 -w inner.pcap` inside the line netns;
  - the supervisor log (`docker logs`), and an hourly `swanctl --list-sas`.
- **For each teardown**, the note records: the INFORMATIONAL message ID, send and retransmit times; every IKE exchange (either side) in the preceding 10 minutes; ESP packets in each direction during the retransmit window (from the outer capture: ESP-in-UDP on 4500 that is not a NAT-T keepalive or IKE); the supervisor's verdict.
- **Verdict**: *rekey/collision-triggered* if the ignored request overlaps or immediately follows (within 60s) a CREATE_CHILD_SA or IKE rekey exchange in at least one of the events and the message ID shows a gap or reuse; *random loss* if it lands in an otherwise quiet window; *inconclusive* if no teardown occurs (Clarification Q1).

## R8. The #137 detection gap (US5)

- **Method**: repeat 097's Vodafone run 1 (probe forced to pass, only P-CSCF black-holed with a /32 to the veth peer), but keep the agent log across the restart: `tail -F /tmp/ims-agent-0.out` into a file outside the container before injecting the fault.
- Expected budget from 097: 120s ping interval + 3×10s ping timeouts + REGISTER attempt + ≤30s tick ≈ 200s; measured 234s. The note accounts for each interval.
- A fix lands here only under FR-011a; otherwise an issue is filed.

## R7 Findings (30s reproduction run, started 2026-10-17 22:20Z)

Both rigs on the probe-forced throwaway (`dpd-098-forcedprobe-*`) with `dpd_delay_sec = 30`.

### Jio, 2026-10-18 04:33:08Z: a LAN outage, not the ePDG

Times are UTC; the Pi's charon log is in IST. Addresses are redacted to roles.

| Time (Z) | Event | Source |
|---|---|---|
| 03:56:28 | Last CHILD_SA rekey (`CREATE_CHILD_SA` 191). The IKE_SA was last rekeyed at 02:03. | charon |
| 04:15–04:29:53 | Every DPD (`INFORMATIONAL` 223–245, every 30s) answered within 1s | charon |
| 04:30:12 | A PPPoE peer on the Pi's LAN terminates its session (`LCP TermReq "User request"`, after 41.6h connected) | journal |
| 04:30:23.8 | DPD request 246 leaves `eth0` | capture |
| 04:30:27.8 | Retransmit 1 leaves `eth0`. **Last packet on the wire, in either direction, until 04:32:03** | capture |
| 04:30:35 / :47 / 04:31:11 / :53 | Retransmits 2–5, plus a NAT-T keepalive at 04:31:48, logged by charon and **absent from the capture** | charon vs capture |
| 04:32:03 | Traffic resumes: outbound ESP first, inbound ESP from 04:32:24 | capture |
| 04:32:07 | The PPPoE session comes back | journal |
| 04:33:08 | `giving up after 5 retransmits`; supervisor: `dead peer verdict: false (inbound ESP 35s before the loss, 23 packets, snapshot 30s old)` | charon, supervisor |
| 04:33:28–:54 | New IKE_SA: `IKE_SA_INIT` answered on its third retransmit, established 46s after the teardown | charon |

There was no `eth0` carrier change (the last `bcmgenet` link flaps were on 2026-10-16). The Pi's packets stopped reaching the device tap while the carrier stayed up. That fits the gateway going away (a LAN router restart, with ARP to it failing and packets dropped in the neighbour queue), and it coincides with a LAN PPPoE peer dropping and reconnecting. The watcher's own ssh to the Pi failed at 04:32:13, in the same window.

**Verdict for this event: neither "random ePDG loss" nor "rekey-triggered".** It was a ~100s local network outage. It was nowhere near a rekey: 34 minutes after the last CHILD_SA rekey, and in a stretch where every exchange was answered within a second.

**What it shows about charon.** The retransmits go out at +0, +4, +11, +24, +47 and +90s, and then charon waits until +165s without sending. Any outage that covers the retransmits up to +90s kills the tunnel, even if the path comes back before +165s; here it came back at +100s. DPD only starts after `dpd_delay` of inbound silence, so a longer delay gives an outage that much longer to clear before the first request:

| `dpd_delay` | Shortest outage that always kills the tunnel | Outage that can survive |
|---|---|---|
| 30s | ≈ 90–120s (the first request lands within 30s of the outage starting) | under ≈ 90s |
| 120s | ≈ 210s | up to ≈ 120s outright, ≈ 90–210s depending on when inbound traffic last arrived |

This ~100s outage would most likely have been survived at 120s.

**What the `false` verdict means, corrected.** `false` says inbound ESP arrived after charon's first unanswered request could have gone out, i.e. the path was working again before charon gave up. That covers two causes: the ePDG ignoring an IKE request while ESP flowed (#120 run C), and a local outage shorter than the give-up window, as here. Telling them apart needs the capture, or at least whether inbound ESP also arrived *early* in the window; the snapshot cannot tell. The docs now say this.

### Vodafone

No dead-peer event so far. More than 800 DPD exchanges, all answered on the first try.
