# E-SYNTH-KEEPOUT-002 — keepout zero or negative radius

**Severity:** error
**Stage:** erc — geometry

## What this means

A `keepout`'s radius is zero or negative. A keepout must be a positively-sized region for the router and manufacturing tooling to honour it.

## Minimal reproduction

```synth
board "x" {
  keepout antenna_zone { radius 0mm }
}
```

## Suggested fix

Set the radius to a strictly positive length such as `2mm`.
