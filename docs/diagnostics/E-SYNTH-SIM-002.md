# E-SYNTH-SIM-002 — part has no SPICE model

**Severity:** error
**Stage:** sim

## What this means

A placed part cannot be represented in the generated SPICE deck. Synth names
the reference and part id. The built-in models are resistor and capacitor;
parts without a model must not be simulated silently.

The value is part of the model: an absent, unparseable, zero or negative
resistor/capacitor value is reported here too.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
}
```

## Suggested fix

Use a part kind with a built-in model (resistor or capacitor) and give it a
value, or wait for the registry `[spice]` table for vendor subcircuits.
