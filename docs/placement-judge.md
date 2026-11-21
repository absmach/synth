# Placement judge: the numbers a placement round is accepted on

**Status:** implementation note, 2026-11-10.
**Scope:** measurement only. No ranking, gate, repair or placer change.

## Why

The placement rounds compare a candidate against the last accepted board on a
handful of numbers that used to live in one-off Python scripts: alignment, the
decoupling ring, the largest empty rectangle, outline area, connector docking
and centring, review-group gaps, header rows, overlaps, parts outside the
outline and mounting-hole corners. A number that only exists in a scratch
script cannot accept or reject the next round.

`synth_place::judge(board, placement) -> PlacementJudgement` is the pure seam.
It reads a finished placement and returns the same rows in a fixed order, plus
a 0–100 composite and a separate hard-gate verdict. It never moves a part.
`place()` does not call it; round 21's compression pass calls it directly from
`finish_outline` as the acceptance check. `fitness` remains the electrical report
(support distance, rail band, mating inset, area efficiency); the judge is the
complementary layout-quality report.

## What it measures

Rows in report order:

| row | measurement | hard limit | score |
| --- | --- | --- | --- |
| `outline_area` | outline width × height, mm² | none | reading |
| `largest_empty` | largest rectangle no courtyard + 1 mm touches, mm² | none | `1 - empty / outline` |
| `alignment` | share of passives in chip-bearing groups sharing a row or column (0.5 mm) with a peer | none | the ratio |
| `ring` | share of group capacitors within 3 mm of a group chip | none | the ratio |
| `area_efficiency` | placed-courtyard envelope / outline | none | `occupancy / 0.55` |
| `docking` | share of wired connectors flush on an edge (0.15 mm) | 1.0 | the ratio |
| `usb_offset` | worst USB body-centre offset from its edge midpoint, mm | 0.5 mm | `1 - offset / 0.5` |
| `group_gap` | least gap between review-group boxes, mm | 2.0 mm | `gap / 2.0` |
| `header_alignment` | worst pin-1 spread of headers on one edge or in one row, mm | 0.5 mm | `1 - spread / 0.5` |
| `overlaps` | overlapping or touching courtyard pairs | 0 | reading |
| `outside_outline` | courtyards not wholly inside the outline | 0 | reading |
| `hole_corner` | worst mounting-hole centre distance to a board corner, mm | 5.5 mm | reading |

The composite is the equal-weight mean of the rows that carry a score,
renormalized over the rows that could be measured, as in `fitness`. A row that
cannot be measured is omitted from the composite and named in `assumptions`;
it is not scored as perfect.

`gate_failures` is the sum of the hard-limit breaches. `passed` is
`gate_failures == 0`. Acceptance compares `passed` first; `score` ranks two
placements that both pass, and the individual rows say why. A structural
defect such as an overlap or an undocked connector is never averaged away by
good alignment.

## Report, round 20

Measured on the two experiment boards at `9e3184aa`, exported through
`place_with_replace` + `finish_outline` (the same path as `SYNTH_PLACE_REPLACE=1`):

| board | score | passed | gaps | area mm² | empty mm² | align | ring | docking | usb | headers | holes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `rtd_afe` | 96.30 | yes | 0 | 2066.7 | 106.7 | 0.935 | 0.857 | 3/3 | 0.00 | n/a | 4.95 |
| `fpv_fc` | 97.59 | yes | 0 | 4471.0 | 82.8 | 0.946 | 0.880 | 5/5 | 0.00 | 0.00 | 4.95 |

These match the round-19 Python metrics (`r16_metrics.py`, `align_metrics.py`)
on the same placements, which is the independent check on the Rust
measurement. `header_alignment` is measured only when two headers share an
edge or an interior row; `rtd_afe` has no such row, so the row is absent and
the composite renormalizes.

## What it does not do

- It does not change the placement, the candidate ranking, the export or the
  route loop.
- It is not an export failure: `passed` is a report field, not a hard stop.
- It does not replace `fitness` or `score.rs`; the three answer different
  questions.
- It does not measure silk labels, routing quality, symmetry, net length or
  schematic legibility.

## Tests

`crates/synth-place/tests/judge.rs` covers:

- the report contract, determinism and that the input placement is unchanged;
- the round-19 known-good values on `rtd_afe` and `fpv_fc`;
- an overlap, an off-centre USB, a lost group gap, an undocked connector and
  a turned header all failing their gates;
- moving every group capacitor away emptying the ring.

The ignored `round_report` test prints the JSON for the two boards:

```sh
cargo test -p synth-place --test judge round_report -- --ignored --nocapture
```
