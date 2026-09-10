# E-SYNTH-RF-002 — multiple RF feeds on one net

**Severity:** error
**Stage:** erc — rf

## What this means

Two or more pins with the `rf_feed` capability sit on the same net. RF nodes are point-to-point — bridging them violates the antenna/feedline impedance contract.

## Minimal reproduction

```synth
board "x" {
  component A1: antenna "ant_chip_2g4"
  component A2: antenna "ant_helical_433mhz"
  component U1: mcu "nrf52840"
  connect U1.ant -> A1.feed
  connect U1.ant -> A2.feed
}
```

## Suggested fix

Use exactly one antenna per RF feed, or pass through an RF switch or balun.
