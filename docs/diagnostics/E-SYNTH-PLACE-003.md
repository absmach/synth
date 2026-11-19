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

## Placement spacing outside the supported range

The same code reports a `placement { spacing = … }` value, in the board file or
`export-kicad --spacing`, outside 1.0 mm to 5.0 mm. The placer is tuned for that
range: below it the parts cannot be routed, above it the board grows without a
routing benefit. The value is never clamped.

## Minimal reproduction

```synth
board "x" {
  layers 2
  placement { spacing = 0.5mm }
}
```

## Suggested fix

Use a spacing from 1.0 mm to 5.0 mm. To proceed anyway, re-run the export with
`--force`: it uses the typed value and prints a warning. Routing is still
judged by the router, so a spacing too tight shows up as open nets.
