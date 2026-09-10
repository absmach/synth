# E-SYNTH-I2C-001 — I²C endpoint connected to non-I²C pin

**Severity:** error
**Stage:** erc — protocol

## What this means

A pin carrying `i2c_sda` or `i2c_scl` capability is connected to a
pin on the other side that advertises neither. I²C is a shared bus
that requires both sides to be open-drain (or open-drain-compatible)
and to participate as I²C peers; connecting an I²C line to a
push-pull GPIO or analog pin will silently break the bus.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  component U2: mcu "rp2350"
  // GP2/GP3 don't advertise i2c capability in the registry.
  connect U1.gp0 -> U2.gp2
}
```

## Suggested fix

Route the I²C bus to a pin that carries `i2c_sda` / `i2c_scl`. The
rule does not emit automatic patches because the correct destination
depends on intent.
