# E-SYNTH-PLACE-002 — no legal position for component

**Severity:** error
**Stage:** placement (Phase 7)

## What this means

The placer's greedy first-fit constraint solver could not find
any grid cell on the chosen board that accommodates the named
component without its courtyard overlapping a previously-placed
component's courtyard. The diagnostic reports the component
refdes, the board size that was considered, and the number of
grid positions tried before giving up.

This is a stricter form of `E-SYNTH-PLACE-001`: there *is*
enough total area, but the ordering heuristic (net-degree
descending) boxed the placer in before the offending component
got its turn. Common when a high-degree IC packs into the
top-left of the board and a later large component (DIP-28,
through-hole connector) can no longer fit.

## Minimal reproduction

```synth
board "boxed_in" {
  // Three connectors that each need ~25 × 6 mm courtyard,
  // placed first because they're high-degree; then an MCU
  // that needs a ~25 × 36 mm DIP-28 courtyard but the
  // remaining columns are only 30 mm wide.
  // ...
}
```

## Suggested fixes

1. **Pin the parts in a placement sidecar.** An absolute entry in
   `<design>.placement.layout.toml` is a constraint the solver is given,
   not a correction applied to its answer: a pinned component is placed
   where the author put it, its courtyard is occupied before the greedy
   search takes its first step, and it is removed from both the search
   and the passive packer. Pinning carries no passive clearance either —
   the 1.5 mm the search reserves around a loose passive is a spacing
   choice, and on a pinned part the author has already made it. Pinning
   every part therefore always places, which is the escape hatch for a
   board the greedy search cannot pack.
2. **Try the next-larger board.** The bounded ladder already grows the canvas
   from the estimate in 1.2× steps, so the first sizes it tried are in the
   diagnostic. A hand-picked `outline` larger than the reported `expected`
   size is the manual version of the same move; it is authoritative once
   declared (ADR 0001).
3. **Choose a smaller footprint variant** for the offending
   component. Swapping `r_generic_0805` → `r_generic_0603` on
   the closest neighbours often frees enough adjacent space.
4. **Wait for slice 2.x backtracking.** The current placer is
   greedy first-fit; it commits to each placement and never
   un-commits. Slice 2.x adds backtracking + connector / RF /
   fixed-component ordering that resolves boxed-in scenarios
   automatically. Until then, the diagnostic's `tried` count
   is your hint of how badly stuck the placer was.

## What's in the diagnostic

- **Code:** `E-SYNTH-PLACE-002`
- **Severity:** error
- **Location:** the source span of the offending component's
  declaration, so an agent can jump directly to the line to
  patch.
- **Message:** "After N grid-cell attempts on a W × H mm
  board, no position accommodates `<refdes>`..."

`synth validate` emits it from the placement preflight only with
`--placement-preflight` (the default stays cheap). From the preflight, the message
carries the rest of the evidence: every canvas the bounded ladder tried (the
declared outline, the board estimate, the grown sizes), the hand hint the
ladder could not relax (`placement_hint { … }`), the number of solves, and the
size the parts need from `W-SYNTH-BOARD-004`. An `error` from the preflight
means the ladder exhausted every canvas and hint-level combination inside its
budget, so the failure is proven rather than assumed. A budget cut-off is
reported as a warning instead, because the absence of a placement was not
established.

The preflight's ladder (`docs/placement-preflight.md`) relaxes hints and grows
the canvas before it reaches this error, so a board that only needs more room
or a soft hint is reported as `W-SYNTH-PLACE-004`, not as this failure.
