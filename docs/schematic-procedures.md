# Synth Schematic Procedures

**Scope:** how Synth turns a `.synth` design into a human-readable,
KiCad-importable schematic — the auto-layout pipeline, KiCad export,
ERC / DRC validation, the live browser preview, and the CLI commands
that drive them.

**Status:** describes the implemented pipeline as of 2026-09-24, cross-checked
against `crates/synth-layout`, `crates/synth-kicad`, `crates/synth-web`,
`crates/synth-cli`, and `crates/synth-validate`. Phases A–E of
[`schematic-quality-plan.md`](schematic-quality-plan.md) are folded in;
§2.1 (regions) and §3.2 (net colours) are the parts that changed most.

Every downstream consumer — the browser preview, the KiCad exporter,
the PCB placer/router — derives the schematic from a single shared
`synth_layout::layout` result so all views agree on the same
component positions and wire routes.

---

## 1. End-to-end flow

```text
.synth (SynthSpec DSL)
   │  synth-parser::parse        → ProgramAst (recoverable diagnostics)
   ▼
AST
   │  synth-ir::lower            + registry lookup → Board (IR)
   ▼
Board (nets / components / diff-pairs)
   │  synth-layout::layout       thematic auto-layout (§2)
   ▼
Layout (placements, rotations, wires, junctions, net labels, power flags)
   ├──> synth-layout::sheets::layout_sheets   split on sheet/import
   │                                          boundaries, large boards only (§3.1)
   ├──> synth-web (live preview, drag-and-drop)          (§5)
   ├──> synth-kicad::export      .kicad_sch/.kicad_sym/.kicad_pcb/bom (§3)
   └──> ERC / DRC validation                             (§4)
```

`layout_sheets` returns exactly one entry for the common case (the
board's single-sheet content fits A2, or it is not splittable), so the
preview, the exporter, and the checks all keep working off the same
single `Layout` unchanged. Only a *large splittable* board — content
past A2 **and** two or more `sheet`/import boundaries — becomes a
hierarchy, described in §6.

All coordinates are millimetres in the sheet's local coordinate system
with `(0, 0)` at the top-left of the page rect.

---

## 2. Auto-layout pipeline (`synth-layout::layout`)

`layout(board) -> Layout` is the single entry point. It runs, in order:

1. **`build_clusters` — pattern (motif) recognition.**
   Recognises repeated sub-circuits and lays each out as a unit.
   The passes run in a fixed order, each claiming components so later
   passes never double-claim:

   | Pass | Pattern        | What it recognises / claims                                                                                       |
   | ---- | -------------- | ----------------------------------------------------------------------------------------------------------------- |
   | 1    | `LedIndicator` | LED + its current-limit resistor (Above)                                                                          |
   | 2    | `UsbEsd`       | connector + ESD diodes (Left)                                                                                     |
   | 3    | `LdoBlock`     | regulator + up to `required_decoupling` caps per rail (Below)                                                     |
   | 4    | `Crystal`      | crystal + two load caps to GND (Below)                                                                            |
   | 5    | `IcBlock`      | full IC: decoupling caps (Below), reset network (Left/Right), pull-up/down resistors (Above)                      |
   | 6    | `I2cBus`       | I2C SDA/SCL pull-ups on a shared rail (Above) — runs **after** `IcBlock` so a full MCU keeps its decoupling/reset |
   | 7    | `Divider`      | rail→R1→mid→R2→gnd two-resistor divider                                                                           |
   | 8    | *orphan caps*  | `attach_orphan_rail_caps`: a cap on a **shared** rail joins the cluster whose anchor draws most from that rail    |
   | 9    | `Singleton`    | every remaining unclaimed component                                                                               |

   Pass 8 exists because the per-anchor `required_decoupling` claim in
   pass 5 cannot see a cap on a merged rail (one net feeding the
   regulator, the MCU and the sensor). Those caps used to fall through
   to `Singleton` and be placed by power-flow layer — 150+ mm from the
   part they decouple.

   **A declared `group` bounds cluster membership.** After every pass,
   `evict_cross_group_members` drops any member whose declared group
   differs from its anchor's and re-emits it as a `Singleton`. The
   pattern passes match on topology alone, so an I²C pull-up declared
   inside the sensor's group can be claimed by the MCU's `IcBlock` two
   groups away; placement would then put it in the *anchor's* region
   while `group_bounds` still measures it as part of its own, stretching
   that group's box across the sheet. Ungrouped boards are unaffected.

### 2.1 Regions (Phase C)

A board that declares `group`s lays out **by region**, not as one band
of columns:

- clusters partition by group; each region is laid out in its own local
  frame, so its true content rectangle is known before packing;
- regions are then shelf-packed to tile the page (reference mechanism 1:
  *the page is a grid of titled regions*). Packing measures the **box**,
  not the bodies — the box reaches above its members for the caption and
  below them for the note block (`group_box_overhang`), and packing on
  body bounds alone let boxes overlap;
- `REGION_SHELF_WIDTHS` (A4/A3/A2 content width) is swept by the fitting
  loop rather than fixed, because which regions share a shelf decides the
  stack's height and only a concrete candidate sheet can judge it;
- `region` / `color` / `title` on the group header (Phase D1) steer the
  packing order and the box hue.

An **ungrouped** board keeps the single-band path, with one addition:
when the band is too wide for the page shape it **shelf-wraps** onto
successive rows. Layers are columns, so a board with a dozen
one-cluster layers used to spread ~290 mm wide and ~118 mm tall on a
420 × 297 mm page.

The fitting loop ranks attempts by
**`(sheet area, shelves, column pitch, aspect mismatch)`**. Shelf-wrapping
and pitch-tightening are spent only on *saving a sheet size*, never on a
page that already fits: wrapping costs the left-to-right power-flow
reading order (a shelf break continues on the next line), and tightening
pushes nets past the span threshold until they degrade into labels.

2. **`place_clusters` — layered placement** (run via the `Placer` trait
   seam; V1 ships `NativeSemanticPlacer`, `crates/synth-layout/src/placer.rs`,
   see §7.8.5 — output is byte-identical to the pre-trait pipeline).
   - Power-flow **layer assignment** (§7.5.5 step 1): connectors/power
     sources in the left columns, signal chain left-to-right, passives
     toward the right. Layers become **columns** (reading direction).
   - **Barycenter crossing reduction** (§7.5.5 step 2): up to 4 sweeps
     ordering each column's clusters to minimise inter-column edge
     crossings, using semantic **strong-vs-weak** net weights
     (`STRONG_NET_WEIGHT=16` / `WEAK_NET_WEIGHT=1`): functional nets
     pull clusters together, power/ground rails only set orientation.
   - **Brandes–Köpf coordinate assignment** (§7.7.4 step 3):
     vertical alignment + horizontal compaction for straight vertical
     alignment of aligned clusters, snapped to the 2.54 mm grid.

3. **`align_to_pin_grid`** — snaps pin terminals to the 1.27 mm
   (50 mil) half-grid.

4. **`classify_power_flags`** — power/ground rails become power symbols
   (`+3V3`, `GND`, `VBUS`, …) instead of explicit wires.

5. **Rotation passes** — `rotate_two_pin_with_power_flags` (VCC up /
   GND down), `rotate_led_chains`, `rotate_usb_esd_diodes`,
   `lock_connector_rotations`.

6. **`resolve_text_overlaps`** — nudges captions, note lines and
   legend runs off each other and off component bodies, then shrinks,
   then drops the lowest-priority run. Symbol Reference/Value fields are
   placed separately by the exporter (§3) and are *not* moved here.

7. **`route_and_label`** — for every signal net:
   - **region crossing** (Phase C2): on a board that declares groups, a
     net whose endpoints live in different regions becomes a label and
     one that stays inside a region is drawn — *wires inside a region,
     labels between regions*. The span threshold below is the secondary
     guard for large single regions, and the only rule on an ungrouped
     board.
   - `classify_net_labels`: nets spanning > 80 mm or with 3+ endpoints
     are truncated into **net labels** (semantic names like `I2C_SCL`);
     unroutable / over-crossing nets get the same escape hatch.
   - `route::route_board`: routes local connections with the
     **Hashimoto–Stevens channel router** (left-edge packing at 2.54 mm
     pitch) for row-spanning nets, an **A\*** grid router for local
     connections, and an **L-route** fallback — all pin-aware
     (a path crossing an unrelated component's pin terminal is rejected
     and re-routed, else truncated to labels).
   - collects **junction** dots for T/split points.

7. **`soft_pin_swap_pass`** (optional, via `layout_with_pin_swaps`) —
   reassigns interchangeable (same-side, same-capability, non-power) IC
   pins when a swap removes a net intersection.

### Sidecar persistence (`layout_with_sidecar`)

User drag offsets are stored in a `<design>.synth.layout.toml` beside
the `.synth` source. `layout_with_sidecar(board, path)` runs the full
auto-layout then overlays the sidecar overrides. The `.synth` file
remains the source of truth for connectivity; the sidecar only changes
visual coordinates.

| Type / function               | Purpose                                                           |
| ----------------------------- | ----------------------------------------------------------------- |
| `ComponentPlacement`          | final centre + rotation                                           |
| `WirePath`                    | one orthogonal polyline per root→endpoint route, plus junctions   |
| `PowerFlag` / `PowerFlagKind` | `Vcc` (arrow up) / `Gnd` (triangle down) rail symbol              |
| `NetLabel`                    | same-named label at each endpoint of a long/labeled net           |
| `Layout`                      | components, wires, junctions, power_flags, net_labels, sheet_size |
| `Layout` methods              | `power_net_ids()`, `labeled_net_ids()`, `placement(id)`           |

---

## 3. KiCad export (`synth-kicad`)

`export(board, out_dir)` writes a complete, deterministic KiCad 10
project into `out_dir`:

| File               | Producer                      | Contents                                                                                                    |
| ------------------ | ----------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `<name>.kicad_pro` | `export.rs`                   | minimal JSON project, deterministic root UUID                                                               |
| `<name>.kicad_sym` | `symbol_lib.rs`               | synthesised symbol library (reuses bundled KiCad stock symbols when available, e.g. `power:GND`/`Device:R`) |
| `sym-lib-table`    | `export.rs`                   | maps the `synth` library nickname (avoids `lib_symbol_issues` ERC warnings)                                 |
| `<name>.kicad_sch` | `schematic.rs`                | the schematic (`kicad_sch` version 20260306)                                                                |
| `<name>.kicad_pcb` | `synth_place` + `synth_route` | placed/routed board                                                                                         |
| `bom.csv`          | `bom.rs`                      | refdes, value, kind, description, symbol, footprint, `lcsc_pn`, `mpn`                                       |

`schematic.rs` emits:

- **symbol instances** positioned from the shared `Layout`
- **one placed symbol per unit** for a multi-unit package (a dual
  op-amp, a quad gate), each with its own `(unit N)`, stacked at the
  package's single placement (`symbol_units` + `unit_offset_mm`); the
  PCB keeps one footprint per package
- **pin functions as KiCad alternates** (`alternates.rs`): the design's
  function name for a pin (`I2C1_SCL`) is declared on the library
  symbol and selected on the instance, so the pin reads the function
  rather than its package name (`PB6`)
- **Reference/Value field auto-placement** away from pins
  (`side_pin_counts` / `choose_field_sides` / `field_anchor`)
- **hidden sourcing fields** on every instance — `MPN` and `LCSC`
  copied from the registry part (empty string when unknown) — so the
  exported `.kicad_sch` is self-sufficient for KiCad BOM tooling and
  the JLCPCB/DigiKey plugin ecosystem without the sidecar `bom.csv`
- **net-class colours** (Phase B1) — see §3.2
- **power symbols** on rails + a stub `wire`
- **`no_connect` markers** on unreached physical pins and
  **`PWR_FLAG` drivers** on undriven power nets (`pin_reconcile`)
- **physical pin reconciliation** (`pin_reconcile.rs`): fans undeclared
  `power_in` legs onto the matching rail and marks unused GPIOs
  no-connect, so KiCad ERC doesn't report `pin_not_connected` /
  `power_pin_not_driven`
- **structured component fields** (`Tolerance`, `Voltage`, `Power`,
  `Dielectric`) as hidden properties, alongside `MPN`/`LCSC`
- **design variants** (`variant "lite" { dnp U3 }`) as KiCad 10's
  native variants: the name/description list in `.kicad_pro`, the
  per-symbol `(variant …)` override in the schematic, and one
  `bom.<variant>.csv` per variant
- **title block** with board name, `revision` (from the board
  statement), deterministic notes ("Generated by synth-eda",
  component/net counts), and — when the board declares
  `manufacturer "…"` — a `Fab: <manufacturer>` note documenting the
  fabrication target (the design-authority `company` field is
  deliberately left blank)
- **deterministic UUID v5** for every entity
  (`derive_entity_uuid(project, kind, name)`), so unchanged input
  re-exports byte-identically for stable diffs.

### 3.2 Net-class colours (Phase B1)

Nets are classified (`synth_layout::netclass`) into `Power`, `Ground`,
`I2C`, `SPI`, `UART`, `USB`, `Clock`, `Reset`, `Default` — reusing
`classify_power_net` for the rails and the `pick_net_label` capability
vocabulary for the protocols, so the colour and the label can never
disagree. An author-declared `netclass "…"` wins, and `color "#rrggbb"`
on it overrides the palette. The palette is fixed and colour-blind-safe
(Okabe–Ito derived), so I²C is the same blue in every design.

The colour is written **twice**, deliberately:

1. `.kicad_pro` `net_settings` — `classes[]` with `schematic_color` /
   `pcb_color`, plus `netclass_assignments` and `netclass_patterns`.
   These are keyed on the name **KiCad** will know the net by, which it
   derives from the drawing at load time: a power symbol's value
   (`+3V3`, `GND`, global, unprefixed) or a local label with its sheet
   path (`/SDA`). Keying them on the IR name (`net_7`) matches nothing,
   and the colours silently never apply.
2. An explicit `(stroke … (color …))` on each wire and `(color …)` in
   each label's font. A net carrying neither a power symbol nor a label
   is auto-named by KiCad (`Net-(U2-BOOT0)`), so no assignment written
   ahead of time can reach it; the explicit stroke reaches every net and
   is what the SVG plot shows.

### 3.1 Multi-sheet export (`sheets.rs`, `multisheet.rs`)

`sheet "…" { … }` blocks are split boundaries, and so is every
`import`ed file (it lowers to a sheet named after the file stem).
`synth-layout::sheets::layout_sheets` splits only when the board is
**large** (single-sheet content overflows A2) *and* **splittable**
(two or more boundaries carry components); otherwise it returns the
single `Layout` unchanged and this path is never taken.

When it does split, `synth-kicad::multisheet` writes:

- one `<board>_<sheet>.kicad_sch` per boundary, and the root
  `<board>.kicad_sch` with the components declared outside any sheet;
- one `(sheet …)` instance per sub-sheet, with a pin per cross-sheet
  net, its `(instances (project (path … (page …))))` block, and the
  `.kicad_pro` sheet list updated to match;
- **cross-sheet signal nets** as hierarchical labels in the
  sub-sheets, joined on the root by same-named local labels (one per
  sheet pin and per root endpoint); **cross-sheet power nets** need
  no labels — power symbols connect globally by value. `pin_reconcile` runs board-wide and each sheet
  emits only its own members, with one `PWR_FLAG` per undriven rail
  project-wide.

`check_sheets` runs the aesthetic rules per sheet and prefixes each
finding with `[<sheet>]`, so page overflow and wire crossings are
judged page-locally instead of against the stale global layout.
Component and net ids are never remapped, so ERC, power inference, and
the PCB flow stay sheet-agnostic.

---

## 4. Validation: ERC, aesthetic ERC, and DRC

### 4.1 Configurable ERC (`<design>.synth.erc.toml`)

`synth validate` auto-loads a sidecar next to the design
(`board.synth` → `board.synth.erc.toml`) and hands it to
`run_erc_with_config`. A malformed sidecar is a hard error, not a silent
fallback; an absent one leaves the defaults in place. See
`E-SYNTH-CONNECT-007` for the `[pin_conflicts]` grammar.

### 4.2 Rule families

Beyond the per-protocol rules, the engine carries three deeper families:

- **Pin-type conflicts** (`E-SYNTH-CONNECT-007`) — a configurable
  severity table modelled on KiCad's ERC matrix. Pairs already owned by
  a dedicated rule (`output`/`output`, `power_output`/`power_output`,
  anything/`do_not_connect`) are deliberately excluded to avoid duplicate
  findings.
- **Voltage domains** — a pull-up above a device's own supply
  (`E-SYNTH-POWER-008`), a regulator fed outside its declared input range
  (`E-SYNTH-POWER-009`), and summed rail load against the regulator's
  current limit (`E-SYNTH-POWER-010`). All three read the registry's
  `operating_conditions` and the declared/deferred rail voltages.
- **Protection and hygiene** — floating CMOS inputs
  (`E-SYNTH-CONNECT-008`), open-drain without a pull-up outside I²C
  (`E-SYNTH-CONNECT-009`), unprotected external connectors
  (`E-SYNTH-ESD-001`), LED current/dissipation (`E-SYNTH-LED-001`),
  case-only net collisions (`E-SYNTH-NAME-007`), single-use declared
  labels (`E-SYNTH-NAME-008`), a ground pin on a non-ground net
  (`E-SYNTH-NAME-009`), and multi-unit parts split across rails
  (`E-SYNTH-NAME-010`).
- **Ratings and variants** — a Class-II ceramic run at a high DC bias
  (`E-SYNTH-CAP-001`, using the structured `dielectric`/`voltage`
  fields), and the variant checks `E-SYNTH-VARIANT-001` (duplicate
  name) / `-002` (unknown refdes). See
  [`variants-and-bom.md`](variants-and-bom.md).
- **Pin muxing** — one pin asked to carry two peripheral functions
  (`E-SYNTH-PINMUX-001`, reported at lowering where both net names are
  still visible), and a named function routed to a pin that does not
  declare it (`E-SYNTH-PINMUX-002`). The function of a net is inferred
  from its name with the same vocabulary the exporter uses to show pin
  alternates — see `docs/multi-unit-and-pin-functions.md`.

Where a rule needs a quantity the design does not state (an LED `Vf`, a
rail voltage, a part's current limit), it declines to fire rather than
guessing.

| Check               | Where                                            | What it finds                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| ------------------- | ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Rule-based ERC**  | `synth-validate::run_erc`                        | `E-SYNTH-*` rules: connectivity, I²C pullups, power output shorts, clock/reset/boot, decoupling counts, etc. `E-SYNTH-POWER-001` auto-inserts missing decoupling caps as a patch                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| **Value-based ERC** | `synth-validate::value` + `E-SYNTH-CRYSTAL-001`  | parses component **values** (SI-prefix aware) and reasons about magnitude, not just topology. `E-SYNTH-CRYSTAL-001` warns when a crystal's two load caps differ by > 10% (e.g. 22 pF vs 33 pF). The `value` parser (`parse_resistance` / `parse_capacitance`) is the shared foundation for future divider-ratio, derating, and power checks                                                                                                                                                                                                                                                                                                           |
| **KiCad ERC**       | `synth-kicad::run_kicad_erc`                     | shells out to `kicad-cli sch erc`, parses the JSON report per sheet                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| **Aesthetic ERC**   | `synth-kicad::schem_erc`                         | in-house `E-SYNTH-SCHEM-001..015`: inverted power symbol (001), wire-crossing density > 5 (002), decoupling cap further than 25 mm of *body gap* from the IC it sits nearest (003), long explicit net > 100 mm (004), junction fan-out > 3 lines (005), junction dot on a foreign net's wire (006), content outside the selected sheet size (007), non-uppercase net name (008), net name > 16 chars (009), ambiguous `VCC`/`VDD`/`VPP` power rail (010). 005–007 encode Sierra Circuits' "Schematic Design Rules" (protoexpress.com/kb/schematic-design-rules/); 008–010 encode the "Rules and guidelines for drawing good schematics" thread (electronics.stackexchange.com/questions/28251). 011 text-run overlap, 012 sheet fill ratio, 013 group regions overlapping or a component outside its region, 014 auto-named net (`net_N`) rendered on the sheet, 015 a group with no `notes` block. `E-SYNTH-VALUE-001` (a generic passive with no `value`) is an error, not aesthetic. Every finding resolves a source span through the entity it names (`attach_locations`) |
| **Schematic DRC**   | `route::drc_wire_crosses_unrelated_pin_terminal` | any routed wire passing over a pin terminal of a component its net does _not_ connect to (the `test_no_wire_crosses_unrelated_pin_terminals` gate)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |

---

## 5. Browser preview (`synth-web` + `synth preview`)

`synth preview <file>` runs an axum HTTP server:

- **`GET /events`** — SSE stream: on every recompile a `BoardView`
  payload (board + layout + diagnostics) is pushed; the Leptos/WASM
  viewer re-renders reactively (no two-language schema drift).
- **SVG schematic** rendered via Leptos `view!` from the shared
  `Layout` (same router and power symbols as the KiCad export).
- **Drag-and-drop** re-positioning; on drag end the browser
  `POST /api/v1/layout/save` with the refdes-keyed offsets, which the
  server writes to the `<design>.synth.layout.toml` sidecar so a later
  reload restores the positions.
- **Diagnostics + Inspector** panels with click-to-cross-probe between
  a diagnostic and the offending component/net.

---

## 6. CLI commands (`synth`)

| Command                       | Purpose                                                                                                           |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `synth validate <file>`       | run parser + IR + ERC, emit diagnostics                                                                           |
| `synth dump-ast` / `dump-ir`  | inspect the AST / IR                                                                                              |
| `synth export-kicad <file>`   | run auto-layout + export the full KiCad project (§3)                                                              |
| `synth fix <file>`            | apply highest-confidence suggested patches (incl. auto-inserted decoupling)                                       |
| `synth layout <file>`         | print the auto-layout JSON (placements, wires, labels, flags)                                                     |
| `synth place` / `synth route` | PCB placement / routing stages                                                                                    |
| `synth drc <file>`            | design-rule check                                                                                                 |
| `synth schema <kind>`         | emit a JSON schema for a protocol artifact                                                                        |
| `synth preview <file>`        | live browser viewer (§5)                                                                                          |
| `synth mcp`                   | stdio/SSE MCP server exposing `synth_validate`, `synth_apply_patch`, `synth_preview_schematic`, `synth_export`, … |

---

## 7. Where the code lives

| Concern                                                  | Crate / module                                                                              |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Layout engine, clusters, routing, DRC                    | `crates/synth-layout` (`lib.rs`, `patterns/`, `route/`, `ops.rs`, `score.rs`, `sidecar.rs`) |
| KiCad .kicad_sch / .kicad_sym / export                   | `crates/synth-kicad` (`schematic.rs`, `symbol_lib.rs`, `export.rs`, `sexp.rs`)              |
| Pin reconciliation & PWR_FLAG                            | `crates/synth-kicad/src/pin_reconcile.rs`                                                   |
| Pin functions (alternates)                               | `crates/synth-kicad/src/alternates.rs`                                                      |
| Multi-unit symbols                                       | `crates/synth-layout/src/kicad_lib_loader.rs` (`symbol_units`), `symbol_lib.rs`, `schematic.rs` |
| Pin-mux ERC                                              | `crates/synth-validate/src/lib.rs` + `deep_erc.rs` (`E-SYNTH-PINMUX-00x`)                    |
| Aesthetic + KiCad ERC                                    | `crates/synth-kicad/src/schem_erc.rs`, `erc_validate.rs`                                    |
| Rule-based ERC / decoupling auto-insert                  | `crates/synth-validate`                                                                     |
| Value parsing + `E-SYNTH-CRYSTAL-001`                    | `crates/synth-validate/src/value.rs`, `crates/synth-validate/src/lib.rs`                    |
| Browser preview                                          | `crates/synth-web` (`schematic.rs`, `state.rs`)                                             |
| Server + CLI surface                                     | `crates/synth-cli` (`main.rs`, `preview.rs`)                                                |
| Registry of parts (pins, `required_decoupling`, symbols) | `registry/parts/`                                                                           |

---

See [`protocol-v1.0.md`](protocol-v1.0.md) for the diagnostic wire
format, and [`kicad-workflows.md`](kicad-workflows.md) for the
post-export KiCad review/validation workflow (ERC gates, BOM and
sourcing discipline, fabrication outputs)._
