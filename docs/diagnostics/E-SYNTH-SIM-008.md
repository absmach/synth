# E-SYNTH-SIM-008 — floating node or no DC path

**Severity:** error
**Stage:** sim

## What this means

A node has no usable DC path. Synth currently reports an unconnected
two-terminal pin because there is no net to emit for it. The full floating-node
and no-DC-path sweep is a separate check; Synth never turns on `rshunt`
silently to hide the problem.

## Minimal reproduction

```synth
board "x" {
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  net "MID" { R3.p1 }
}
```

`R3.p2` is on no net.

## Suggested fix

Wire the pin to a net, or remove the part from the simulated circuit. A
series capacitor or a removed `ignore` part that leaves a node floating is the
same class of error.
