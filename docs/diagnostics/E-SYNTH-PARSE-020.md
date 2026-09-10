# E-SYNTH-PARSE-020 — unexpected attribute inside keepout

**Severity:** error
**Stage:** parse

## What this means

Phase 1 recognizes only `radius <value><unit>` inside a `keepout` body.
Any other token is rejected. Later phases will add layer-scoped keepouts
and polygon shapes.

## Minimal reproduction

```synth
board "x" {
  keepout antenna {
    shape circle
  }
}
```

## Suggested fix

Remove the unsupported attribute or replace it with a recognized one.
