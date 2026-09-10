# E-SYNTH-PARSE-007 — expected `.` between component and pin

**Severity:** error
**Stage:** parse

## What this means

An endpoint in a `connect` (or other endpoint-bearing) statement is
written as `<component>.<pin>`. The parser found something other than
`.` after the component identifier.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  component U2: secure_element "atecc608"
  connect U1 spi0 -> U2.spi
}
```

## Suggested fix

Insert `.` between the component and the pin.
