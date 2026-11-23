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

A `[[source]]` drives exactly one target. It must have a `net` or a `pin`, not
both. A net source is a non-empty, newline-free net name with `dc` and/or `ac`,
and no `level`; the same net may not be declared twice. A pin source is a
non-empty `REF.PIN` with a non-empty reference and pin name (split at the first
`.`), requires `level = "high"` or `"low"`, and rejects `dc`/`ac` because the
voltage comes from the registry's `[pins.drive]` data. The same pin may not be
declared twice. `level` without `pin` is also rejected.

Exactly one analysis must be selected: `op` or `ac`. `[analysis.ac]` requires
a positive finite `start` (default 1.0), a finite `stop` greater than `start`
(default 1e6), and `points_per_decade` between 1 and 1000 (default 100). The
sweep must contain at least two points:
`points_per_decade * log10(stop/start) >= 1`. A smaller sweep such as
`.ac dec 1 10 55` makes real ngspice 42 hang, so it is rejected here before the
run; the message names the range and says to widen the sweep or raise
`points_per_decade`.

The target fields depend on the measure kind:

- `node_voltage` needs a non-empty `node` and rejects `element` and `drop_db`.
  It requires the `op` analysis.
- `current` needs a non-empty `element` and rejects `node` and `drop_db`. It
  requires the `op` analysis.
- `relative_db` needs a non-empty `node` and rejects `element`. Its `drop_db`
  defaults to 3.0 and must be finite and greater than 0. It requires the `ac`
  analysis, and at least one source must carry an `ac` magnitude.
- `pin_current` needs a non-empty `pin` (`REF.PIN`) and rejects `node`,
  `element`, `drop_db`, `at_frequency`, `min` and `max`; the limit comes from
  the pin's `[pins.drive]` data. It requires the `op` analysis and a
  `[[source]]` with the same pin.

A `current` measure `element` that names no placed part is reported here.
So is an element listed under `ignore`, and a current measure on a registry
subcircuit or an unmodelled part, where there is no built-in device current
to read.

A pin source reference that names no placed part or no pin of it, a pin that
is not connected to a net, a pin on the ground net, a `pin_current` measure
with no matching source, and two sources that resolve to the same net are all
reported here. The duplicate-source message names both source descriptions and
the net; the second source is left out of the deck.

`[options] plot` must be a non-empty path without a newline or a single quote,
and it requires the `ac` analysis; the path is emitted single-quoted into the
deck.

Synth also rejects a source on the ground net and a measure node that names
no design net. A measure on the ground net itself is rejected because the
node is `0` and ngspice has no vector for it; ground is 0 V by definition.
A newline in a testbench string value (ground, a source net, a
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
