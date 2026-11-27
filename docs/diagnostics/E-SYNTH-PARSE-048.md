# E-SYNTH-PARSE-048 — malformed ground_domain item

**Severity:** error
**Stage:** parse

## What this means

Inside `ground_domain <net> { … }` the parser accepts two items:
`join <REFDES>` and `crossing <net> stitch <join | REFDES>`. It found
something else, or a `crossing` without `stitch`, or no reference designator
where one was expected.

## Minimal reproduction

```synth
board "x" { layers 2 ground_domain AGND { crossing SCLK join } }
```

## Suggested fix

Write `crossing SCLK stitch join` (or `stitch C20` for a stitching capacitor),
and `join R_GND` for the part that bridges the grounds.
