# E-SYNTH-POWER-010 — rail load exceeds the regulator's current limit

**Severity:** error
**Stage:** erc — power

## What this means

The declared maximum current draw of every load on a regulator's output
rail, summed, exceeds the regulator's own maximum
(`operating_conditions.max_current_ma`). The rail will sag or the
regulator will enter thermal shutdown under worst-case load.

## Minimal reproduction

```synth
board "x" {
  component U1: regulator "ams1117_3v3"   // 800mA
  component U2: mcu "atmega328p"          // 200mA each
  component U3: mcu "atmega328p"
  component U4: mcu "atmega328p"
  component U5: mcu "atmega328p"
  component U6: mcu "atmega328p"          // 5 × 200mA = 1000mA > 800mA
  connect U1.vout -> U2.vcc
  connect U1.vout -> U3.vcc
  connect U1.vout -> U4.vcc
  connect U1.vout -> U5.vcc
  connect U1.vout -> U6.vcc
}
```

## Suggested fix

Split the rail across two regulators, choose a higher-current regulator,
or reduce the load. Require engineering headroom with
`power_budget_headroom_pct` (e.g. `20.0`) in `<design>.synth.erc.toml`.
