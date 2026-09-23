# E-SYNTH-POWER-009 — regulator input outside its operating range

**Severity:** error
**Stage:** erc — power

## What this means

A regulator's input rail sits outside the input-voltage range its
registry entry declares (`operating_conditions.min_voltage_v` /
`max_voltage_v`). An LDO rated 3.5–15 V fed from a 3.3 V rail will not
regulate; one rated 6 V max fed from 12 V will fail.

## Minimal reproduction

```synth
board "x" {
  component J1: connector "header_1x4"
  component U1: regulator "ams1117_3v3"   // needs 3.5–15V
  power "VIN33" 3.3v
  connect J1.p1 -> U1.vin as "VIN33"      // 3.3V is below the 3.5V minimum
}
```

## Suggested fix

Feed the regulator from a rail inside its range, or choose a part whose
range covers the source. The range comes from the registry, so correcting
it there fixes every design.
