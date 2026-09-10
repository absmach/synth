# E-SYNTH-PARSE-026 — expected number with unit

**Severity:** error
**Stage:** parse

## What this means

An attribute like `impedance` or `radius` requires a value with an
explicit engineering unit. The parser found something else in that
position.

## Minimal reproduction

```synth
board "x" {
  diff_pair A B { impedance 90 }
}
```

## Suggested fix

Append a valid unit suffix (e.g., `90ohm`, `20mm`).
