# E-SYNTH-CAP-001 — Class-II ceramic used at a high DC bias

**Severity:** warning
**Stage:** erc

## What this means

A Class-II ceramic capacitor (X5R, X7R, X7S, Y5V, Z5U, …) sits on a rail
at more than the configured fraction (default 50 %) of its rated
voltage. Class-II dielectrics lose effective capacitance under DC bias —
an X7R "10 µF" at 80 % of its rating may deliver a small fraction of
nominal — so a design that trusts the nominal value can end up
under-decoupled.

The rule needs the structured values Phase 7 adds: the `dielectric` and
`voltage` fields on the capacitor, plus a known rail voltage. If any of
those is missing it declines rather than guessing, and Class-I
(C0G/NP0) parts are never flagged.

## Minimal reproduction

```synth
board "x" {
  component U1: regulator "ams1117_3v3"
  component C1: capacitor "c_generic_0603" dielectric "X7R" voltage "6.3V"
  power "+5V" 5v
  connect U1.vout -> C1.p1 as "+5V"
  connect C1.p2 -> "GND"
  connect U1.gnd -> "GND"
  connect U1.vin -> "VIN"
}
```

## Suggested fix

Derate the effective capacitance in the design, pick a higher-voltage
part, or use a Class-I (C0G/NP0) capacitor. The threshold is
configurable: `ceramic_dc_bias_threshold` in `<design>.synth.erc.toml`.
