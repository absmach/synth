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

## What the rule reads

The rule needs all of these. If any is missing, it does nothing.

- The regulator part declares `feedback_vref_min_v` and `feedback_vref_max_v`
  under `[operating_conditions]`.
- The feedback net has exactly one resistor to ground and one to the output.
  Both have a readable `value`.
- Both resistors declare `tolerance "<n>%"`. A generic resistor has no
  default tolerance.
- A load `power_input` pin on the output net has `voltage_min_v` or
  `voltage_max_v`, or its part has `min_voltage_v` or `max_voltage_v`.
  The rule uses the tightest bound on each side.

## Minimal reproduction

The reference is 0.796 V to 0.804 V. The load allows 1.188 V to 1.212 V.
The divider is 10k over 20k, nominally 1.2 V.

```synth
component R1: resistor "r_generic_0402" value "10k" tolerance "1%"
component R2: resistor "r_generic_0402" value "20k" tolerance "1%"
```

The output ranges from 1.1861 V to 1.2141 V, outside the window.

## Suggested fix

Use tighter resistors. Here 0.1 % parts give 1.1932 V to 1.2068 V:

```synth
component R1: resistor "r_generic_0402" value "10k" tolerance "0.1%"
component R2: resistor "r_generic_0402" value "20k" tolerance "0.1%"
```

If 0.1 % parts still leave the window, the message says so. Then pick a
regulator with a tighter reference or widen the load's allowed window.

## Not covered

The rule ignores feedback pin leakage, temperature drift, and load and line
regulation. It does not warn on a thin margin.
