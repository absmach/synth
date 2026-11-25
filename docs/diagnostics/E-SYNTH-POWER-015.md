# E-SYNTH-POWER-015 — feedback divider margin is unproven

**Severity:** warning
**Stage:** erc — power

## What this means

A regulator has a two-resistor feedback divider, but the worst-case
analysis ([`E-SYNTH-POWER-013`](E-SYNTH-POWER-013.md)) cannot run because an
input is missing. Synth does not assume a tolerance, a reference range or a
load window, so the output margin is not checked. This is incomplete
evidence, not a pass.

The message lists each missing input:

- the regulator part's `feedback_vref_min_v` / `feedback_vref_max_v`;
- a tolerance on either resistor (`tolerance "1%"`, `value "10k 1%"`, or a
  chip-resistor MPN that encodes one);
- a supply window on a load of the output net (`voltage_min_v` /
  `voltage_max_v` on the load pin, or the load part's
  `operating_conditions`).

## Suggested fix

Add the missing data. Take reference ranges from the regulator data sheet's
electrical characteristics table, and cite the table in a comment in the
part file. Never copy a typical value into both bounds.
