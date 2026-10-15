# W-SYNTH-DRC-020 — too much current per via

**Severity:** warning
**Stage:** drc

## What this means

A net that declares a `current` spreads it across its vias; the current divided
by the number of vias on the net exceeds the 1.5 A a via (0.3 mm drill, 10 °C
rise) is assumed to take. Widening the trace will not fix this — the vias are
the bottleneck.

The estimate assumes the vias share the current evenly, which is optimistic
when they sit on one path. Advisory.

## Minimal reproduction

A `netclass "PWR" { current 3A }` net routed through a single via.

## Suggested fix

Add vias so the current divides, or use a larger drill.
