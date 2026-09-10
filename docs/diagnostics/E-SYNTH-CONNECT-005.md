# E-SYNTH-CONNECT-005 — net has no driver

**Severity:** warning
**Stage:** erc — connectivity

## What this means

Every endpoint on the net is an input or analog pin. Nothing on the board sets the net's level, so it floats indefinitely.

## Minimal reproduction

```synth
board "x" {
  component U1: ic "sn74hc165_pload"
  component U2: ic "sn74hc165_pload"
  connect U1.e -> U2.f  // two inputs, no driver
}
```

## Suggested fix

Add an output, bidirectional pin, or pull-resistor that establishes the net's level.
