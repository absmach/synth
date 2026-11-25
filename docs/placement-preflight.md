# Placement preflight: validate knows whether the parts pack

**Status:** implementation note, 2026-11-25.

## Why

`synth validate` used to judge the source, the netlist and the board estimate.
None of those runs the placer. The CM4 carrier passed validate with zero
blocking diagnostics, then failed an hour later in `synth place` with
`E-SYNTH-PLACE-002` on `TP_NEXTRST`: the greedy packing had boxed itself in on
a 148 × 108 mm canvas. The parts, the hints and the canvas were all visible to
validate; the placer just never looked.

The second half of the same failure was the placer's verdict. `E-SYNTH-PLACE-002`
says "this ordering failed on this canvas", not "no legal placement exists". A
hard hint or a few millimetres of canvas can be the difference, and nothing
retried.

## What validate does now

`synth validate` runs the real placer through the bounded ladder in
`synth_place::place_legal` and reports what it finds. The search costs time on
a dense board, so `--no-placement-preflight` restores the source-and-netlist
check.

The ladder, in order:

1. **The production solve**, hints as written. A board that already places is
   returned unchanged, so nothing that worked before moves.
2. **A fixed canvas**: the declared `outline` when there is one, else the board
   estimate from `W-SYNTH-BOARD-004`. The estimate is the size the parts need,
   not the size the greedy search happened to build.
3. **The hint levels** on that canvas: as written, every hard hint demoted to
   soft, every hint dropped except a connector's edge, then every hint
   dropped. Each level reports exactly which hints it relaxed.
4. **Canvas growth** from the fixed size in 1.2× steps to a ceiling of 2×, and
   never past 300 mm a side. The estimate is tried before the growth when the
   declared outline is smaller than it.

A connector's `edge` is where the enclosure puts it, so it stays hard through
step 3. Only the last level drops it, and the diagnostic then names it.

Every attempt is bounded by a wall-clock budget (60 s) and an attempt ceiling
(21 solves). The budget is checked between attempts: one solve cannot be
interrupted, so a board that places slowly can overrun it.

### Severities

| Result | Severity |
| --- | --- |
| Placed as written, review passes | info, with the placed board size |
| Placed after relaxing a hint or growing the canvas | warning, naming every relaxed hint, every canvas tried and the final size |
| Placed, but the visual review requires revision | info, with the revision named |
| No placement on any canvas, hint level or growth step inside the budget | error `E-SYNTH-PLACE-002`, naming the part, its hint, the canvases tried and the size the parts need |
| The budget ran out before the ladder finished | warning: the board was not proven either way |

An error means the ladder exhausted every combination it was allowed, not that
a mathematical proof exists.

## Test points

**Test points are placed after all other parts, nearest free spot to their
net, never a hard failure.** A wired `testpoint` is not a feasibility
constraint. It stays in the packing order while it fits, so a board that
already places comes out exactly as it did before; when the solve fails, it
retries with every wired test point removed, then places them after the
circuit and the board features. The deferred pass searches the whole usable
rectangle, ordered by pad-to-pad distance to the pins on the probe's own net,
on the top side first and the bottom side second; a through-hole part or a
mounting hole blocks both sides, and so does every keepout. An explicit
`near`, `edge` or `region` hint is kept: the search runs inside the hint's
hard region first. When neither side has a free cell the nearest cell is used
anyway and the review warns, naming the probe. The probe stays in the
schematic and the BOM; only its position was deferred, so dropping it would be
worse than a courtyard collision the author can see.

The limit is plain: late placement shortens the stub, but it does not make the
route pass the point. The probe is still a pad on its net, and a router that
never reaches that pad leaves it unconnected. Inserting the probe on already
routed copper is the better method and is a later step. `test_point_stubs`
reports each probe's pad-to-pad distance as the stub-length proxy.

## The sidecar

`synth validate` resolves `<design>.placement.layout.toml` (or the deprecated
shared name) the same way `synth place` and the export do, and reports two
placements when one exists:

- **the sidecar placement**, with its size, dead area, gate failures, review
  verdict, and how many parts the sidecar pins. A stale sidecar that omits
  newly added parts says so; those parts are solved from scratch.
- **a from-scratch placement**, through the same bounded ladder, so a poor or
  stale floorplan is visible before an export depends on it.

The from-scratch run ignores the sidecar entirely. If the sidecar cannot be
applied at all, its diagnostic is a warning and the from-scratch result decides
the verdict.

## `place_with_replace`

The RePlAce seam used to build its fallback with `place_with_export_sweep`
before it tried anything, and returned that fallback immediately when the board
carried a hard `near` hint. A hard-hinted board therefore never reached the
solver: the sweep's own failure was the seam's answer.

The order is now the one the name promises. RePlAce runs first, with the hints
as written. The export sweep is built only when the solve is refused, and if
the sweep cannot pack the board either, the bounded legal ladder runs as the
last resort. `SYNTH_PLACE_REPLACE=1` reaches the solver on hard-hinted boards.
When the default arrangement cannot pack the board at all there is no seed for
RePlAce: the relaxed fallback is returned directly, because it is already the
legal placement the caller asked for, and an oversized seed only spends minutes
in a solver whose legalizer then refuses it.

## What the preflight does not do

- It does not replace the visual review, the judge or the fitness report; it
  only reports their verdict.
- It does not prove routability. A legal placement is a packing result.
- It does not choose a sidecar. It reports both placements; the author decides.


## Measured

**2026-11-25, code `ef8e8492`, synth 0.1.4, rustc 1.100.0 (0ef1bc386 2026-11-09),
cargo 1.100.0 (3acc79c0c 2026-11-05), kicad-cli 10.0.7.** The board is the
committed CM4 carrier from `origin/syn-407-rebased` (c991a084), extracted with
`git archive` into a scratch directory and run with the `synth` built from the
commit above. The branch worktree was not touched. The design carries ten
pre-existing ERC errors (nine `E-SYNTH-COMP-002` undefined pin, one
`E-SYNTH-PROCESS-002`), so `synth validate` exits 1 in both runs; none of them
is a placement diagnostic.

### Validate, with and without the sidecar

| Run | Preflight diagnostics | Time |
| --- | --- | ---: |
| sidecar present | warning `W-SYNTH-PLACE-004` "placement sidecar": 108.4 × 81.3 mm (8 809 mm²), dead area 5.7 %, 112 gate failures, pins 184 of 184. info `W-SYNTH-PLACE-004` "placement preflight": 169.6 × 169.6 mm (28 758 mm²), dead area 43.4 %, 97 gate failures, 6 solves, canvases 141.3 × 141.3 (four times) then 169.6 × 169.6, 60 s budget, review requires revision | 185.7 s |
| sidecar absent | info `W-SYNTH-PLACE-004` "placement preflight": the same 169.6 × 169.6 mm line; no sidecar line | 152.9 s |

The estimate is 141.3 × 141.3 mm (19 971 mm²), the #513 number. The four
141.3 mm attempts are the four hint levels; the fifth attempt is the 1.2×
canvas, where the placement succeeds with the hints as written.

### Placements

| Placement | Board (mm) | Area (mm²) | dead_area_ratio | gate_failures | Review | Time |
| --- | --- | ---: | ---: | ---: | --- | ---: |
| committed manual floorplan (sidecar, 184/184 pinned) | 108.38 × 81.28 | 8 809.1 | 0.0572 | 112 | requires revision | 45.2 s |
| from-scratch, legal ladder | 169.58 × 169.58 | 28 758.2 | 0.4364 | 101 | requires revision | 29.5 s |
| committed PCB Edge.Cuts (reference) | 100.0 × 80.0 | 8 000.0 | — | — | — | — |

The from-scratch board is 3.26× the manual floorplan's area and 7.6× its dead
area share. The ladder did not relax a hint (`level AsWritten`, `dropped []`);
it grew the canvas from 141.3 to 169.6 mm. That is a legal placement, not a
better floorplan.

`place_with_replace` with `SYNTH_PLACE_REPLACE=1`: the default baseline fails
on `D_ETH2` at 148 × 108 mm, the relaxed fallback is returned with
`fell_back = true`, and the result is the same 169.58 × 169.58 mm placement
(28 758.2 mm², dead 0.4364, 101 gate failures, review requires revision). Time
226.0 s, most of it the export sweep inside the fallback. RePlAce itself is not
seeded because there is no baseline arrangement to seed it from.

### Test points

From-scratch placement, where the late probe pass runs. All eleven probes are
on the top side, all found a free cell, and no probe was placed at the
nearest-cell fallback.

| TP | nearest pad | mm | side | free cell |
| --- | --- | ---: | --- | --- |
| TP_5V | `C_HDMI_VCCB.p1` | 13.90 | Top | yes |
| TP_3V3 | `R_VBUS.p1` | 7.40 | Top | yes |
| TP_1V8 | `C_OUT8.p1` | 20.50 | Top | yes |
| TP_CM4_3V3 | `J_CM4A.p86` | 27.44 | Top | yes |
| TP_CM4_1V8 | `J_CM4A.p90` | 34.51 | Top | yes |
| TP_RUN_PG | `J_CM4A.p92` | 27.00 | Top | yes |
| TP_NEXTRST | `J_CM4A.p100` | 20.00 | Top | yes |
| TP_GLOBAL_EN | `C_GEN.p1` | 7.39 | Top | yes |
| TP_PG5V | `U1.pg` | 31.73 | Top | yes |
| TP_PG3V3 | `R_PG2.p2` | 27.79 | Top | yes |
| TP_PG1V8 | `R_PG3.p2` | 7.17 | Top | yes |

Sidecar placement: the sidecar pins all 184 parts, so these are the manual
positions. `TP_5V` is the one probe the review reports as having no free spot.

| TP | nearest pad | mm | side | free cell / review |
| --- | --- | ---: | --- | --- |
| TP_5V | `D_HDMI5.vcc` | 27.70 | Top | review: no free spot (sidecar-pinned) |
| TP_3V3 | `U4.vdda33_3` | 4.87 | Top | yes |
| TP_1V8 | `L3.p2` | 19.74 | Top | yes |
| TP_CM4_3V3 | `J_CM4A.p86` | 30.22 | Top | yes |
| TP_CM4_1V8 | `J_CM4A.p90` | 31.67 | Top | yes |
| TP_RUN_PG | `J_CM4A.p92` | 53.48 | Top | yes |
| TP_NEXTRST | `J_CM4A.p100` | 45.42 | Top | yes |
| TP_GLOBAL_EN | `R_GEN.p2` | 42.59 | Top | yes |
| TP_PG5V | `R_PG.p2` | 30.13 | Top | yes |
| TP_PG3V3 | `R_PG2.p2` | 29.49 | Top | yes |
| TP_PG1V8 | `R_PG3.p2` | 31.36 | Top | yes |

The stub lengths are a proxy: late placement shortens the stub but the router
still has to reach each pad. `TP_RUN_PG` at 53.48 mm in the sidecar and
`TP_CM4_1V8` at 34.51 mm from scratch are the two longest.

### Test counts

The full scoped suite (`cargo test --no-fail-fast -p synth-place -p
synth-validate -p synth-cli`) ran 736 tests: 736 passed, 0 failed, 14 ignored
across 66 targets — synth-cli 157, synth-place 336, synth-validate 243. The
14 ignored are the pre-existing `#[ignore]` report tests. Two later changes
(the 60 s budget and the unseeded-RePlAce return) were re-run on their own
targets: `synth-place --test replace` 4, `synth-place --lib legal::` 4,
`synth-cli --test placement_preflight` 4, `synth-validate --test hard_hints`
12, all passing.

`cargo fmt --all -- --check` and `cargo clippy -p synth-place -p synth-validate
-p synth-cli --all-targets -- -D warnings` are clean.

### Golden hashes and CI

No golden was regenerated. The placement snapshots
(`crates/synth-place/tests/snapshots.rs`, 10 passed) and the board-estimate
fitted-dimension tests (`crates/synth-validate/tests/board_estimate.rs`, 18
passed) are unchanged; the three unstable `place()` boards have no test
points, so the probe deferral does not move them. `fixtures/kicad-reference/`
and `qualification/**/SHA256SUMS` are untouched. The `synth-kicad` export
tests (`replace_seam_export`, `placement_hints_pcb`) are outside the scoped
command and were not re-run; the export code is unchanged. CI
(`.github/workflows/ci.yml`) is unchanged: the same fmt, clippy and test
commands, run here scoped to the three touched crates.

## See also

- [`docs/diagnostics/W-SYNTH-PLACE-004.md`](diagnostics/W-SYNTH-PLACE-004.md)
- [`docs/diagnostics/E-SYNTH-PLACE-002.md`](diagnostics/E-SYNTH-PLACE-002.md)
- [`docs/diagnostics/W-SYNTH-BOARD-004.md`](diagnostics/W-SYNTH-BOARD-004.md)
  and `W-SYNTH-BOARD-005`: the estimate and the hard hints the ladder relaxes.
