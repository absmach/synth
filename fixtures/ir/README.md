# IR fixtures

SynthSpec inputs used as golden tests for AST → IR lowering.

Each `*.synth` here is curated to:

- use only parts present in the seed registry (`registry/parts/`);
- exercise a specific IR feature (component resolution, net
  union-find, diff_pair lowering, unit normalization, keepout
  geometry);
- snapshot to a stable IR JSON via insta.

Snapshots live next to the test binary at
`crates/synth-ir/tests/snapshots/`.
