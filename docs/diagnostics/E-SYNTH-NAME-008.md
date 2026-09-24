# E-SYNTH-NAME-008 — declared net label used only once

**Severity:** warning
**Stage:** erc — naming

## What this means

A user-declared net name (`net "…"`, `power "…"`, or `connect … as "…"`)
ends up on fewer than two endpoints. The label connects nothing: either
a second endpoint was forgotten, or the declaration is dead. Nets that
are *not* explicitly named and have one endpoint are reported by
`E-SYNTH-CONNECT-002` instead, so exactly one rule fires per case.

## Minimal reproduction

```synth
board "x" {
  component R1: resistor "r_generic_0603"
  net "ONLY_ONCE" { R1.p1 }   // declared, but wired to nothing else
}
```

## Suggested fix

Wire a second endpoint to the net, or delete the declaration.
