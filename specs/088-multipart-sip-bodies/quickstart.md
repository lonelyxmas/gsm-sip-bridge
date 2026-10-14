# Quickstart: verifying multipart SIP bodies

## Automated

```bash
make format && make lint && make test
cargo test -p gsm-sip-bridge ims::body           # MIME decoder matrix
cargo test -p gsm-sip-bridge ims::agent::inbound # invite_body_decision, 415/400
cargo test -p gsm-sip-bridge ims::sdp            # T2 golden answer, e2e, no conf
cargo test -p gsm-sip-bridge ims::agent::tests   # OPTIONS Accept, SMS media type
```

Fixtures are synthetic: `+919000000000` for numbers, RFC 5737 addresses
(`192.0.2.x`, `198.51.100.x`, `203.0.113.x`), `example.invalid` hosts, no real
coordinates. The T2-shaped fixture keeps the captured layout: PIDF part first
with `Content-Disposition: render;handling=optional` and a `Content-ID`,
per-part `Content-Length` headers, unquoted `boundary=boundary1`, SDP part
without disposition, blank line before the close delimiter.

## Regression (maintainer rigs)

1. Vodafone (local host) and Jio (Pi, `pi@192.168.100.2`): place an inbound
   call from a mobile to each line. Expect the call to ring the PBX, connect,
   and carry audio both ways for 30 s — bare-SDP path, unchanged.
2. On Jio, after deploying, confirm registration stays up across at least two
   carrier `OPTIONS` keepalives (the `OPTIONS` `Accept` value changed) and one
   inbound call does not get torn down with `cause=503`.
3. `vowifi-status` shows both lines registered.

## Live T2 (issue reporter)

Ask the #103 reporter to:

1. Deploy the build, place three calls from a T2 subscriber.
2. Expect: no `415`, a log line
   `outer=multipart/mixed selected=application/sdp skipped=[application/pidf+xml] outcome=sdp`,
   PBX rings, answered call carries audio ≥ 30 s.
3. Check that no carrier `UPDATE` is answered `405` during the call.
4. Repeat one MegaFon call to confirm the plain-SDP path is unchanged.
