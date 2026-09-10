# E-SYNTH-RF-001 — RF feed without a keepout

**Severity:** warning
**Stage:** erc — rf

## What this means

A pin carrying `rf_feed` capability is present on the board, but no `keepout` is declared. RF feedlines need exclusion zones around them to avoid coupling and impedance disturbance.

## Minimal reproduction

```synth
board "x" {
  component A1: antenna "ant_chip_2g4"
  component U1: mcu "nrf52840"
  connect U1.ant -> A1.feed
  // no keepout — RF-001 fires
}
```

## Suggested fix

Add a `keepout` declaration around the antenna feed area with an appropriate radius (commonly 3–5 mm).
