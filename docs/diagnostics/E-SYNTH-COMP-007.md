# E-SYNTH-COMP-007 — `no_connect` on a wired pin

**Severity:** error
**Stage:** resolve

## What this means

A `no_connect` statement names a pin that is also an endpoint of a net or
`connect` statement. The pin cannot be both intentionally unused and wired:
the wiring wins, so the suppression would hide a live ERC finding instead of
recording an intent. Lowering refuses the statement.

## Minimal reproduction

```synth
board "x" {
  component U1: ic "ch340c"
  power "VCC" 5v
  net "VCC" { U1.vcc }
  no_connect U1.vcc
}
```

## Suggested fix

Decide what the pin is:

- wired: remove the `no_connect` and let ERC judge the wired pin;

```synth
net "VCC" { U1.vcc }
```

- intentionally unused: remove the wire and keep the declaration;

```synth
no_connect U1.vcc
```
