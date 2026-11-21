# Synth Code Changes Documentation

This document records the architectural and functional code changes introduced in the codebase, rebased cleanly on top of latest upstream `origin/main` in commit [`d1cea0de`](file:///D:/AYSynth) on branch `main`.

---

## 1. Summary of Changes

| Area | Component / Crate | Key Additions / Modifications |
| :--- | :--- | :--- |
| **Placement Engine** | [`crates/synth-place`](file:///D:/AYSynth/crates/synth-place) | Added **Directional Net Affinity Engine**, **Physical Pad Geometry Classification**, and **Non-destructive Placement Analysis**; added integration & baseline test suites. |
| **KiCad Windows Discovery** | [`synth-cli`](file:///D:/AYSynth/crates/synth-cli), [`synth-drc`](file:///D:/AYSynth/crates/synth-drc), [`synth-kicad`](file:///D:/AYSynth/crates/synth-kicad), [`synth-layout`](file:///D:/AYSynth/crates/synth-layout), [`synth-registry`](file:///D:/AYSynth/crates/synth-registry), [`synth-router`](file:///D:/AYSynth/crates/synth-router) | Automated discovery of KiCad 8.0, 9.0, and 10.0 executables (`kicad-cli.exe`), bundled Python interpreters, footprint libraries (`.kicad_mod`), symbol tables, and router plugins across standard Windows drive installations (`C:\`, `D:\`). |
| **Schematic Hierarchy** | [`synth-kicad`](file:///D:/AYSynth/crates/synth-kicad) | Fixed root sheet UUID symbol path formatting and single-sheet detection condition. |
| **Electrical Rule Check (ERC)** | [`synth-validate`](file:///D:/AYSynth/crates/synth-validate) | Filtered ground domain nets in component supply voltage resolution to avoid false positive pin driver conflicts. |

---

## 2. Detailed Breakdown by Crate

### A. Placement Engine Enhancements (`crates/synth-place`)

#### 1. Directional Net Affinity Engine
* **File**: [`crates/synth-place/src/directional_affinity.rs`](file:///D:/AYSynth/crates/synth-place/src/directional_affinity.rs) (New, 777 lines)
* **Purpose**: Computes spatial attractive pull vectors between major ICs and connected peripheral components based on the actual physical exit face (North, South, East, West) of the connecting pins.
* **Key Components**:
  - `DirectionalAffinityEngine`: Analyzes board nets, discovers major multi-pin ICs, and builds affinity vectors targeting connected passives and peripheral ICs.
  - `AffinityVector`: Encapsulates primary direction (`PadSide`), vector strength/weight (scaling with net significance and bus coupling), and pin exit coordinates.
  - `ComponentAffinity`: Aggregates pin-level affinities per destination component.
  - `placement_penalty(&self, placements)`: Calculates placement penalty score by penalizing components placed on opposite faces of their pin exit direction, discouraging wire wrap-arounds and routing congestion.
  - Power/ground net filtering: Automatically ignores GND, VCC/VDD rails, and high-degree nets (such as global resets) that would otherwise skew placement geometry.

#### 2. Physical Pad Geometry & Side Classification
* **File**: [`crates/synth-place/src/pad_geometry.rs`](file:///D:/AYSynth/crates/synth-place/src/pad_geometry.rs) (New, 509 lines)
* **Purpose**: Extracts real physical pad locations from footprints and classifies each pad's exit edge relative to the component body center.
* **Key Components**:
  - `PadSide`: Enum representing `North`, `South`, `East`, `West`, and `Center` (thermal / exposed ground pads).
  - `PhysicalPad`: Records pin ID, pad center point, width, height, and computed side.
  - `classify_pad_side(rel_x, rel_y, width, height)`: Geometric quadrant classifier with thresholding for square and rectangular IC packages.
  - Supports QFP, QFN, SOIC, TSSOP, BGA, and edge connector orientations, taking package rotation (`0°`, `90°`, `180°`, `270°`) into account.

#### 3. Non-Destructive Placement Analyzer
* **File**: [`crates/synth-place/src/analysis.rs`](file:///D:/AYSynth/crates/synth-place/src/analysis.rs) (New, 548 lines)
* **Purpose**: Provides a read-only, deterministic evaluation of placement quality without mutating the board or component coordinates.
* **Key Metrics**:
  - **Geometric Integrity**: Detects courtyard overlaps and components outside board edge bounds.
  - **Connectivity & Wirelength**: Calculates half-perimeter wire length (HPWL) for non-power signal nets, average net spans, and maximum span outliers.
  - **Congestion & Density**: Evaluates quadrant component density, pin distribution, and estimated channel utilization.
  - **Critical Subsystems**: Checks proximity of decoupling capacitors to supply pins and RF section isolation.
  - Deterministic report generator implementing `std::fmt::Display` for diagnostic outputs.

#### 4. Crate Interface & Keepout Center Handling
* **File**: [`crates/synth-place/src/lib.rs`](file:///D:/AYSynth/crates/synth-place/src/lib.rs)
* **Changes**:
  - Exposed new submodules: `analysis`, `directional_affinity`, and `pad_geometry`.
  - Re-exported core types: `PlacementAnalysisReport`, `DirectionalAffinityEngine`, `AffinityVector`, `ComponentPadGeometry`, `PadSide`, `PhysicalPad`.
  - Updated `keepout_circles`: Added support for keepout definitions specifying explicit coordinates (`keepout.at`) and named anchor components (`keepout.anchor`).

#### 5. Verification Test Suites
* **File**: [`crates/synth-place/tests/baseline_analysis.rs`](file:///D:/AYSynth/crates/synth-place/tests/baseline_analysis.rs) (New, 87 lines)
  - Regression tests confirming byte-deterministic output on reference designs (`env_logger.synth`).
* **File**: [`crates/synth-place/tests/directional_affinity_integration.rs`](file:///D:/AYSynth/crates/synth-place/tests/directional_affinity_integration.rs) (New, 71 lines)
  - Integration test verifying affinity calculations on real fixtures (`ethernet_phy.synth`).

---

### B. Windows Toolchain & KiCad Discovery

Standard Linux/macOS paths (`/usr/share/kicad`, `/usr/bin/kicad-cli`) fail on Windows systems unless environment variables are explicitly passed. Automated discovery paths were added across the compiler pipeline:

| Crate | Modified File | Discovery Enhancements |
| :--- | :--- | :--- |
| **`synth-cli`** | [`crates/synth-cli/src/main.rs`](file:///D:/AYSynth/crates/synth-cli/src/main.rs#L6326) | Replaced direct `kicad-cli` process invocation with `synth_drc::kicad_cli::binary()`. |
| **`synth-drc`** | [`crates/synth-drc/src/kicad_cli.rs`](file:///D:/AYSynth/crates/synth-drc/src/kicad_cli.rs#L21-L36) | Added search fallbacks across `C:\Program Files\KiCad\{10.0, 9.0, 8.0}\bin\kicad-cli.exe` and `D:\Program Files\KiCad\{10.0, 9.0, 8.0}\bin\kicad-cli.exe`. |
| **`synth-kicad`** | [`crates/synth-kicad/src/fab.rs`](file:///D:/AYSynth/crates/synth-kicad/src/fab.rs#L146-L165) | Added Windows binary search paths for fabrication output exports. |
| **`synth-layout`** | [`crates/synth-layout/src/footprint_resolve.rs`](file:///D:/AYSynth/crates/synth-layout/src/footprint_resolve.rs#L111-L124) | Added Windows footprint paths (`share\kicad\footprints`) for standard installations on drives `C:` and `D:`. |
| **`synth-layout`** | [`crates/synth-layout/src/kicad_footprint_loader.rs`](file:///D:/AYSynth/crates/synth-layout/src/kicad_footprint_loader.rs#L161-L170) | Added Windows system paths for `.kicad_mod` direct file loading. |
| **`synth-layout`** | [`crates/synth-layout/src/kicad_lib_loader.rs`](file:///D:/AYSynth/crates/synth-layout/src/kicad_lib_loader.rs#L44-L52) | Added search locations for KiCad `fp-lib-table` configuration files. |
| **`synth-registry`** | [`crates/synth-registry/src/kicad_pin_check.rs`](file:///D:/AYSynth/crates/synth-registry/src/kicad_pin_check.rs#L50-L58) | Added Windows symbol library directories (`share\kicad\symbols`). |
| **`synth-router`** | [`crates/synth-router/src/capability.rs`](file:///D:/AYSynth/crates/synth-router/src/capability.rs#L372-L432) | Added resolution for in-repo `tools/KiCadRoutingTools`, `LOCALAPPDATA`, Windows Python paths (`.venv\Scripts\python.exe`), and KiCad bundled Python binaries. |

---

### C. Compiler Core Bug Fixes

#### 1. Schematic Export Hierarchy & Sheet Detection
* **File**: [`crates/synth-kicad/src/schematic.rs`](file:///D:/AYSynth/crates/synth-kicad/src/schematic.rs)
* **Changes**:
  - In `build_schematic_from_layout`, initialized root sheet `symbol_path` to `Some(format!("/{root_uuid}"))` instead of `None`, preserving sheet UUID nesting.
  - In `build_sheet_schematic`, refined the single-sheet determination check from `render.symbol_path.is_none()` to `render.name.is_none() && render.root_furniture.is_none()`, preventing incorrect schematic sheet splitting.

#### 2. Deep ERC Component Supply Voltage Check
* **File**: [`crates/synth-validate/src/deep_erc.rs`](file:///D:/AYSynth/crates/synth-validate/src/deep_erc.rs)
* **Changes**:
  - In `component_supply_v`, added an explicit check:
    ```rust
    if domains.get(net).is_some_and(synth_ir::PowerDomainKind::is_ground) {
        continue;
    }
    ```
  - Prevents ground nets from being considered positive voltage supply drivers during deep electrical validation.

---

## 3. Git Commit Reference

* **Commit**: [`d1cea0ded0b556f7ef57d9f7ad957814a07d3910`](file:///D:/AYSynth) (rebased on `origin/main` at `404ab92c`)
* **Message**: `feat(place): add directional affinity, pad geometry, and placement analysis`
* **Changes**: 16 files changed, 2,161 insertions(+), 37 deletions(-)
