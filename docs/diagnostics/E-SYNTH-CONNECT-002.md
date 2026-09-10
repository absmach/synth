# E-SYNTH-CONNECT-002 — single-endpoint net

**Severity:** warning
**Stage:** erc — connectivity

## What this means

A `connect` statement attached only one pin to a net (often the same endpoint twice). Wires must reach two distinct pins to do anything electrical.

## Minimal reproduction

```synth
board "x" {
  component R1: resistor "r_generic_0603"
  connect R1.p1 -> R1.p1
}
```

## Suggested fix

Either delete the offending `connect` line or extend it to a real second endpoint.
