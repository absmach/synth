# E-SYNTH-SCHEM-012 — sheet fill ratio below threshold

**Severity:** info
**Stage:** schematic aesthetic ERC

## What this means

The content bounding box covers less than 45% of the chosen sheet's area — the sheet is roomier than the design needs (e.g. content in the top half of an A3 page with the bottom half empty).

## Minimal reproduction

A two-component board placed on an A3 sheet.

## Suggested fix

Nothing is wrong — this is advisory. If the named smaller sheet fits, the exporter already compacts onto it; otherwise tighten the layout (fewer, squarer cluster columns) so the page reads uniformly dense.
