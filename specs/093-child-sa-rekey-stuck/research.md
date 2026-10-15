# Research: stuck CHILD_SA rekey rebuild (093)

## R1 — What `swanctl --list-sas` reports for a CHILD_SA's lifetime (pinned 5.9.3 fork)

**Decision**: Read the `expires in Ns` field from the line under each CHILD_SA header. The newest usable CHILD_SA is the one with the highest `#N` (vici's `uniqueid`) whose state is in `USABLE_CHILD_SA_STATES`.

**Evidence**: sampled live from the Vodafone line on this host on 2026-10-15 (identities replaced):

```text
ims0: #3, ESTABLISHED, IKEv2, 0123456789abcdef_i* fedcba9876543210_r
  local  '0001010000000001@nai.epc.mnc001.mcc001.3gppnetwork.org' @ 192.0.2.10[4500] [2001:db8::1]
  remote '203.0.113.1' @ 203.0.113.1[4500]
  AES_CBC-128/HMAC_SHA1_96/PRF_HMAC_SHA1/MODP_1024
  established 10433s ago, rekeying in 3594s
  ims0: #12, reqid 1, INSTALLED, TUNNEL-in-UDP, ESP:AES_CBC-128/HMAC_SHA1_96
    installed 1109s ago, rekeying in 2167s, expires in 2852s
    in  cea6576a (-|0x00000017),  21600 bytes,   242 packets,     4s ago
```

`installed + expires in = 1109 + 2852 = 3961s`, which is the 3960s `life_time` that #107 recorded. `installed + rekeying in = 3276s`, which is the soft lifetime minus that SA's random jitter (up to 360s). Both are the daemon's defaults; the swanctl template sets no lifetime.

vici computes `rekey-time` and `life-time` as `deadline - now`. swanctl prints each only when the daemon has a deadline. So `expires in` is the time left to the hard expiry, whatever the rekey state. `rekeying in` probably goes negative once a rekey is overdue, but nothing here depends on that.

**Alternatives considered**:
- *Count ticks in `REKEYING`* (#127's proposal). Rejected. A rejected rekey is answered within one round trip, and the daemon schedules the retry a few seconds later. Between retries the CHILD_SA reads `INSTALLED`. #107's 105 rejections over 658s average one every ~6s. A 30s poll samples the in-flight state rarely and at random, so the counter would keep resetting.
- *`installed Ns ago` against a fixed age*. Rejected. It encodes the lifetime twice: once in the daemon and once here. `expires in` is the daemon's own deadline.
- *Query vici directly*. Rejected. The supervisor already runs `swanctl --list-sas --ike <conn>` every tick (FR-001: no extra query).

## R2 — Threshold

**Decision**: `REKEY_STUCK_THRESHOLD = 300s`.

**Rationale**: a natural rekey starts between `rekey_time - rand_time` and `rekey_time` (3240–3600s after install). That leaves 360–720s of hard lifetime. Even a rekey that needs every retransmit (4+7+13+23+42s ≈ 90s before the response) completes with 270s+ left only in the extreme case. That case is a rekey that is not working anyway: the IKE_SA would be declared dead at the same point. Below 300s, a CHILD_SA still being the same `#N` means its replacement did not happen. The first tick to see that has at least 270s before expiry (one 30s poll interval), against a rebuild that takes ~5s plus the 087 P-CSCF wait (bounded at `POST_RECOVERY_PCSCF_WAIT`).

**Alternatives considered**: a config key. Rejected, consistent with 087: no new keys, and the value follows from the daemon defaults the project pins (spec Assumptions).

## R3 — Where the decision lives

**Decision**: in `line_supervisor::tick_steady_state_with_pool`, after the structural checks pass and before the P-CSCF-change check. The engine reports the observation; the tick decides.

- `SteadyStateHealth::Ok` becomes `Ok(Option<ChildSaLifetime>)`. strongSwan fills it from the same listing it already parsed (FR-001). SWu reports `Ok(None)` (FR-013).
- `SteadyTracking` gains the per-line guard state (data-model.md). It already is the "state between ticks" struct, and it is `Copy`/`Default`.
- The rebuild itself is the existing `terminate` + `reinitiate` pair, returning `Recovered { reason: ChildSaRekeyStuck }`. The orchestrator's existing renegotiation branch then does the 087 P-CSCF wait, the agent restart, the flapping-window record and the tunnel signal. `recovery_renegotiates(ChildSaRekeyStuck)` is true without any change to that function, so FR-009 holds without new alert plumbing.

**Alternatives considered**: a separate engine method that lists SAs again. Rejected because it would run a second query per tick (FR-001).

## R4 — Rejection reason for the log (clarification Q2)

**Decision**: a new parser `child_sa_rejection_notify(charon_log, conn) -> Option<String>` returns the notify of the most recent CREATE_CHILD_SA response in the current IKE_SA session that carried an error notify. Examples: `INVAL_SEL`, `TS_UNACCEPT`, `NO_PROP`, `TEMP_FAIL`. It uses the same session scoping as `child_rekey_invalid_selectors_session`. It is called only when the tick has already decided to rebuild, so it costs nothing on a healthy line, and it never triggers anything on its own.

**Evidence**: #107's log shape `parsed CREATE_CHILD_SA response 12 [ N(INVAL_SEL) ]` (DBG1, present at `filelog default = 1`). A successful rekey response carries `SA No TSi TSr`. An error response carries only `N(...)`.

## R5 — Call state reaching the supervisor (User Story 3)

**Decision**: the VoWiFi agent writes its active-call count to a per-line file beside the P-CSCF files: `/tmp/pcscf-N.calls`, one decimal number and a newline. It uses the existing `pcscf_files::write_atomic`. The supervisor reads it with `runner.read_file` on the tick that found a stuck rekey, and only then.

- It writes `0` at the very start of `run_inner`, before any transport or registration work. A previous agent killed mid-call therefore cannot leave a stale `1` past the next agent's start, whether or not that agent ever registers.
- It rewrites the file on every `set_active_calls`. `AgentObservability` gets an optional calls-file path, set only on the VoWiFi path (from `pcscf_source`). VoLTE stays untouched.
- A missing, unreadable or unparseable file counts as "no active call" (spec US3 scenario 4). The worst case for a stale file is the window between an agent dying mid-call and its successor starting (the agent supervisor's 5s restart). The forced-rebuild margin bounds even that.

**Alternatives considered**:
- *Ask the daemon over the control socket.* The daemon has `active_calls` from the agent's reports. But the supervisor only *sends* on that socket today, and a request/response path is far more machinery than one file.
- *A heartbeat or mtime freshness check.* Rejected. A call can run for an hour without its count changing, so mtime says nothing. Writing at agent start is what actually closes the stale window.
- *A PID check of the writer.* Rejected as more moving parts than the start-of-run reset.

## R6 — Forced-rebuild margin

**Decision**: `REKEY_STUCK_FORCE_MARGIN = 90s`. The first tick at or under 90s has at least 60s before expiry, which is enough for a forced terminate and initiate. The deferral window is therefore at most 300 − 60 = 240s, about 4 minutes of call protected.

## R7 — Log-once guards

**Decision**: three `Option<u32>` fields in `SteadyTracking`, each holding the CHILD_SA `#N` it applies to. Each clears implicitly when the newest usable `#N` changes: it simply stops matching.

- `rekey_stuck_rebuilt`: the CHILD_SA a planned rebuild was already issued for (FR-006).
- `rekey_stuck_deferral_logged`: the CHILD_SA whose deferral was already logged (FR-012).
- `lifetime_missing_logged`: the CHILD_SA whose missing `expires in` was already logged (FR-011).

A respawned daemon numbers CHILD_SAs from 1 again, so a stale guard could in principle match a new CHILD_SA. The process-died branch resets the guards along with the other counters, and any tick without a usable CHILD_SA clears them, so that cannot happen.

## R8 — Testing approach

- Parser tables in `engines.rs` built from the live R1 shape (synthetic identities): one CHILD_SA, the natural-rekey overlap (`REKEYED #12` plus `INSTALLED #13`), `REKEYING`, a missing `expires in`, another connection's CHILD_SA, and a non-usable newer SA (`INSTALLING #13`).
- Tick tests in `line_supervisor.rs` with the existing `FakeEngine`, which is the established harness for this state machine and has a written justification there. They cover: at threshold, above threshold, one rebuild per `#N`, a new `#N` re-arms, deferral during a call, forced under the margin, unknown call state, missing lifetime logged once, and structural failures taking precedence.
- An orchestrator-level test (`orchestrate.rs`'s existing steady-state harness on `MockCommandRunner`) that a stuck rekey goes through the renegotiation path. It checks for exactly one agent restart and that the rebuild counts toward the flapping window.
- Live: a natural rekey on the Vodafone line logs no planned rebuild (SC-002). The stuck path cannot be produced on Vodafone or Jio (spec Assumptions).
