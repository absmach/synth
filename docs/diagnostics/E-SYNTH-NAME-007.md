# E-SYNTH-NAME-007 — net names differ only by case

**Severity:** error
**Stage:** erc — naming

## What this means

Two distinct nets are named such that they differ only in letter case
(`SDA` and `Sda`). Netlist tooling, fab netlists, and KiCad's own
resolution fold case inconsistently, so the pair can silently merge (one
net disappearing) or collide in the fab data.

## Minimal reproduction

```synth
board "x" {
  component R1: resistor "r_generic_0603"
  component R2: resistor "r_generic_0603"
  net "SDA" { R1.p1 }
  connect R2.p1 -> R2.p2 as "Sda"
}
```

## Suggested fix

Rename one net so the distinction survives case folding — e.g. `SDA_MAIN`
and `SDA_AUX`.
