# E-SYNTH-PARSE-017 — expected layer count (positive integer)

**Severity:** error
**Stage:** parse

## What this means

A `layers` statement is written as `layers <N>` where `<N>` is a bare
integer. The parser found something other than an integer literal.

## Minimal reproduction

```synth
board "x" { layers 2mm }
```

## Suggested fix

Provide a bare integer. Common values: `2`, `4`, `6`, `8`.
