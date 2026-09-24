# E-SYNTH-UNIT-002 — registry unit tags disagree with the symbol's units

**Severity:** error (more tags than the symbol has units) / warning (tags without a stock symbol)
**Stage:** erc — naming

## What this means

A part's registry `unit` tags (e.g. `"A"`, `"B"` for a dual op-amp) promise
more units than its KiCad symbol declares via its
`<Symbol>_<unit>_<style>` sub-symbols — or the part carries several unit
tags but has no `kicad_symbol` mapping at all.

The exporter stacks one placed symbol per unit from the *symbol's*
inventory; the registry tags are only grouping metadata for
`E-SYNTH-NAME-010`. When the two disagree, the schematic and the
rail-split check reason about different packages.

## Minimal reproduction

(a part tagging pins `A`/`B`/`C` whose symbol declares 2 units)

```synth
board "x" {
  component U1: opamp "triple_tagged_dual_symbol"
}
```

## Suggested fix

Fix the registry `unit` tags, or point the part at the correct
multi-unit symbol. A tagged part with no stock symbol exports as a single
unit today — give it a multi-unit `kicad_symbol` to get per-unit bodies.
