# E-SYNTH-MODULE-005 — nested module instantiation is not supported

**Severity:** error
**Stage:** lower

## What this means

A module body contains another `use`. Modules are a source-level
reuse construct that flatten to concrete parts, so instantiating a
module inside a module would need a recursive expansion whose net
naming and refdes prefixes are ambiguous. Compose modules at the board
level instead.

## Minimal reproduction

```synth
board "x" {
  module "Inner" (a: input) {
    component R1: resistor "r_generic_0603"
    connect a -> R1.p1
  }
  module "Outer" (a: input) {
    component R2: resistor "r_generic_0603"
    connect a -> R2.p1
    use "Inner" as N1 {
      a -> "NESTED"
    }
  }
  use "Outer" as O1 {
    a -> "IN"
  }
}
```

## Suggested fix

Move the inner `use` up to the board body and wire the two instances
together with an explicit `connect`.
