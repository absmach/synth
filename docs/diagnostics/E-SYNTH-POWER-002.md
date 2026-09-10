# E-SYNTH-POWER-002 — power outputs shorted

**Severity:** error
**Stage:** erc — power

## What this means

Two pins with electrical type `power_output` (regulators, supplies) were placed on the same net. Connecting two sources of different voltages is a short circuit; connecting two of the same voltage creates uncontrollable current sharing.

## Minimal reproduction

```synth
board "x" {
  component U1: regulator "ams1117_3v3"
  component U2: regulator "ams1117_5v"
  connect U1.vout -> U2.vout
}
```

## Suggested fix

Insert an ORing diode, load switch, or remove one of the regulators from the rail.
