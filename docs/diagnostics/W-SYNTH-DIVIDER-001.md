# W-SYNTH-DIVIDER-001 — degenerate resistor-divider ratio

**Severity:** warning
**Stage:** erc — analog (value-based)

## What this means

A rail→R1→mid→R2→gnd divider was recognised (the same topology the schematic layouter clusters as `Divider`), but its mid-point sits below 5% or above 95% of the rail. The compiler cannot know the intended output voltage, so this is a warning, not an error — yet such extreme ratios are almost never intended. The usual causes are R1/R2 swapped in placement or an order-of-magnitude value typo (`10k` vs `100k`).

## Minimal reproduction

```synth
board "x" {
  component U1: regulator "ams1117_3v3"
  component R1: resistor "r_generic_0603" value "1M"
  component R2: resistor "r_generic_0603" value "1k"
  connect U1.vout -> R1.p1
  connect R1.p2 -> R2.p1
  connect R2.p2 -> U1.gnd
  // mid at ~0.1% of rail — W-SYNTH-DIVIDER-001 fires
}
```

## Suggested fix

Check the R1/R2 order against the schematic and the value magnitudes against the design intent. Either resistor with a missing or unparseable `value` skips the check — give both resistors explicit SI values the parser accepts (`10k`, `4.7k`; the European `4k7` form is not supported).

## See also

`E-SYNTH-CRYSTAL-001`, `E-SYNTH-POWER-006` (the other value-based rules); the `Divider` cluster in `docs/schematic-procedures.md`.
