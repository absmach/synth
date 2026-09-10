# E-SYNTH-RESET-001 — reset pin floating

**Severity:** warning
**Stage:** erc — reset

## What this means

A pin with `reset` capability that is *not* marked `required` on its part is left unconnected. The chip's reset state depends on this pin, so floating it is risky.

## Minimal reproduction

(intentionally omitted — most reset pins are required and covered by E-SYNTH-CONNECT-001 instead)

## Suggested fix

Tie the pin to a defined reset network — typically a pull-up resistor to VCC plus a reset cap to GND.
