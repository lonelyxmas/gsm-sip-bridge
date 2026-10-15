# Contract: supervisor log lines (093)

All lines use the existing `[supervise] line N:` prefix. `<conn>` is the line's connection name (`ims0`, `ims1`, ...).

## Planned rebuild (FR-005)

```text
[supervise] line 0: ims0 CHILD_SA #12 rekey overdue (expires in 290s); rebuilding before the hard expiry
[supervise] line 0: ims0 CHILD_SA #12 rekey overdue (expires in 290s, last rejection N(INVAL_SEL)); rebuilding before the hard expiry
[supervise] line 0: ims0 CHILD_SA #12 rekey overdue (expires in 75s); rebuilding before the hard expiry despite an active call
```

The orchestrator's existing line follows:

```text
[supervise] line 0: recovered from ChildSaRekeyStuck
```

## Deferral (FR-012), once per CHILD_SA

```text
[supervise] line 0: ims0 CHILD_SA #12 rekey overdue (expires in 290s); deferring the rebuild while a call is active (forced at 90s)
```

## Missing lifetime (FR-011), once per CHILD_SA

```text
[supervise] line 0: ims0 CHILD_SA #12 reports no expiry; stuck-rekey detection is off for it
```

## Unchanged

`recovered from ChildSaMissing` and `recovered from ChildSaStuck` keep their meaning: the CHILD_SA has already gone.
