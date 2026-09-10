# E-SYNTH-PARSE-018 — layer count out of range

**Severity:** error
**Stage:** parse

## What this means

The `layers` value must be in the range `[1, 64]`. Phase 1 enforces
this as a syntactic check; later phases will refine the upper bound
per manufacturer profile.

## Minimal reproduction

```synth
board "x" { layers 9999 }
```

## Suggested fix

Use a realistic layer count. Common values: `2`, `4`, `6`, `8`.
