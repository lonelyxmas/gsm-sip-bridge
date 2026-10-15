# Quickstart: verifying 093

## Unit and harness tests

```bash
make test
cargo test -p gsm-sip-bridge supervise::line_supervisor::tests::rekey_stuck
cargo test -p gsm-sip-bridge supervise::engines::tests::child_sa_lifetime
```

## Live: a natural rekey triggers nothing (SC-002)

On the Vodafone line on this host, find when the CHILD_SA will rekey:

```bash
docker exec gsm-sip-bridge-test swanctl --list-sas --ike ims0 | grep -A1 'ims0: #'
#   ims0: #12, reqid 1, INSTALLED, ...
#     installed 1109s ago, rekeying in 2167s, expires in 2852s
```

After `rekeying in` has passed, the CHILD_SA number has gone up, and:

```bash
docker logs gsm-sip-bridge-test 2>&1 | grep -E 'rekey overdue|ChildSaRekeyStuck'   # expect nothing
```

## Live: a simulated stuck rekey (optional)

No carrier on the test hardware rejects rekeys. To watch the planned rebuild end to end on a disposable line, hold the rekey off so the CHILD_SA ages past 300s left. One way is to block the line's IKE port to the ePDG just before `rekeying in` reaches 0, so the rekey exchange cannot complete. Remove the block once `rekey overdue` is logged. Expect, in order:

1. `ims0 CHILD_SA #N rekey overdue (expires in ...s); rebuilding before the hard expiry`
2. `recovered from ChildSaRekeyStuck`
3. `P-CSCF synced from new negotiation: ...`
4. Agent restart and `registered`.

If the block outlasts the IKE_SA's dead-peer timeout, the IKE_SA is lost instead (`IkeSaLost`, cause `dead-peer timeout`). That is a separate, correct path.
