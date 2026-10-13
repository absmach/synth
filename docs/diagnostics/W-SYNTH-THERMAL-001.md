# W-SYNTH-THERMAL-001 — component junction temperature is high

**Severity:** warning
**Stage:** erc

## What this means

A part that declares both its dissipation (`max_power_mw`) and its
junction-to-ambient resistance (`thermal_resistance_c_per_w`) in the registry
estimates a junction temperature `Tj = 25 °C + P · Rθja`. Above 85 °C a warning
fires.

This is the same estimate the `synth_evaluate_thermal_si` oracle computes, moved
into the compiler so it runs in `synth validate` / `synth check`. It is an
estimate from the two declared figures, not a simulation; it ignores copper
planes, thermal vias and neighbouring heat sources.

## Minimal reproduction

A part whose registry entry declares `max_power_mw = 1500` and
`thermal_resistance_c_per_w = 60` (1500 mW × 60 °C/W ≈ 90 °C rise).

## Suggested fix

Add copper or thermal vias, lower the dissipation (a more efficient part, a
series element), or reduce the ambient. If the figure is wrong, correct
`max_power_mw` / `thermal_resistance_c_per_w` in the registry entry.
