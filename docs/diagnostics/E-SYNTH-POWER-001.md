# E-SYNTH-POWER-001 — missing decoupling capacitors

**Severity:** warning
**Stage:** erc — required support

## What this means

The part's manifest declares `required_decoupling` for one of its power pins, but fewer than the required number of capacitors are present on the net carrying that pin.

## Minimal reproduction

```synth
board "x" {
  component U1: regulator "ams1117_3v3"
  // No cap on vin — POWER-001 fires.
  connect U1.gnd -> U1.gnd
}
```

## Suggested fix

Place a capacitor close to the power pin and connect it between the power net and ground.
