# Research: rebuild on an exhausted P-CSCF pool

## R1. Signal source: the pool state file, not the status port

- **Decision**: the supervisor reads `/tmp/pcscf-N.state` every tick.
- **Rationale**: the agent starts its status listener only after its first successful registration (`ims/agent/mod.rs`, `register_via_pool` then `run_status_listener`). Consider an agent restarted onto the same dead P-CSCF: it fails startup, exits and is respawned without ever answering. A supervisor relying on `StatusQuery` would rebuild once and then go silent, which is exactly the case the probe covers today.
- The file is written atomically (`pcscf_files::write_atomic`) on every mark and every successful registration, in both the startup phase and the running phase.
- **Alternatives**: `StatusQuery` was rejected for the reason above. A new agent-to-supervisor socket would mean more moving parts than reading a file.

## R2. Telling fresh marks from stale ones: `marked_at_unix`, no version bump

- **Decision**: add `marked_at_unix: u64` with `#[serde(default)]` to each persisted mark. The agent sets it to "now" whenever `on_failure` sets or renews a mark.
- The file `version` stays 1. An agent from before this field ignores it (serde ignores unknown fields by default; `PersistedPool` has no `deny_unknown_fields`). A file from before this field reads as 0, which is earlier than any baseline, so it never counts (FR-008).
- **Rationale**: marks survive agent restarts (FR-015 of spec 091), and they survive a rebuild that returns the same address (`replace_pool` keeps marks for addresses still advertised). Without a marking time, a stale mark left over from the episode that caused the previous rebuild would cause another immediately.
- **Alternatives**:
  - Derive the time from `until − mark_duration(failures)`. Rejected: wrong for `retry-after` marks, and it couples the supervisor to the agent's backoff formula.
  - Have the supervisor snapshot the file at rebuild time and diff later. Rejected: more state, and it is lost across a supervisor restart.
  - Bump the version. Rejected: the agent discards a whole file with an unknown version, so a downgrade would lose every mark.

## R3. The baseline, `negotiated_at`

- **Decision**: `SteadyTracking.negotiated_at: Option<SystemTime>`, set by the orchestrator at the point where the supervisor restarts (or first lets start) the agent onto a freshly negotiated P-CSCF:
  - strongSwan: on entry to the steady loop; after the `pkill` in the renegotiated branch of `Recovered`; and after the `pkill` in `PcscfChanged`.
  - swu: on entry; after the `pkill` in `Recovered` when `recovery_renegotiates(reason)`; and on `PcscfChanged`.
- With `None`, the check never fires.
- **Rationale**: during a rebuild, the old agent keeps running until `await_fresh_pcscf` returns (up to `POST_RECOVERY_PCSCF_WAIT`). With the tunnel down it can time out a REGISTER and mark the P-CSCF. If the baseline were taken when the rebuild is issued, those marks would count against the new tunnel and cause a second rebuild at once. Taking it after the old agent is killed means only an agent started on the new tunnel can mark against it.
- A route repair (`DefaultRouteMissing`) does not move the baseline: no negotiation happens, and no-route failures are never marked anyway.
- **Alternatives**: the time the log `mark` is taken. Rejected for the reason above.

## R4. Which mark causes count

- **Decision**: every cause except `retry-after`. That is `gm-keepalive`, `register-timeout`, `transport` and `endpoint-failure:*`.
- **Rationale**: on the Vodafone path the first mark is `gm-keepalive`, set when the keepalive escalation fires (`mark_session_pcscf`), before any REGISTER. Excluding it would delay detection by a full REGISTER timeout, or miss it when the escalation is all that marks. `retry-after` is the P-CSCF explicitly asking for a pause (spec clarification). The cause strings are the agent's `FailureCause` `Display` values; the supervisor matches `cause == "retry-after"` only, so a new agent cause counts by default.

## R5. Where the alert episode lives

- **Decision**: in `metrics::ingest`, as fields on the **supervisor's** `AgentRecord`:
  - `pool_exhausted_since: Option<Instant>`
  - `pool_exhausted_alert_phase: AlertPhase`
  - `pool_exhausted_rebuilds: u32`
  - `pool_exhausted_stale_ims_epoch: Option<u64>`

  The address list lives in a small side map keyed by module id, used only for the alert text, because `AgentRecord` is `Copy` and a `Vec` would break that.
- **Rationale**: the daemon is the only place that sees both signals. The supervisor reports the rebuilds, and the IMS agent reports `registered`. The supervisor heartbeats its record every report interval, so the threshold is evaluated without a timer. Using `decide_transition` and `record_alert_outcome` unchanged gives the same Pending, Alerted and all-clear semantics as `gm_connection_lost`.
- **Ending the episode**: the IMS agent's reporter epoch is random per process start. At every exhaustion rebuild, the epoch of the IMS record live at that moment is noted as stale. A `registered = Some(true)` report from any other epoch ends the episode. The pre-rebuild agent can still heartbeat `registered = true` while it is being replaced, but only a process started after the latest rebuild can end the episode.
- **Alternatives**:
  - Track the episode in the supervisor. Rejected: the supervisor cannot see registration.
  - End on any `registered = true`. Rejected: the stale heartbeat would end every episode at once.
  - End when the IMS record's unhealthy streak clears. Rejected: a fresh agent that registers on its first attempt never reports `false`, so nothing would clear.

## R6. Calls on swu

- **Decision**: the swu loop passes `line_in_call` instead of `|| false`.
- **Rationale**: the closure was `false` only because swu never has a stuck rekey to hold. It is now also the FR-007 deferral input. On swu, `check_rekey_stuck` is still never reached (`Ok(None)`), so this changes nothing else.

## R7. swu and the alert

- **Decision**: on swu, the rebuild works but the counter and the alert do not.
- **Rationale**: the swu loop has no supervisor `Reporter` (spec 096 made the same call for the counter). Neither live rig runs swu, and adding a reporter to that loop is separate work. This is documented in the configuration doc row.

## R8. Black-holing a P-CSCF on Vodafone (IPv6 Gm)

- **Decision**: add a host route for the P-CSCF into the veth, inside the line netns, then delete it afterwards:

  ```bash
  ip -n ims0 -6 route add <pcscf>/128 dev veth-ims0
  ```

- **Rationale**: the route exists, so `route_to` succeeds and failures are attributed to the P-CSCF. Packets go out on the veth, where nothing answers for that address: neighbour discovery fails, the connect fails and keepalives go unanswered.
- A `blackhole` route would read as no-route and would never mark (memory: pi-blackhole-one-pcscf-without-iptables).
- Jio uses the IPv4 `/32 via 10.99.0.2 dev veth-ims0` form, as in the 091 gate. The live run confirms that the IPv6 form produces marks (quickstart step 1, first check).
