# E-SYNTH-BOARD-001 — board declared with zero layers

**Severity:** error
**Stage:** erc — board

## What this means

The board's `layers` attribute is 0. A board needs at least one copper layer for any routing to occur.

## Minimal reproduction

```synth
board "x" { layers 0 }
```

## Suggested fix

Set `layers` to 1 (single-sided) or higher.
