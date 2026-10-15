# Feature Specification: Per-line inner address family for the ePDG tunnel

**Feature Branch**: `092-epdg-inner-address-family`
**Created**: 2026-10-14
**Status**: Draft
**Input**: GitHub issue #126, the fix for #107 (CHILD_SA rekey rejected with `INVALID_SELECTORS`). Add an opt-in, per-line `[vowifi]` setting for the inner address family requested from the ePDG: `dual` (the default, unchanged), `ipv4` or `ipv6`. The default must produce exactly the tunnel configuration shipped today.

## Background

Some ePDGs accept the first data tunnel (CHILD_SA) and then reject every rekey of it with `INVALID_SELECTORS`. The one seen so far is mnc020.mcc250, from the #107 reporter. The sequence on that gateway:

1. The bridge asks for both an IPv4 and an IPv6 inner address and for traffic of both families.
2. The ePDG grants IPv4 and IPv6 traffic selectors, but assigns only an IPv4 inner address and only IPv4 P-CSCFs.
3. With no IPv6 address to bind to, the local IKE daemon drops the IPv6 selector from the tunnel it installs.
4. An hour later the rekey offer is rebuilt from that narrowed state and no longer matches what the ePDG granted. The ePDG answers `INVALID_SELECTORS` until the tunnel hard-expires.

Specs 086 and 087 made the recovery faster. The bridge now detects the expired tunnel and rebuilds it. On this gateway the line still loses about 60 seconds of service and does a full IMS re-registration roughly every hour.

The fault only occurs when IPv6 inside the tunnel is already unusable: no IPv6 inner address and no IPv6 P-CSCF. On such a line, asking for IPv4 only costs nothing, and in the reporter's controlled trials an IPv4-only negotiation rekeyed cleanly in 2 of 2 fresh sessions. When a carrier does assign IPv6, the IPv6 selector survives and the fault cannot occur, so those lines stay on `dual`.

The alternative was a patch to the IKE daemon fork that replays the ePDG's original selectors on rekey. It was rejected for these reasons:

- It would change behaviour for every line, because all lines share one IKE daemon.
- It has never had a clean container image build.
- It was validated on one gateway for two rekeys.
- The project would have to carry it through every fork update.

## Clarifications

### Session 2026-10-14

- Q: When a line's tunnel rekey is rejected with `INVALID_SELECTORS`, should the bridge say so in its own log? → A: Yes. When a `dual` line's rekey is answered `INVALID_SELECTORS`, log one warning per tunnel session that names the line and suggests `ipv4`. Never switch automatically.
- Q: With `tunnel_engine = "swu"`, what happens to an `ipv4` or `ipv6` value? → A: The bridge refuses to start and names where the value came from. It is not ignored, with or without a warning.
- Q: What should the config key be called? → A: `tunnel_address_family`, in both `[vowifi]` and `[[vowifi.line]]`.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Stop the hourly outage on an affected ePDG (Priority: P1)

An operator runs a VoWiFi line on a carrier whose ePDG rejects data-tunnel rekeys with `INVALID_SELECTORS`. The line drops for about a minute every hour and re-registers with IMS. The operator finds that symptom described in the example configuration and sets this line's inner address family to `ipv4`. After recreating the container, the line rekeys its tunnel in place every hour, with no outage and no re-registration.

**Why this priority**: This is the reason for the feature. It removes a recurring, user-visible outage (missed calls and SMS during the gap) on affected carriers.

**Independent Test**: Set `ipv4` on one line and inspect the rendered tunnel configuration for that line: it requests only an IPv4 inner address and only IPv4 traffic. Live confirmation needs the affected ePDG (see SC-003).

**Acceptance Scenarios**:

1. **Given** a line configured with inner address family `ipv4`, **When** the tunnel configuration for that line is generated, **Then** it requests only an IPv4 inner address and only IPv4 remote traffic.
2. **Given** a line configured with `ipv4` on the affected ePDG, **When** the tunnel reaches its natural rekey time, **Then** the rekey succeeds without `INVALID_SELECTORS`, and no tunnel-missing or tunnel-stuck recovery runs.
3. **Given** a line configured with `ipv4`, **When** the line registers with IMS, **Then** registration, calls and SMS work exactly as on `dual`, because that line's P-CSCFs are IPv4.
4. **Given** a line on `dual` behind the affected ePDG, **When** its rekey is rejected with `INVALID_SELECTORS` (repeatedly, within one tunnel session), **Then** the bridge logs exactly one warning for that session, naming the line and suggesting `ipv4`.

---

### User Story 2 - Existing deployments are unaffected (Priority: P1)

An operator on Vodafone, Jio or Airtel upgrades without touching their configuration. Every line keeps negotiating exactly as before.

**Why this priority**: These are the carriers this project is used on. Their negotiation is proven, and any change to the offer the ePDG sees is a regression risk that can only be caught on a live line. The change has to be inert by default.

**Independent Test**: Generate the tunnel configuration with the setting absent and with it set explicitly to `dual`. In both cases every setting line is byte-for-byte identical to what the current release generates. Comment lines may differ (see FR-003).

**Acceptance Scenarios**:

1. **Given** a configuration that does not mention the setting, **When** the tunnel configuration is generated, **Then** every setting line is byte-for-byte identical to the current release's output.
2. **Given** a line explicitly set to `dual`, **When** the tunnel configuration is generated, **Then** it is identical to the output when the setting is absent.

---

### User Story 3 - One line changed, others untouched (Priority: P2)

An operator runs two VoWiFi lines on different carriers in one container. Only one of them sits behind the affected ePDG. They set `ipv4` on that line alone, and the other line keeps `dual`.

**Why this priority**: Multi-line deployments are supported and all lines share one IKE daemon. A setting that leaked across lines would bring back the main objection to the daemon patch.

**Independent Test**: Configure two lines, one `ipv4` and one left at the default. Each line's generated tunnel configuration reflects only its own value.

**Acceptance Scenarios**:

1. **Given** a top-level default of `dual` and one line overriding it with `ipv4`, **When** both lines' tunnel configurations are generated, **Then** only the overriding line requests IPv4 only.
2. **Given** a top-level default of `ipv4` and one line overriding it with `dual`, **When** both are generated, **Then** the overriding line requests both families and the other requests IPv4 only.

---

### User Story 4 - A typo fails loudly (Priority: P2)

An operator mistypes the value, for example `ip4` or `IPv4 only`. The bridge refuses to start and names the bad value, the line it belongs to, and the accepted values, rather than falling back to a default without saying so.

**Why this priority**: If an invalid value quietly fell back to `dual`, the operator would think the fix was applied while the outage continued. They could lose an hour or more before noticing.

**Independent Test**: Start with an unknown value at the top level, then on a single line. Both fail configuration validation with a message that names the field and the accepted values.

**Acceptance Scenarios**:

1. **Given** an unknown value on a `[[vowifi.line]]`, **When** configuration is loaded, **Then** loading fails with an error naming the field, the offending value and the accepted values.
2. **Given** an unknown value in `[vowifi]`, **When** configuration is loaded, **Then** loading fails the same way.

---

### Edge Cases

- **`ipv6` on a carrier that assigns only IPv4**: the tunnel gets no usable inner address and the line cannot register. The bridge does not override the operator's choice. The example configuration notes that `ipv6` is only for carriers known to assign IPv6, and that `dual` is the safe choice when unsure.
- **`ipv4` on a carrier that later starts assigning IPv6**: the line keeps working on IPv4 and does not pick up IPv6. On such a carrier the rekey fault cannot occur anyway, so the operator can switch back to `dual`. The example configuration says so.
- **Legacy `swu` tunnel engine**: it does not use this tunnel configuration. `tunnel_engine` covers the whole deployment, so with `swu` selected, an `ipv4` or `ipv6` value anywhere (in `[vowifi]` or on any line) stops the bridge from starting. This matches how a PC/SC reader line is refused under `swu`. `dual` or no value is accepted. An operator switching to `swu` has to remove or reset the setting first.
- **Changing the value on a running deployment**: takes effect when the container is recreated. There is no live reload, as with the other per-line tunnel settings.
- **Card-reader-backed lines**: the setting works the same as on modem-backed lines.
- **Value case**: values are lowercase (`dual`, `ipv4`, `ipv6`). Other spellings are unknown values (User Story 4).
- **P-CSCFs of the other family (spec 091)**: the IKE daemon asks the ePDG only for P-CSCF addresses of the families it requested an inner address for. So an `ipv4` line normally receives only IPv4 P-CSCFs, and spec 091's pool for that line holds only IPv4 addresses. If an ePDG sends a P-CSCF of the other family anyway, that address cannot be reached through the line's tunnel. Spec 091 deliberately does not mark a P-CSCF unavailable on a "network unreachable" error. So without a guard, the address would stay eligible and use up backoff attempts that could go to a reachable IPv4 member. FR-013 excludes such addresses.
- **Hourly rebuilds and spec 091's pool**: today, on the affected ePDG, each hourly tunnel rebuild produces a new negotiation, which replaces the line's P-CSCF pool and drops its unavailability marks (spec 091 FR-014). Once `ipv4` stops the rebuilds, the pool and marks persist as spec 091 intends. During the remaining expiry gap on `dual`, spec 091 sets no marks, because the tunnel is down and "network unreachable" does not count as a P-CSCF failure. That is correct, and this feature changes nothing about it.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST accept an inner address family setting, keyed `tunnel_address_family`, with exactly three values: `dual`, `ipv4` and `ipv6`.
- **FR-002**: The setting MUST be available in `[vowifi]` as the default for all lines, and in each `[[vowifi.line]]` as an override for that line. A line without its own value inherits the `[vowifi]` value. With neither set, the value is `dual`.
- **FR-003**: With `dual`, every setting line (non-comment line) of the generated tunnel configuration MUST be byte-for-byte identical to the current release's: it requests an IPv4 and an IPv6 inner address, and IPv4 and IPv6 remote traffic. Comment lines may change. The template's comments are copied into the rendered file and have to describe the new placeholders, and the IKE daemon ignores comments (research R2).
- **FR-004**: With `ipv4`, the generated tunnel configuration MUST request only an IPv4 inner address and only IPv4 remote traffic. Every other part of the line's tunnel configuration MUST stay unchanged.
- **FR-005**: With `ipv6`, the generated tunnel configuration MUST request only an IPv6 inner address and only IPv6 remote traffic. Every other part of the line's tunnel configuration MUST stay unchanged.
- **FR-006**: A line's value MUST affect only that line's tunnel configuration, even though all lines share one IKE daemon.
- **FR-007**: An unknown value at either level MUST be a configuration error. The error MUST name the field, the offending value, the line if the value came from a line, and the accepted values. The bridge MUST NOT start with an unknown value.
- **FR-008**: When `tunnel_engine = "swu"`, an `ipv4` or `ipv6` value in `[vowifi]` or in any `[[vowifi.line]]` MUST be a startup error. The error MUST name where the value came from (`[vowifi]` or the specific line) and say the setting needs the `strongswan` engine. The bridge MUST NOT start with the value ignored, with or without a warning.
- **FR-009**: When a line's tunnel starts, the bridge MUST log the line's effective inner address family, so an operator can confirm from the logs that an override took effect.
- **FR-009a**: When a line on `dual` has a tunnel rekey answered with `INVALID_SELECTORS`, the bridge MUST log one warning per tunnel session (the counter resets when the tunnel is rebuilt). The warning MUST name the line, say that the ePDG rejected the rekey's traffic selectors, and suggest `tunnel_address_family = "ipv4"` for that line. A line already on `ipv4` or `ipv6` gets no such suggestion; the rejection is still visible in the IKE log. The bridge MUST NOT change the setting automatically.
- **FR-010**: `config.toml.example` MUST document the setting under both `[vowifi]` and `[[vowifi.line]]`. The text MUST give:
  - the three values and what each requests;
  - the symptom in words an operator will recognise: the IKE log shows `CREATE_CHILD_SA` responses carrying `N(INVAL_SEL)`, the line loses service for about a minute roughly every hour, and the bridge logs tunnel-missing or tunnel-stuck recoveries;
  - the remedy for that symptom (`ipv4`), when to go back to `dual`, and the risk of `ipv6` on an IPv4-only carrier.
- **FR-011**: `RELEASE_NOTES.md` MUST get an entry under `## Unreleased`, in the project's existing style. It MUST say the change is opt-in, that the default leaves negotiation unchanged, and reference #126 and #107.
- **FR-012**: The feature MUST NOT modify the IKE daemon or ship a patch to it.
- **FR-013**: On a line set to `ipv4` or `ipv6`, a P-CSCF address of the other family MUST NOT be chosen: not as the supervisor's single selection, and not as a failover candidate in the line's spec 091 pool. If an ePDG advertises such an address, it MUST be logged once per negotiation as ignored. On `dual`, selection and pool behaviour MUST stay exactly as spec 091 defines them.

### Key Entities

- **Inner address family** (config key `tunnel_address_family`): a per-line choice of `dual`, `ipv4` or `ipv6`. It decides which inner address families the line asks the ePDG for, and which remote traffic the line's tunnel carries. Its effective value is resolved in this order: the line's own value, then the `[vowifi]` value, then `dual`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With the setting absent or `dual`, every setting line of the generated tunnel configuration for every line matches the current release's byte for byte. Checked by an automated comparison against a snapshot taken before the change, with comment lines removed.
- **SC-002**: Automated tests cover each of the three values, both levels of precedence and an unknown value at each level. All pass.
- **SC-003**: On the #107 reporter's line set to `ipv4`, at least 2 consecutive natural tunnel rekeys complete with no `INVALID_SELECTORS` and no tunnel-missing or tunnel-stuck recovery. The reporter runs this check, since the affected ePDG cannot be reached from the maintainers' rigs.
- **SC-004**: On that same line, the hourly loss of about 60 seconds of service and the hourly IMS re-registration stop: zero rekey-driven re-registrations across the observation window in SC-003.
- **SC-005**: Lines left on the default on Vodafone and Jio register, place and receive calls as before after upgrading. No change appears in their tunnel negotiation.
- **SC-006**: Switching an affected line back to `dual` only means changing one value and recreating the container. No other step is needed.

## Assumptions

- The setting follows the existing per-line override pattern used by `reg_rejected_cooldown_sec`: set in `[vowifi]`, overridable per `[[vowifi.line]]`.
- The setting applies only to the `strongswan` tunnel engine, which is the default. The legacy `swu` engine has its own fixed dual-family handling and is out of scope.
- The IMS layer needs no changes. On an affected line every P-CSCF is IPv4 and the inner address is IPv4, so IPv4-only requests remove nothing the line uses.
- The bridge detects `INVALID_SELECTORS` only to point the operator at the setting (FR-009a). It never switches families itself; the setting stays an explicit operator choice.
- Taking effect on container recreation, with no live reload, is consistent with how the other tunnel settings behave.
- The config key is `tunnel_address_family`, chosen to sit next to `tunnel_engine` and to make clear it acts on the ePDG tunnel, not the outer link to the ePDG. The prose calls the concept the "inner address family".
- No changes are needed to `[volte]`, since VoLTE does not use the ePDG tunnel.
- Builds on spec 091 (P-CSCF pool and failover, merged in PR #130), which publishes every P-CSCF from each negotiation and fails over between them. The family a line requests limits which P-CSCFs that pool can contain (Edge Cases, FR-013).
- The IKE daemon requests P-CSCF attributes only for the families it asks for an inner address in. This is confirmed in upstream strongSwan 5.9.3's `p-cscf` plugin, where the request follows the requested inner-address families. The pinned Osmocom fork is based on 5.9.3 and is expected to behave the same; its server blocked the source fetch. Planning should confirm it from a live or container IKE log (the CPRQ attributes requested on an `ipv4` line).
