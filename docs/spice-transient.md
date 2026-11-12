# SPICE transient analysis

Status: accepted, 2026-11-13. Covers #458.

## Current support

`synth sim` runs an operating point (`op`) or an AC decade sweep (`ac`). The
analysis is selected in `[analysis]`:

```toml
[analysis]
op = true
```

Sources are DC (and AC for a sweep). Transient is **not** implemented yet, so a
PWM, a digital edge, a settling time or an LED dimming average cannot be
simulated. The capability descriptor says so; a design that asks for it fails
validation rather than silently running an OP.

## The intended shape

```toml
[analysis.tran]
stop = "5ms"
step = "1us"

[[source]]
net = "PWM"
kind = "pulse"
low = 0.0
high = 3.3
period = "100us"
duty = 0.5
rise = "10ns"
fall = "10ns"
```

with measures `average`, `rms`, `peak_to_peak`, `settling_time`, `rise_time`
and `overshoot` over a window after start-up.

## Safety limits

- Cap the number of points and reject a deck that exceeds it.
- Keep the existing process-tree kill and wall-clock timeout; transient runs
  are the ones most likely to hang.
- Report convergence failure as `E-SYNTH-SIM-003` with ngspice's reason.
- SPICE `M` is milli; keep the unit conversion in one place.

## Out of scope

Firmware co-simulation, Verilog-A, RF and multi-millisecond switching-converter
transients.
