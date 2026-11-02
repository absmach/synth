# Placement fitness: an electrical read on a PCB placement

**Status:** implementation note, 2026-10-30.
**Scope:** measurement only. No ranking, gate, repair or placer change.

## Why

The existing placement review answers "does this board look structurally
believable?" and the existing `score.rs` report answers a narrower question
with centre-to-centre distances and a 5 mm / 40 % gate. Neither tells an agent
whether the parts are where the circuit needs them.

On the three reference boards, the current `place()` output measures:

| board | mean support distance | rail-band passives | worst mating inset | gate failures |
| --- | --- | --- | --- | --- |
| `sensor_logger` | 16.94 mm | 18 % | 8.33 mm | 10 |
| `env_logger` | 14.98 mm | 0 % | 5.23 mm | 9 |
| `secure_tracker` | 8.71 mm | 0 % | 3.80 mm | 4 |

That is the gap this report makes visible.

## What it measures

- **Support distance** — for every capacitor the cluster recognizer binds to
  an anchor pin (`DecouplingCap`, `RailCap`, `OrphanRailCap`, `LoadCap`), the
  pad-to-pad distance to that pin. Limits are 3 mm for a decoupling capacitor
  and 5 mm for rail, orphan-rail and load capacitors; a part's declared
  `max_distance_mm` overrides the default. Reports mean, worst and failures.
- **Rail band** — the share of two-pin passives whose courtyard centre sits in
  the strip along the top edge (10 % of board height, clamped to 4–10 mm),
  excluding passives within 2 mm of a connector from both numerator and
  denominator. Target is 0 %.
- **Mating inset** — the distance from a connector's courtyard to the outline
  edge it faces. The declared `MatingFace` is rotated with the placement; an
  undeclared part uses the nearest edge. A body outside the outline is a
  negative inset and fails rather than reading as flush. Limit is 3 mm.
- **Area efficiency** — placed courtyard envelope divided by the outline. The
  score saturates at 55 % occupancy and never gates; it is a density read, not
  a "smaller is better" objective.

Each metric is normalized to `[0, 1]`; the 0–100 composite is 100 × the
equal-weight mean over the metrics that could be measured. Missing metrics are excluded and
listed in `assumptions`, together with any fallback (no footprint geometry, no
pad offset, no declared mating face). `gate_failures` is reported separately
and is never folded into the score.

## What it does not do

- It does not change the placer, the export, the candidate ranking or the
  route loop.
- It is not a hard gate. A board can report failures and still export.
- It does not replace `score.rs`; that report stays until the two are
  deliberately consolidated.
- It does not measure schematic legibility, alignment, symmetry or cluster
  flow.

## Integration

`synth_place::fitness(board, placement) -> PlacementFitness` is the public
seam. The MCP `synth_describe_placement` handler merges the report into its
response, so an agent sees it without the placement description itself paying
for the measurement. `describe_placement` and the placer's refine loop are
unchanged.

## Qualification

Run the corpus report:

```sh
cargo test -p synth-place --test fitness corpus_report -- --ignored --nocapture
```

On this branch: 62 example and fixture designs placed, 0 placement failures,
47 with zero gate failures and 15 with at least one. Metric coverage is
limited by the corpus — only 8 designs have bound support capacitors, 8 have
connectors, 15 have two-pin passives and 29 have enough geometry for area
efficiency; most fixtures are parser or error cases.

The normal test suite covers the report contract, deterministic JSON, a
stranded capacitor changing support distance, a body outside the outline
failing mating with a negative inset, the rail-band ratio rising when a
passive enters the top strip, missing metrics appearing in assumptions, the
MCP response carrying the block, and an empty board scoring zero.

## Provenance

The idea of a multi-metric placement report came from reviewing the
Copperhead tool. This implementation, its definitions, thresholds and
vocabulary are original; no Copperhead source, identifier, comment or constant
is reproduced.
