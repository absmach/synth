# E-SYNTH-I2C-002 — I²C net has no pullup resistor

**Severity:** warning
**Stage:** erc — protocol

## What this means

I²C is open-drain; SDA and SCL require external pullups to a supply rail. If no resistor sits on the bus, the master cannot release the line.

## Minimal reproduction

```synth
board "x" {
  component U1: sensor "bmp280_pressure"
  component U2: sensor "bme680_env"
  connect U1.sda -> U2.sda  // no pullup on the bus
}
```

## Suggested fix

Add a resistor between SDA (and one between SCL) and the bus supply rail (typically 4.7 kΩ for 3.3 V).
