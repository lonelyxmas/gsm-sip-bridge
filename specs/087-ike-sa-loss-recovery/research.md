# Research: specs/087-ike-sa-loss-recovery

Each item: **Decision**, **Rationale**, **Alternatives considered**. No `NEEDS CLARIFICATION` remains.

## R1 — Telling "no IKE_SA" apart from "IKE_SA without a CHILD_SA"

**Decision**: `list_sas_has_ike_sa(output, conn)` is true when a **column-0** line starts with `"{conn}: #"` in `swanctl --list-sas --ike <conn>` output, in any state. `steady_state_health` maps:

| list-sas shows | Health |
|---|---|
| no `{conn}: #` header | `IkeSaMissing` |
| header, no usable indented CHILD_SA (`list_sas_has_child_sa` false) | `ChildSaMissing` (unchanged #118 two-strike path) |
| usable CHILD_SA | `Ok` |

**Rationale**: the shape was confirmed on the live Vodafone container on 2026-10-13. The IKE_SA is the column-0 line `ims0: #1, ESTABLISHED, IKEv2, …`, and the CHILD_SA is the two-space-indented `  ims0: #3, reqid 1, INSTALLED, …`. vici's list-sas enumerates every IKE_SA in charon's manager, half-open ones (`CONNECTING`) included. So an initiate in flight from the previous tick shows a header and goes down the two-strike path, rather than being re-fired, with an agent restart, every tick. The `CONNECTING` listing is re-confirmed in quickstart step 1.

**Alternatives considered**: parsing `IKE_SA … state change` log lines (rejected: logged at level 2, we run `default = 1`); a vici query for the IKE_SA by unique id (rejected: needs a new client; list-sas is already queried every tick).

## R2 — Attributing the IKE_SA loss cause

**Decision**: `ike_sa_loss_cause(charon_log, conn) -> IkeSaLossCause` looks only at this connection's lines (`<conn|N>` prefix, via `lines_for_conn`) after the most recent `IKE_SA <conn>[N] established` line:

- `received DELETE for IKE_SA` → `CarrierDelete`
- `giving up after N retransmits` → `DeadPeer`
- neither → `Unknown`

If both appear, the later one wins.

**Rationale**: both messages are DBG1 in strongSwan 5.9 (`ike_delete.c`, `task_manager_v2.c`), so they are present at our `filelog default = 1`. `ike_name = yes` already attributes lines to a connection (relied on by `latest_pcscf_batch`). Scoping to "after the last establishment" stops an old DELETE from a previous session being blamed. A loss we caused by hand (`swanctl --terminate`) logs neither, so `Unknown` is the honest answer.

**Alternatives considered**: a per-cause metric (rejected in clarification Q3); the vici event stream (`ike-updown`) (rejected: needs a long-lived vici listener).

## R3 — Graceful-first teardown without a fixed delay

**Decision**: `StrongswanEngine::terminate`:

1. `timeout 8 env <prefix> swanctl --terminate --ike <conn> --timeout 5`. This is non-forced: charon sends the DELETE and waits up to 5s for the response.
2. `timeout 10 env <prefix> swanctl --list-sas --ike <conn>`. If the query **succeeds and shows no IKE_SA header**, teardown is acknowledged. Log `IKE_SA deleted (acknowledged)`.
3. Otherwise (still listed, **or the query failed**), run the existing `timeout 10 … --terminate --ike <conn> --force`. Log `ePDG did not acknowledge DELETE within 5s; forced`.
4. Return. **No sleep** on either path.

The worst case is 8 + 10 + 10 = 28s, under the shutdown read guard; this is documented at the constants. The shutdown path (`shutdown.rs`) is already non-forced with a 5s bound and is unchanged.

**Rationale**: an acknowledged DELETE is the ePDG's own statement that the old session is gone, which is exactly what #106's guessed 4s sleep tried to approximate. Deciding by presence rather than by swanctl's exit status avoids depending on how swanctl reports a timed-out terminate. A failed presence query (broken vici) is treated as "present", because the forced delete is the only safe step then. On the forced path the ePDG is not answering, so a delay would buy nothing. charon's own retry (`keyingtries = 0`, `retry_initiate_interval = 10`) handles a first initiate it refuses.

**Alternatives considered**: a fixed post-terminate sleep (#106; rejected: guesses at a timing the ePDG reports directly); `--force --timeout` together (rejected: `--timeout` cancels `--force`'s no-wait behaviour, per the existing comment); graceful-only (rejected: #107's 52s stall behind a pending exchange).

## R4 — Recognising the *new* negotiation's P-CSCF

**Decision**: before each `tick_steady_state`, `mark = engine.log_mark(runner)` (the charon log's current byte length). `extract_pcscf_after(log, conn, mark)` runs `latest_pcscf_batch` on `log[mark..]`, or on the whole log if `mark > log.len()` (rotated or truncated). It returns `pick_stable_pcscf(batch)` **only if that batch was closed** by a `CHILD_SA … established` line inside the slice. `latest_pcscf_batch` gains a "closed" flag in its return value. `fresh_pcscf_since(runner, mark)` wraps the read.

**Rationale**: today's sync calls `latest_pcscf` straight after a detached `--initiate`, so it always reads the previous batch. That is the defect. A byte marker is exact, cheap, and needs no change to the log format. The live log is ~31KB after 3h, so re-reading it every second for up to 20s costs nothing. Requiring a closed batch means a half-logged negotiation is never accepted. The `<conn|N>` filter stops another line's negotiation from counting.

**Alternatives considered**: `is_tunnel_established` (rejected: it scans the whole history and is true immediately, which is why #106's proposed poll could not work); querying the VIP over vici (rejected: carries no P-CSCF); timestamp parsing (rejected: the log's time format is not configured for it).

## R5 — Exactly one agent restart per rebuild

**Decision**: a per-line `Arc<AtomicBool>` "rebuild in progress", shared by the steady loop and the agent loop:

- The steady loop sets it before the P-CSCF wait.
- After the wait (synced or timed out), it writes `pcscf_source_path`, `pkill`s the agent, and clears the flag.
- The agent loop, before spawning, waits (1s polls) while the flag is set. That wait is bounded by `POST_RECOVERY_PCSCF_WAIT` + 5s, so a bug can never wedge the line.
- The wait checks `shutting_down` each poll and holds no lock that shutdown waits on.

**Rationale**: during the wait, the old agent (bound to a dead address) can exit on its own. The agent loop would then respawn it 5s later with the stale P-CSCF, and the post-sync `pkill` would make a second restart, breaking SC-002. Gating the spawn keeps it to one.

**Alternatives considered**: no gate (rejected: SC-002); killing the agent *before* the wait (rejected: the loop respawns it after 5s, before the new P-CSCF is known, which is the same race).

## R6 — Where to clear leaked Gm IPsec state

**Decision**: in the agent loop, immediately before `runner.spawn(ip netns exec <netns> … vowifi-ims-agent …)`, and for the **strongswan engine only**:

1. Count the entries from `run_in_netns(netns, ["ip","xfrm","state"])` and `["ip","xfrm","policy"]`.
2. If either count is nonzero, run `["ip","xfrm","state","flush"]` and `["ip","xfrm","policy","flush"]` in the netns, and log `cleared K stale Gm SAs / P policies before agent start`.

**Rationale**: the line namespace holds only the IMS agent's Gm state. The ePDG CHILD_SA lives in the host namespace, keyed by `if_id` (see `epdg_iface.rs`). At this point no agent for the line runs, so nothing live is cleared, unlike #106's proposal, which flushed under a running agent and would have cut Gm signalling. The agent has no SIGTERM handler, so this is the one place cleanup reliably happens.

**SWu engine**: skipped. Its dialer manages its own tunnel, and it has not been verified to keep no kernel XFRM state in the netns. Recorded as a known gap; it may be enabled later once verified.

**Alternatives considered**: a SIGTERM handler in the agent calling `session.cleanup()` (rejected: out of scope, and a SIGKILL would still leak); flushing in the steady loop (rejected: a live agent); per-SA deletes keyed by the old registration (rejected: the supervisor does not know the agent's SPIs or ports).

## R7 — Alerting on a flapping tunnel

**Decision**:

**Supervisor side** (orchestrate, per line): a `RebuildWindow { window, threshold: 3, rebuilds: VecDeque<Instant>, latched: bool }`, with `window = config.alerts.tunnel_failure_thresholds.unhealthy_sec`.
- `record(now)` is called on every recovery where `recovery_restarts_agent(reason)` is true.
- `tick(now)` runs every steady loop iteration: it drops entries older than `window`, latches when `rebuilds.len() >= threshold`, and unlatches once the newest rebuild is at least `window` old.
- A change in the latch is reported as `AgentState.tunnel_flapping`. Logs: `tunnel flapping (N rebuilds within Ws)` / `tunnel no longer flapping`.

**Ingest side** (`apply_state`, supervisor reports):
- The supervisor's tunnel health is `child_sa_up && !tunnel_flapping`.
- When `tunnel_flapping` becomes true and the supervisor record has no open streak, the streak is opened **backdated by the tunnel-failure threshold**, so the existing folded decision fires on that same report.
- The streak closes only when the CHILD_SA is up and the tunnel is no longer flapping. Recovery and all-clear then follow the existing phase machine: once per outage, one all-clear.
- A continuous outage and flapping share the one streak, so they cannot alert twice for overlapping time.
- The record keeps `streak_opened_by_flapping` for the alert text.

**Rationale**: reuses #118's fold, phases and in-flight handling unchanged. Only the input definition of "unhealthy" widens. The window is computed where the rebuilds happen, so no timestamps cross the process boundary.

**Alternatives considered**: a separate alert category and config (rejected in Q2); counting rebuilds in ingest from `child_sa_up` transitions (rejected: transitions can be missed between heartbeats, and a CHILD_SA re-initiate is not an IKE_SA rebuild); backoff (rejected in Q2).

## R8 — The loss cause in the alert text

**Decision**: on each `IkeSaLost` recovery the supervisor reports `AgentState.ike_sa_loss_cause = Some(cause)`. The reporter heartbeats the last state, so it sticks until the next loss. Ingest stores it as `last_ike_sa_loss_cause` on the supervisor record. The TunnelFailure Failure description becomes one of:

- streak from an outage: `VoWiFi line's tunnel non-established for over {unhealthy_sec}s (last IKE_SA loss: {cause})`
- streak from flapping: `VoWiFi line's tunnel rebuilt repeatedly (3+ times within {unhealthy_sec}s; last IKE_SA loss: {cause})`
- the suffix is omitted when no cause was ever reported.

Recovered text is unchanged.

**Rationale**: answers "carrier or us?" at a glance (spec Q3). `IkeSaLossCause` is a closed enum, so neither wire nor text cardinality can grow.

**Alternatives considered**: a cause metric label (rejected in Q3); free-form log excerpts in the alert (rejected: could leak subscriber identifiers from the charon log).
