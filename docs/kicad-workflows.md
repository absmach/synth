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
2. **Electrical-rule tuning goes through the ERC sidecar.** Add
   `board.synth.erc.toml` beside the design to override the pin-type
   conflict table and the deeper-check thresholds; leave it absent for
   the defaults.
3. **Visual tuning goes through the sidecar.** Edit
   `<design>.synth.layout.toml` directly or use `synth_mutate_layout` — never nudge symbol coordinates inside the `.kicad_sch`.
4. **Sourcing data lives in the registry**, not in the drawing. Each
   `registry/parts/*.synth.toml` carries `mpn`, `lcsc_pn`, footprint,
   and provenance; the exporter stamps hidden `MPN` / `LCSC` fields
   onto every schematic instance and mirrors them into `bom.csv`.
   Fix a part number in the registry (or the `value` statement), not
   on the symbol.
5. **The compiler's ERC is the first gate, KiCad's ERC is the second.**
   Synth's 80+ `E-SYNTH-*` rules run before export; `kicad-cli sch
erc` validates the exported artifact after. Both must be clean
   before anything ships (see §1).
6. **A check that could not run is not a check that passed.** When
   `kicad-cli` is missing, unsupported, too slow, or writes a report
   Synth cannot read, that stage is reported as `unknown` with the
   tool, version, command, stderr, and a machine-stable reason.
   `unknown` blocks any command that asked for verification, and
   `--force` does not override it. A plain `synth export-kicad` is the
   one exception: it records the `unknown` and still writes the
   project, because that output claims nothing about being verified.
   Use `--verification-report FILE` to capture the evidence, or read
   `stages.manufacturing.native` from `synth check --fab --json`.
7. **A DRC run that finished is reported by its real counts.** After the
   export, `synth export-kicad` reads KiCad's DRC report and prints
   `errors N, unconnected pads N, warnings N`. _errors_ are KiCad rule
   violations and are listed one by one; _unconnected pads_ are the
   report's separate `unconnected_items` list, pads that belong to a net
   but have no copper path yet (an incomplete route, which still
   exports so it can be finished by hand); _warnings_ are advisory
   findings that are not rule violations. The line ends with
   `Zero-DRC gate clean` only when all three are zero. Errors and
   unconnected pads make the `kicad_drc` stage `fail`, with the counts in
   its `detail`; its `violations` count is errors plus unconnected pads,
   while `NativeDrcOutcome.violations` stays errors only. Warnings alone
   do not fail it. In release mode (`export-kicad
   --gerbers`/`--drill`/`--step`, or `synth check --fab`) a failed
   `kicad_drc` stage exits non-zero and `--force` does not override it. A
   plain export still writes the board and exits 0 on unconnected pads;
   only errors fail it, as before.

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

### Routing is always external

Copper is never generated by Synth. Every export routes through an external
engine — FreeRouting by default, KiCadRoutingTools on request — and then checks
the result independently of what the engine reported about itself. The
un-routed export is preserved as `<name>.synth.kicad_pcb` for review.

```text
synth export-kicad board.synth --out output/board
```

`--autoroute` no longer exists. It used to opt in to the external router; the
router is now the only path, and passing it is an error that points here.

#### Prerequisites

Install both engines, idempotently and without root:

```text
scripts/setup-routing-engines.sh
```

It downloads the pinned FreeRouting JAR into `tools/freerouting/`, clones
KiCadRoutingTools under `~/.local/share/synth/`, layers a virtualenv over an
interpreter that imports `pcbnew` so its Python dependencies install without
touching the system packages, builds KRT's Rust router, and prints the
environment Synth needs. Re-run it any time; it skips what is already there.
`--check` reports what is installed without changing anything.

FreeRouting needs a Java runtime and a FreeRouting JAR; both are found
automatically, and a missing one is reported as `router_unavailable` with the
paths that were searched. The helper that drives FreeRouting also imports
KiCad's `pcbnew` module, which ships with **KiCad's own Python** and is often
absent from the `python3` first on `PATH`. Point the pipeline at the right
interpreter when that happens:

```text
SYNTH_FREEROUTING_PYTHON=/usr/bin/python3.14 synth export-kicad board.synth --out output/board
```

The failure is named `python_bindings_missing` and says so, rather than
surfacing a Python traceback as an engine crash. `synth routers` reports what
is installed and usable.

The four terminal states are distinct, and only one is fabricable:

| State               | Meaning                                                  |
| ------------------- | -------------------------------------------------------- |
| `routed`            | Independently validated, fabrication-profile compliant   |
| `validation_failed` | Copper was rejected: topology loss, open nets, DRC errors |
| `review_required`   | Copper exists but a required check could not be performed |
| `router_unavailable`| The engine is not installed or failed to start           |

Only `routed` can pass the release gate. The other three leave the board in
place for review and return exit status 1; `--force` does not override any of
them, for the same reason it does not override a KiCad DRC error. A check that
could not be performed is never treated as a clean check.

Every run writes a record next to the board:

| File                          | Contents                                         |
| ----------------------------- | ------------------------------------------------ |
| `<name>.routing.json`         | Run record: state, provenance, statistics, verdict |
| `<name>.<engine>.log`         | Engine session log                               |
| `<name>.freerouting.json`     | Engine's own provenance report                   |
| `<name>.freerouting.ses`      | FreeRouting's Specctra session result            |
| `<name>.connectivity.json`    | Independent connectivity and topology findings   |
| `<name>.drc.json`             | `kicad-cli pcb drc` result over the routed board  |

#### Escape feasibility (before routing)

A fine-pitch package can be unescapable at the chosen process long before a
router runs, and the failure arrives as mysterious necked-down vias rather
than as an answer. Every run therefore reports the dense packages whose pads
cannot be fanned out at the fabrication floor, in the run record's `escape`
field and on the terminal:

```text
escape: U2 (Package_QFP:LQFP-48_7x7mm_P0.5mm, 48 pads, 0.500 mm pitch):
  adjacent pins are 0.500 mm apart and the pads leave 0.200 mm between them;
  a fabrication-floor via needs 0.727 mm between centres, so the drill that
  would fit is 0.073 mm; even a minimum track needs 0.254 mm of gap
  fix: route this package on more layers, use via-in-pad if the process
  allows, or choose a coarser-pitch part
```

It is advisory, not a gate: a router can still complete such a board by
staggering vias or necking tracks. What it cannot change is the arithmetic,
so the finding states it — the pitch the package offers, the via the floor
requires, and the drill that would actually fit. (Idea borrowed from
[TraceMaker](https://github.com/DingoOz/TraceMaker)'s `escape` command.)

#### Trying every engine at once

Engines disagree on the same board — one completes a net the other cannot, or
uses far fewer vias. `--best-of` runs every installed engine, validates each
result independently, and keeps the best attempt:

```text
synth export-kicad board.synth --out output/board --best-of
synth route board.synth --best-of
```

The winner is chosen by the independent verdict, never by an engine's own
report: validated state first, then fewer blocking findings, fewer vias, and
shorter copper. Each attempt gets its own subdirectory, so every result
survives for inspection, and an engine that is not installed simply does not
win — the run degrades to the engines present rather than failing.

#### Learning from earlier runs

Set `SYNTH_ROUTING_OUTCOMES_DIR` (or pass `log_routing_outcomes` over MCP) and
every run appends a record — board hash, engine, state, open nets, vias,
copper length, and the retry order it recommends. A later run on the same
board reads the best prior attempt back and prints the order that worked
best *before* routing, so a retry starts from where the last one left off:

```text
SYNTH_ROUTING_OUTCOMES_DIR=~/.local/share/synth/outcomes \
  synth export-kicad board.synth --out output/board
```

Unset, nothing is written: logging is opt-in.

#### The fabrication floor follows the board's process

The minimums the gate enforces — trace width, clearance, drill, annular ring —
come from the board's `manufacturer` (`jlcpcb`, `pcbway`, `oshpark`) via the
same profiles `synth drc` uses, not from a built-in default. A design that
declares a finer process as its manufacturer is judged against that process.

The independent check re-reads the routed board and compares it to the
baseline: footprint, pad and net topology must be unchanged, each net must be
continuous through real copper, and track/via geometry must meet the
fabrication floor. A net completed through a filled zone — a ground plane
reached by stitching vias — counts as connected; counting only tracks would
report every plane-backed net as open however correctly it was routed. Zones
are refilled before DRC so clearances are checked against copper that actually
exists.

To compare two engines by hand, route each and run the same check over both:

```text
synth route board.synth --out-dir /tmp/fr --freerouting-jar tools/freerouting/freerouting-2.4.1.jar
synth route board.synth --out-dir /tmp/krt --router kicad-routing-tools \
  --kicad-routing-tools-repo /tmp/KiCadRoutingTools
kicad-cli pcb drc --output /tmp/fr-drc.rpt /tmp/fr/fr/board.kicad_pcb
kicad-cli pcb drc --output /tmp/krt-drc.rpt /tmp/krt/krt/board.kicad_pcb
```

Compare signal connectivity, KiCad DRC violations, zone-island reports,
runtime, via count, minimum clearance, and via-in-pad usage.

#### KiCadRoutingTools comparison router

[KiCadRoutingTools](https://github.com/drandyhaas/KiCadRoutingTools) is an
optional external Rust-accelerated A* router with rip-up/reroute, multi-layer
routing, plane handling, and connectivity checkers. Synth does not vendor it;
install it in a separate checkout and Python environment, then use the adapter
in `tools/kicad_routing_tools_route.py`:

```text
scripts/setup-routing-engines.sh            # installs the checkout and its Python

python3 tools/kicad_routing_tools_route.py \
  output/board/board.synth.kicad_pcb \
  output/board/board.kicad_routingtools.kicad_pcb \
  --repo "${KICAD_ROUTING_TOOLS_REPO}" \
  --python /usr/bin/python3.14
```

The same path is available directly from Synth. FreeRouting is the default;
selecting KiCadRoutingTools requires its checkout and dependency-aware Python
interpreter:

```text
synth export-kicad board.synth --out output/board \
  --router kicad-routing-tools \
  --kicad-routing-tools-repo /tmp/KiCadRoutingTools \
  --kicad-routing-tools-python /tmp/krt-venv/bin/python
```

Synth runs KRT in conservative production-review mode by default: it uses
`--escalation board --strict-sizes`, keeps vias 0.1 mm away from same-net SMD
pads and paste openings, and runs KRT's independent connectivity checker.
Synth still writes the routed board when routing is incomplete, but returns
exit status 1 and labels the result review-only. KRT's own report is written
next to the board as `<name>.krt-stats.json`.

Use `--krt-escalation fab` only when the selected fabrication process has been
explicitly approved to relax the board's declared rules. Use
`--krt-allow-via-in-pad` only when filled-and-capped via-in-pad is part of the
fabrication specification. The production gate rejects open connection groups,
delivered dimensions below the board floor, and unapproved via-in-pad sites.
`--krt-fab-tier standard|advanced|auto` and `--krt-fab-overrides FILE` expose
KRT's fabrication capability floor when a manufacturer-specific override is
needed.

KRT may adapt to
fabrication floors and may write project DRC settings; review those changes
explicitly rather than treating a zero-unconnected result as automatic
production approval.

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
  (Synth auto-places them; check visually in an exported PDF).

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
  assumed; avoid microphonic dielectrics in audio paths. This is
  checked automatically when the capacitor carries `dielectric` and
  `voltage` structured values — see `E-SYNTH-CAP-001` and
  [`variants-and-bom.md`](variants-and-bom.md).
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

### Schematic documentation source

Design documentation lives in the `.synth` source, not in KiCad:

- `component R7: resistor "r_generic_0603" dnp` marks do-not-populate:
  the symbol exports `(dnp yes)`, the part is left out of `bom.csv`
  and `pnp.csv`, and ERC still checks it like any other part.
- `notes "Title" { "line one" … }` renders a titled text block —
  under the group's outline box when written inside a `group`, else
  stacked at the sheet's bottom-left.
- Every `group` gets a caption plus an outline box, and every
  connector gets a `pin: net` legend generated from the netlist.

### Multi-sheet export (`sheet` blocks)

`sheet "Power" { … }` blocks (and `import`ed files, which lower to a
sheet named after the file stem) are split boundaries. A board whose
single-sheet content still fits A2 exports as one `.kicad_sch`,
byte-identical to before — the split is a large-board repair, not a
restructuring of small ones. Past A2, with two or more boundaries, the
export becomes a hierarchy:

- one `<board>_<sheet>.kicad_sch` per boundary, plus the root
  `<board>.kicad_sch` carrying the components declared outside any
  sheet and one sheet instance per sub-sheet;
- cross-sheet **signal** nets join through hierarchical labels on
  sub-sheets and same-named local labels on the root (one per sheet
  pin and per root endpoint); cross-sheet **power** nets need no
  labels at all — power symbols connect globally by value;
- a dragged component's sidecar override records the sheet it was
  placed on, so moving it to another sheet invalidates the stale
  (sheet-local) coordinates instead of misplacing it.

Verify with `kicad-cli sch erc <board>.kicad_sch` on the root: the
whole hierarchy resolves from there.

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

### Placement JSON coordinate contract

The placement payload that leaves Synth — the MCP
`synth_place_with_hints` `component_placements` field and the CLI
`synth place` document — is in **footprint-origin coordinates**. Each
component's `center` is the point KiCad writes as the footprint's
`(at x y)`: the physical origin of the footprint on the board, in
nanometres. An overlay of the JSON on the exported `.kicad_pcb`
therefore needs no correction.

This is deliberately **not** the same as the internal
`synth_place::ComponentPlacement::center`, which is the courtyard-bbox
centre the placer, DRC and router reason about. The two differ by the
footprint's courtyard offset for asymmetric footprints (USB
receptacles, DIP headers):

```
published_center = courtyard_center - rot(courtyard_offset)
```

Serialize internal placements only through `synth_place::to_external`;
the KiCad exporter and `to_external` both derive `(at x y)` from the
shared `synth_place::footprint_origin` helper, so the board and the
published placement cannot drift apart. Emitting the internal courtyard
centre here shifts every downstream consumer — notably the synth-ee
placement visualisation — relative to the exported board.

The persisted `<design>.placement.layout.toml` sidecar is a separate,
schema-versioned board-millimetre contract consumed by
`synth_write_layout_override`; treat it as its own coordinate space and
never assume a value copied from one is valid in the other.

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
reviewer can check coverage, not folklore. The guidelines below are
the ones common to standard schematic design-rule practice, resting
on IEEE reference-designator conventions and IPC-2221/IPC-2612-1.

| Guideline                                           | Synth mechanism                                                                                                     |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Page size by complexity                             | auto sheet-size selection (`synth-layout`)                                                                          |
| Grid system                                         | 1.27 mm pin grid snap + 2.54 mm placement grid                                                                      |
| Title block                                         | emitted: name, revision, notes, fab target                                                                          |
| Notes on schematic                                  | title-block comments 1–3                                                                                            |
| Standard refdes letters                             | `E-SYNTH-NAME-004` (IEEE table)                                                                                     |
| Refdes uniqueness / letter start                    | `E-SYNTH-NAME-001/002/003`                                                                                          |
| Stock-library symbols, inputs left / power top      | registry `kicad_symbol` + KiCad stock symbols; KLC rules for new parts                                              |
| Polarized-component polarity                        | KG `polarized_cap_miswired` (`E-SYNTH-KG-001`)                                                                      |
| Junction dots                                       | router collects junctions; crossing rules `E-SYNTH-SCHEM-005/006`                                                   |
| Net labels uppercase                                | `E-SYNTH-SCHEM-008`                                                                                                 |
| Short net names (preferably ≤ 4 letters)            | `E-SYNTH-SCHEM-009` — **deliberately 16 chars**: semantic names (`I2C_SCL`) beat brevity for agent-generated sheets |
| Remove open nets                                    | `E-SYNTH-CONNECT-*` + no-connect reconciliation                                                                     |
| Signal flow left→right, power top, ground bottom    | layered placement + power-symbol orientation passes                                                                 |
| Readability of parallel connections                 | `E-SYNTH-SCHEM-002/004/005` + cluster alignment                                                                     |
| Crystal proximity                                   | `Crystal` cluster + `E-SYNTH-SCHEM-003` (decoupling distance)                                                       |
| ERC                                                 | rule-based + value-based + KiCad ERC (§1 gates)                                                                     |
| Netlist verification                                | pin reconciliation + KiCad `--schematic-parity` DRC                                                                 |
| Complete BOM: MPN, package, vendor                  | registry-carried `mpn`/`lcsc_pn`, hidden `MPN`/`LCSC` instance fields, `bom.csv`, supply-chain validation           |
| Decoupling on all ICs                               | `E-SYNTH-POWER-001` (manifest) + KG `ic_decoupling` (undeclared)                                                    |
| Test points + expected voltages (advanced practice) | KG `rail_test_points` — catalog entry until a test-point part exists                                                |

Not yet applicable: revision-history page, table of contents, block
diagram sheet. Hierarchical sheets **are** emitted
(§P26): a board whose single-sheet content overflows A2 and whose
components span two or more `sheet` blocks (or imported files) exports
as one `.kicad_sch` per sheet plus a root carrying sheet instances, with
cross-sheet signal nets on hierarchical labels. Small boards stay a
single sheet.

---

_See also: [`schematic-procedures.md`](schematic-procedures.md) for the
export pipeline internals, [`diagnostics/README.md`](diagnostics/README.md)
for `E-SYNTH-*` meanings, and [`protocol-v1.0.md`](protocol-v1.0.md) for
the MCP wire format._
