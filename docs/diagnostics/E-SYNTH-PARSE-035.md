# E-SYNTH-PARSE-035 — unknown schematic page size

**Severity:** error
**Stage:** parse

## What this means

A `schematic { paper = … }` block named a page size that is not one of the standard sizes. The value is matched case-insensitively and surrounding whitespace is ignored, so `"a3"` and `" A3 "` are both accepted; anything else is not a page.

Accepted values are `A5`, `A4`, `A3`, `A2`, `A1` and `A0`. Omitting the block entirely is the same as `paper = "A4"`.

## Minimal reproduction

```synth
board "x" {
  schematic { paper = "A9" }
  component U1: regulator "ams1117_3v3"
}
```

## Suggested fix

```synth
schematic { paper = "A4" }
```

The auto-fix replaces the value with `A4`, the default, which fits most designs.

Note that the requested page is a starting point rather than a hard limit: if the content does not fit, the layout enlarges the page (see `overflow` in `schematic-procedures.md`). A5 is therefore honoured as asked, but a design whose content exceeds it is drawn on the next size up rather than being clipped. `A5` is also never chosen automatically — the content fitter starts at `A4`, because the placer ranks candidate pages smallest-first and an `A5` rung there would re-lay-out every existing design onto a more cramped sheet.
