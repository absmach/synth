// SPDX-License-Identifier: Apache-2.0

//! Stage B candidate placement (§7.8.5 of the implementation plan).
//!
//! The layout pipeline is staged: Stage A recognizes motifs
//! ([`crate::build_clusters`]), Stage B places the recognized
//! clusters on the sheet, Stage C routes and labels, Stage D scores.
//! This module is the Stage B seam: a [`Placer`] turns a clustered
//! board into a placed [`Layout`] (component positions and sheet
//! size only — routing, labels and power flags are applied by
//! [`crate::layout_with_placer`] afterwards, identically for every
//! placer).
//!
//! V1 ships exactly one implementation, [`NativeSemanticPlacer`]:
//! the Sugiyama + Brandes–Köpf cluster placer with strong/weak net
//! weighting (§7.7.4, §7.8.4). The trait exists so the Stage D
//! scorer and CI gate have a real interface to rank placers
//! *against* — a second placer (e.g. a CEM-based macro floorplanner)
//! can be added later without touching Stage C/D/E or either
//! consumer crate. No second placer is planned for V1 (§7.8.9).

use synth_ir::Board;

use crate::{Cluster, Layout};

/// Turns Stage A clusters into placed component geometry (Stage B).
///
/// Implementations must be deterministic: identical `board` and
/// `clusters` produce a byte-identical [`Layout`]. The scorer gate
/// in CI (§7.8.11) enforces this on the reference corpus.
pub trait Placer {
    /// Stable identifier for diagnostics and scoring reports.
    fn name(&self) -> &'static str;

    /// Place every cluster from `board`'s Stage A recognition pass.
    ///
    /// Returns a [`Layout`] whose `components` carry positions and
    /// rotations and whose `sheet_size` is computed; wires, labels,
    /// power flags and junctions are filled in later by the shared
    /// pipeline and must be left empty.
    fn place(&self, board: &Board, clusters: &[Cluster]) -> Layout;
}

/// The V1 native placer: barycenter cluster ordering with
/// strong/weak semantic net weighting (§7.8.4) and Brandes–Köpf
/// coordinate assignment (§7.7.4), snapping to the 2.54 mm grid.
///
/// Delegates to the same `place_clusters` pipeline the crate has
/// shipped since Phase 5.7 — extracting it behind this trait changed
/// no output (the snapshot and golden suites stayed byte-identical).
#[derive(Debug)]
pub struct NativeSemanticPlacer;

impl Placer for NativeSemanticPlacer {
    fn name(&self) -> &'static str {
        "native-semantic"
    }

    fn place(&self, board: &Board, clusters: &[Cluster]) -> Layout {
        crate::place_clusters(board, clusters)
    }
}

/// The placer [`crate::layout`] uses. A single static instance:
/// placement is stateless.
pub fn default_placer() -> NativeSemanticPlacer {
    NativeSemanticPlacer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_placer_reports_a_stable_name() {
        assert_eq!(default_placer().name(), "native-semantic");
    }
}
