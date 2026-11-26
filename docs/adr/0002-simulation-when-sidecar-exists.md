# 0002 — Simulation runs when a sidecar exists

**Status:** accepted (2026-11-20)

## Context

`synth sim` was opt-in: `validate`, `export` and `place` never invoked the
simulator, so a board with a `<design>.sim.toml` could pass `synth check`
without its electrical checks ever running. Running simulation unconditionally
is not viable: most designs have no testbench, and many parts have no SPICE
model.

## Decision

`synth check` runs the sidecar when one exists and reports a `simulation`
stage. No sidecar, or a failed source gate, is `skipped` and non-blocking. A
sidecar that cannot run — because ngspice is missing, the run timed out, the
report was unreadable, or setup failed — is `unknown` and blocks, like the
other unknown physical stages. A limit violation, a missing measurement, or
another blocking simulation diagnostic is `fail` and blocks. An ngspice error
(`E-SYNTH-SIM-006`) is `unknown` instead: the run did not finish, so there is
no evidence either way. A sidecar that runs but asserts
nothing — every measure lacks a `min` or `max` — is `unknown` too: a testbench
that asserts nothing is the same failure as one that cannot run.

`validate`, `export` and `place` still never run ngspice. Simulation remains a
separate, explicit testbench; `synth check` only invokes it when the designer
provided one.

## Consequences

- A green `synth check` covers the testbench the design declares, not every
  net or every part.
- A sidecar now makes ngspice a required tool for the check; this is
  deliberate, because a testbench that cannot run must not read as a pass.
  A testbench that asserts nothing is evidence of nothing, so it blocks the
  same way.
- The report must state coverage (`modelled`, `ignored`, `unmodelled`,
  `measured`) so a partial simulation is not mistaken for full-board
  verification.
- Designs without a sidecar keep their previous check behaviour.
