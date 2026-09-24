# E-SYNTH-CONNECT-009 — open-drain net has no pull-up

**Severity:** warning (configurable via `open_drain_no_pullup`)
**Stage:** erc — connectivity

## What this means

A net carries an `open_drain_low` / `open_drain_high` pin but no
pull-up resistor, so it can never reach a valid high level. The I²C
case is owned by `E-SYNTH-I2C-002` (a pull-up there is a protocol
requirement); this rule covers every other open-drain bus — shared
interrupts, reset lines, wired-AND handshakes.

## Minimal reproduction

```synth
board "x" {
  component U1: ic "lm555_timer"
  component C1: capacitor "c_generic_0603"
  connect U1.disch -> C1.p1        // open-drain, no pull-up on the net
  connect C1.p2 -> U1.gnd
}
```

## Suggested fix

Add a resistor from the net to its logic rail, sized for the bus
capacitance and required rise time.
