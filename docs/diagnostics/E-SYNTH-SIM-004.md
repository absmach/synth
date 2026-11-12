# E-SYNTH-SIM-004 — measurement limit

**Severity:** error
**Stage:** sim

## What this means

A measurement was compared with its optional `min` and `max`. Outside the
limits is a blocking error that makes `synth sim` exit non-zero; inside is an
informational diagnostic with the same code, so a passing check is still
visible in the report. Limits are inclusive. A measure with neither `min` nor
`max` is only reported as `MEASURED`; it has nothing to pass or fail.

ngspice prints measured values with six significant digits, so the printed
value can differ from the true circuit value by up to half a unit in its last
printed digit. Synth compares at that print precision: a value that differs
from a limit by no more than that half-unit is treated as on the limit, and a
difference of one printed step or more is still an out-of-limit error.

## `pin_current` limits

A `pin_current` measure has no `min` or `max` in the sidecar. Its upper limit
is derived from the matching `[[source]]` and the pin's `[pins.drive]` data:

| Source level | Limit field     |
| ------------ | --------------- |
| `high`       | `max_source_ma` |
| `low`        | `max_sink_ma`   |

The measured value is milliamps (`abs(@V<N>[i]) * 1000`). A pass is an
informational diagnostic. A failure is blocking and names all of: the measure,
the measured milliamps, the pin, the level, the datasheet field and limit, and
the `datasheet_ref`:

```
measure `pin_current` = 6.6 mA is above the `U1.gpio5` high pin limit
max_source_ma = 4 mA (datasheet: Table 5-1); ngspice prints 6 significant digits
```

## Minimal reproduction

With a 12 V source and two 10 kΩ resistors, `MID` is 6 V:

```toml
[[measure]]
name = "vmid"
kind = "node_voltage"
node = "MID"
min = 4.0
max = 5.5
```

`min` may not exceed `max`; that is `E-SYNTH-SIM-007`.

## Suggested fix

Change the divider values or the declared limits so the measured value is in
range. Do not widen a limit to hide a wrong circuit.
