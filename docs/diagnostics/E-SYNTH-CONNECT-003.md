# E-SYNTH-CONNECT-003 — no-connect pin is wired

**Severity:** error
**Stage:** erc — connectivity

## What this means

The registry marked the pin `no_connect`, which by datasheet convention must be left floating. Wiring it can introduce shorts or back-feed silicon.

## Minimal reproduction

(intentionally omitted — depends on a part with a `no_connect` pin)

## Suggested fix

Remove the `connect` that touches the offending pin, or pick a different pin that the datasheet permits to use.
