# E-SYNTH-VALUE-001 — generic passive with no value

**Severity:** error
**Stage:** erc — required support

## What this means

A component whose registry part is generic (`c_generic_*`, `r_generic_*`, `l_generic_*`) declares no `value`. The schematic renders a stock-symbol default (`C`, `R`) and the BOM carries the registry part id instead of an orderable value.

## Minimal reproduction

```synth
board "x" {
  component R1: resistor "r_generic_0603"
  connect R1.p1 -> R1.p2
}
```

## Suggested fix

Add an explicit `value` (`component R1: resistor "r_generic_0603" value "10k"`). The diagnostic carries an inferred starting value as a patch — a cap on a manifest decoupling net takes that manifest value, an I2C pull-up suggests `4.7k`, an LED series resistor suggests `330R` — but the inference is a guess: confirm it against the circuit before accepting.
