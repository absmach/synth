# E-SYNTH-LED-001 — LED current or dissipation out of range

**Severity:** error (over-current), warning (resistor dissipation)
**Stage:** erc — power

## What this means

An LED and its series resistor are checked with real arithmetic:
`I = (V_rail − Vf) / R`. The forward voltage comes from the part's
description/id (the registry states it textually, e.g. `~2.0V Vf`), the
rail voltage from the power domain map, and the resistance from the
component `value`. Two failures are reported:

- the current exceeds the part's maximum (`led_max_current_ma`, 20 mA by
  default), or
- the resistor dissipates more than 0.1 W (a 0603's practical limit).

A LED with **no** series resistor at all is reported too.

## Minimal reproduction

```synth
board "x" {
  component U1: regulator "ams1117_5v"
  component R1: resistor "r_generic_0603" value "33"
  component D1: led "led_red_0603"        // Vf ≈ 2.0V
  connect U1.vout -> R1.p2
  connect R1.p1 -> D1.anode
  connect D1.cathode -> U1.gnd
  // (5V − 2V) / 33Ω ≈ 91mA — far above the 20mA maximum
}
```

## Suggested fix

Raise the series resistance (`(5 − 2) / 0.02 ≈ 150Ω` for 20 mA), or use
a higher-power resistor. Tune the limit with `led_max_current_ma`.
