# E-SYNTH-PARSE-032 — unexpected keyword inside variant body

**Severity:** error
**Stage:** parse

## What this means

A `variant` block contains something other than a `dnp` list. Variant
bodies currently accept `dnp` followed by one or more component refdes.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  variant "lite" {
    remove U1
  }
}
```

## Suggested fix

Use `dnp <refdes> …`, e.g. `variant "lite" { dnp U1 }`.
