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
pub mod clusters;
pub mod diff_pairs;
pub mod imports;
pub mod lower;
pub mod modules;
pub mod multiboard;
pub mod power_domains;
pub mod procurement;
pub mod units;

#[cfg(test)]
pub(crate) mod test_support;

pub use board::{
    Board, Component, ComponentId, Couple, DiffPair, EdgeRate, EscapeIntent, EscapeMethod, Group,
    Keepout, Net, NetClass, NetEndpoint, NetId, Note, PinId, PlacementConstraint, PlacementEdge,
    PlacementPriority, PlacementRegion, PlacementSide, ProcessRequirement, SchematicOverflow,
    SchematicPaper, Stackup, StackupLayer, Variant,
};
pub use clusters::{
    ClusterKind, ClusterMember, FunctionalCluster, MemberBinding, MemberRole, recognize_clusters,
};
pub use diff_pairs::{
    PairConnection, ResolvedPair, pair_connections, resolve_pairs, resolve_pairs_with_budgets,
};
pub use imports::{
    FsImportLoader, ImportLoadError, ImportLoader, MAX_IMPORT_DEPTH, MAX_IMPORT_SIZE,
    MemoryImportLoader, ResolveResult as ImportResolveResult, resolve as resolve_imports,
};
pub use lower::{LowerResult, lower};
pub use modules::{BusBundle, Deviation, ModuleDesc, expand as expand_modules};
pub use multiboard::{InterBoardPinMapping, MultiBoardProject, MultiBoardValidationResult};
pub use power_domains::{PowerDomainKind, PowerDomainMap, infer_power_domains};
pub use procurement::{
    AssemblyDisposition, CandidateRequirements, PartAssignment, ProcurementError, ProcurementLock,
    lock_board, lock_board_with_dispositions, rank_candidates, record_substitution,
};
/// Re-export of the registry's `Pin` type so consumers of the IR
/// don't need to depend on `synth-registry` directly.
pub use synth_registry::Pin;
pub use units::{
    Capacitance, Current, DielectricConstant, Frequency, Impedance, Length, Resistance, Voltage,
};
