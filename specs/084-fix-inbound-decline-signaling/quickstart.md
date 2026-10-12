# Quickstart: verifying decline signaling

## Automated

```bash
make format && make lint && make test
```

The following tests cover the feature:

- `pjsua-safe`: `last_status` stub round-trip.
- `vowifi`: `classify_pbx_disconnect` table, and `wait_for_pbx_answer`
  driven with a stub call.
- `ims::agent::inbound`: the decline response for each reason (status and
  To-tag), and `await_pbx_answer` over a real local UDP socket for
  rejected, no-answer and failed.
- `ims::agent`: `RecentDeclines` bounds and expiry, and the `log_ack`
  classification.
- `ims::observability`: the reason → metric mapping, including
  `pbx_failed`.

## Live (release gate, FR-011)

The two rigs:
- **Vodafone:** the local rig.
- **Jio VoLTE:** the remote Pi. Recreate the container; don't just
  restart it.

Call each line from a test handset, and capture the Gm signalling with the
agent's SIP message log or a `tcpdump` inside the container.

1. **Reject:** reject on the extension. Expect `486 Busy Here` with the
   `180`'s To-tag, a matching ACK, no new INVITE within 60 s, and a call
   record of `declined:pbx_rejected:<486|603>`.
2. **No answer:** let it ring out. Expect `480`, a call record of
   `declined:pbx_no_answer`, and the metric `ring_timeout`.
3. **Setup failure** (Vodafone only): stop the PBX or SIP server before
   calling. Expect `480` (not `486`) and the metric `bridge_setup_failed`.
4. **Redial:** within 5 s of a reject, call again. It must ring.

T2: ask the issue #108 reporter to repeat steps 1 and 2 after release.
