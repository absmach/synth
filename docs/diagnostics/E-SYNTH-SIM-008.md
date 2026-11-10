# E-SYNTH-SIM-008 — floating node or no DC path

**Severity:** error
**Stage:** sim

## What this means

Every net in the simulated circuit needs at least two connections and a DC
path to the ground net. A capacitor is an open circuit at DC, so two
capacitors in series leave the middle net with no path; so does a net with a
single pin, or an unconnected part that leaves its neighbour dangling. The
message names the net.

A part listed under `ignore` in the testbench is removed before this check, so
ignoring the only part that connects a net reports the now-floating net.

Synth never turns on `rshunt` silently to hide the problem: a floating node is
a design error, not a solver setting.

## Minimal reproduction

```synth
board "x" {
  component C1: capacitor "c_generic_0805" value "1u"
  component C2: capacitor "c_generic_0805" value "1u"
  net "VCC" { C1.p1 }
  net "GND" { C2.p2 }
  connect C1.p2 -> C2.p1 as "MID"
}
```

`MID` has two connections but no DC path to `GND`.

## Suggested fix

Add a resistor or another DC path across the floating net, wire the dangling
pin, or remove the part from the simulated circuit. A series capacitor is the
same class of error when the node has no other path to ground.
