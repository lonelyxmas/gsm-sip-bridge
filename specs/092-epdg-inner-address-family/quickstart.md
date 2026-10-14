# Quickstart: verifying `tunnel_address_family`

## 1. Offline checks (any machine)

```bash
make test        # config, render, engine and detector tests (research R10)
make lint
```

Render one line's connection by hand and compare the families:

```bash
for f in dual ipv4 ipv6; do
  cargo run -q -p gsm-sip-bridge --bin gsm-sip-bridge -- render swanctl-epdg \
    --template-path docker/strongswan/swanctl-epdg.conf.template \
    --conn-name ims0 --imsi 001010000000001 --mcc 001 --mnc 001 \
    --epdg-ip 192.0.2.1 --if-id 23 --updown-script /tmp/u.sh \
    --address-family "$f" | grep -E '^\s*(vips|remote_ts) ='
done
```

Expected:

- `dual` prints `vips = 0.0.0.0, ::` and `remote_ts = 0.0.0.0/0, ::/0`;
- `ipv4` prints only the IPv4 values;
- `ipv6` prints only the IPv6 values.

## 2. `dual` is unchanged on a live line (local Vodafone rig)

Use the `test/` deployment, with its pinned image tag, own config, `.env` and volume. After changing the config, **recreate** the container (`docker compose up -d --force-recreate`); don't restart it. A restart keeps stale `/tmp` state.

1. Leave `tunnel_address_family` unset. Recreate.
2. Confirm the supervisor logs `tunnel address family: dual` and the line registers.
3. Diff the line's `epdg-0.conf` against the previous image's, with comment lines removed: `grep -v '^\s*#'`. There should be no difference.

## 3. `ipv4` on a live line, and what charon actually requests (research R3)

1. Set `[[vowifi.line]].tunnel_address_family = "ipv4"` on the Vodafone line. Recreate.
2. In the shared charon log, check the IKE_AUTH request's `CPRQ(...)` list. Expect `ADDR` and `P-CSCF4`, and no `ADDR6` or `P-CSCF6`. This confirms that the pinned fork's `p-cscf` plugin follows the requested inner-address families.
3. Confirm every `received P-CSCF server IP` line is IPv4, the line registers, and an inbound and an outbound call both connect.
4. If the ePDG still sends an IPv6 P-CSCF, confirm `ignoring P-CSCF ... (tunnel_address_family = ipv4)` appears and `vowifi-status` shows only IPv4 members in `pcscf: ... (pool: ...)`.
5. Wait past one natural CHILD_SA rekey (`rekey_time` 3600 s, plus up to 360 s of random jitter). Confirm it rekeys in place with no `ChildSaMissing`.
6. Set the line back to `dual` and recreate (SC-006).

## 4. The affected ePDG (SC-003 and SC-004): the #107 reporter

We can't reach this ePDG (mnc020.mcc250) ourselves. Ask the reporter to:

1. Run the build with the line on `dual` first and confirm the new warning appears at the first rejected rekey:
   `ePDG rejected the CHILD_SA rekey's traffic selectors (INVALID_SELECTORS); ...`
2. Set `tunnel_address_family = "ipv4"` and recreate.
3. Observe at least 2 consecutive natural CHILD_SA rekeys. Each should show `CREATE_CHILD_SA response` with `SA`, `TSi` and `TSr`, and no `N(INVAL_SEL)`. There should be no `ChildSaMissing` or `ChildSaStuck` recovery, and no hourly IMS re-registration.
4. Send back the charon log and supervisor log for the window. Strip subscriber identities first.
