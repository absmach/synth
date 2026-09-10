# E-SYNTH-RF-003 — RF net missing trace impedance constraint

**Severity:** warning
**Stage:** erc — rf

## What this means

An RF feed net is present in the design, but no controlled impedance specification (50 Ω target) or RF constraint has been declared for its net or differential pair. RF trace routing requires controlled impedance to prevent signal reflections and mismatch loss.

## Minimal reproduction

```synth
board "rf_imp_test" {
  layers 2
  component U1: rf "ant_chip_2g4"
  component U2: mcu "nrf52840"

  connect U1.rf_in -> U2.antenna
}
```

## Suggested fix

Add an impedance constraint or differential pair declaration specifying 50ohm target impedance for the RF signal net.
