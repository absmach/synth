# E-SYNTH-BOARD-002 — board has no components

**Severity:** warning
**Stage:** erc — board

## What this means

The board declares no `component` statements. Either the file is a work-in-progress shell, or the components were removed and not yet replaced.

## Minimal reproduction

```synth
board "x" { layers 2 }
```

## Suggested fix

Add at least one component to the board.
