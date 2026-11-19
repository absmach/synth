# E-SYNTH-BOARD-003 — duplicate outline statement

**Severity:** error
**Stage:** lowering

## What this means

The board declares a second `outline` statement. A board has one exact
rectangle; the first declaration stands and the second one is refused, so the
exported Edge.Cuts can never depend on statement order.

A first `outline` also survives a later one whose width or height fails its
unit check: a bad literal reports its own unit error and does not undo the
size the board already declared.

## Minimal reproduction

```synth
board "x" {
  layers 2
  outline 60mm x 30mm
  outline 80mm x 40mm
}
```

## Suggested fix

Keep one `outline` statement and delete the other. If the two sizes disagree,
decide which one is the board and write only that one.
