# E-SYNTH-ROUTE-001 — unrouted net

**Severity:** error
**Stage:** routing (Phase 8)

## What this means

The router could not connect all of a net's endpoints with
copper traces. After the negotiated-congestion loop hit its
iteration cap (8 rounds per plan §10.2 Stage C), the named net
still had pad endpoints that no path on F.Cu could reach.

The router refuses to emit a partial trace that leaves an
electrically-required connection floating — the resulting board
would silently mis-fab. Returning this diagnostic is the
soundness contract.

## What's in the diagnostic

- **Code**: `E-SYNTH-ROUTE-001`
- **Severity**: error
- **Location**: the source span of the offending net's first
  endpoint, so an agent can jump directly to the `connect`
  statement that introduced it
- **Message**: includes the source/target pad coordinates of
  the segment that couldn't close. Slice 6 reports *a* witness;
  slice 6.x will extend with the *minimum* witness (the smallest
  set of components / keepouts whose removal would make the
  route exist, per plan §10.5).

## Why routes fail

Three structural reasons in V1, ordered by frequency:

1. **Board too crowded.** Component courtyards consume too much
   of the board area for the router to find channels.
   Mitigation: choose smaller footprint variants (0805 → 0603),
   or declare a larger `board_outline`.
2. **High-fanout net wants a ground plane.** GND and primary
   power rails on a 2-layer board are unrouteable as traces —
   they expect a copper *zone* on layer B.Cu. Phase 9's zone /
   pour engine handles them; until then, this diagnostic fires
   for every power net that doesn't fit a star topology.
3. **Placement isolated a component.** A connector or RF part
   landed in a position no other component can reach without
   crossing a forbidden zone. Mitigation: declare a constraint
   that fixes the connector's position to a board edge slot.

## Suggested fixes

Slice 6 carries no `Patch` payloads; routing patches require
modelling "relax constraint X" / "split net into bus + leaf"
as `PatchKind` primitives, which is post-V1 work. For now the
diagnostic is informational. The cheapest manual fixes:

- Add `layers 4` to the board declaration to give the router
  two more routing planes.
- Set a larger explicit `board_outline` to relieve crowding.
- Identify high-fanout power nets in the message body; Phase 9
  zone pours will route them automatically.

## Why this is an error, not a warning

A board with unrouted nets is electrically broken. The router
could silently emit "best effort" traces that miss the
unrouteable pads, but that would propagate a wrong board
through `kicad export → gerber → fab` without any other stage
catching it. The diagnostic stops the pipeline at the earliest
point where the failure is visible.
