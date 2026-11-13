# E-SYNTH-PLACE-003 — a part does not fit the declared outline

**Severity:** error
**Stage:** placement

## What this means

The design declares a fixed board rectangle with `outline`, and a placement
sidecar puts a part's courtyard outside it. A declared outline is authoritative:
Synth reports the part instead of silently growing the board or sliding the
part inward.

A mounting hole is the one exception: only its drill has to stay inside the
outline, so the hole may sit on the edge.

## Minimal reproduction

```synth
board "x" {
    outline 60mm x 30mm
    component R1: resistor "r_generic_0603" { value "10k" }
}
```

with `x = 65` for `R1` in `x.placement.layout.toml`.

## Suggested fix

Move the part inside the rectangle, or declare a larger `outline`.
