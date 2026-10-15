# Contract: per-line call-state file (093, User Story 3)

## Path

`<selection>.calls` beside the selection file. For line N that is `/tmp/pcscf-N.calls`, next to `/tmp/pcscf-N`, `.pool` and `.state` (specs/091). The path is built by `ims::pcscf_files::calls_path`, which both sides use.

## Format

One unsigned decimal integer followed by a newline: how many calls occupy the line, `0` or `1` today. A call counts from the moment it starts being set up: an inbound call while it rings, an outbound call while it is being placed. It counts until the call ends or the attempt fails, not only once it connects.

## Writer: the VoWiFi IMS agent

- Writes `0` at the start of every run, before any transport or registration work.
- Rewrites the file whenever that number changes: on `AgentObservability::set_active_calls`, around an inbound INVITE's ringing (`set_inbound_setup`), and once per dispatch-loop pass for an outbound attempt (`set_outbound_setup`, which writes only on a change).
- Writes through `write_atomic` (temp file plus rename), so a reader never sees a torn value.
- Never fails because of the file: write errors are ignored, at most logged at debug.
- The VoLTE agent does not write it.

## Reader: the line's tunnel supervisor

- Reads the file only on a tick that has found a stuck rekey.
- `> 0` means a call is active. A missing, unreadable or unparseable file means no call is active (never defers).
