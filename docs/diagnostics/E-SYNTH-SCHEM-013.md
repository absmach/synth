# E-SYNTH-SCHEM-013 — group regions overlap

**Severity:** warning
**Stage:** schematic aesthetic ERC

## What this means

The region-based placement (Phase C1) failed to keep groups apart — either two group boxes overlap, or a component declaring one group physically sits inside another group's box. A caption would then title another region's parts.

## Minimal reproduction

A sidecar `layout.toml` dragging a component from one group into another group's area.

## Suggested fix

Remove the conflicting sidecar override, or move the component back among its own group's parts. The placement pass keeps declared groups contiguous automatically; only manual drags can break it.
