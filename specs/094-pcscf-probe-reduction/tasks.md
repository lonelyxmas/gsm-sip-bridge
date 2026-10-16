# Tasks: One P-CSCF probe per line (094)

**Input**: `specs/094-pcscf-probe-reduction/` (spec.md, plan.md, research.md, data-model.md, contracts/, quickstart.md)

**Tests**: required. The constitution defaults to TDD, and FR-015 names the cases. Write each test before the code it covers and watch it fail first.

All paths are relative to `gsm-sip-bridge/src/` unless stated otherwise. Before every commit: `make format && make lint && make test`, with a private `CARGO_TARGET_DIR` (for example `CARGO_TARGET_DIR=$PWD/target-094`; memory: shared target dir clobbered by worktrees). Every commit message ends with the Co-Authored-By trailer. Fixtures use synthetic addresses only (`192.0.2.x`, `198.51.100.x`, `10.9.x.x`), never live P-CSCF addresses or subscriber identities (CLAUDE.md).

## Phase 1: Setup

- [X] T001 Confirm the branch is `094-pcscf-probe-reduction` on top of `origin/main` and that `make test` is green before any change. Read `supervise/orchestrate.rs` `start_line_tail` (the "Idle-tunnel keepalive" block), `commands/healthcheck.rs` `check_line`/`evaluate_with_metrics`, and `vowifi/mod.rs` `query_status`/`print_status`.

## Phase 2: Foundational (blocks US1 and US2)

- [X] T002 In `supervise/runner.rs`, give `MockCommandRunner` a recorder: a `tcp_connect_attempts: Mutex<Vec<(Option<String>, String, u16)>>` field (netns, host, port), pushed by both `tcp_connect_ok` (`None`) and `tcp_connect_ok_in_netns` (`Some(netns)`) before they look up the seeded result. Expose it with `pub fn tcp_connect_attempts(&self) -> Vec<(Option<String>, String, u16)>`. Add one test in the runner's mock tests showing both calls are recorded in order. No behaviour change elsewhere. It lands in the US1 commit.

## Phase 3: User Story 1: an idle line no longer runs the discard-only keepalive (P1) 🎯 MVP

**Goal**: with default config, `start_line_tail` starts no keepalive loop. With `idle_keepalive = true` it starts today's loop at `keepalive_interval_sec`. The supervisor probe is untouched.
**Independent test**: the config and orchestrate unit tests below. Live: about 20 SYNs to `:5060` per 10 minutes per line (quickstart).

- [X] T003 [US1] Config key, tests first. In the `config/mod.rs` tests, next to the existing `assert_eq!(cfg.vowifi.keepalive_interval_sec, 20)` default test:
  - change that assertion to `30`
  - add `assert!(!cfg.vowifi.idle_keepalive)` for an empty `[vowifi]`
  - add a test that `idle_keepalive = true` parses to `true`
  - add a test that `idle_keepalive = "yes"` fails to load, with an error mentioning `idle_keepalive`
  - add a test that `idle_keepalive = true` under a `[[vowifi.line]]` entry is rejected as an unknown key, matching how other unknown line keys are tested
  - add a test that `keepalive_interval_sec = 0` is still rejected while `idle_keepalive` is absent (the range check always runs)

  Then implement:
  - `config/raw.rs`: `pub idle_keepalive: bool` in `RawVowifi` (the field name is the TOML key; do not rename it). Default `false`, and `keepalive_interval_sec: 30` in `impl Default for RawVowifi`.
  - `config/mod.rs`: `pub idle_keepalive: bool` on `VowifiConfig`, with a doc comment: off by default since #120; the supervisor already probes the P-CSCF every 30s; the extra traffic suppresses strongSwan DPD/NAT-T; turn on only for a carrier that drops idle bearers (#106). Update the `keepalive_interval_sec` doc comment to say it only applies when `idle_keepalive` is on. Set the test/default constructor at the `keepalive_interval_sec: 20` line to `30` and `idle_keepalive: false`.
  - `config/build.rs`: carry `idle_keepalive: raw.idle_keepalive` into `VowifiConfig`, beside `keepalive_interval_sec`.

  Fix any other struct literal of `VowifiConfig`/`RawVowifi` the compiler points at (tests and fixtures).
- [X] T004 [US1] Keepalive gate, tests first, in the `supervise/orchestrate.rs` tests module:
  - `idle_keepalive_interval(&cfg)` is `None` for `VowifiConfig::default()`-style config, and `Some(Duration::from_secs(45))` with `idle_keepalive = true, keepalive_interval_sec = 45`.
  - `idle_keepalive_once(&mock, "ims0", "/tmp/pcscf-0")`, with the file seeded `"192.0.2.10\n"`, makes exactly one attempt `(Some("ims0"), "192.0.2.10", 5060)` per `tcp_connect_attempts()`. With the file blank or missing it makes none. The connect's result (seed true or false) changes nothing observable (it returns `()`).

  Then implement, per plan.md design note 2:
  - `fn idle_keepalive_interval(v: &VowifiConfig) -> Option<Duration>`.
  - `fn idle_keepalive_once(runner: &dyn CommandRunner, netns: &str, pcscf_path: &str)`, holding today's loop body verbatim (read file, trim, skip if empty, `tcp_connect_ok_in_netns(netns, addr, line_supervisor::PCSCF_SIP_PORT)`, discard). Move the long comment about the zombie `timeout` processes and the 2026-08-17 outage with it.
  - In `start_line_tail`, replace the hard-coded `Duration::from_secs(30)` block (including the "read by the caller and threaded through would be cleaner" comment) with: `if let Some(interval) = idle_keepalive_interval(&ctx.config.vowifi) { std::thread::spawn(move || loop { idle_keepalive_once(&*runner, &netns, &pcscf_path); runner.sleep(interval); }); }`. Keep `let _ = initial_pcscf;` inside the closure, or drop the binding outside it if clippy objects to it being unused when the gate is off.
  - Update `start_line_tail`'s doc comment ("+ the idle-tunnel keepalive") to say the keepalive is optional, off by default (`[vowifi].idle_keepalive`, #120).
- [X] T005 [US1] Startup log per contracts/logs.md. In `supervise/orchestrate.rs` `start_vowifi_subsystem`, before any line starts, log once at `info` with fields `idle_keepalive` and, when on, `interval_sec`, using the exact off and on messages from the contract. If `start_vowifi_subsystem` already has a test harness through `MockCommandRunner`, put the message-choosing logic in a small pure fn (`fn idle_keepalive_log_line(v: &VowifiConfig) -> String`) and unit test both messages. Otherwise test only that fn.
- [X] T006 [US1] Grep `supervise/` for any other caller of the old keepalive or reader of `keepalive_interval_sec` (expect none beyond `commands/config.rs`). Confirm `line_supervisor.rs` and `engines.rs` are unchanged in `git diff` (FR-007). Commit: `feat(vowifi): idle-tunnel keepalive behind [vowifi].idle_keepalive, off by default (#120)`.

**Checkpoint**: US1 is shippable alone. With the HEALTHCHECK still probing, an idle line drops from about 360 to about 240 connects per hour.

## Phase 4: User Story 2: the HEALTHCHECK asks the agent instead of probing the P-CSCF (P1)

**Goal**: the HEALTHCHECK never connects to a P-CSCF. A line is faulted only on no agent answer or Gm `failed`. The run stays inside 5s.
**Independent test**: the healthcheck unit tests (one per row of contracts/healthcheck.md), all asserting `tcp_connect_attempts()` is empty, plus the loopback tests of the bounded query.

- [X] T007 [P] [US2] Bounded status query, tests first, in the `vowifi/mod.rs` tests. Use a real `std::net::TcpListener` on `127.0.0.1:0`; this is integration, not a mock:
  - (a) A peer that reads the request and writes a `RegistrationStatusReply` line gets that reply back.
  - (b) A peer that accepts and never writes: `query_status_bounded(addr, 1s, Some(300ms))` returns `Err` in under about 1s (assert elapsed < 1s).
  - (c) A port with no listener (bind, take the port, drop the listener) returns `Err` quickly.
  - (d) A peer that writes `not json\n` returns `Err`.

  Then implement `pub fn query_status_bounded(addr: &str, connect: Duration, read: Option<Duration>) -> BridgeResult<ControlMessage>`: the same as `query_status`, plus `stream.set_read_timeout(read)` before reading. Make `query_status` call it with `(3s, None)`, so `vowifi-status` behaviour is unchanged. Doc comment: why the HEALTHCHECK needs the read bound (research R3).
- [X] T008 [US2] Agent-status seam in `supervise/runner.rs`, after T007:
  - `pub enum AgentStatusProbe { Reply(crate::vowifi::control::ControlMessage), NoAnswer(String) }` (`Debug, Clone, PartialEq, Eq`), documented like `HttpProbe`.
  - Trait method `fn agent_status(&self, addr: &str) -> AgentStatusProbe;`, with a doc comment that names the `http_get` precedent and why it is routed through the runner (research R3).
  - `RealCommandRunner`: `query_status_bounded(addr, 1s, Some(1s))`, mapping `Ok` to `Reply` and `Err(e)` to `NoAnswer(e.to_string())`.
  - `MockCommandRunner`: an `agent_statuses: Mutex<HashMap<String, AgentStatusProbe>>` seeded by `pub fn set_agent_status(&self, addr: &str, probe: AgentStatusProbe)`. Unseeded addresses return `NoAnswer("unseeded".into())`. Record queried addresses for `pub fn agent_status_queries(&self) -> Vec<String>`.

  If `supervise::runner` cannot depend on `vowifi::control` without a cycle, put `AgentStatusProbe` in `vowifi::control` and import it into the runner instead.
- [X] T009 [US2] HEALTHCHECK decision, tests first, in the `commands/healthcheck.rs` tests:
  1. Change the `line(index)` fixture's `veth_local_addr` to the bare `format!("10.9.{index}.1")`. Real resolutions store a bare IP (`config/mod.rs` default `10.99.0.1`), and the agent address is built from this field. Add a helper `fn agent_addr(index) -> String` returning `format!("10.9.{index}.1:{}", crate::vowifi::AGENT_A_STATUS_PORT)`, and helpers to build `RegistrationStatusReply` with a given `gm_connection` and an optional active P-CSCF (all other fields default/None).
  2. Add one test per verdict row in contracts/healthcheck.md. Each seeds an addressed interface and a non-empty `/tmp/pcscf`, seeds `set_agent_status`, asserts the `Health` verdict and, where relevant, the `Display` string, and asserts `runner.tcp_connect_attempts().is_empty()`:
     - `up` → Healthy
     - `NoAnswer("connection refused")` → `AgentNotResponding { addr: "10.9.0.1:5071", reason }`
     - `Reply(StatusQuery)` → `AgentNotResponding` with `unexpected reply`
     - `failed since 2026-10-16T00:00:00+00:00` with P-CSCF `192.0.2.10` → `GmConnectionFailed`, Display contains `(P-CSCF 192.0.2.10)`
     - `failed since …` with no P-CSCF → Display without the parenthesis
     - `reconnecting since … (attempt 2)` → Healthy
     - `""` (older agent) → Healthy
     - `up` with `can_answer: false, blocked_reason: Some("busy")` → Healthy
     - `state: "Unregistered"` with `up` → Healthy
     - no address on the interface → `TunnelInterfaceHasNoAddress`, and `agent_status_queries()` is empty
     - blank P-CSCF file → Healthy, and `agent_status_queries()` is empty
  3. Rewrite the existing tests that seed `set_tcp_connect_ok_in_netns(…, 5060, …)` to seed the agent instead, keeping their intent:
     - `a_configured_line_not_found_is_unhealthy_alongside_an_otherwise_healthy_resolved_line`
     - `cs_disabled_does_not_suppress_real_vowifi_line_checks`
     - `a_line_with_an_address_and_a_reachable_pcscf_is_healthy` (rename it to `…_and_a_responsive_agent_is_healthy`)
     - `a_line_whose_pcscf_does_not_answer_sip_is_unhealthy` (rename it to `a_line_whose_agent_does_not_answer_is_unhealthy`)
     - `every_line_is_checked_and_faults_are_reported_per_line`
  4. Replace `the_pcscf_is_probed_inside_the_lines_namespace_not_the_default_one` with `the_healthcheck_never_connects_to_the_pcscf`. Seed the P-CSCF as reachable both in the namespace and by default, seed a responsive agent, assert Healthy and zero connect attempts. Then seed the agent as `NoAnswer` and assert the line is faulted even though the P-CSCF "answers". Keep the old test's history in its doc comment: the default-namespace bug, now moot.

  Then implement, per data-model.md and plan.md design note 4:
  - Replace `LineFault::PcscfUnreachable` with `AgentNotResponding { addr, reason }` and `GmConnectionFailed { gm_connection, pcscf: Option<String> }`, with doc comments and `Display` exactly as in contracts/healthcheck.md.
  - `fn classify_agent_reply(addr: &str, probe: AgentStatusProbe) -> Option<LineFault>`, using the data-model.md classification table. Use `gm_connection.starts_with("failed")` and `pcscf.as_ref().map(|p| p.active.clone()).filter(|a| !a.is_empty())`.
  - In `check_line`, keep steps 1 and 2. Replace the `tcp_connect_ok_in_netns` block (and its namespace comment) with the agent query on `format!("{}:{}", line.veth_local_addr, crate::vowifi::AGENT_A_STATUS_PORT)`. Remove the now-unused `PCSCF_SIP_PORT` const. Update the module doc's `TCP connect, not ICMP, to the P-CSCF` bullet to describe the agent query and point at #120.
- [X] T010 [US2] Concurrency (research R5). Test first: with three lines, where line 1's agent is `NoAnswer` and lines 0 and 2 are `up`, `line_faults` is exactly `[(1, AgentNotResponding …)]`, in line order. Repeat with lines seeded in reverse index order in the resolution and assert the faults keep resolution order. Then, in `evaluate_with_metrics`, run `check_line` per resolved line inside `std::thread::scope` (one spawned thread per line, joined in order, a panic propagated as a panic) instead of the sequential `filter_map`. Add a comment that the per-query 1s+1s bound and the concurrency together keep the run under Docker's 5s timeout for any `max_lines`.
- [X] T011 [US2] Check `commands/healthcheck.rs`'s `run()` still prints each fault as `line N: <fault>`. Grep the crate for `PcscfUnreachable`: only `supervise/` (`DegradeReason::PcscfUnreachable`) may remain. Confirm `tcp-probe` (`run_tcp_probe`) is untouched (FR-014). Commit: `feat(healthcheck): ask each line's IMS agent instead of probing the P-CSCF (#120)`.

**Checkpoint**: US1 and US2 together give about 120 connects per line per hour (SC-001) and zero from the HEALTHCHECK (SC-002).

## Phase 5: User Story 3: an operator can turn the keepalive back on and see that it is on (P2)

**Goal**: the key is documented, exported and visible.
**Independent test**: the export test, plus reading the docs against contracts/config.md.

- [ ] T012 [P] [US3] In `commands/config.rs`, test first: the export contains `IDLE_KEEPALIVE=false` by default and `IDLE_KEEPALIVE=true` when set, on the line after `KEEPALIVE_INTERVAL`. Then add `("IDLE_KEEPALIVE", v.idle_keepalive.to_string())` after the `KEEPALIVE_INTERVAL` entry. If an existing test asserts the full export text, update it, including `KEEPALIVE_INTERVAL=30`.
- [X] T013 [P] [US3] `config.toml.example` (repo root): replace the uncommented `keepalive_interval_sec = 20` and its comment with the commented-out block from contracts/config.md (`# idle_keepalive = false`, `# keepalive_interval_sec = 30`, with the #120 note). Check `sample_configs/` and `test/` for configs that set `keepalive_interval_sec`, and leave them as they are; an explicit value stays valid.
- [X] T014 [P] [US3] `docs/configuration.md` `[vowifi]` table: add an `idle_keepalive` row (boolean, `false`, with what it does, why it is off since #120 with a link, and when to turn it on: #106-style idle-bearer eviction; it takes effect when the container is recreated). Change the `keepalive_interval_sec` row to default `30`, "Interval for the idle-tunnel keepalive; used only when `idle_keepalive = true`."
- [ ] T015 [P] [US3] `docs/operations.md`: near the existing HEALTHCHECK text, add a short paragraph on what the HEALTHCHECK checks per VoWiFi line. Tunnel interface address, then (once a P-CSCF is captured) the line's IMS agent status on `:5071`: unhealthy on no answer or Gm `failed`, not on `reconnecting`. The P-CSCF is no longer probed by the HEALTHCHECK; the supervisor still probes it and repairs. Include the two new stderr fault strings so an operator can grep for them.
- [ ] T016 [US3] `CHANGELOG.md` `[Unreleased]`: add a `### Changed` section if it is missing, with one entry (bold only the summary, per memory). Suggested: **One P-CSCF probe per VoWiFi line.** The idle-tunnel keepalive is now off by default (`[vowifi].idle_keepalive`, and it honours `keepalive_interval_sec`, default 30). The container HEALTHCHECK reads each line's IMS agent status instead of connecting to the P-CSCF, and reports `IMS agent … not responding` or `Gm connection failed`. About two thirds fewer connects on an idle tunnel ([#120](https://github.com/selvakn/gsm-sip-bridge/issues/120)). Commit: `docs: idle_keepalive, keepalive_interval_sec and the agent-based HEALTHCHECK (#120)`.

## Phase 6: Polish and live verification

- [ ] T017 Grep `docs/`, `README.md`, `docker/` and `specs/094-*/` for statements that the HEALTHCHECK probes the P-CSCF, or that the keepalive always runs, and fix any stale wording. Run `make format && make lint && make test` once more on the final tree.
- [ ] T018 Live, Vodafone rig (local host). Build and run the branch with the pinned local test setup in `test/` (memory: local docker tests go in `test/`), default config, and **recreate** the container (memory). Follow quickstart.md "Live" steps 1, 3 and 4: soak through at least one REGISTER refresh and one CHILD_SA rekey with the in-namespace SYN capture running, run the HEALTHCHECK by hand three times and once with the line's agent killed, then 10 minutes with `idle_keepalive = true`. Record SYNs/h, registration continuity, any `dead peer`/`IkeSaLost`, and the HEALTHCHECK exit codes and stderr.
- [ ] T019 Live, Jio Pi (`pi@192.168.100.2`, docker-compose v1, arm64 image via the `arm-build-200` skill, next `jio-gmN` tag; memory: remote Pi Jio deployment). Default config, recreate the container, then about 1h covering at least one REGISTER refresh with the SYN capture (quickstart step 2). Record the same measurements.
- [ ] T020 Write `specs/094-pcscf-probe-reduction/live-verification.md` with T018 and T019 results against SC-001 to SC-005 and SC-004a, using no real numbers or addresses (redact P-CSCF IPs to role names). Mark the spec Status as `Implemented`. Commit: `docs(094): live verification on Vodafone and Jio (#120)`.
- [ ] T021 Open the PR against `main`, referencing #120 item 1 and stating that the supervisor-probe removal (item 2) is out of scope. The PR body ends with the Claude Code attribution line.

## Dependencies

- T001 → T002 → US1 (T003 → T004 → T005 → T006).
- US2 needs T002 (the recorder) only, not US1: T007 → T008 → T009 → T010 → T011. US1 and US2 touch disjoint files except `supervise/runner.rs` (T002 and T008 in sequence).
- US3: T012 needs T003 (the config field). T013–T015 need nothing but are best written after US1 and US2 so the wording matches. T016 is last in the phase.
- Phase 6 needs every story merged on the branch. T018 and T019 can run at the same time on the two rigs. T020 needs both.

## Parallel opportunities

- After T002: T003 (config) and T007 (`vowifi/mod.rs` bounded query) touch different files.
- In US3: T012, T013, T014 and T015 are independent files.
- T018 (Vodafone) and T019 (Jio) run on separate hardware.

## Implementation strategy

1. **MVP = US1** (T001–T006): gating the keepalive alone removes a third of the probes and restores strongSwan's own DPD/NAT-T behaviour, which is the part of run B that matters on the wire.
2. **US2** (T007–T011) removes the second third and fixes the HEALTHCHECK's unbounded worst case.
3. **US3** (T012–T016) makes the escape hatch discoverable.
4. **Live verification** (T017–T021) is required before merge (FR-016). Commit per phase: four code and docs commits plus one verification commit.
