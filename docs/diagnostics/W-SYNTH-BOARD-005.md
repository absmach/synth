# W-SYNTH-BOARD-005 — hard placement hint fixes the board span

**Severity:** warning
**Stage:** validate — placement hints

## What this means

A component declares `priority: hard`, which the placer may not relax. A
board-edge connector (`connector`, `jack`, `receptacle`) with an `edge` hint
and a mounting hole held by an `edge` or `region` hint are physical: the
enclosure fixes them. A hard `near` is electrical: it binds a part to its
anchor (a decoupling capacitor at its supply pin, the switch loop at a
regulator, a crystal at its oscillator, ESD at its connector, a series
termination), and the cluster can still move with the board, so it is not
reported.

What blocks compaction is a hard `edge` or `region` hint on any other part, or
a hard `near` chain that ends in one: it pins the cluster against a board side.
This warning reports those hints.

A `receptacle` edge counts as physical here even though the placer's flush fit
currently recognizes `connector` and `jack` only, so a receptacle may sit
slightly inset from the edge it is pinned to.

Measured on the gateway (120 parts, fitted board, `place_with_export_sweep`):
with the hints as written it fits 157.4 × 113.4 mm (17 849 mm²). Making the 38
hints this warning reports soft — U4's centre region, U5's and U6's bottom-left
regions, U7's right edge, and the 34 hard `near` chains that end in them — fits
the same 157.4 × 113.4 mm (17 849 mm²), so on this board the span-fixing hints
change no board span (0 % area change). An earlier measurement that softened
every non-physical hard hint also relaxed the electrical `near` hints at pins,
which this warning now leaves hard.

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

Make the span-fixing hint `soft` unless an enclosure or mating constraint fixes
the part:

```synth
placement_hint { region centre priority: soft }
```

Keep `priority: hard` for a connector edge, a mounting hole's corner, and any
part that must sit at a pin (decoupling, the regulator's switch loop, a
crystal, ESD at its connector, series terminations).

## See also

`W-SYNTH-BOARD-004` (declared outline larger than the parts need).
