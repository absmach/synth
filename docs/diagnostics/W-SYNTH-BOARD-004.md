# W-SYNTH-BOARD-004 — declared outline is far larger than the parts need

**Severity:** warning (info when no outline is declared)
**Stage:** validate — board estimate

## What this means

Every component contributes its footprint courtyard. Summed at the 26 % fill
that sets the fitted square (the placer's 45 % packing target leaves less room
for routing channels), and widened by the export's tight edge rim, that is
roughly the board the parts get when placement fits the outline. A declared
`outline` whose area is more than 1.5× that estimated area leaves the
difference as dead board: unused copper, a larger panel slot, and a size no one
computed. The rectangle is authoritative whatever its size (ADR 0001); the
oversize only wastes board, it does not cause fitting to stop.

When the source declares no `outline`, the same code is emitted at info
severity with the estimate, so a size can be checked before it is pinned.

The estimate assumes a compact fit. A hard `edge` or `region` hint on a part
with no mechanical reason, or a `near` chain ending in one, fixes a board span
and can enlarge the fitted board: `W-SYNTH-BOARD-005` reports those hints, and
the estimate deliberately names the compact size they would otherwise allow.

## Minimal reproduction

```synth
board "gateway" {
  layers 4
  outline 180mm x 120mm
  component U1: ic "stm32g030k8_lqfp32"
  component R1: resistor "r_generic_0603"
}
```

## Suggested fix

Remove the `outline` statement and let placement fit the board to its parts,
or size the outline from the estimate in the diagnostic (`expected`).

Keep a declared outline when a mechanical or mating constraint fixes the size,
and state that reason on the board:

```synth
outline 180mm x 120mm reason "fits the enclosure"
```

A non-empty reason suppresses this warning.

## See also

`docs/adr/0001-board-outline.md` (a declared outline replaces the fitted edge).
