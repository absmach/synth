# W-SYNTH-BOARD-005 — hard placement hint on a non-physical part

**Severity:** warning
**Stage:** validate — placement hints

## What this means

A component declares `priority: hard`, which the placer may not relax. A
board-edge connector (`connector`, `jack`, `receptacle`) with an `edge` hint
and a mounting-hole corner are physical: the enclosure fixes them. A hard
`near`, `region` or `edge` on any other part pins it without a mechanical
reason and blocks the compact placement.

A `receptacle` edge counts as physical here even though the placer's flush fit
currently recognizes `connector` and `jack` only, so a receptacle may sit
slightly inset from the edge it is pinned to.

Measured on the gateway board: with the hints as written the export fit
157.4 × 113.4 mm; with the 90 hard hints on parts that are not edge connectors
or mounting holes made soft it fit 118.0 × 115.5 mm, 23.6 % less area. The
hinted board is 31 % larger by area.

The warning names the hint and, for an `edge`/`region` hint, the distance it
pins the part from the far side. For a `near` chain it names the part the
chain follows and that part's distance. It also names the board size the parts
need.

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
