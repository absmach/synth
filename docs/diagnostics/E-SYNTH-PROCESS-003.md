# E-SYNTH-PROCESS-003 — declared via geometry is below the process floor

**Severity:** error
**Stage:** erc

## What this means

A `netclass` declares `via_diameter` or `via_drill`, and the result cannot be
drilled by the profile the board's `manufacturer` resolves to.

Three ways to get here:

| Finding | Meaning |
| --- | --- |
| no copper | The diameter does not clear its own drill. That is a hole, not a via. |
| drill below the floor | The hole is smaller than the tier's smallest drill. |
| ring below the floor | Diameter minus drill, halved, is less copper than the tier leaves around a hole. |

The check exists because the alternative is worse than the hardcoded constant
it replaced. Without it a board could ask for a 0.15 mm drill on a tier whose
floor is 0.3 mm, the exporter would write it into the KiCad net class, the
router would honour it under `strict_sizes`, and the first sign of trouble
would be the fab's quote or the panel.

Judged on the geometry the board is *exported* with. Declaring one half is
checked against the default for the other — 0.6 mm diameter over a 0.3 mm
drill — so `via_diameter 0.3mm` alone is a via with no copper. A class
declaring neither makes no claim and is not judged.

## Minimal reproduction

```synth
board "b" {
  layers 4
  manufacturer "jlcpcb"
  netclass "ESCAPE" {
    via_diameter 0.45mm
    via_drill 0.15mm
  }
}
```

## Suggested fix

Declare a via the tier can drill. JLC's standard floor is 0.3 mm with a
0.13 mm ring, so `via_diameter 0.56mm` over `via_drill 0.3mm` is the smallest
it permits — and the via the escape arithmetic in `synth_router::escape`
sizes when it reports whether a package can be fanned out.

A finer via means a finer tier, which is a `manufacturer` change and a price.
