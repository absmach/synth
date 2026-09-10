# E-SYNTH-NAME-001 — duplicate component refdes

**Severity:** error
**Stage:** erc — naming

## What this means

Two components in the same board declare the same reference designator. Refdes is the human and BoM-level identity of a part; duplicates make the design unrenderable in any downstream tool.

## Minimal reproduction

```synth
board "x" {
  component R1: resistor "r_generic_0603"
  component R1: resistor "r_generic_0603"
}
```

## Suggested fix

Rename one of the duplicates (e.g., `R2`).
