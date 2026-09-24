# E-SYNTH-MODULE-004 — unbound module port

**Severity:** error
**Stage:** lower

## What this means

A module port has no binding in the instantiation. Every declared
port must be bound — otherwise the module's internals would reference
a net the instantiating board never provided. This also fires when an
interface port is referenced inside the module but bound as a whole to
a bus that is missing one of the interface's members.

## Minimal reproduction

```synth
board "x" {
  module "M" (a: input, b: output) {
    component R1: resistor "r_generic_0603"
    connect a -> R1.p1
    connect R1.p2 -> b
  }
  use "M" as X {
    a -> "IN"
  }
}
```

## Suggested fix

Add the missing `port -> net` line to the `use` block. For an
interface port bound as a bundle, make sure the target bus declares
every interface member.
