# E-SYNTH-CONNECT-008 — floating digital input

**Severity:** warning
**Stage:** erc — connectivity

## What this means

An `input` pin on an active part that is otherwise in use has no
connection at all. A floating CMOS input sits at an undefined level and
draws crowbar current; an unused gate input can oscillate and inject
noise. Required pins are handled by `E-SYNTH-CONNECT-001`, and a wholly
unconnected part by `E-SYNTH-CONNECT-006`, so this rule covers the
middle case: a used part with a stray input.

## Minimal reproduction

```synth
board "x" {
  component U1: ic "lm555_timer"
  component C1: capacitor "c_generic_0603"
  connect U1.vcc -> C1.p1
  connect U1.gnd -> C1.p2
  // trig / rst / ctrl / thr are non-required inputs and float
}
```

## Suggested fix

Tie the input to a rail (directly or through a resistor), drive it, or
mark it no-connect so the intent is explicit.
