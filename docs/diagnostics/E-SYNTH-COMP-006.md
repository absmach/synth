# E-SYNTH-COMP-006 — `no_connect` names an unknown pin

**Severity:** error
**Stage:** resolve

## What this means

A `no_connect` statement names a reference designator or pin that does not
exist. Recording it would suppress nothing and hide the typo, so lowering
refuses it.

## Minimal reproduction

```synth
board "x" {
  component U1: ic "ch340c"
  no_connect U1.cts, U1.not_a_pin
}
```

## Suggested fix

Use a pin name (or number) the registry part actually has:

```synth
no_connect U1.cts
```
