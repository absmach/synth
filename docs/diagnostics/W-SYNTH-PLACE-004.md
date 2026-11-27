# W-SYNTH-PLACE-004 — placement preflight report

**Severity:** info on the default note and on a clean preflight; warning when the ladder relaxed a hint or grew the canvas, and when a placement sidecar is stale or poor
**Stage:** validate — placement preflight

## What this means

`synth validate` does not run the placer by default. When a board carries a
hard placement hint, the default run emits this code at info severity with
one line: run `--placement-preflight` to test that the board places. With that
flag (or `placement_preflight` on `synth_validate`) the real placer runs
through its bounded ladder and reports what it got. The diagnostic carries the board size, area, dead-area ratio and
gate failures of the placement, plus the attempts it took. When a hint had to
be relaxed or the canvas had to grow, the message names every relaxed hint and
every canvas tried, and the severity is warning rather than info.

The same code reports a placement sidecar when one exists: the size, dead
area, gate failures and review verdict of the sidecar placement, beside a
from-scratch placement. A sidecar that omits parts is named as stale, and those
parts are solved from scratch.

A from-scratch placement that is legal but still needs a visual revision stays
info and names the revision: the review is the review's job, not the
preflight's. The board can be placed; nothing here blocks an export.

## Minimal reproduction

```synth
board "tight" {
  layers 2
  outline 40mm x 70mm
  component J1: connector "header_1x20" {
    placement_hint { edge: top priority: hard }
  }
}
```

The hard edge forces the header horizontal, wider than the board. The ladder
drops the connector edge as a last resort and reports it:

```
warning: [W-SYNTH-PLACE-004] placement needs a relaxed hint or a larger canvas
```

## Suggested fix

Read the message. When it names a hint, decide whether the electrical or
mechanical reason still holds; if not, make it soft or remove it. When it
names a grown canvas, either accept the size or fix the hint that forced it.
When it reports a sidecar, compare it with the from-scratch placement and
update or delete the sidecar.

Speed is the default; add `--placement-preflight` when the packing itself is
in question.

## See also

`docs/placement-preflight.md` for the ladder, the budget and the sidecar
comparison.
