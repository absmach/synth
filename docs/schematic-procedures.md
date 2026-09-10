# Synth Schematic Procedures

**Scope:** how Synth turns a `.synth` design into a human-readable,
KiCad-importable schematic — the auto-layout pipeline, KiCad export,
ERC / DRC validation, the live browser preview, and the CLI commands
that drive them.

**Status:** describes the implemented pipeline as of 2026-08-25, cross-checked
against `crates/synth-layout`, `crates/synth-kicad`, `crates/synth-web`,
`crates/synth-cli`, and `crates/synth-validate`.

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
   ├──> synth-web (live preview, drag-and-drop)          (§5)
   ├──> synth-kicad::export      .kicad_sch/.kicad_sym/.kicad_pcb/bom (§3)
   └──> ERC / DRC validation                             (§4)
```

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
   | 8    | `Singleton`    | every remaining unclaimed component                                                                               |

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

6. **`route_and_label`** — for every signal net:
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
- **Reference/Value field auto-placement** away from pins
  (`side_pin_counts` / `choose_field_sides` / `field_anchor`)
- **hidden sourcing fields** on every instance — `MPN` and `LCSC`
  copied from the registry part (empty string when unknown) — so the
  exported `.kicad_sch` is self-sufficient for KiCad BOM tooling and
  the JLCPCB/DigiKey plugin ecosystem without the sidecar `bom.csv`
- **power symbols** on rails + a stub `wire`
- **`no_connect` markers** on unreached physical pins and
  **`PWR_FLAG` drivers** on undriven power nets (`pin_reconcile`)
- **physical pin reconciliation** (`pin_reconcile.rs`): fans undeclared
  `power_in` legs onto the matching rail and marks unused GPIOs
  no-connect, so KiCad ERC doesn't report `pin_not_connected` /
  `power_pin_not_driven`
- **title block** with board name, `revision` (from the board
  statement), deterministic notes ("Generated by synth-eda",
  component/net counts), and — when the board declares
  `manufacturer "…"` — a `Fab: <manufacturer>` note documenting the
  fabrication target (the design-authority `company` field is
  deliberately left blank)
- **deterministic UUID v5** for every entity
  (`derive_entity_uuid(project, kind, name)`), so unchanged input
  re-exports byte-identically for stable diffs.

---

## 4. Validation: ERC, aesthetic ERC, and DRC

| Check               | Where                                            | What it finds                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| ------------------- | ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Rule-based ERC**  | `synth-validate::run_erc`                        | `E-SYNTH-*` rules: connectivity, I²C pullups, power output shorts, clock/reset/boot, decoupling counts, etc. `E-SYNTH-POWER-001` auto-inserts missing decoupling caps as a patch                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| **Value-based ERC** | `synth-validate::value` + `E-SYNTH-CRYSTAL-001`  | parses component **values** (SI-prefix aware) and reasons about magnitude, not just topology. `E-SYNTH-CRYSTAL-001` warns when a crystal's two load caps differ by > 10% (e.g. 22 pF vs 33 pF). The `value` parser (`parse_resistance` / `parse_capacitance`) is the shared foundation for future divider-ratio, derating, and power checks                                                                                                                                                                                                                                                                                                           |
| **KiCad ERC**       | `synth-kicad::run_kicad_erc`                     | shells out to `kicad-cli sch erc`, parses the JSON report per sheet                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| **Aesthetic ERC**   | `synth-kicad::schem_erc`                         | in-house `E-SYNTH-SCHEM-001..010`: inverted power symbol (001), wire-crossing density > 5 (002), decoupling cap > 15 mm from its IC (003), long explicit net > 100 mm (004), junction fan-out > 3 lines (005), junction dot on a foreign net's wire (006), content outside the selected sheet size (007), non-uppercase net name (008), net name > 16 chars (009), ambiguous `VCC`/`VDD`/`VPP` power rail (010). 005–007 encode Sierra Circuits' "Schematic Design Rules" (protoexpress.com/kb/schematic-design-rules/); 008–010 encode the "Rules and guidelines for drawing good schematics" thread (electronics.stackexchange.com/questions/28251) |
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
