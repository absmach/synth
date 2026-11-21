# Synth

Synth is a compiler and check pipeline for text-described circuit boards.

## Simulation

**Simulation sidecar**:
The `<design>.sim.toml` file beside a `.synth` source that declares sources,
analysis, measures and limits for `synth sim`.
_Avoid_: test file, sim config

**Simulated part**:
A placed component the emitter wrote into the SPICE deck, because it has a
usable built-in or registry `[spice]` model.
_Avoid_: covered part, included part

**Ignored part**:
A placed component named by the sidecar's `ignore` list and deliberately left
out of the deck for a known modelling gap.
_Avoid_: skipped part, excluded part

**Simulation coverage**:
The four lists a simulation report uses to state what it used and measured:
`modelled`, `ignored`, `unmodelled` and `measured`.
_Avoid_: test coverage, net coverage

**Simulation stage**:
The `simulation` entry in a `synth check` report, with status `pass`, `fail`,
`skipped` or `unknown`.
_Avoid_: sim check, spice gate

**Skipped**:
A simulation-stage status meaning there was no sidecar entry, or the source
gate failed. It is not evidence of correctness and does not block the check.
_Avoid_: passed, not applicable

**Unknown**:
A simulation-stage status meaning the sidecar exists but the simulator could
not run — missing tool, timeout, unreadable report, or setup failure. It
blocks the check, because absence of evidence is not a pass.
_Avoid_: failed, skipped
