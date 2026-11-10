# E-SYNTH-SIM-002 — part has no SPICE model

**Severity:** error
**Stage:** sim

## What this means

A placed part cannot be represented in the generated SPICE deck. Synth names
the reference and part id, so the uncovered part is visible in the report.

Built-in models exist for resistors, capacitors, inductors, and diodes/LEDs
that declare a forward voltage and current. Zener, TVS, bridge-rectifier and
other diode kinds need those forward fields or a registry `[spice]` model;
Synth does not guess a diode behaviour for them.

The value is part of the model: an absent, unparseable, zero or negative
resistor/capacitor/inductor value is reported here too. `4k7` is not accepted;
write `4.7k`. Inductance values are in henries (`10uH`, `4.7nH`).

A part the testbench lists under `ignore` is left out of the deck and this
report. That is for a known gap, not for hiding an accident: the part also
stops conducting in the topology check, so removing a part that was the only
DC path reports `E-SYNTH-SIM-008`.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
}
```

## Suggested fix

Use a part kind with a built-in model and give it a value, add
`forward_voltage_v` and `forward_current_ma` to the part's
`[operating_conditions]`, add a registry `[spice]` model, or mark the part
`ignore` in the testbench when the gap is deliberate.
