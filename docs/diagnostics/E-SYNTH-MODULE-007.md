# E-SYNTH-MODULE-007 — duplicate instance label

**Severity:** error
**Stage:** lower

## What this means

Two `use` blocks share the same instance label. The label names the
instance's sheet and, by default, its refdes prefix — so a duplicate
would merge two instances' pages and collide their reference
designators.

## Minimal reproduction

```synth
board "x" {
  module "M" (a: input) {
    component R1: resistor "r_generic_0603"
    connect a -> R1.p1
  }
  use "M" as X {
    a -> "IN"
  }
  use "M" as X {
    a -> "IN2"
  }
}
```

## Suggested fix

Give each instance a unique label (`use "M" as X2 { … }`), or set an
explicit `prefix` so the refdes prefixes stay distinct.
