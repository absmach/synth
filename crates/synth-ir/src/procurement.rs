// SPDX-License-Identifier: Apache-2.0

//! Frozen, per-instance procurement assignments.
//!
//! This module deliberately sits in the IR crate: the same assignment record
//! is consumed by validation, KiCad export, the CLI, and SaaS adapters.  A
//! registry part describes a reusable candidate; a [`ProcurementLock`] records
//! the exact candidate chosen for each design instance and variant.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use synth_registry::Part;

use crate::{Board, Component, ComponentId, Variant};

/// How a component is handled for a particular build variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssemblyDisposition {
    SupplierAssembly,
    HandAssembly,
    Dnp,
    Mechanical,
}

/// One immutable assignment of a design instance to a reviewed registry part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartAssignment {
    pub component_id: ComponentId,
    pub refdes: String,
    pub variant: String,
    pub part_id: Option<String>,
    pub manufacturer_part_number: Option<String>,
    pub lcsc_part_number: Option<String>,
    pub footprint: Option<String>,
    pub disposition: AssemblyDisposition,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substitution_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_reason: Option<String>,
}

/// Versioned artifact that freezes all assignments used by a build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcurementLock {
    pub schema_version: String,
    pub design_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_revision: Option<String>,
    pub variant: String,
    pub assignments: Vec<PartAssignment>,
}

/// Requirements used to rank already-loaded registry candidates. Ranking is
/// advisory; only [`lock_board`] turns a selected candidate into a release
/// artifact.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateRequirements {
    pub lcsc_part_number: Option<String>,
    pub manufacturer_part_number: Option<String>,
    pub footprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedCandidate {
    pub part_id: String,
    pub score: u8,
    pub verified: bool,
}

/// Rank candidates deterministically, rewarding exact identity and package
/// matches while keeping unreviewed entries behind reviewed ones.
pub fn rank_candidates(
    requirements: &CandidateRequirements,
    candidates: &[Part],
) -> Vec<RankedCandidate> {
    let mut ranked: Vec<_> = candidates
        .iter()
        .map(|part| {
            let mut score = 0;
            if requirements
                .lcsc_part_number
                .as_deref()
                .is_some_and(|value| part.lcsc_pn.as_deref() == Some(value))
            {
                score += 4;
            }
            if requirements
                .manufacturer_part_number
                .as_deref()
                .is_some_and(|value| part.mpn.as_deref() == Some(value))
            {
                score += 3;
            }
            if requirements
                .footprint
                .as_deref()
                .is_some_and(|value| part.kicad_footprint.as_deref() == Some(value))
            {
                score += 2;
            }
            let verified = !part.is_unverified();
            if verified {
                score += 1;
            }
            RankedCandidate {
                part_id: part.id.to_string(),
                score,
                verified,
            }
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.part_id.cmp(&b.part_id))
    });
    ranked
}

/// Preserve a reviewable substitution trail instead of overwriting an
/// approved assignment in place.
pub fn record_substitution(
    original: &PartAssignment,
    mut replacement: PartAssignment,
    reason: impl Into<String>,
) -> PartAssignment {
    replacement.substitution_of.clone_from(&original.part_id);
    replacement.change_reason = Some(reason.into());
    replacement
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcurementError {
    UnresolvedPart { refdes: String },
    UnverifiedPart { refdes: String },
    MissingSupplierIdentity { refdes: String },
    MissingFootprint { refdes: String },
    MissingDisposition { refdes: String },
}

impl std::fmt::Display for ProcurementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnresolvedPart { refdes } => write!(f, "{refdes}: no registry part is assigned"),
            Self::UnverifiedPart { refdes } => write!(f, "{refdes}: registry part is not reviewed"),
            Self::MissingSupplierIdentity { refdes } => write!(
                f,
                "{refdes}: supplier assembly requires an exact MPN and LCSC part number"
            ),
            Self::MissingFootprint { refdes } => write!(
                f,
                "{refdes}: procurement lock requires a reviewed KiCad footprint"
            ),
            Self::MissingDisposition { refdes } => write!(
                f,
                "{refdes}: non-supplier assembly requires an explicit disposition"
            ),
        }
    }
}

impl std::error::Error for ProcurementError {}

/// Build a lock for the default variant. Any unresolved populated instance is
/// rejected instead of being silently turned into a generic purchasing row.
pub fn lock_board(
    board: &Board,
    variant: Option<&Variant>,
) -> Result<ProcurementLock, ProcurementError> {
    lock_board_with_dispositions(board, variant, &BTreeMap::new())
}

/// Build a lock while explicitly declaring hand-assembly or mechanical items.
pub fn lock_board_with_dispositions(
    board: &Board,
    variant: Option<&Variant>,
    dispositions: &BTreeMap<String, AssemblyDisposition>,
) -> Result<ProcurementLock, ProcurementError> {
    let variant_name = variant.map_or_else(|| "default".to_string(), |v| v.name.clone());
    let variant_dnp: BTreeSet<&str> =
        variant.map_or_default(|v| v.dnp.iter().map(String::as_str).collect());
    let mut assignments = Vec::with_capacity(board.components.len());
    for component in &board.components {
        let disposition = if component.dnp || variant_dnp.contains(component.refdes.as_str()) {
            AssemblyDisposition::Dnp
        } else if let Some(disposition) = dispositions.get(&component.refdes) {
            *disposition
        } else {
            AssemblyDisposition::SupplierAssembly
        };
        assignments.push(assignment_for(component, &variant_name, disposition)?);
    }
    assignments.sort_by(|a, b| a.refdes.cmp(&b.refdes));
    Ok(ProcurementLock {
        schema_version: "synth.procurement-lock.v1".to_string(),
        design_name: board.name.clone(),
        design_revision: board.revision.clone(),
        variant: variant_name,
        assignments,
    })
}

fn assignment_for(
    component: &Component,
    variant: &str,
    disposition: AssemblyDisposition,
) -> Result<PartAssignment, ProcurementError> {
    let part = component.part.as_ref();
    if disposition != AssemblyDisposition::Mechanical && part.is_none() {
        return Err(ProcurementError::UnresolvedPart {
            refdes: component.refdes.clone(),
        });
    }
    let verified = part.is_some_and(|p| !p.is_unverified());
    if part.is_some() && !verified && disposition != AssemblyDisposition::Dnp {
        return Err(ProcurementError::UnverifiedPart {
            refdes: component.refdes.clone(),
        });
    }
    if disposition == AssemblyDisposition::SupplierAssembly
        && !part.is_some_and(|p| p.mpn.is_some() && p.lcsc_pn.is_some())
    {
        return Err(ProcurementError::MissingSupplierIdentity {
            refdes: component.refdes.clone(),
        });
    }
    if disposition != AssemblyDisposition::Dnp && part.is_none_or(|p| p.kicad_footprint.is_none()) {
        return Err(ProcurementError::MissingFootprint {
            refdes: component.refdes.clone(),
        });
    }
    Ok(PartAssignment {
        component_id: component.id,
        refdes: component.refdes.clone(),
        variant: variant.to_string(),
        part_id: part.map(|p| p.id.to_string()),
        manufacturer_part_number: part.and_then(|p| p.mpn.clone()),
        lcsc_part_number: part.and_then(|p| p.lcsc_pn.clone()),
        footprint: part.and_then(|p| p.kicad_footprint.clone()),
        disposition,
        verified,
        substitution_of: None,
        change_reason: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{component, part, plain_board};

    #[test]
    fn lock_rejects_unresolved_populated_instance() {
        let mut board = plain_board(vec![component(0, "R1", part("resistor", vec![]))], vec![]);
        board.components[0].part = None;
        let error = lock_board(&board, None).expect_err("generic part must remain unresolved");
        assert!(matches!(error, ProcurementError::UnresolvedPart { .. }));
    }

    #[test]
    fn lock_records_exact_supplier_identity_and_variant_dnp() {
        let mut resistor = part("resistor", vec![]);
        resistor.mpn = Some("MFG-10K".into());
        resistor.lcsc_pn = Some("C10000".into());
        resistor.kicad_footprint = Some("Resistor_SMD:R_0603_1608Metric".into());
        let mut board = plain_board(vec![component(0, "R1", resistor)], vec![]);
        board.variants.push(Variant {
            name: "lite".into(),
            description: None,
            dnp: vec!["R1".into()],
        });
        let lock = lock_board(&board, Some(&board.variants[0])).expect("valid lock");
        assert_eq!(lock.schema_version, "synth.procurement-lock.v1");
        assert_eq!(lock.assignments[0].disposition, AssemblyDisposition::Dnp);
        assert_eq!(
            lock.assignments[0].lcsc_part_number.as_deref(),
            Some("C10000")
        );
    }

    #[test]
    fn ranking_prefers_exact_reviewed_identity() {
        let mut exact = part("exact", vec![]);
        exact.mpn = Some("MFG-10K".into());
        exact.lcsc_pn = Some("C10000".into());
        exact.kicad_footprint = Some("R0603".into());
        let other = part("other", vec![]);
        let ranked = rank_candidates(
            &CandidateRequirements {
                lcsc_part_number: Some("C10000".into()),
                manufacturer_part_number: Some("MFG-10K".into()),
                footprint: Some("R0603".into()),
            },
            &[other, exact],
        );
        assert_eq!(ranked[0].part_id, "exact");
        assert!(ranked[0].score > ranked[1].score);
    }
}
