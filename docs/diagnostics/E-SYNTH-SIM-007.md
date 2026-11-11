# E-SYNTH-SIM-007 — malformed testbench or name collision

**Severity:** error
**Stage:** sim

## What this means

The `<design>.sim.toml` sidecar could not be read, parsed or validated, or
two Synth names collapse to one SPICE name after sanitizing. SPICE is
case-insensitive, so `MID` and `mid` both become node `n_MID`; two references
such as `R1` and `r1` both become element `RR1`. The message names the
offending field or pair.

Validation also rejects an absent ground, no sources, no measures, unknown
fields, non-finite numbers, a non-positive or oversized `[options] timeout`,
`min` greater than `max`, and a measure with no node. Synth also rejects a
source on the ground net and a measure node that names no design net. A
newline in a testbench string value (ground, a source net, a measure node or
an `ignore` reference) is rejected as well: a newline ends a SPICE line, so
it could inject deck commands.

## Minimal reproduction

A design with no sidecar beside it:

```sh
cp examples/divider/divider.synth /tmp/divider.synth
synth sim /tmp/divider.synth
```

or a sidecar with a typo:

```toml
gronud = "GND"
```

or a design with `net "MID" { … }` and `net "mid" { … }`.

## Suggested fix

Create the missing `<design>.sim.toml`, fix the TOML field name or value, or
rename one of the colliding Synth nets or references. Use the field names
documented for the testbench.
