# E-SYNTH-SIM-007 — malformed testbench or name collision

**Severity:** error
**Stage:** sim

## What this means

The `<design>.sim.toml` sidecar could not be parsed or validated, or two Synth
names collapse to one SPICE name after sanitizing. SPICE is case-insensitive,
so `MID` and `mid` both become node `n_MID`; two references such as `R1` and
`r1` both become element `RR1`. The message names the offending field or pair.

Validation also rejects an absent ground, no sources, no measures, unknown
fields, non-finite numbers, `min` greater than `max`, and a source on the
ground net.

## Minimal reproduction

```toml
gronud = "GND"
```

or a design with `net "MID" { … }` and `net "mid" { … }`.

## Suggested fix

Fix the TOML field name or value, or rename one of the colliding Synth nets or
references. Use the field names documented for the testbench.
