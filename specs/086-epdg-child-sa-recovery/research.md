# Research: ePDG CHILD_SA health detection and bounded recovery

All findings below were checked against the code on `main` (19313e4) and the live
Vodafone container (`gsm-sip-bridge-test`, strongSwan 5.9.3) on 2026-10-12.

## R1 — What a usable CHILD_SA looks like in `swanctl --list-sas`

**Decision**: parse the pretty text output. A CHILD_SA is usable when a line matches all of the following:
- exactly two leading spaces, then `<conn>: #` (the child's name equals the connection name in
  `docker/strongswan/swanctl-epdg.conf.template`);
- its third comma-separated field (after `#N` and `reqid R`) is one of `INSTALLED`,
  `REKEYING`, `REKEYED` or `UPDATING`.

**Evidence** (live, identities removed):

```
ims0: #1, ESTABLISHED, IKEv2, 472cde2f48c4dde1_i* e8ca0583f4b574a6_r
  local  '…@nai.epc.mnc043.mcc404.3gppnetwork.org' @ 192.168.15.10[4500] [<v6>]
  remote '203.88.4.88' @ 203.88.4.88[4500]
  AES_CBC-128/HMAC_SHA1_96/PRF_HMAC_SHA1/MODP_1024
  established 13141s ago, rekeying in 231s
  ims0: #4, reqid 1, INSTALLED, TUNNEL-in-UDP, ESP:AES_CBC-128/HMAC_SHA1_96
    installed 3059s ago, rekeying in 510s, expires in 901s
    in  c8d8037d (-|0x00000017),  59380 bytes,   662 packets,    19s ago
    out bf0de445 (-|0x00000017),  98080 bytes,  1273 packets,    19s ago
    local  <v6>/128
    remote ::/0
```

`list_sas_has_child_sa` (`gsm-sip-bridge/src/supervise/engines.rs:123`) currently matches
line 1, the IKE_SA. It is a port of the original bash `grep '^ims:'`, which had the same bug.

The state names come from strongSwan 5.9.3's `child_sa_state_names`: CREATED, ROUTED,
INSTALLING, INSTALLED, UPDATING, REKEYING, REKEYED, RETRYING, DELETING, DELETED, DESTROYING.
REKEYING and REKEYED still forward traffic. During #107's failing rekeys the old SA kept carrying
traffic for 11 minutes, so those states must count as present (spec FR-001, SC-004).

**Alternatives considered**:
- `swanctl --list-sas --raw` (vici dump): structured, but the existing code and tests are built around
  the pretty output, and the raw format is longer to fixture. Rejected for now (Principle V). The
  parser is the one function whose output format is pinned by a live fixture.
- `ip xfrm state` per `if_id`: shows kernel state directly, but means a second command per tick
  (FR-011 says no extra queries) and loses the rekey-state distinction.

## R2 — Escalation placement and state

**Decision**: replace `tick_steady_state`'s `unreachable_streak: &mut u32` parameter with
`&mut SteadyTracking { pcscf_unreachable, child_sa_missing, child_sa_usable }`. The
`ChildSaMissing` arm increments `child_sa_missing`:
- at 1: re-initiate only, reason `ChildSaMissing` (as today);
- at `CHILD_SA_MISSING_STRIKES` (2): reset the count, `terminate` + `reinitiate`, reason
  `ChildSaStuck`. `recovery_restarts_agent` returns true for this reason.

Every other health result resets the count. There is no backoff (clarification Q4, FR-006a).

**Why the arm already sits in the right place**: health is evaluated before
`repair_default_route` (`line_supervisor.rs:400`). Once detection works, the #107 sequence
(the route repair reset the reachability clock) can't recur, which satisfies FR-007. A
unit test pins that ordering.

**Rationale for a struct over a second `&mut u32`**: the tick already threads loop-owned state
through a parameter, as the comment at `orchestrate.rs:1621` explains. A third value
(`child_sa_usable`, R5) would make it three positional `&mut`s, which is easy to mix up at the
24 test call sites.

**Alternatives considered**:
- Count misses inside `StrongswanEngine` with a `RefCell`. That hides the state from
  `tick_steady_state` tests that drive `FakeEngine`, so it was rejected.
- Escalate to `restart_process` (respawn charon). The daemon is shared by every line
  (`SharedCharon` docs), so this would break FR-008. Rejected.

## R3 — Non-waiting, bounded teardown

**Decision**: `StrongswanEngine::terminate` runs
`timeout 10 env STRONGSWAN_CONF=<shared> swanctl --terminate --ike <conn> --force`.

**Evidence**:
- In the shipped 5.9.3, `swanctl --terminate --help` lists `--force (-f) terminate IKE_SA without
  waiting, unless timeout is set`. So swanctl's own `--timeout` must **not** be passed, or `--force`
  stops skipping the wait.
- `/usr/bin/timeout` is BusyBox v1.37.0 in the runtime image. `shutdown.rs`'s `TerminateIke`
  step already uses `timeout 5 … swanctl --terminate --ike` (TERMINATE_TIMEOUT_SECS).
- Every steady-state recovery calls `TunnelEngine::terminate` (TunVanished, PcscfUnreachable,
  `restart_process` for ViciBroken/ProcessDied, and the new ChildSaStuck). Changing the
  one method covers clarification Q2 ("all recovery paths").

**Trade-off accepted**: `--force` destroys the local IKE_SA even if the DELETE never reaches the
ePDG. The ePDG notices through its own DPD (the #107 log shows 30s INFORMATIONALs), and the
new IKE_AUTH for the same identity replaces the old session.

**Alternatives**: shutdown keeps its own 5s, non-forced call. Teardown at container stop has
different goals and is out of scope.

## R4 — How the supervisor reports the new signal

**Decision**:
- Add `AgentKind::Supervisor` (`"supervisor"`) and `AgentState.child_sa_up: Option<bool>`.
- Each strongSwan line spawns one `observability::reporter::Reporter`, using:
  - `Reporter::spawn(config.control.socket_path, AgentKind::Supervisor, line.card_id, agent_report_interval_seconds)`
  - created in `start_vowifi_line_strongswan` (`orchestrate.rs:1584`) **before** `establish_line_tunnel`,
    which satisfies FR-012 / clarification "down from the moment supervision starts".

**Evidence**:
- `LineStartup` already carries `config: &AppConfig` (`orchestrate.rs:127`).
- The IMS agent's `module_id` is `card_id` from the same line manifest
  (`commands/vowifi.rs:50` → `load_line_config` → `l.card_id`). So the supervisor's
  `line.card_id` labels the same `module` as the agent's gauges.
- `Reporter` re-sends the last state as a heartbeat at `agent_report_interval_seconds`
  (default 10s) from its own thread. A tick blocked in establishment or teardown therefore never
  makes the record go stale. The staleness threshold is 3× `[metrics].agent_report_interval_seconds` (45s by default; `metrics/server.rs`).
- Liveness is keyed by `(AgentKind, module_id)` (`ingest.rs:125`), so a Supervisor record never
  collides with the Ims record for the same line.
- Side benefit: `gsm_sip_bridge_agent_up{agent="supervisor",module=…}` now shows whether the
  supervisor itself is reporting.

**Compatibility (FR-016)**: `child_sa_up` follows the
`#[serde(skip_serializing_if = "Option::is_none", default)]` pattern of its siblings, so
older reports parse and older readers ignore the field. A new `AgentKind` value can't be
parsed by an older daemon. That is acceptable: the daemon and supervisor are the same binary in
the same image (one `gsm-sip-bridge` build), so a cross-version pair isn't a deployment
shape this project supports. The contract records this.

**Alternatives considered**:
- Let the IMS agent probe the CHILD_SA. The agent runs inside the line's netns. XFRM state lives
  in the namespace charon runs in, so the agent can't see it, and it isn't running during
  initial establishment. Rejected.
- Supervisor writes a file the agent forwards: two hops and a stale-file problem. Rejected.

## R5 — What the gauge reports per tick

**Decision**: `child_sa_up = true` only when the tick's health check positively confirmed a
usable CHILD_SA (`SteadyStateHealth::Ok`). It is false for `ChildSaMissing`, `TunVanished`,
`ViciBroken` and a dead charon (the process-died branch). The tick records this in
`SteadyTracking::child_sa_usable`. The loop reports it after every tick, so no second
`--list-sas` call is needed (FR-011).

During establishment the reporter is created holding `Some(false)`, and the loop reports
`Some(true)` at "tunnel UP". The swu loop never creates a supervisor reporter (FR-015).

A healthy CHILD_SA with an unreachable P-CSCF stays `true`. That fault belongs to registration
and Gm signals, not this gauge.

## R6 — Folding the CHILD_SA signal into the tunnel-failure alert without double alerts

**Context**: alert state today is per `(AgentKind, module)` record. `apply_report` runs
`decide_transition(tunnel_unhealthy_since, &mut tunnel_alert_phase, …)` per record, and
`record_alert_outcome` later matches the delivery result back by `(key, category, generation)`,
where generation is the record's `tunnel_unhealthy_since` (`ingest.rs:405`). If the Supervisor
record simply got its own TunnelFailure decision, one outage seen by both the agent
(`tunnel_up=false`) and the supervisor (`child_sa_up=false`) would alert twice. That breaks FR-017a.

**Decision**: one tunnel-failure decision per **line (module)** while a Supervisor record
exists for that module.
- `apply_state` for `AgentKind::Supervisor` maintains the Supervisor record's
  `tunnel_unhealthy_since` from `child_sa_up`, mirroring how Ims maintains it from `tunnel_up`.
- When either an Ims or a Supervisor report for module M is applied, and a Supervisor record
  for M exists (or is the record being written), the TunnelFailure decision is taken **once**,
  on the Supervisor record:
  - `effective_since = min` of the two records' `tunnel_unhealthy_since` (whichever is `Some`).
    This is "either signal unhealthy continuously for ≥ threshold".
  - `effective_since = None` (recovered) only when both are healthy.
  - The decision uses the Supervisor record's `tunnel_alert_phase`. The `effective_since`
    used is stored as a new `tunnel_alert_generation: Option<Instant>` on that record, and
    `record_alert_outcome` compares against it for TunnelFailure.
  - The Ims record's own TunnelFailure decision is skipped for M.
- With no Supervisor record for M (swu lines, VoLTE `Volte` kind, or before the supervisor's first
  report), today's per-record behaviour is unchanged.

The whole decision happens under the single `liveness()` lock that `apply_report` already
holds, so reading the sibling record is race-free.

**Alert text**: the TunnelFailure description uses `key.0.as_str()`. The owning key is now
`supervisor`, so the text reads "supervisor line's tunnel…". It changes to describe the line
rather than the reporter (e.g. "VoWiFi line's tunnel non-established…"), and `ALERTS_PHONE_MAP`
lookup by `key.1` (module) keeps working.

**Alternatives considered**:
- Separate `ChildSaLost` alert category: contradicts clarification Q1 (no new alert
  type or setting). Rejected.
- Supervisor-only ownership (ignore the agent's `tunnel_up` for alerts): doesn't meet FR-017a's
  "either signal". The agent's signal also covers registration-driven outages while the CHILD_SA
  is fine. Rejected.

## R7 — Testing approach (Constitution I)

- Tests on `tick_steady_state` and the parser use the existing `FakeEngine` and `MockCommandRunner`. Both
  already carry the mock justification (charon, a real ePDG and a SIM can't run in CI). Parser
  fixtures are the live 5.9.3 output from R1 with identities synthesised.
- Ingest and alert tests call the real `apply_report` / `decide_transition` with backdated `Instant`s,
  the pattern already established in `ingest.rs` tests.
- A metrics-endpoint integration test sends a real `AgentReport` from a Supervisor over the control
  socket, following `tests/test_observability_ingest.rs`.
- The escalation path's live behaviour is out of reach (Vodafone answers CREATE_CHILD_SA), so it is
  covered by unit tests only, per spec SC-008.
