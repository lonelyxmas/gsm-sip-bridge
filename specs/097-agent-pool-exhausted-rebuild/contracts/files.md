# Contract: the pool state file, with `marked_at_unix`

Path: `/tmp/pcscf-N.state`, i.e. `pcscf_files::state_path(selection_path)`. Written atomically (temp file plus rename) by the agent only. Read by the agent at start-up, and by the supervisor every steady-state tick (new).

```json
{
  "version": 1,
  "last_registered": "10.0.0.10",
  "marks": [
    {"addr": "10.0.0.11", "until_unix": 1790000330, "failures": 1,
     "cause": "gm-keepalive", "marked_at_unix": 1790000000}
  ]
}
```

## Compatibility

| Writer | Reader | Behaviour |
|---|---|---|
| new | new | Full behaviour. |
| old (no `marked_at_unix`) | new agent | The marks load and steer failover as before. `marked_at` reads as `UNIX_EPOCH`. |
| old | new supervisor | `marked_at_unix = 0` < baseline, so the line is never exhausted and there is no rebuild (FR-008). |
| new | old agent | The unknown field is ignored, so the marks load and steer failover as before. |

## Rules

- `on_failure` sets `marked_at_unix` to now each time it inserts or renews a mark.
- `on_registered` removes the mark, as before.
- `replace_pool` drops marks for departed addresses and keeps the rest, as before, `marked_at` included.
- `version` stays 1.
