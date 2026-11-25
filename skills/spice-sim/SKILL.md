---
name: spice-sim
description: Use when a Synth board needs an electrical check with ngspice, when a design has a `<design>.sim.toml` testbench, or when `synth check` reports a simulation stage.
---

# SPICE simulation

## What it is

`synth sim` compiles a design, emits a SPICE deck from its simulated parts,
and runs the `<design>.sim.toml` sidecar through ngspice. `synth check` runs
the same testbench automatically when the sidecar exists, and reports a
`simulation` stage.

Simulation is partial. It does not prove power sequencing, signal integrity,
EMC, thermal behaviour, or firmware.

## Before writing

- The sidecar must sit beside the source, named `<design>.sim.toml`.
- Built-in models cover resistors, capacitors, inductors, LEDs and diodes.
  Other parts need a registry `[spice]` model.
- Every non-ignored part must have a model. Use `ignore` only for a known,
  named gap; never add a part to `ignore` just to make a run pass.
- Limits belong in the sidecar and must come from a datasheet. If there is no
  source for a limit, leave the measure unbounded and say it is not covered.

## Author

Try the mechanical draft first:

```sh
synth sim --suggest design.synth
```

It recognises a few shapes and never guesses a voltage. Review the generated
sidecar before using it.

For anything else, write the sidecar by hand:

```toml
ground = "GND"

[[source]]
net = "VIN"
kind = "dc"
dc = 12.0

[analysis]
op = true

[[measure]]
name = "vout"
kind = "node_voltage"
node = "VOUT"
min = 5.7
max = 6.3
```

Use Synth net names and refdes values, not sanitized SPICE names.

## Run

```sh
synth sim design.synth          # PASS / FAIL / MEASURED lines
synth sim --json design.synth   # measures, diagnostics, coverage
synth check design.synth        # includes the simulation stage
```

Read the measured values, not just the exit code. `MEASURED` means the measure
had no limits; it is not a pass.

`synth check` reports `simulation` as `pass`, `fail`, `skipped` or `unknown`:

- `skipped`: no sidecar, or the source gate failed.
- `pass`: the testbench ran, at least one measure had a limit, and every
  blocking diagnostic is clear.
- `fail`: a blocking `E-SYNTH-SIM-*` diagnostic other than
  `E-SYNTH-SIM-006`, including an out-of-limit measure.
- `unknown`: the sidecar exists but the simulator could not run (ngspice is
  missing, timed out, reported an error, the report was unreadable, or setup
  failed, for example the registry did not load), or it ran but checked no
  limits.

`fail` and `unknown` block the check. `skipped` and `pass` do not.

## Report coverage

Use `coverage` from the JSON result:

- `modelled`: parts the deck used.
- `ignored`: refdes values from the sidecar.
- `unmodelled`: parts with no usable model.
- `measured`: net, element and pin targets named by measures.

`limits_checked` counts measures with a `min` or `max`; zero means the run
checked no limits, and `synth check` reports that as `unknown`. A `simulation`
stage also carries `testbench_sha256`, so a pass is tied to the sidecar
revision.

Say what the run covered and what it did not. Do not describe a partial
simulation as full-board verification.
