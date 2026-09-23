# E-SYNTH-POWER-007 — conflicting rail voltages shorted together

**Severity:** error
**Stage:** lowering — named nets

## What this means

Two `power` declarations state different nominal voltages for one
net (e.g. `power "RAIL" 3.3v` and `power "RAIL" 5v` merged by a
shared name). Rails at different voltages must not be shorted, so
lowering reports the conflict and keeps the first-declared voltage
for power-domain inference.

## Minimal reproduction

```synth
board "x" {
  component R1: resistor "r_generic_0603"
  component R2: resistor "r_generic_0603"
  power "RAIL" 3.3v
  power "RAIL" 5v
  connect R1.p1 -> R2.p1 as "RAIL"
}
```

## Suggested fix

Keep a single `power` declaration per rail name, or split the nets
so each voltage lives on its own net.
