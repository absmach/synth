# W-SYNTH-BOARD-005 — hard placement hint on a non-physical part

**Severity:** warning
**Stage:** validate — placement hints

## What this means

A component declares `priority: hard`, which the placer may not relax. A
connector edge and a mounting-hole corner are physical: the enclosure fixes
them. A hard `near`, `region` or `edge` on any other part pins it without a
mechanical reason and blocks the compact placement.

Measured on the gateway board: with the hints as written the export fit
157.4 × 113.4 mm; with the 90 non-connector, non-mounting-hole hard hints made
soft it fit 118.0 × 115.5 mm, 31 % less area.

The warning names the hint and, for an `edge`/`region` hint or a `near` chain
ending in one, the distance it pins the part from the far side. It also names
the board size the parts need.

## Minimal reproduction

```synth
board "hint" {
  layers 2
  component U1: ic "stm32g030k8_lqfp32" {
    placement_hint { region centre priority: hard }
  }
}
```

## Suggested fix

Make the hint `soft` unless the part must stay where it is:

```synth
placement_hint { region centre priority: soft }
```

Keep `priority: hard` only for a connector edge (`edge left`) or a mounting
hole's corner.

## See also

`W-SYNTH-BOARD-004` (declared outline larger than the parts need).
