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

1. **Try the next-larger board.** The placer escalates
   through standard sizes (100×80, 160×100, 200×150, 230×230 mm)
   but a tight design may need a hand-picked larger outline.
   (Future work: the placer will accept a user-declared
   `board_outline` in SynthSpec.)
2. **Choose a smaller footprint variant** for the offending
   component. Swapping `r_generic_0805` → `r_generic_0603` on
   the closest neighbours often frees enough adjacent space.
3. **Wait for slice 2.x backtracking.** The current placer is
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
