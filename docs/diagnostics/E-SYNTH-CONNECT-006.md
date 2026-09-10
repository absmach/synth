# E-SYNTH-CONNECT-006 — orphan component

**Severity:** warning
**Stage:** erc — connectivity

## What this means

A `component` declaration has no `connect` statement touching any of its pins. It will sit on the board doing nothing — almost always a forgotten net.

## Minimal reproduction

```synth
board "x" {
  component R1: resistor "r_generic_0603"  // never wired
  component R2: resistor "r_generic_0603"
  component R3: resistor "r_generic_0603"
  connect R2.p1 -> R3.p1
}
```

## Suggested fix

Either wire the component into the design or delete its declaration.
