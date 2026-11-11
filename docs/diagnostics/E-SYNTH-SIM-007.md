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
`min` greater than `max`, and a source with neither `dc` nor `ac`. A source's
`ac` magnitude must be finite and non-negative.

Exactly one analysis must be selected: `op` or `ac`. `[analysis.ac]` requires
a positive finite `start` (default 1.0), a finite `stop` greater than `start`
(default 1e6), and `points_per_decade` between 1 and 1000 (default 100).

The target fields depend on the measure kind:

- `node_voltage` needs a non-empty `node` and rejects `element` and `drop_db`.
  It requires the `op` analysis.
- `current` needs a non-empty `element` and rejects `node` and `drop_db`. It
  requires the `op` analysis.
- `relative_db` needs a non-empty `node` and rejects `element`. Its `drop_db`
  defaults to 3.0 and must be finite and greater than 0. It requires the `ac`
  analysis.

A `current` measure `element` that names no placed part is reported here.
So is an element listed under `ignore`, and a current measure on a registry
subcircuit or an unmodelled part, where there is no built-in device current
to read.

`[options] plot` must be a non-empty path without a newline or a single quote,
and it requires the `ac` analysis; the path is emitted single-quoted into the
deck.

Synth also rejects a source on the ground net and a measure node that names
no design net. A newline in a testbench string value (ground, a source net, a
measure node, a measure element, a plot path or an `ignore` reference) is
rejected as well: a newline ends a SPICE line, so it could inject deck
commands.

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
