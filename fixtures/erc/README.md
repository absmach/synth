# ERC fixtures

Phase 3 fixtures exercising the registry resolver and ERC rules.

Each test case is a **twin pair**:

- `pass__<name>.synth` — valid design; expected to emit zero
  error-level diagnostics through the full validate pipeline (parse →
  resolve → ERC).
- `<CODE>__<name>.synth` — same design with exactly one rule
  violated; expected to emit the named diagnostic code.

The shared name (`<name>`) ties the two together. The snapshot test
asserts both expectations and verifies that recovery / rule scope is
correct.
