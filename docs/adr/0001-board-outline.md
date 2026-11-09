# 0001 — A declared outline replaces the fitted board edge

**Status:** accepted (2026-11-04)

## Context

The exporter always fitted Edge.Cuts round the placed courtyards and added a
per-board margin. On a dense board that margin is 4 mm, and a board feature such
as a mounting hole kept a 2.2 mm rim of its own. A mechanical enclosure could
therefore not pin a board's size: moving a mounting hole toward the edge only
moved the fitted edge with it, and an edge connector such as J_B2B could not sit
flush. The only escape was to edit the generated `.kicad_pcb`, which the next
export silently destroys.

## Decision

A board-level statement `outline <width> x <height>` fixes the board rectangle.
Its bottom-left corner is board coordinate `(0, 0)`. When present:

- Edge.Cuts is exactly the declared rectangle. Placement does not fit, tighten
  or compact the outline, and the export does not run its margin/seed sweeps.
- Placement-sidecar coordinates are used as written. A part that does not fit
  is a placement error, not a reason to grow the board.
- A `mounting_hole` may overhang the rectangle as long as its drill stays
  inside, so a hole can sit on the edge.
- The four-layer ground-plane fence moves from the corners to the top and
  bottom edge midpoints. Corner vias landed beside corner mounting holes and
  read as a second hole; one through via already ties every ground layer.

Boards without the statement keep the historical fitted behaviour.

## Consequences

- Board size is a source-level contract and can be reviewed like any other.
- Two boards that mate can declare the same interface coordinates.
- Placement inside a declared rectangle can fail; the failure is reported
  instead of being absorbed by a larger board.
- The per-board margin constants still govern boards without an outline.
- Non-rectangular outlines and cutouts are out of scope; a later statement can
  extend the form without renaming this one.
