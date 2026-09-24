# E-SYNTH-MODULE-001 — unknown module

**Severity:** error
**Stage:** lower

## What this means

A `use "NAME" as LABEL { … }` names a module that is not declared
anywhere in the design or in a file it imports. Module declarations
are collected from the whole program, so the declaration may sit in an
imported library file — but it must exist and the name must match
exactly (names are case-sensitive).

## Minimal reproduction

```synth
board "x" {
  use "SensorChannel" as CH1 {
    vdd -> "3V3"
  }
}
```

## Suggested fix

Declare the module with `module "NAME" (…) { … }`, or correct the
name. If the module lives in another file, add the `import`.
