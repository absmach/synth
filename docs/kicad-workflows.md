# KiCad Workflow Knowledge Base

**Scope:** how to work with the KiCad projects Synth exports — validation
gates, parts and BOM discipline, symbols and footprints, PCB review,
fabrication outputs, Gerber inspection, panelization, and product renders.

**Audience:** human engineers and coding agents driving Synth through the
CLI (`synth validate`, `synth export-kicad`, …) or the MCP server
(`synth_validate`, `synth_apply_patch`, `synth_export`, …).

---

## 0. Ownership doctrine (read this first)

```
board.synth   ← the ONLY thing you edit
   │  synth export-kicad
   ├── board.kicad_sch / .kicad_sym / .kicad_pro   ← build artifacts
   ├── board.kicad_pcb                             ← build artifacts
   └── bom.csv                                     ← build artifact
board.synth.layout.toml   ← optional visual drag offsets (sidecar)
```

1. **Never hand-edit generated files.** They are byte-deterministic
   re-exports; any manual edit is silently destroyed on the next
   export and breaks the stable-diff guarantee. Connectivity, values,
   part numbers, DNP decisions: change the `.synth` source.
2. **Visual tuning goes through the sidecar.** Drag positions in the
   `synth preview` browser, or edit `<design>.synth.layout.toml`
   directly — never nudge symbol coordinates inside the `.kicad_sch`.
3. **Sourcing data lives in the registry**, not in the drawing. Each
   `registry/parts/*.synth.toml` carries `mpn`, `lcsc_pn`, footprint,
   and provenance; the exporter stamps hidden `MPN` / `LCSC` fields
   onto every schematic instance and mirrors them into `bom.csv`.
   Fix a part number in the registry (or the `value` statement), not
   on the symbol.
4. **The compiler's ERC is the first gate, KiCad's ERC is the second.**
   Synth's 80+ `E-SYNTH-*` rules run before export; `kicad-cli sch
erc` validates the exported artifact after. Both must be clean
   before anything ships (see §1).

---

## 1. Schematic review and electrical validation

### Gate order

| #   | Gate                                                                                     | Command                                                                                                        | Bar                                     |
| --- | ---------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | --------------------------------------- |
| 1   | Rule-based ERC (connectivity, decoupling, boot/clock/reset, I²C pull-ups, power shorts)  | `synth validate board.synth`                                                                                   | zero blocking `E-SYNTH-*`               |
| 2   | Value-based ERC (SI-aware magnitudes, e.g. `E-SYNTH-CRYSTAL-001` load-cap mismatch)      | included in gate 1                                                                                             | zero errors; warnings reviewed          |
| 3   | Export                                                                                   | `synth export-kicad board.synth`                                                                               | succeeds without `E-SYNTH-CAPABILITY-*` |
| 4   | KiCad ERC on the artifact                                                                | `kicad-cli sch erc --format report --severity-error --severity-warning --exit-code-violations board.kicad_sch` | **zero errors, zero warnings**          |
| 5   | Aesthetic ERC (power-symbol orientation, crossings, decoupling distance, net-name style) | built into Synth (`E-SYNTH-SCHEM-001..010`)                                                                    | warnings triaged, not ignored           |

Store gates 1–5 in a project `build.sh` (or Makefile) and re-run the
whole chain after every source change — determinism makes this cheap
and re-runs diff-stable.

### Professional review checklist (per sheet)

- Every pin either connected or explicitly no-connect (Synth's pin
  reconciliation handles this; confirm `kicad-cli sch erc` agrees).
- Power symbols point the right way: `+3V3`/`VDD` arrows up, `GND`
  triangles down; `PWR_FLAG` only at supply sources.
- Net labels uppercase, ≤ 16 chars, differential pairs suffixed
  `_P`/`_N` or `+_`/`_-`.
- Title block carries name, revision, and fab target (Synth emits
  these from the `board` statement automatically).
- Reference/Value fields must not collide with wires or each other
  (Synth auto-places them; check visually in `synth preview` or an
  exported PDF).

---

## 2. Parts, BOM, and sourcing

### Where part decisions live

- **Registry first.** A part number change is a registry (or `.synth`
  `value`) edit followed by re-export — never a BOM-file edit.
- **Never edit `bom.csv` or the generated BOM presets.** If a part
  number must change, the change happens upstream in the source, or
  it does not happen.
- Synth instances already carry `MPN` and `LCSC` hidden fields (KiCad
  BOM tooling and the JLCPCB plugin ecosystem read exactly these
  field names). Do not invent alternative field spellings.

### Component-class selection rules

- **Ceramic capacitors:** derate for applied DC bias (an X7R "10 µF"
  at 80 % rated bias may deliver a fraction of nominal); check the
  dielectric temperature spec; respect the package size the design
  assumed; avoid microphonic dielectrics in audio paths.
- **Resistors:** verify power rating against dissipated power with
  margin; prefer thin-film for low-noise/sensitive analog nodes.
- **Inductors:** non-standard packages are the risk — prefer parts
  whose footprint already exists in KiCad's libraries; pin down
  value, rated current, saturation current, and DCR before layout.
- **RF matching:** C0G/NP0 capacitors and high-Q inductors only.
- **ICs:** the schematic `value` must carry the _full_ manufacturer
  part number; a partial snippet is a sourcing defect. Synth's
  registry `mpn` field is the authoritative spelling.

### Fab-house BOM formats

JLCPCB / PCBWay / NextPCB expect their own column mappings (Comment,
Designator, MPN, LCSC, DNP, …). Generate them from the exported
`.kicad_sch` with `kicad-cli sch export bom --preset <fab>
--format-preset CSV` (see §5). Treat the fab BOM as a build output:
regenerate, don't maintain.

---

## 3. Symbols and footprints

Synth already prefers KiCad stock symbols/footprints via the
registry's `kicad_symbol` / `kicad_footprint` fields (e.g.
`Device:R`, `Regulator_Linear:AMS1117-3.3`). Author new library
entries only when stock does not cover the part:

- **Reuse check first** — the stock library almost certainly has the
  0603 resistor, the SOIC-8, the SOT-23. Do not synthesize what
  already exists.
- **Symbols:** keep inputs left, outputs right, grounds bottom,
  power top; merge duplicated pins into proper pin groups; use real
  electrical pin types (`power_in`, `passive`, …) — wrong types flood
  ERC with false positives that train everyone to ignore ERC.
  Validate against the KLC standard
  (<https://gitlab.com/kicad/libraries/klc>); `kicad-library-tools`
  can generate and lint. Ground-truth the pin table against the
  datasheet (or easyeda2kicad by LCSC number) and have a human review
  the pin table before trusting the part.
- **Footprints:** prefer the generators in
  `kicad-library-tools` (QFN/LGA/etc. YAML-driven); describe a
  datasheet land pattern in words before encoding it; always request
  the 3D model. Flag non-standard pad shapes and missing generator
  categories for human review rather than improvising.
- **Symbol-integrity gate:** after any registry symbol change,
  re-export and diff — pin numbers and names must be identical to the
  previous export. Synth's deterministic UUIDs make this diff
  mechanical.

---

## 4. PCB review

- **DRC is a gate, not a suggestion:** `kicad-cli pcb drc --format
report --severity-error --severity-warning --refill-zones
--schematic-parity --exit-code-violations board.kicad_pcb` must be
  clean before handoff. `--schematic-parity` catches drift between
  the schematic and the board — with Synth both come from one IR, so
  any parity error is a bug worth escalating.
- **Placement doctrine** (already partially encoded in Synth's
  thematic clusters — `IcBlock`, `LdoBlock`, `Crystal`): decoupling
  capacitors belong at the IC pins, oriented to minimize ground loop
  area and trace inductance; horizontal connectors live near board
  edges; keep packages aligned. Optimize placement for _engineer
  modifiability_, not a one-shot perfect floorplan.
- **Review by rendering, not by imagination:** export board-layer
  SVGs/PNGs (`kicad-cli pcb export svg`, `pcb render`) and actually
  look; render specific regions to check 3D-model interference.
- **Board outline:** rounded or chamfered corners by default;
  confirm with the engineer. Double-sided assembly is an explicit
  engineer decision, never a default.

---

## 5. Fabrication exports

Standard export chain (adapt per fab; keep it scripted in the
project's `build.sh`):

```bash
kicad-cli version
kicad-cli sch erc --output build/board-erc.rpt \
  --format report --units mm \
  --severity-warning --severity-error --exit-code-violations board.kicad_sch
kicad-cli pcb drc --output build/board-drc.rpt \
  --format report --units mm --refill-zones --schematic-parity \
  --severity-warning --severity-error --exit-code-violations board.kicad_pcb
kicad-cli sch export pdf --output build/schematic.pdf board.kicad_sch
kicad-cli sch export bom --preset JLCPCB --format-preset CSV \
  --output build/bom_JLCPCB.csv board.kicad_sch
kicad-cli sch export netlist --format kicadxml \
  --output build/netlist.xml board.kicad_sch
kicad-cli pcb export gerbers --output build/gerbers/ board.kicad_pcb
kicad-cli pcb export drill  --output build/gerbers/ board.kicad_pcb
```

- **Position files:** JLCPCB and NextPCB want a 5-column CSV
  (Designator, Mid X, Mid Y, Layer `T|B`, Rotation). KiCad's native
  `.pos` output doesn't match; convert it.
- **Everything lands in `build/`.** Versioned outputs, never
  overwriting sources.
- BOM presets per fab are configuration, checked into the project —
  regenerate from them, never hand-patch the CSVs.

---

## 6. Gerber review

Generate Gerbers for _review_ into a scratch directory if they don't
already exist, then render and **look** at them:

```bash
uv pip install pygerber
pygerber gerber convert png build/gerbers/F_Cu.gbr -o f_cu.png -d 600
```

Check for: unexpected shorts between nets, vias intersecting traces
or pads, annular-ring violations, silkscreen over pads, and overall
fabricability. A DRC pass is necessary but not sufficient — the
render is where silly layer-stack mistakes become visible.

---

## 7. Panelization (KiKit)

- Verify `kikit --version` before starting.
- Inspect the source board for edge hazards first: castellations,
  edge connectors, antennas, plated edges, components overhanging
  `Edge.Cuts`.
- Resolve grid size, routing gap, tabs, frame, and the fab's panel
  limits _before_ generating; position tooling holes and fiducials
  relative to the panel outline.
- Emit strict JSON presets; generate versioned panels into a
  `build/` subdirectory without touching the source board; render
  and inspect the panel before reporting done.

Sensible starting values: 2 mm routing gap · 3 mm tabs · 0.5 mm
mouse-bite drill at 0.8 mm spacing · 5 mm full frame · 2 mm
frame-to-array clearance · no frame break cuts.

---

## 8. Product renders (Blender)

For realistic product imagery from a KiCad project:

1. Regenerate the STEP/GLB for "latest" — never trust a stale GLB by
   filename (`kicad-cli pcb export step|glb --force --no-dnp
--subst-models`, including tracks/pads/zones/silkscreen/mask and
   `--cut-vias-in-body` where supported).
2. **DRC first.** Stop on violations unless the engineer explicitly
   authorized proceeding, and record that decision in the handoff.
3. Inspect the exported scene (outline, holes, mounting cutouts)
   before rendering; replace missing vendor models with a stated
   deterministic fallback.
4. Render hero / near-overhead top / detail / back views with Cycles
   - denoising and **inspect the actual PNGs** — a clean exit code is
     not a render review.
5. Handoff names: output directory, gallery URL, DRC status, missing
   -model fallbacks, and remaining geometry simplifications.

Material targets that read as real hardware: deep royal-blue LPI
soldermask (moderately glossy, low coat roughness), warm translucent
FR4 edges, pale metallic-gold ENIG (not orange), subdued silver SAC
solder fillets on dry passives, matte nylon connector bodies.

---

## 9. Handoff gate (all workflows)

Before declaring any Synth-exported design "ready":

- [ ] `synth validate` — zero blocking diagnostics
- [ ] `kicad-cli sch erc` — zero errors, zero warnings
- [ ] `kicad-cli pcb drc` (with `--schematic-parity`) — clean
- [ ] Re-export is byte-identical (determinism intact; no
      hand-edits lurking in generated files)
- [ ] Every instance carries `MPN`/`LCSC`; `bom.csv` regenerated, not
      edited
- [ ] Visual pass done on the schematic (title block, field
      collisions, power-symbol orientation) and on Gerber/board
      renders
- [ ] Liberties taken (value substitutions, footprint substitutions,
      unreviewed parts) reported explicitly to the engineer

---

## 10. Standards coverage map

Where the published schematic guidelines land in Synth — so a
reviewer can check coverage, not folklore. Sources: Sierra Circuits,
"How to Draw and Design a PCB Schematic"
(protoexpress.com/blog/how-to-draw-design-pcb-schematic/, 17
guidelines + checklist) and AIVON, "Decoding PCB Schematics"
(aivon.com/blog/pcb-design/decoding-pcb-schematics-a-guide-for-\
effective-diagnostics/), plus the underlying IEEE refdes conventions
and IPC-2221/IPC-2612-1 they cite.

| Guideline (source)                                             | Synth mechanism                                                                                                     |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Page size by complexity (Sierra 1)                             | auto sheet-size selection (`synth-layout`)                                                                          |
| Grid system (Sierra 3)                                         | 1.27 mm pin grid snap + 2.54 mm placement grid                                                                      |
| Title block (Sierra 4)                                         | emitted: name, revision, notes, fab target                                                                          |
| Notes on schematic (Sierra 5)                                  | title-block comments 1–3                                                                                            |
| Standard refdes letters (Sierra 10)                            | `E-SYNTH-NAME-004` (IEEE table)                                                                                     |
| Refdes uniqueness / letter start                               | `E-SYNTH-NAME-001/002/003`                                                                                          |
| Stock-library symbols, inputs left / power top (Sierra 11)     | registry `kicad_symbol` + KiCad stock symbols; KLC rules for new parts                                              |
| Polarized-component polarity (Sierra checklist 2)              | KG `polarized_cap_miswired` (`E-SYNTH-KG-001`)                                                                      |
| Junction dots (Sierra 12)                                      | router collects junctions; crossing rules `E-SYNTH-SCHEM-005/006`                                                   |
| Net labels uppercase (Sierra 12)                               | `E-SYNTH-SCHEM-008`                                                                                                 |
| Short net names (Sierra 12: "preferably ≤ 4 letters")          | `E-SYNTH-SCHEM-009` — **deliberately 16 chars**: semantic names (`I2C_SCL`) beat brevity for agent-generated sheets |
| Remove open nets (Sierra 12)                                   | `E-SYNTH-CONNECT-*` + no-connect reconciliation                                                                     |
| Signal flow left→right, power top, ground bottom (Sierra 12)   | layered placement + power-symbol orientation passes                                                                 |
| Readability of parallel connections (Sierra 13)                | `E-SYNTH-SCHEM-002/004/005` + cluster alignment                                                                     |
| Crystal proximity (Sierra 14)                                  | `Crystal` cluster + `E-SYNTH-SCHEM-003` (decoupling distance)                                                       |
| ERC (Sierra 15)                                                | rule-based + value-based + KiCad ERC (§1 gates)                                                                     |
| Netlist verification (Sierra 16)                               | pin reconciliation + KiCad `--schematic-parity` DRC                                                                 |
| Complete BOM: MPN, package, vendor (Sierra 17, checklist 6/10) | registry-carried `mpn`/`lcsc_pn`, hidden `MPN`/`LCSC` instance fields, `bom.csv`, supply-chain validation           |
| Decoupling on all ICs (Sierra checklist 9)                     | `E-SYNTH-POWER-001` (manifest) + KG `ic_decoupling` (undeclared)                                                    |
| Test points + expected voltages (AIVON advanced tips)          | KG `rail_test_points` — catalog entry until a test-point part exists                                                |

Not yet applicable (single-sheet V1): alphabetical page names,
revision-history page, table of contents, off-page connectors, block
diagram sheet (Sierra 2/6/7/8/9). These arrive with hierarchical
sheets — the current compact single-sheet design is documented in
[`schematic-procedures.md`](schematic-procedures.md).

---

_See also: [`schematic-procedures.md`](schematic-procedures.md) for the
export pipeline internals, [`diagnostics/README.md`](diagnostics/README.md)
for `E-SYNTH-*` meanings, and [`protocol-v1.0.md`](protocol-v1.0.md) for
the MCP wire format._
