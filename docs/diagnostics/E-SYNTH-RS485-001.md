# E-SYNTH-RS485-001 — RS-485 termination is always connected

**Severity:** warning
**Stage:** erc — protocol

## What this means

A resistor sits straight across the RS-485 A and B lines, with nothing in
series. Termination belongs only at the two ends of a bus. On a board that
sits in the middle of a multidrop bus, or next to an endpoint that already
terminates, the resistor loads the line and can stop communication.

The lines are recognised by the `rs485_a` and `rs485_b` pin capabilities on
the transceiver, so a transceiver part without those tags is not checked. A
two-pin resistor whose pins land on those two nets is treated as the
terminator, whatever its value. Only the same net is followed: a terminator
behind a series resistor or ferrite is not seen.

## Minimal reproduction

```synth
board "x" {
  component U1: ic "my_transceiver"      // pins tagged rs485_a / rs485_b
  component R1: resistor "r_generic_0603" value "120"
  connect U1.a -> R1.p1
  connect U1.b -> R1.p2                  // always across the bus
}
```

## Suggested fix

Put a jumper or switch in series with the terminator, so it can be left out
when the board is not at an end of the bus.
