# E-SYNTH-MODULE-002 — unknown module parameter

**Severity:** error
**Stage:** lower

## What this means

An instantiation sets a parameter the module does not declare:

```synth
use "SensorChannel" as CH1 (r_pull = 4.7kohm) { … }
```

`r_pull` must appear as `param r_pull: … = …` in the module body.
The default from the declaration is used for every parameter the
instantiation does not override, so setting an unknown one is always a
mistake rather than an alternative spelling.

## Minimal reproduction

```synth
board "x" {
  module "M" (a: input) {
    component R1: resistor "r_generic_0603"
    connect a -> R1.p1
  }
  use "M" as X (r_pull = 4.7kohm) {
    a -> "IN"
  }
}
```

## Suggested fix

Declare the parameter in the module body (`param r_pull:
resistance = 4.7kohm`), or remove the override.
