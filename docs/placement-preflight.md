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

## See also

- [`docs/diagnostics/W-SYNTH-PLACE-004.md`](diagnostics/W-SYNTH-PLACE-004.md)
- [`docs/diagnostics/E-SYNTH-PLACE-002.md`](diagnostics/E-SYNTH-PLACE-002.md)
- [`docs/diagnostics/W-SYNTH-BOARD-004.md`](diagnostics/W-SYNTH-BOARD-004.md)
  and `W-SYNTH-BOARD-005`: the estimate and the hard hints the ladder relaxes.
