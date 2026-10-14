# Quickstart: validating 090

## Q0: Automated (CI)

```bash
make format && make lint && make test
```

This covers, all with an injected clock and no socket except the real UDP pair in the `LoopState` tests:

- **US1 (unanswered SUBSCRIBE):**
  - Timer F at 128 s, then the confirming probe, answered and unanswered.
  - A send failure.
  - A late response at 100 s.
  - A superseded response.
  - A probe sent mid-call, with the re-registration deferred.
  - No stacking while `force_renewal` is already set.
  - The ladder abandoned when a qualifying failure lands.
- **US2 (confirmation, refresh, termination):**
  - Timer N.
  - A NOTIFY that arrives before the 2xx.
  - Refresh at `expires − headroom`.
  - Every `terminated` reason.
  - A renewal discarding the old timers.
- **US3 (NOTIFY matching):** `200` for current and superseded NOTIFYs, `481` for unknown ones, and the 32-entry,
  expiry+128 s memory.
- **US4 (error responses):** `423`, `489`, `503` with `Retry-After`, and other error responses on the
  5 s → 600 s backoff (SC-006: never two SUBSCRIBEs inside one backoff interval).
- **US5 (status):** status rendering, the reply round trip with and without the new fields, and the CLI lines.

## Q1: Live, steady state on real carriers (blocking: SC-003, SC-005, research R6 risk)

Run on **both** of these, using the pinned-tag local setup in `test/` for Vodafone:

- the Jio line on the remote Pi (VoWiFi, `respond_on_client` on);
- the local Vodafone VoWiFi rig.

1. Deploy the build. Let the line register and run for at least 3 hours, so at least 3 renewals happen.
2. Count:
   ```bash
   grep -c '"sent reg-event SUBSCRIBE"'                <log>
   grep -c '"reg-event SUBSCRIBE accepted"'            <log>
   grep -c '"reg-event subscription confirmed"'        <log>
   grep -c 'refused 481'                               <log>
   grep -c 'superseded reg-event subscription'         <log>
   grep -c 'outside an active transaction'             <log>
   grep -c 'qualifying endpoint failure'               <log>
   grep -c 'accepted but never confirmed'              <log>
   ```
   Expect:
   - `sent` = `accepted` = `confirmed`, one per registration, plus any refreshes.
   - `refused 481` = **0**.
   - `outside an active transaction` = 0 for SUBSCRIBE responses.
   - `qualifying endpoint failure` = 0.
   - `never confirmed` = 0.
   - Superseded NOTIFYs, if any, never followed by `"our current binding was deregistered"`.
3. Stop and revisit research R6 if any current-session NOTIFY was refused `481`. The log line carries both
   identifier sets.

## Q2: Live, dead path (best effort: SC-001)

On the local Vodafone rig inside the privileged test container:

1. Wait for `"reg-event subscription confirmed"`.
2. Black-hole the Gm path: `iptables -I INPUT -p esp -j DROP; iptables -I OUTPUT -p esp -j DROP` in the line's
   netns.
3. Cause a SUBSCRIBE. Either wait for a refresh, or restart only the dispatch loop's subscription with a short
   requested expiry in a debug build.
4. Expect `"reg-event SUBSCRIBE unanswered; probing the Gm path"` 128 s after the send, then `"qualifying endpoint
   failure: re-registering"` within a further 10 s, `state: Failed` in `vowifi-status`, and re-registration
   attempts on the existing backoff.
5. Remove the rules and confirm the line re-registers and re-subscribes.

This one is best effort: the black hole also stops the ordinary keepalive, which may reach its own ladder first.
The CI tests are the authority for Timer F's arithmetic.

## Q3: Status output

```bash
gsm-sip-bridge vowifi-status   # and volte-status on a VoLTE deployment
```

On a healthy line, expect these two lines alongside `gm_connection: up`:

```text
reg_subscription: confirmed, expires <ts>
reg_subscription_last_failure: none
```
