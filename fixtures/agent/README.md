# Agent harness corpus

Seeded broken designs that the reference agent harness runs through
the **validate → apply patches → re-validate** loop.

Each fixture is a `.synth` source file that emits at least one
diagnostic carrying a machine-applicable `suggested_fix`. The harness
asserts convergence (clean validate) within 5 iterations on
≥80% of the corpus.

Convergence semantics:

- **Converges**: re-validate emits no error- or fatal-severity
  diagnostics after ≤5 fix iterations.
- **Stalls**: a fix iteration produces no new patches but errors
  remain.
- **Diverges**: a fix iteration introduces new errors that the next
  iteration cannot resolve. Should never happen with deterministic
  patches; treated as a regression if it does.

Adding a fixture:

1. Write a broken `.synth` file under this directory.
2. Run `cargo test -p synth-validate --test agent_harness` and
   confirm it either converges (count toward the gate) or fails
   loudly enough to investigate why.
3. If the fixture is intentionally **non-converging** (e.g., it tests
   that the harness handles a rule with no machine fix), prefix
   the filename with `noconverge__`.
