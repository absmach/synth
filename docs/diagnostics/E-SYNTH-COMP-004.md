# E-SYNTH-COMP-004 — undefined component refdes

**Severity:** error
**Stage:** resolve

## What this means

A `connect` endpoint references a refdes that was never declared in
the board. Either the refdes is misspelled or the `component`
declaration is missing.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  connect U1.gp0 -> U7.sda
}
```

## Suggested fix

Add the missing `component` declaration, or correct the spelling.
