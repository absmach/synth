# E-SYNTH-PARSE-012 — expected `:` after refdes

**Severity:** error
**Stage:** parse

## What this means

A `component` declaration has the shape `component <refdes> : <kind> <part>`.
The parser found something other than `:` after the refdes.

## Minimal reproduction

```synth
board "x" {
  component U1 mcu "rp2350"
}
```

## Suggested fix

Insert `:` between the refdes and the kind.
