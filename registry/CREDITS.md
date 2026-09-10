# Registry credits & licensing

This file records the licensing posture of every upstream source that
contributes to the Synth component registries. It exists so that the
provenance of shipped part data (`registry/parts/**`, the "Tier-1"
registry) is auditable, per Phase 15 / R15.11.

## KiCad stock libraries — CC-BY-SA 4.0

Parts created via `synth part import kicad <lib_id>` derive their pin
inventory from the KiCad project's stock symbol libraries
(`kicad-symbols`, `kicad-footprints`), which are licensed under
**Creative Commons Attribution-ShareAlike 4.0**.

Synth is Apache-2.0. CC-BY-SA 4.0 §4(b) requires that adapted material
carry the same license; the practical consequences for this repository
are:

- Part entries whose pin data was extracted from KiCad stock libraries
  are attributable to the KiCad project and its contributors
  (<https://github.com/KiCad/kicad-symbols>, <https://github.com/KiCad/kicad-footprints>).
- The attribution in this file satisfies the credit requirement for the
  aggregated registry; the ShareAlike obligation attaches to the derived
  pin tables themselves. If you redistribute a registry containing
  KiCad-derived entries, keep this file (or an equivalent notice) with
  them.
- No KiCad artwork (2D/3D footprint graphics) is copied into Synth; only
  the factual pin inventory (numbers, names, electrical types) is
  recorded in `*.synth.toml`.

## LCSC / EasyEDA — reference only, no redistribution

Parts created via `synth part import lcsc <C-number>` are converted at
import time from the component CAD document LCSC/EasyEDA serves for that
part.

- **Raw LCSC/EasyEDA JSON or CSV responses are never committed to this
  repository.** Only the derived, clean-room-converted
  `<id>.kicad_mod` / `<id>.synth.toml` artifacts land here.
- For bulk part metadata (stock, pricing), link to the community
  JLCPCB parts mirror (<https://github.com/yaqwsx/jlcparts>) instead of
  redistributing LCSC data.
- See the clean-room attestation in
  `crates/synth-registry/src/easyeda.rs`.
- The **JLCPCB Parts Library** (<https://jlcpcb.com/parts>) is not a
  separate source: JLCPCB is LCSC's sister company and every listing
  uses the identical C-number ("JLCPCB Part #, formerly also LCSC Part
  #" per their own documentation). `synth part import lcsc` already
  covers it — there is no separate JLCPCB import path.

## SnapEDA / UltraLibrarian — file-only, never contacted directly

`synth part import kicad-zip` (CLI) / `synth_import_part` with
`source: "kicad-zip"` (MCP) reads a `.kicad_sym` + `.kicad_mod` pair out
of a zip the _user_ already downloaded through their own browser
session by choosing "Export to KiCad" on either site.

- **Synth never contacts snapeda.com or ultralibrarian.com.** Neither
  offers a public, self-serve API, and both Terms of Service
  explicitly prohibit automated/scripted access: SnapEDA forbids "any
  automated means—including robots, scrapers, crawlers, spiders" and
  separately forbids building "services substantially similar to the
  Site"; UltraLibrarian forbids "any robot or other automated means to
  access or gather content from the Website." A live importer for
  either would require a signed partnership agreement, which Synth
  does not have.
- The zip parser (`crates/synth-layout/src/kicad_zip.rs`) is a
  clean-room reader written from each vendor's own published _import_
  documentation (e.g. SnapEDA's guidance to extract the zip as-is and
  keep the folder structure intact) plus generic ZIP/`.kicad_sym`
  format knowledge — same posture as the LCSC/EasyEDA converter's
  relationship to `easyeda2kicad.py` above. It does **not** derive
  from [`Import-LIB-KiCad-Plugin`](https://github.com/Steffen-W/Import-LIB-KiCad-Plugin)
  (Steffen-W, GPL-3.0), the existing community plugin that imports
  these same SnapEDA/UltraLibrarian zips into KiCad — no code or
  internal structure is copied from it or any other third-party
  importer. See the clean-room attestation in
  `crates/synth-layout/src/kicad_zip.rs`.
- SnapEDA's own Design Files are CC BY-SA 4.0 (with a "Design Exception
  1.0" permitting board-design use without share-alike propagating to
  the whole board); UltraLibrarian asserts its CAD content is
  Cadence/manufacturer-owned. Either way, only the derived, minimal pin
  inventory and footprint geometry a user already legitimately
  downloaded is copied into the Tier-2 registry — same posture as the
  LCSC/EasyEDA path above.

## Review gate

Tier-1 entries are subject to the review gate (R15.10,
`.github/workflows/registry-review.yml`): any added or modified part
file must carry a non-empty `[provenance].reviewed_by`. Unreviewed or
experimental parts belong in the Tier-2 user registry
(`SYNTH_USER_REGISTRY_DIR`), which is never shipped.

## Growth queue (R15.12)

The target of ≥300 verified Tier-1 parts is tracked via bulk
`synth part import kicad --batch` runs (results land unverified) plus a
human review pass that sets `reviewed_by`. Run `synth registry doctor`
to list every part still awaiting review.
