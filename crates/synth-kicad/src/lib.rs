// SPDX-License-Identifier: Apache-2.0

//! KiCad 8 export for the Synth EDA compiler.
//!
//! Consumes a [`synth_ir::Board`] and writes a self-contained KiCad
//! project directory:
//!
//! ```text
//! <out>/
//!   <board>.kicad_pro       project metadata (JSON)
//!   <board>.kicad_sch       schematic (s-expression)
//!   <board>.kicad_sym       embedded symbol library (s-expression)
//!   bom.csv                 bill of materials
//! ```
//!
//! Design properties the exporter holds to per plan §0.1 and §6.2:
//!
//! 1. **Deterministic UUIDs.** Every entity (sheet, symbol instance,
//!    wire) receives a uuid v5 derived from a stable name. Re-running
//!    the exporter on identical IR produces byte-identical output —
//!    no diff churn.
//! 2. **Embedded symbol library.** Synth does not import KiCad's
//!    public symbol libraries (that would re-couple us to upstream).
//!    Instead we synthesize a minimal `.kicad_sym` file per project
//!    by walking the registry parts referenced in the IR.
//! 3. **One-way.** KiCad is the destination, not the source. The
//!    exporter never reads `.kicad_*` files.
//!
//! Phase 4 V1 layout choices that are intentional placeholders:
//!
//! - Components placed in a horizontal row, 25.4 mm apart, all on
//!   schematic page 1.
//! - Symbols drawn as rectangles with pins evenly spaced on the
//!   left side regardless of pin direction.
//! - Wires drawn as star topology per net (first endpoint to each
//!   other endpoint, no orthogonal routing).
//!
//! ELK-driven auto-layout, real symbol artwork, and orthogonal wire
//! routing are Phase 4.5+.

#![forbid(unsafe_code)]
// `build_footprint_instance` (pcb.rs) is intentionally a long, linear
// s-expression builder; splitting it would obscure the 1:1 mapping to the
// KiCad file format.
#![allow(clippy::too_many_lines)]

mod bom;
mod erc_validate;
mod export;
mod fab;
pub mod import;
mod pcb;
pub mod pin_reconcile;
mod pnp;
pub mod schem_erc;
pub mod schematic;
pub mod sexp;
mod symbol_lib;
mod uuid_v5;

pub use erc_validate::{run_kicad_erc, ErcRunError, KicadErcItem, KicadErcViolation};
pub use export::{export, export_with_sidecar, ExportError, ExportResult};
pub use fab::{run as run_fab, FabArtifacts, FabError, FabRequest};
pub use import::{import_project, ImportError, NormalisedRecord, IMPORTER_VERSION};
pub use pin_reconcile::physical_terminal;
pub use pnp::build_pnp_csv;
pub use schem_erc::{
    check as check_schem_erc, check_with_config as check_schem_erc_with_config, SchemErcConfig,
};
pub use symbol_lib::build_pwr_flag_fallback;
