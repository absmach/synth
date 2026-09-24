# E-SYNTH-PINMUX-001 — one pin asked to carry two functions

**Severity:** error
**Stage:** lower

## What this means

Two connections share one pin, and the two net names are *peripheral
functions* from different protocols — `I2C1_SCL` and `UART1_TX`, say.
A package leg can only serve one function at a time, so shorting the two
function nets onto it is a pin-mux mistake, not merely a naming clash.

The pin is named for whichever function came first, and both nets merge
into one — so the design silently loses the second function's intent
while the schematic shows a single net.

The rule is reported at lowering, where both names are still visible.
When the conflicting names are *not* functions (two power rails, two
arbitrary labels) the generic `E-SYNTH-NAME-005` owns the case instead,
so exactly one of the two fires.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  connect U1.gp0 -> "I2C1_SCL"
  connect U1.gp0 -> "UART1_TX"
}
```

## Suggested fix

Route one of the functions to a different pin, or split the nets. A pin
that supports both functions can only carry one of them in a given
design.
