# W-SYNTH-PART-UNVERIFIED — Component uses an unverified part

**Severity:** warning
**Stage:** erc — board

## What this means

Every part in the registry may carry a `provenance` block describing how it was
authored and whether it has been engineering-reviewed:

```toml
[provenance]
source = "seed"           # seed | generated | imported | authored
generator = ""             # tool/version that produced the entry, e.g. "synth-part-import-lcsc 0.1"
datasheet_url = ""          # also populates the exported symbol's `Datasheet` property
reviewed_by = ""            # empty => not yet reviewed
reviewed_at = ""            # ISO-8601 review date
upstream_lcsc_pn = ""       # set when the part came from EasyEDA/LCSC
```

A legacy part with **no** `[provenance]` section at all is treated as `source =
"seed"` for back-compat (shipped, core-team proof-read) and is trusted. A part
is **unverified** when it *does* carry a `[provenance]` section but
`reviewed_by` is empty — this covers `generated` (`synth part import lcsc`),
`imported` (`synth part import kicad`), and `authored` (`synth_author_part` /
`create_part_stub`) parts alike. The `UnverifiedPartRule`
(`W-SYNTH-PART-UNVERIFIED`) fires for each board component whose resolved part
is unverified.

This catches the common failure mode where a brand-new, hand-pinned part (e.g. a
very new module with no official KiCad footprint) is placed on a board and sent to
fabrication before anyone has eyeballed the pinout against the datasheet. It is
advisory only during interactive validate/preview and does **not** block
compilation there — but it **does** block a manufacturing export: `synth
export-kicad --gerbers/--drill/--step` refuses to proceed while any unverified
part is on the board, unless `--allow-unverified-parts` is passed (see
Gating below).

## Minimal reproduction

```synth
board "unverified_repro" {
  layers 2

  component U1: mcu "esp32_c61_mini"   // part has provenance.source = "authored", reviewed_by = ""
}
```

```console
$ synth registry doctor
W-SYNTH-PART-UNVERIFIED: part `esp32_c61_mini` has no reviewer

$ synth validate board.synth --format json
// ... { "code": "W-SYNTH-PART-UNVERIFIED", "title": "component uses an unverified part", ... }
```

## Suggested fix

1. Review the part's pinout and footprint against the authoritative datasheet.
2. Once confirmed, fill in `provenance.reviewed_by` (e.g. your initials or a
   ticket id) — the warning clears immediately on the next validate/doctor run.
3. For parts imported from a supplier (R15.4 LCSC/JLCPCB or R15.5 KiCad stock),
   `provenance.source` is already set to `generated`/`imported` by the importer;
   imported parts are still subject to review before their reviewer field is set.

To scaffold a new, correctly-structured placeholder part, use `synth part stub
<id> --pins <N>` / `create_part_stub(id, pins)` (exposed via the registry
loader), which emits a TOML skeleton with `provenance.source = "authored"` and
an empty `reviewed_by`, so the warning fires until the part is actually
reviewed. `E-SYNTH-COMP-001` (unknown part) offers this, plus
`synth_search_registry_web` and `synth_import_part`, as `suggested_actions` on
the diagnostic itself — see `docs/diagnostics/E-SYNTH-COMP-001.md`.

## Gating

- `W-SYNTH-PART-UNVERIFIED` is a `Severity::Warning` in category
  `ErcCategory::Board`. The reference agent harness treats it as advisory and does
  not stall on it during interactive validate/preview.
- `synth export-kicad` gates on it at the manufacturing boundary: schematic,
  PCB, and BOM files always export normally, but requesting fab artifacts
  (`--gerbers`, `--drill`, or `--step`) refuses with a non-zero exit and lists
  the offending part ids unless `--allow-unverified-parts` is also passed.
  This is the `--allow-unverified-parts` flag referenced in §18.8.2 of the
  implementation plan.
