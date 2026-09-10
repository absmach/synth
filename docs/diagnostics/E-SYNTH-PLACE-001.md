# E-SYNTH-PLACE-001 — placement area insufficient

**Severity:** error
**Stage:** placement (Phase 7)

## What this means

The total footprint courtyard area of every component in the board
exceeds the largest board size the placer supports. No standard
sheet can hold the design at the configured fill ratio (45% of
board area allocated to component courtyards, 55% reserved for
routing channels per plan §9.2).

The placer refuses to return a "best effort" partial placement —
when no legal placement exists, it returns this diagnostic
instead, per the compiler-correctness contract.

## Minimal reproduction

A board that declares so many components (or such large
footprints) that even the maximum standard sheet (230 × 230 mm,
~53000 mm²) cannot fit them at 45% fill:

```synth
board "too_big" {
  // 1000 generic resistors at ~26 mm² courtyard each →
  // ~26000 mm² of components, which at 45% fill needs
  // ~58000 mm² of board — beyond the 53000 mm² max.
  // ...
}
```

## Suggested fixes

The placer surfaces this as a structured diagnostic so an agent
or human can pick a remediation:

1. **Shrink footprint variants.** A `c_generic_0805` cap has
   ~6.9 × 3.8 mm courtyard; the same-value `c_generic_0402`
   variant is ~2.0 × 1.7 mm — almost 8× smaller. Substituting
   smaller passive packages reclaims most of the area.
2. **Split across sub-boards.** Use SynthSpec `import` to
   break the design into independently-placed sub-boards
   connected by board-to-board headers. Each sub-board
   places independently.
3. **Declare an explicit `board_outline`** larger than the
   standard sheets the placer considers. (Future work: the
   placer will accept a user-declared outline up to JLC's
   special-order sizes.)

## Why this is an error, not a warning

A placement that doesn't include every component is not a
placement KiCad will export and DRC will accept. Returning a
partial placement would propagate "valid-looking but missing
parts" through the pipeline and surface as a fab-time mismatch
between the BOM and the actual board. The diagnostic catches
the problem at the earliest point — placement — so the agent
can patch the source before any artifact is written.
