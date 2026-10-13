# W-SYNTH-SI-001 — stackup impedance is outside the standard envelope

**Severity:** warning
**Stage:** erc

## What this means

With a declared `stackup`, the outer-layer microstrip characteristic impedance at
a 0.2 mm reference trace is outside 30–100 Ω. A signal layer that is too thick
pushes every trace to a high impedance and needs very wide traces to reach 50 Ω;
too thin does the opposite.

The value is the closed-form estimate from the declared dielectric (the same one
`synth_evaluate_thermal_si` reports), not a field solve. It is a sanity check on
the stack, independent of whether any particular net declares an impedance
target — those nets are checked separately by `E-SYNTH-IMPEDANCE-*`.

## Minimal reproduction

```synth
board "b" {
  layers 2
  stackup {
    copper 0.035mm
    insulator 1.5mm er 4.4
    copper 0.035mm
  }
}
```

## Suggested fix

Choose a thinner signal dielectric for the layer traces run on, or accept the
envelope and route with the wider traces the stack demands.
