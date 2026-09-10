# E-SYNTH-POWER-003 — power input without a source

**Severity:** warning
**Stage:** erc — power

## What this means

A `power_input` pin sits on a net whose only members are sinks (no `power_output`, no passive component that could be sourced elsewhere).

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "stm32g030"
  component U2: mcu "stm32g030"
  connect U1.vdd -> U2.vdd  // no supply anywhere
}
```

## Suggested fix

Wire the rail to a regulator output, battery, or external power connector.
