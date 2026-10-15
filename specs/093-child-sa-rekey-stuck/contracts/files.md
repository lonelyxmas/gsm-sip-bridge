# Contract: per-line call-state file (093, User Story 3)

## Path

`<selection>.calls` beside the selection file. For line N that is `/tmp/pcscf-N.calls`, next to `/tmp/pcscf-N`, `.pool` and `.state` (specs/091). The path is built by `ims::pcscf_files::calls_path`, which both sides use.

## Format

One unsigned decimal integer followed by a newline: the line's active-call count, `0` or `1` today.

## Writer: the VoWiFi IMS agent

- Writes `0` at the start of every run, before any transport or registration work.
- Rewrites the file every time it reports its active-call count (`AgentObservability::set_active_calls`).
- Writes through `write_atomic` (temp file plus rename), so a reader never sees a torn value.
- Never fails because of the file: write errors are ignored, at most logged at debug.
- The VoLTE agent does not write it.

## Reader: the line's tunnel supervisor

- Reads the file only on a tick that has found a stuck rekey.
- `> 0` means a call is active. A missing, unreadable or unparseable file means no call is active (never defers).
