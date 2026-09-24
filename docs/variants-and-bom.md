# Variants and BOM data

Phase 7. Two things that make the BOM more than a parts list: named
build **variants** (same board, different population) and **structured
component values** (tolerance, voltage/power rating, dielectric) that
turn into KiCad symbol fields a reviewer or a check can read.

## Design variants

### Syntax

```synth
board "hub" {
  component U3: sensor "bme680_env"
  component U4: sensor "bmp280_pressure"

  variant "lite" description "pressure only" {
    dnp U3
  }
}
```

A `variant "NAME"` block lists the refdes left unpopulated in it, with
an optional `description`. The schematic and layout are shared; only the
population differs. Everything is board-scope (a variant may also sit in
a `group` or `sheet`).

### Mapping to KiCad 10

KiCad 10 has native design variants, split across two files:

- **`.kicad_pro`** → `schematic.variants` is an array of
  `{ "name", "description"? }` — the authoritative *list* (this is what
  the GUI picker and `kicad-cli` enumerate).
- **`.kicad_sch`** → each affected symbol's
  `(instances (project "…" (path "…" (reference "…") (unit N)
  (variant (name "…") (dnp yes) …))))` block carries the per-symbol
  *override*. Only symbols that differ are written.

Both schemas were taken from KiCad's source (`SCHEMATIC_SETTINGS`'s
`m_VariantDescriptions` serializer for the project file, and the
schematic parser/writer for the symbol block), and verified against
`kicad-cli sch export bom --variant lite`, which marks the DNP part.

The variant block lives in the symbol's `(instances …)` block, which a
single-sheet export otherwise omits. Synth only adds that block to
symbols a variant touches, so a design with no variants stays
byte-identical to before.

### BOM output

The base build is `bom.csv`. Each declared variant also gets
`bom.<variant>.csv` with the variant's do-not-populate overrides applied
on top of the component `dnp` flags. KiCad's own BOM export reads the
same data through `--variant`.

Validation: `E-SYNTH-VARIANT-001` (duplicate name) and
`E-SYNTH-VARIANT-002` (unknown refdes). Field/value overrides within a
variant are deferred — only `dnp` is supported today.

## Structured component values

### Syntax

```synth
component C1: capacitor "c_generic_0603" value "100nF" tolerance "10%" voltage "25V" dielectric "X7R"
component R1: resistor  "r_generic_0603" value "10k"  tolerance "1%"  power_rating "0.1W"
```

The fields may appear in any order, before and/or after a `{ … }` body,
alongside `dnp` and `placement_hint`. They map to canonical KiCad field
names (`Tolerance`, `Voltage`, `Power`, `Dielectric`) and are emitted as
**hidden symbol properties**, exactly like the existing `MPN`/`LCSC`
fields — so KiCad BOM tooling reads them from the schematic.

`power_rating` is spelled out rather than `power` because `power` is
already the rail-declaration statement keyword (`power "+3V3" 3.3v`);
the parser would otherwise swallow a following `power` line as a
component attribute.

### What the data unlocks

`E-SYNTH-CAP-001` (warning) uses the new fields: a Class-II ceramic
(X5R/X7R/X7S/Y5V/Z5U) on a rail above a configurable fraction (default
50 %) of its rated voltage is flagged, because DC bias cuts effective
capacitance. It fires only when the dielectric and voltage rating are
stated *and* the rail voltage is known; otherwise it declines rather
than guessing, and Class-I (C0G/NP0) parts are never flagged. The
threshold is `ceramic_dc_bias_threshold` in `<design>.synth.erc.toml`.

This is the check `docs/kicad-workflows.md` calls for under "Ceramic
capacitors: derate for applied DC bias" — it could not exist before the
values were in the source.

## Tests

- `synth-parser` unit tests: `parse_variant_block`,
  `parse_component_structured_values`.
- `synth-ir` integration tests (`tests/variants.rs`): a variant lowers
  with its DNP set; structured values land on the component.
- `synth-validate` tests: `E-SYNTH-VARIANT-001/002`,
  `E-SYNTH-CAP-001` (fires, stays quiet for C0G / low bias, and needs
  both fields).
- `synth-kicad` tests: the variant block and hidden fields are exported;
  a design without variants emits no variant block; the variant BOM
  applies its overrides.
