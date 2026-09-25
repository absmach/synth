# E-SYNTH-SCHEM-011 — overlapping text runs

**Severity:** warning
**Stage:** schematic aesthetic ERC

## What this means

Two free-text runs on the sheet (group captions, design-note lines, connector-legend lines) still overlap after the layout `resolve_text_overlaps` pass, which nudges runs apart, shrinks line runs one step, and drops only the lowest-priority lines.

## Minimal reproduction

Two long note titles authored at the same position, in a layout too crowded for the pass to separate them.

## Suggested fix

Move one of the runs (add a `notes` block to a different group, or shorten the text). Reference/value labels are placed by the exporter after layout — if the overlap involves those, move the component instead.
