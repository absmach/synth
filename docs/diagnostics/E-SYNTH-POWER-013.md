# E-SYNTH-POWER-013 — feedback divider can leave the load's supply window

**Severity:** error
**Stage:** erc — power

## What this means

An adjustable regulator sets its output from a divider on the `feedback`
pin: `Vout = Vref * (1 + Rtop / Rbottom)`. The nominal output can sit inside
the supply window of the loads while resistor tolerance and reference
accuracy push the real output outside it.

The rule computes the lowest and highest output over the corners of those
tolerances. The lowest output uses the minimum reference, a low top resistor
and a high bottom resistor. The highest output uses the opposite. The rule
reports an error only when that range leaves the window of a load on the
output net.

Two related codes come from the same analysis:

- [`E-SYNTH-POWER-014`](E-SYNTH-POWER-014.md) (warning): the worst case stays
  inside the window, but with less margin than the threshold.
- [`E-SYNTH-POWER-015`](E-SYNTH-POWER-015.md) (warning): an input is
  missing, so the margin is unproven. Missing data is never assumed.

## What the rule reads

The analysis needs all of these:

- The regulator part declares `feedback_vref_min_v` and `feedback_vref_max_v`
  under `[operating_conditions]`.
- The feedback net has exactly one resistor to ground and one to the output.
  Both have a readable `value`.
- Each resistor has a tolerance. The rule looks in this order:
  - `tolerance "1%"` on the component,
  - a percent token in the value (`value "10k 1%"`),
  - the tolerance letter of a Yageo `RC`/`RT`/`AC`/`AT` or Panasonic
    `ERJ`/`ERA` chip-resistor MPN (`RC0603FR-0710KL` is 1 %).

  A generic resistor has no default tolerance.
- A load `power_input` pin on the output net has `voltage_min_v` or
  `voltage_max_v`, or its part has `min_voltage_v` or `max_voltage_v`.
  The rule uses the tightest bound on each side.

The rule also uses these when they are present:

| Input | Where | Effect |
| --- | --- | --- |
| `feedback_leakage_max_a` | regulator `[operating_conditions]` | `± IFB × Rtop` on the output |
| `load_regulation_pct`, `line_regulation_pct` | regulator `[operating_conditions]` | widen both corners by their sum |
| `tempco "100ppm"` | each resistor | adds `tempco × max(|Tmax − 25|, |Tmin − 25|)` to its tolerance |
| `temp_range "-40..85"` | the regulator component | the temperature range for `tempco` |

## Minimal reproduction

The reference is 0.796 V to 0.804 V. The load allows 1.188 V to 1.212 V.
The divider is 10k over 20k, nominally 1.2 V.

```synth
component R1: resistor "r_generic_0402" value "10k" tolerance "1%"
component R2: resistor "r_generic_0402" value "20k" tolerance "1%"
```

The output ranges from 1.1861 V to 1.2141 V, outside the window.

## Suggested fix

The message names the loosest tolerance class that brings the worst case
inside the window, with the output it gives. It tries 5 %, 1 %, 0.5 %,
0.25 %, 0.1 % and 0.05 %. Here 0.5 % parts give 1.1900 V to 1.2100 V:

```synth
component R1: resistor "r_generic_0402" value "10k" tolerance "0.5%"
component R2: resistor "r_generic_0402" value "20k" tolerance "0.5%"
```

If 0.05 % parts still leave the window, the message says so. Then pick a
regulator with a tighter reference or widen the load's allowed window.
