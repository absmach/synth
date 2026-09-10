# E-SYNTH-POWER-004 — Power net residual energy anomaly

**Severity:** warning
**Stage:** erc — anomaly

## What this means

Statistical anomaly detection flagged a power distribution net with an unusually high load-to-decoupling ratio (Residual Energy Score > 3.0σ above net pattern baseline). This indicates potential power rail instability or missing bulk/decoupling capacitance.

## Minimal reproduction

```synth
board "power_anomaly_test" {
  layers 2
  component U1: mcu "rp2350"
  component U2: modem "bg95"
  component U3: secure_element "atecc608"
  component J1: connector "header_1x4"

  connect J1.p1 -> U1.vdd_io
  connect J1.p1 -> U1.vdd_core
  connect J1.p1 -> U2.vbatt
  connect J1.p1 -> U3.vcc
}
```

## Suggested fix

Add decoupling or bulk storage capacitors to the power rail to stabilize transient voltage drop under heavy load switching.
