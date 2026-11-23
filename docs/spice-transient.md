# SPICE transient analysis

Status: accepted, 2026-11-13. Covers #458.

## What is implemented

`synth sim` runs an operating point (`op`), an AC decade sweep (`ac`) or a
pulse-driven transient (`tran`). The analysis is selected in `[analysis]`:

```toml
[analysis.tran]
stop = "5ms"
step = "1us"
```

`stop` and `step` are durations: a number of seconds, or a number with an
`ns`, `us`, `ms`, `ps` or `s` suffix. A bare `m` is rejected — SPICE `M` is
milli, and a unit trap in a wall-clock budget is expensive.

The deck emits `tran <step> <stop>` and reads every measure back from a
`wrdata` file ngspice writes into the run directory, so the arithmetic
(average, ripple, settling, edge time) happens in Synth, not in ngspice's
`.meas`.

### Safety limits

- `stop/step` is the number of output points ngspice writes. It is capped at
  **100 000**: a sidecar typo (`step = "1ns"` against a millisecond stop)
  would otherwise mean hours of wall clock and gigabytes of output. A
  testbench over the cap is `E-SYNTH-SIM-007` before any process starts;
  raise the step or shorten the run.
- The wall-clock budget and process-tree kill are unchanged
  (`E-SYNTH-SIM-005`). A transient is the run most likely to hang, so keep
  the default 10 s budget in mind when picking a `stop`.
- Durations are parsed in one place, `parse_duration` in `synth-sim`, and
  emitted with explicit exponents (`PULSE(0 3.3 0 1e-7 1e-7 5e-5 1e-4)`), so
  SPICE's milli-`M` trap cannot reach the deck.
- A transient convergence failure aborts ngspice's control block, so
  `SYNTH_DONE` is never printed. That surfaces as `E-SYNTH-SIM-003` with
  ngspice's reason (`doAnalyses: Timestep too small …`) in the message. A
  singular matrix or a failed gmin/source step still reports
  `E-SYNTH-SIM-006`, exactly as for `op` and `ac`.

## Pulse sources

A source with `kind = "pulse"` drives a repeating waveform:

```toml
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

The emitted line is `PULSE(Vlow Vhigh 0 Trise Tfall Pwidth Pperiod)` with a
zero delay and `Pwidth = duty * period`; every number is an exponent, so
`period = "100us"` becomes `1e-4` and can never read as 100 seconds.

A source drives exactly one target, as before:

- **A net** carries its own `low` and `high` (plus the timing); `dc`/`ac` on
  a pulse source is `E-SYNTH-SIM-007`.
- **An IC pin** takes both levels from its `[pins.drive]` block (`low_v`,
  `high_v`) and the timing from the sidecar. `level`, `low` and `high` are
  rejected on a pulse pin source. The pin's `output_resistance_ohm` stays in
  series, exactly as for a DC pin source.

All timing fields (`period`, `duty`, `rise`, `fall`) are required; `duty`
must be greater than 0 and at most 1, `rise + fall` must fit inside the
period, and a net pulse needs `low < high`. A pulse source needs the `tran`
analysis; `pin_current` still requires `op`, so the two cannot share a file.

## Transient measures

Six kinds read a waveform over a window after start-up:

| Kind            | Value                                                    | Unit |
| --------------- | -------------------------------------------------------- | ---- |
| `average`       | Mean over the window                                     | `V`/`A` |
| `rms`           | Root mean square over the window                         | `V`/`A` |
| `peak_to_peak`  | Maximum minus minimum over the window                    | `V`/`A` |
| `settling_time` | Time from the window start until the waveform enters and stays inside the tolerance band around its final value | `s` |
| `rise_time`     | Time between the first upward crossings of `low_threshold` and `high_threshold` (interpolated between samples) | `s` |
| `overshoot`     | `(peak - final)/final * 100`, where final is the window's last sample | `%` |

Every kind targets one waveform: a `node` voltage, or — for `average`,
`rms` and `peak_to_peak` — an `element` current (`@R1[i]`, saved like the
`current` measure saves it). `settling_time` needs `tolerance` (an absolute
band around the final value); `rise_time` needs `low_threshold` below
`high_threshold`; the other kinds reject both. The window defaults to the
whole run and can be narrowed with `window_start`/`window_end` (durations,
inside `0 .. stop`, with `window_end > window_start`). `min`/`max` limits
and the `-004` reporting work exactly as for the other measures.

A waveform that never enters its settling band, never crosses a rise
threshold, or ends at 0 V (making a percentage overshoot meaningless) is
`E-SYNTH-SIM-003` naming the measure — never a defaulted value.

Sampling: ngspice writes one row per output point at the sidecar's `step`,
plus extra rows at source breakpoints. Edge times are interpolated between
the two samples that straddle a threshold, so their accuracy is bounded by
the step; a check that cares about an edge should draft `step` at or below
a fifth of the edge. A computed value is rounded to ngspice's six printed
digits before it is compared with its limits, so the
[print-precision rule](spice-sim.md#print-precision) is one rule for every
measure.

## Suggested testbenches

`--suggest` recognises three transient shapes, each driven by a pin whose
registry entry carries `[pins.drive]` and a `[pins.pwm]` block
(`pwm_max_hz` plus its `datasheet_ref`). The period is `1/pwm_max_hz` — the
fastest PWM the design may generate — the duty is a test choice of 0.5, and
every number in the comment cites its source:

- **LED PWM dimming.** Average current through the series resistor over
  four whole cycles after one start-up cycle; the band is the DC diode
  equation scaled by the duty, ±5%.
- **RC filter on a PWM pin.** Peak-to-peak ripple and average over four
  whole cycles after five RC time constants. The ripple band comes from the
  steady-state charge/discharge arithmetic (`k = exp(-period/tau)`), ±25%,
  and the average is `duty * V`, ±5%.
- **MOSFET gate drive.** `rise_time` and `overshoot` on the gate node,
  thresholds at 10%/90% of the drive swing. The step resolves the gate time
  constant — driver `output_resistance_ohm` (plus any series gate resistor)
  times the transistor's `[pins.gate]` `input_capacitance_pf` — and the
  window covers one edge plus twenty of those time constants. The
  transistor must carry a registry `[spice]` model, and the rail behind its
  load is sourced from the design's declared `power` voltage. The measures
  carry no limits: they report the edge, and the review sets the policy.

One transient timing per file: two tran shapes that need different step or
window arithmetic are not covered, like an `op` and an `ac` measure.

## Out of scope

Firmware co-simulation, Verilog-A, RF, `pwl` and `sin` sources, DC sweeps,
switching converters beyond a few milliseconds, and mixing SPICE dialects
per part. A `pwm_max_hz` in the registry bounds what the design may
generate but says nothing about the firmware's actual duty cycle; the
drafted 0.5 is a test excitation, not a circuit value.
