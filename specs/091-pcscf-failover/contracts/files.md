# Contract: per-line P-CSCF files

The existing selection file is listed for completeness. **Its format and writers are unchanged** (FR-004).

| Path (VoWiFi line N) | Writer | Readers | Format |
|---|---|---|---|
| `/tmp/pcscf-N` | supervisor (unchanged) | agent (`EpdgTransport`), `healthcheck`, keepalive loop | one address, trailing newline optional |
| `/tmp/pcscf-N.pool` | supervisor (new) | agent | one address per line, in negotiation order, deduplicated |
| `/tmp/pcscf-N.state` | agent (new) | agent | JSON, below |
| `/tmp/volte-pcscf-<card_id>.state` | VoLTE agent (new) | VoLTE agent | JSON, below |

The base paths come from `[vowifi].pcscf_source_path`, indexed per line by `vowifi::discovery`. `.pool` and `.state` are
that path with a suffix, so a deployment that moves the base moves all three together.

## `.pool`

```text
10.0.0.10
10.0.1.10
2402:8100::10
```

- **Writes**: atomic (write `<path>.tmp`, then rename). Always written **before** `/tmp/pcscf-N` at the three points
  where the selection is published: establishment, `PcscfChanged`, and the post-recovery sync.
- **Set-change rewrites**: also rewritten, on its own, when a steady-state tick sees the advertised *set* change.
  This does not restart the agent. A reorder of the same set writes nothing.
- **Reading**: blank lines and lines that do not parse as an IP address are skipped. A missing file, or one with no
  valid lines, means "no pool published": the agent uses `[selection]` (FR-006).
- **Freshness**: read at agent startup and again before every registration attempt. The agent never caches it across
  attempts.

## `.state`

```json
{
  "version": 1,
  "last_registered": "10.0.1.10",
  "marks": [
    { "addr": "10.0.0.10", "until_unix": 1792000000, "failures": 2, "cause": "register-timeout" }
  ]
}
```

| Field | Type | Notes |
|---|---|---|
| `version` | `u32` | `1`. Any other value is ignored as a whole (empty state, one `WARN`). |
| `last_registered` | `string \| null` | dropped on load if not in the current pool |
| `marks[].addr` | `string` | IP address |
| `marks[].until_unix` | `u64` | wall-clock expiry; at or before now means expired (its count is kept) |
| `marks[].failures` | `u32` | consecutive qualifying failures; reset by a successful registration through `addr` |
| `marks[].cause` | `string` | `FailureCause` display string; informational |

- **Writes**: atomic (temp file, then rename), after every mark and every successful registration, and after a
  `replace_pool` that changed anything.
- **Write failures**: logged at `WARN` and otherwise ignored. Failover still works in-process; only restart survival
  is lost.
- **Reading**: a corrupt, unreadable or missing file is empty state. Entries for addresses not in the current pool are
  dropped (FR-014).
- **Lifetime**: the files live in `/tmp`. They survive agent restarts and container restarts, but not container
  recreation or a host reboot. Staleness is bounded by `until_unix` and by pool reconciliation.
