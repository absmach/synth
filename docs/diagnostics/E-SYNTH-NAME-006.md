# E-SYNTH-NAME-006 — net joined to an unknown or conflicting netclass

**Severity:** error
**Stage:** lowering — named nets

## What this means

A `net`, `power`, or `connect` statement joins a netclass with
`class "NAME"`, but no `netclass "NAME" { … }` declaration exists in
the board (unknown netclass), or two different declared netclasses
are joined to one net (conflicting netclasses). In the unknown case
the net lowers with no class; in the conflict case the first known
class wins.

## Minimal reproduction

```synth
board "x" {
  component R1: resistor "r_generic_0603"
  component R2: resistor "r_generic_0603"
  net "SIG" class "NOPE" { R1.p1, R2.p1 }
}
```

## Suggested fix

Declare the class (`netclass "NOPE" { trace_width 0.2mm }`), fix the
spelling of the `class` clause, or keep a single `class` clause per
net.
