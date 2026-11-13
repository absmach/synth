# W-SYNTH-THERMAL-002 — junction temperature was not estimated

**Severity:** warning
**Stage:** erc

## What this means

A part supplies a rail, so its `max_power_mw` is the package rating rather than
the dissipation this board creates. Synth therefore estimates the junction
temperature from the regulator's input/output voltages and the current its
output rail draws. When any of those is not declared, the check cannot run.

This is not a pass. It records that thermal behaviour remains a human review.

## Minimal reproduction

```synth
board "x" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component C1: capacitor "c_generic_0805" value "10uF"
  connect U1.vin -> C1.p1
  connect U1.gnd -> C1.p2 as "GND"
}
```

No `power` rail voltage or load current is declared, so no estimate is
possible.

## Suggested fix

Declare the input and output rails with their voltages and give the load a
declared current:

```synth
power "+5V" 5v { U1.vin }
power "+3V3" 3.3v { U1.vout U2.vdd }
```

Then the estimate in `W-SYNTH-THERMAL-001` can run.
