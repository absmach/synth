# E-SYNTH-CONNECT-001 — required pin not connected

**Severity:** error
**Stage:** erc — connectivity

## What this means

A part defines one or more pins with `required = true` (typically
power inputs, ground, reset). The resolver found that the component
exists in the design but the required pin has no `connect` statement
attached to it.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  // U1.vdd_io, U1.vdd_core, U1.gnd, U1.run all required by the part —
  // none connected here, so the rule fires once per missing pin.
}
```

## Suggested fix

Add a `connect` statement to each floating required pin. The rule
does not yet emit automatic patches because the choice of net
(decoupling cap, power rail, ground plane) is a design decision.
