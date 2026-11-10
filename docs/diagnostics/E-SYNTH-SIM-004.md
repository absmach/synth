# E-SYNTH-SIM-004 — measurement limit

**Severity:** info when inside the declared limit, error when outside
**Stage:** sim

## What this means

A measurement was compared with its optional `min` and `max`. Inside the
limits is an informational diagnostic; outside is a blocking error that makes
`synth sim` exit non-zero. Limits are inclusive.

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
