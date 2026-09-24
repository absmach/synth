# E-SYNTH-PINMUX-002 — a function routed to a pin that cannot carry it

**Severity:** error
**Stage:** erc

## What this means

A net whose *name* names a peripheral function (`I2C1_SCL`, `UART0_TX`,
`SPI1_MOSI`, `USB_DP`) reaches a pin that does not declare that function.
The pin's registry `capabilities` are the mux table, so a pin listing
`gpio`, `spi_mosi`, `uart_tx` but not `i2c_scl` cannot be the I²C clock.

This complements the capability-consistency rules (`E-SYNTH-I2C-001`,
`-SPI-001`, `-UART-001`, `-USB-001`), which fire only when a net carries
a *dedicated* peripheral pin. When every endpoint is a muxable GPIO no
dedicated peer exists, so the protocol rules stay silent and the net's
own name is the only signal of intent — that is this rule's case.

A pin that declares no capabilities at all (a resistor, a capacitor) is
never judged: it legitimately sits on the net.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  connect U1.gp0 -> "I2C1_SCL"   // gp0 has no `i2c_scl`
  connect U1.gp1 -> "I2C1_SCL"
}
```

## Suggested fix

Move the net to a pin that lists the function (`gp1` lists `i2c_scl`),
or rename the net if it is not really that function.
