# Feature Specification: One P-CSCF probe per line: idle keepalive off by default, HEALTHCHECK reads the agent's status

**Feature Branch**: `118-pcscf-probe-reduction`  
**Created**: 2026-10-16  
**Status**: Draft  
**Input**: User description: "Address issue #120, item 1 ("safe now"): reduce the P-CSCF TCP 5060 probes from three to one per line. (a) The idle-tunnel keepalive thread in orchestrate.rs (which connects to the P-CSCF every 30s and discards the result) must NOT be deleted; instead move it behind a configuration option and disable it by default. (b) Point the Docker HEALTHCHECK (healthcheck.rs) at the agent's status instead of probing the P-CSCF over TCP. The supervisor's steady-state tick probe (line_supervisor.rs via engines.rs pcscf_reachable) stays unchanged — it is the only thing that triggers repair_default_route and PcscfUnreachable rebuilds (single-P-CSCF carriers like Vodafone), and the DPD/NAT-T keepalive behaviour seen in soak run B (supervisor-only probe, 4.5h, clean on Vodafone, zero DPD retransmits) is the target state. Context: soak data in GitHub issue #120 — baseline ~360 connects/line/hour, 96% of idle-tunnel packets; run B removed two thirds of the probes with no regressions."

## Background

Every VoWiFi line today opens a TCP connection to its P-CSCF's SIP port (5060) through the ePDG tunnel from **three** places, each every 30 seconds:

1. **The supervisor's steady-state tick.** It acts on the result: a failed probe leads to a default-route repair (`DefaultRouteMissing`) or a tunnel rebuild (`PcscfUnreachable`).
2. **The idle-tunnel keepalive thread**, started per line alongside the IMS agent. It **discards the result**. It was written to keep the bearer's NAT mapping alive, from before the bridge had any other traffic on an idle tunnel. Its interval is hard-coded to 30s; the existing `[vowifi].keepalive_interval_sec` setting (default 20) is ignored.
3. **The container `HEALTHCHECK`**, which runs every 30s and, for each line, checks the tunnel interface has an address and then probes the P-CSCF the same way.

Issue #120 measured this on two live lines, Vodafone IN and Jio IN, both VoWiFi over the strongSwan engine, with 4.5-hour idle soaks:

- At baseline the three probes make about **360 connects per line per hour**, which is **96% of the idle tunnel's packets** (about 2,300 outer packets and 286 KB an hour), against 90 packets for the agent's own OPTIONS ping.
- **Run B, the supervisor probe only** (keepalive thread and HEALTHCHECK probe off), ran 4.5h on Vodafone with no problems: registered throughout, zero DPD retransmits, and strongSwan's own DPD and NAT-T keepalives back in operation (~155 IKE/h, ~110 NAT-T/h).
- **Run C, no probes at all**, caused one false dead-peer teardown on each carrier, a lost default route was never repaired, and a dead P-CSCF on a single-P-CSCF carrier never recovered. The supervisor probe is the only thing that triggers those repairs, so it **stays**. Removing it is a separate, later piece of work in #120 that needs three replacements first.

This feature makes run B the shipped default. The keepalive thread is **kept, behind a configuration option that is off by default**, so an operator whose carrier turns out to need the extra traffic (for example an S2b eviction like #106) can switch it back on without a new build. The HEALTHCHECK stops probing the P-CSCF and asks the line's IMS agent instead. The agent already holds a live view of its own signalling connection to the P-CSCF.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - An idle line no longer runs the discard-only keepalive (Priority: P1)

An operator runs one or more VoWiFi lines with default configuration. Once the bridge has started, each line's tunnel carries one P-CSCF reachability probe per 30 seconds (the supervisor's), not two. Nothing else about the line changes: it registers, refreshes, rekeys, takes and places calls exactly as before.

**Why this priority**: The keepalive thread is half of the probes being removed, and the only part of this feature that changes the tunnel's own traffic pattern. Run B is the evidence that this is safe.

**Independent Test**: Start a line with default configuration and capture inside the line's namespace for 10 minutes. Count TCP SYNs to the P-CSCF's port 5060. With the HEALTHCHECK not running (or after Story 2), expect about 20 (one per 30s). Expect the startup log to say the idle keepalive is off. Unit-level: the line-startup path, given a configuration with the option off, starts no keepalive loop; given it on, starts one.

**Acceptance Scenarios**:

1. **Given** a configuration that does not mention the new option, **When** a VoWiFi line starts, **Then** no idle-tunnel keepalive loop is started for that line, and the bridge logs once at startup that the idle-tunnel keepalive is disabled.
2. **Given** the option is explicitly turned on, **When** a VoWiFi line starts, **Then** the keepalive loop is started for that line exactly as it behaves today (re-reads the line's P-CSCF file every cycle, connects to its port 5060 inside the line's namespace with the same 3s connect timeout, and discards the result), and the bridge logs once that it is enabled, with its interval.
3. **Given** the option is on, **When** the keepalive runs, **Then** its interval comes from `[vowifi].keepalive_interval_sec` instead of the hard-coded 30s.
4. **Given** the option is off, **When** the supervisor's steady-state tick runs, **Then** its P-CSCF probe, its default-route repair and its `PcscfUnreachable` rebuild behave exactly as they do today.
5. **Given** several lines, **When** the option is on or off, **Then** it applies the same way to every line.

---

### User Story 2 - The container HEALTHCHECK asks the agent instead of probing the P-CSCF (Priority: P1)

An operator, or Docker/compose restart policy, relies on the container's health status. The HEALTHCHECK still reports unhealthy when a line cannot work: daemon down, tunnel interface without an address, configured line not found, registration lapsed, line's IMS agent not responding, or the agent reporting that its signalling connection to the P-CSCF has failed. It reaches that verdict without opening any connection to a P-CSCF.

**Why this priority**: This is the other half of the probes being removed (a third of today's total), and the HEALTHCHECK is what operators and restart policies watch. It must stay at least as able to detect a broken line as it is now.

**Independent Test**: Unit tests over the HEALTHCHECK's decision logic with a mock runner and mock agent replies: the agent answering healthy, not answering at all, reporting its Gm connection failed, reporting reconnecting, an older agent that leaves out the connection field, and an agent that is mid-call. Assert that no P-CSCF connect is attempted in any case. On a live line, run `gsm-sip-bridge healthcheck` while capturing in the line's namespace and confirm it causes no SYN to the P-CSCF and exits 0. Stop the line's agent and confirm it exits non-zero, naming the line.

**Acceptance Scenarios**:

1. **Given** a line whose tunnel interface has an address and whose agent answers the status query reporting its Gm connection `up`, **When** the HEALTHCHECK runs, **Then** the line is healthy and no TCP connection is made to its P-CSCF.
2. **Given** a line whose agent does not answer the status query within the HEALTHCHECK's per-line time budget (refused, timed out or garbled reply), **When** the HEALTHCHECK runs, **Then** the line is reported unhealthy with a fault that names the line and says its agent is not responding.
3. **Given** a line whose agent reports its Gm connection as `failed`, **When** the HEALTHCHECK runs, **Then** the line is reported unhealthy with a fault that names the line and the agent's reported reason and active P-CSCF when it gives them.
4. **Given** a line whose agent reports its Gm connection as `reconnecting`, **When** the HEALTHCHECK runs, **Then** the line is **not** faulted for it. Reconnection is a transient state the agent is already handling, and the container's retry window allows for it.
5. **Given** a line whose agent is busy with a call, or whose PBX-side registration is down, **When** the HEALTHCHECK runs, **Then** the line is not faulted for either: neither is about P-CSCF reachability, which is the check being replaced.
6. **Given** an agent from an older build whose reply leaves out the Gm connection field, **When** the HEALTHCHECK runs, **Then** an answered query counts as healthy for this check, and the line is not faulted for missing information.
7. **Given** a line whose tunnel interface has no address, **When** the HEALTHCHECK runs, **Then** it reports `TunnelInterfaceHasNoAddress` exactly as today, without querying the agent.
8. **Given** a line that has not reported a P-CSCF yet (empty or missing P-CSCF file), **When** the HEALTHCHECK runs, **Then** it is treated as today: not a fault on its own. The agent check still applies once the agent is running.
9. **Given** the existing checks (metrics endpoint down, configured line not found, lapsed registration from `/metrics`, VoWiFi disabled, circuit-switched disabled, zero resolved lines), **When** the HEALTHCHECK runs, **Then** each behaves and reports exactly as it does today.

---

### User Story 3 - An operator can turn the keepalive back on and see that it is on (Priority: P2)

An operator on a carrier that turns out to evict idle bearers, or who wants to repeat the #120 soak comparison, sets the option to on, recreates the container, and gets the old keepalive back with no new build. The configuration reference documents the option, its default, why it is off, and when to turn it on.

**Why this priority**: The thread is kept rather than deleted so this escape hatch exists. Without documentation and a visible startup log line it is too easy to miss.

**Independent Test**: Set the option on in a test config, start the bridge, confirm the startup log line and about 2 SYNs per 30s to the P-CSCF in the line's namespace (supervisor and keepalive). Check `docs/configuration.md` lists the option and that `keepalive_interval_sec` now says it only applies when the keepalive is on.

**Acceptance Scenarios**:

1. **Given** `docs/configuration.md`, **When** an operator reads the `[vowifi]` section, **Then** it lists the new option with its default (off), says what it does and why it is off by default (with a reference to #120), and says `keepalive_interval_sec` only applies when it is on.
2. **Given** a config with an invalid value for the option (not a boolean), **When** the bridge loads the configuration, **Then** it is rejected with an error naming `vowifi.<option>`, consistent with the other `[vowifi]` keys.
3. **Given** the bridge's `config` export command, **When** it prints the effective `[vowifi]` settings, **Then** the new option is included alongside `KEEPALIVE_INTERVAL`.

---

### Edge Cases

- **Agent restarting during the HEALTHCHECK** (for example the ~22s `IkeSaLost` recovery or a planned rebuild): a single check fails with "agent not responding". The container goes unhealthy only after its existing retries (3 × 30s), the same tolerance today's P-CSCF probe gets during a rebuild. No change to the Docker `HEALTHCHECK` timing is required.
- **HEALTHCHECK time budget with several lines**: the Docker timeout is 5s for the whole run. Per-line agent queries must be bounded so that a hung agent on one line cannot make the whole check time out without naming that line. Today's per-line TCP probe has a 3s connect timeout, and the new query must fit the same total budget as the lines it replaces.
- **Agent alive but P-CSCF actually dead, on a single-P-CSCF carrier** (finding 4 in #120): the agent's Gm connection goes `failed` or stays `reconnecting`. The supervisor's own probe still detects it and rebuilds the tunnel. The HEALTHCHECK is an observer, so it is acceptable that it reports what the agent reports rather than doing its own probe.
- **Default route lost in the namespace** (finding 3 in #120): unchanged. The supervisor's probe fails and `repair_default_route` runs. This feature does not touch that path.
- **Option on, P-CSCF file missing or empty**: the keepalive skips the cycle and sleeps, as it does today.
- **`keepalive_interval_sec` out of range**: rejected at config load as it already is, even when the keepalive is off, so turning it on later cannot surface a latent bad value.
- **Non-strongSwan tunnel engine** (SWu): the keepalive option applies the same way. The HEALTHCHECK's agent query is engine-independent.

## Requirements *(mandatory)*

### Functional Requirements

**Idle-tunnel keepalive**

- **FR-001**: The idle-tunnel keepalive loop MUST be kept in the codebase (not deleted), and MUST only start for a line when a new `[vowifi]` boolean option (proposed name `idle_keepalive`) is on.
- **FR-002**: The option MUST default to **off** when absent from the configuration.
- **FR-003**: When on, the loop MUST behave as it does today (re-read the line's P-CSCF file every cycle, TCP connect to its port 5060 inside the line's namespace with the runner's 3s probe, discard the result), except that its interval MUST come from `[vowifi].keepalive_interval_sec`.
- **FR-004**: The default of `[vowifi].keepalive_interval_sec` MUST change from 20 to 30, so that turning the keepalive on reproduces the cadence that was measured as the #120 baseline.
- **FR-005**: The bridge MUST log once at startup whether the idle-tunnel keepalive is on or off, and when on, its interval.
- **FR-006**: The option MUST be validated at config load like other `[vowifi]` keys, included in the `config` export, and documented in `docs/configuration.md` together with the clarified meaning of `keepalive_interval_sec`.
- **FR-007**: The supervisor's steady-state P-CSCF probe and everything it triggers (`DefaultRouteMissing` repair, `PcscfUnreachable` rebuild, failure counting, alerts) MUST be unchanged.

**HEALTHCHECK**

- **FR-008**: The HEALTHCHECK MUST NOT open any connection to a line's P-CSCF.
- **FR-009**: For each resolved line whose tunnel interface has an address, the HEALTHCHECK MUST query the line's IMS agent for its status over the existing agent status query (the one `vowifi-status` uses), bounded by a per-line timeout that keeps the whole run inside the container's HEALTHCHECK timeout.
- **FR-010**: A line MUST be reported unhealthy, with a fault that names it, when its agent does not give a usable answer to the status query.
- **FR-011**: A line MUST be reported unhealthy, with a fault that names it, when its agent reports its Gm connection as `failed`. The fault MUST include the agent's active P-CSCF when the reply has one.
- **FR-012**: A line MUST NOT be faulted because its agent reports its Gm connection `reconnecting`, reports it is busy with a call, reports its PBX-side registration down, or leaves out the Gm connection field.
- **FR-013**: The existing `PcscfUnreachable` HEALTHCHECK fault MUST be replaced by the agent-based faults of FR-010 and FR-011. All other HEALTHCHECK checks and their ordering (metrics endpoint, configured-line-not-found, lapsed registration, tunnel interface address, VoWiFi/CS disabled shortcuts) MUST be unchanged.
- **FR-014**: The `tcp-probe` subcommand MUST keep working, since the supervisor and the optional keepalive use it for in-namespace probes.

**Verification**

- **FR-015**: Unit tests MUST cover FR-001–FR-003 (option off starts no loop, on starts one with the configured interval) and FR-008–FR-013 (each agent reply shape in User Story 2, plus an assertion that no P-CSCF connect is made).
- **FR-016**: The change MUST be checked on at least one live line (Vodafone rig) by capturing inside the line's namespace and counting connects to the P-CSCF's port 5060 with default configuration.

### Key Entities

- **Idle keepalive option**: a per-deployment `[vowifi]` boolean, off by default. It applies to every VoWiFi line and controls whether each line's discard-only P-CSCF keepalive runs. The cadence comes from `keepalive_interval_sec`.
- **Agent status reply**: the line agent's existing answer to a status query. It includes registration state, Gm connection health (`up` / `reconnecting since …` / `failed since …`), call-answer readiness with a blocked reason, and the active P-CSCF and pool. The HEALTHCHECK reads only the fields needed for FR-010–FR-012.
- **HEALTHCHECK line fault**: the per-line reason the container is unhealthy. `PcscfUnreachable` is replaced by an agent-not-responding fault and an agent-reports-Gm-failed fault.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With default configuration, an idle line's tunnel carries about **120 P-CSCF port-5060 connects per hour** (one every 30s), down from about 360: a two-thirds reduction, verified by an in-namespace capture of at least 10 minutes on the Vodafone rig.
- **SC-002**: Running the HEALTHCHECK causes **zero** connections to any P-CSCF, verified by capture during at least 3 consecutive HEALTHCHECK runs.
- **SC-003**: The HEALTHCHECK still reports the container unhealthy within its existing window (start period aside, 3 failed checks at 30s) when a line's agent is stopped, and names the line.
- **SC-004**: With default configuration, a live line stays registered across a soak covering at least one REGISTER refresh and one CHILD_SA rekey, with **zero** false dead-peer teardowns, matching #120 run B.
- **SC-005**: With the option on, the per-line connect rate returns to about **240 per hour** (supervisor plus keepalive, HEALTHCHECK excluded), showing the escape hatch works without a rebuild.
- **SC-006**: `make format`, `make lint` and `make test` pass. No existing HEALTHCHECK, supervisor or config test changes behaviour except those asserting the removed `PcscfUnreachable` HEALTHCHECK fault and the `keepalive_interval_sec` default.

## Assumptions

- Run B in #120 (Vodafone, 4.5h, supervisor probe only) is enough evidence to make "keepalive off" the default on every carrier. The Jio soak in #120 was run with zero probes (run C), not with the supervisor probe alone. The supervisor probe is what prevented the run C failures, so the Jio check in SC-004 is a confirmation rather than a precondition. It is recommended but not blocking.
- The agent's existing status query (used by `vowifi-status`) is reachable from where the HEALTHCHECK runs, via the line's control port, and answers quickly enough to fit the 5s HEALTHCHECK timeout for the deployments' line counts (up to `max_lines`).
- "Gm connection `failed`" is the agent's own judgement that it cannot reach a P-CSCF. That is the closest observer-side equivalent of the removed probe, and it is the signal the HEALTHCHECK should surface. `reconnecting` is transient and left to the container's retries and to the supervisor.
- Lapsed registration is already caught by the HEALTHCHECK through `/metrics` (spec 039, FR-020), so the agent check does not need to duplicate it.
- Changing `keepalive_interval_sec`'s default from 20 to 30 affects no running behaviour, because the value was never read by the keepalive. Its only other consumer is the `config` export.
- No Docker `HEALTHCHECK` directive change (interval, timeout, retries, start period) is needed.

## Out of Scope

- Removing or changing the supervisor's steady-state P-CSCF probe, and the three replacements #120 says it needs first (per-tick structural route check, agent liveness reported to the supervisor, DPD tuning).
- The possible IPv6 gap in `has_default_route` (#120 finding 5).
- Any change to strongSwan DPD or NAT-T settings, tunnel proposals or lifetimes.
- Per-line (rather than per-deployment) control of the idle keepalive.
- VoLTE lines. None of the three probes exist on the VoLTE path: the keepalive is started only by the two VoWiFi line-startup paths (strongSwan and SWu), the supervisor's steady-state probe belongs to the ePDG tunnel supervisor, and the HEALTHCHECK only walks VoWiFi lines (with VoWiFi disabled it returns on the metrics endpoint alone). VoLTE has no ePDG IPsec tunnel whose DPD or NAT mapping the probes could affect. The only P-CSCF "probes" in VoLTE are the startup address-discovery methods (DHCPv6, PCO, DNS), not reachability checks. This feature does not change VoLTE.
