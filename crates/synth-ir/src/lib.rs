// SPDX-License-Identifier: Apache-2.0

//! Canonical semantic IR for the Synth EDA compiler.
//!
//! Everything downstream of the parser — ERC, placement, routing,
//! DRC, KiCad export, SynthJSON view projection — consumes [`Board`]
//! and the indexed types ([`ComponentId`], [`PinId`], [`NetId`]).
//!
//! Three invariants the IR maintains, per plan §0.1 and §4:
//!
//! 1. **Rust types are truth.** SynthJSON is a projection of the IR;
//!    the IR does not round-trip through JSON to reconstruct state.
//! 2. **Integer base units everywhere.** All physical quantities use
//!    integer base units ([`units::Length`] nm, [`units::Voltage`] µV,
//!    etc.). No floating-point geometry inside the IR.
//! 3. **Every IR node carries an `originating_span`** for diagnostic
//!    provenance back to the SynthSpec source.

#![forbid(unsafe_code)]

pub mod board;
pub mod imports;
pub mod lower;
pub mod multiboard;
pub mod power_domains;
pub mod units;

pub use board::{
    Board, Component, ComponentId, DiffPair, Keepout, Net, NetEndpoint, NetId, PinId,
    PlacementConstraint, PlacementEdge, PlacementPriority, PlacementRegion, PlacementSide,
};
pub use imports::{
    resolve as resolve_imports, FsImportLoader, ImportLoadError, ImportLoader, MemoryImportLoader,
    ResolveResult as ImportResolveResult, MAX_IMPORT_DEPTH, MAX_IMPORT_SIZE,
};
pub use lower::{lower, LowerResult};
pub use multiboard::{InterBoardPinMapping, MultiBoardProject, MultiBoardValidationResult};
pub use power_domains::{infer_power_domains, PowerDomainKind, PowerDomainMap};
/// Re-export of the registry's `Pin` type so consumers of the IR
/// don't need to depend on `synth-registry` directly.
pub use synth_registry::Pin;
pub use units::{Capacitance, Current, Frequency, Impedance, Length, Resistance, Voltage};
