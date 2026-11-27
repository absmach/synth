# E-SYNTH-POWER-017 — signal crosses a ground domain with no stitching path

**Severity:** warning
**Stage:** erc — power

## What this means

A signal net connects a part grounded on one declared `ground_domain` to a
part grounded on another, and no `crossing` declares how its return current
gets across the split. Without a path, the return current detours to the join
and the loop it makes radiates.

A part whose pins touch two domain grounds (a mixed-signal converter, the join
itself, a stitching capacitor) straddles the split and does not place a signal
on either side. Ground nets and supply rails are not checked.

## Minimal reproduction

```synth
ground_domain AGND { join R1 }
ground_domain DGND { join R1 }
net "SCLK" { U_ADC.sclk U_MCU.sck }   // U_ADC on AGND, U_MCU on DGND
```

## Suggested fix

Route the signal over the join and say so with
`crossing SCLK stitch join`, or place a stitching capacitor between the two
grounds beside the crossing and name it: `crossing SCLK stitch C20`. The
declaration is a claim about the layout; review it on the routed board.
