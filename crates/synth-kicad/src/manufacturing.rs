// SPDX-License-Identifier: Apache-2.0

//! Strict JLCPCB manufacturing exports derived from one procurement lock.
//!
//! The generic BOM/PnP writers remain permissive for editable designs. These
//! profile writers are deliberately strict: they only emit supplier-assembly
//! assignments present in a [`synth_ir::ProcurementLock`], reject missing
//! placements and inner-layer parts, and retain correction provenance.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use synth_geometry::{nm_to_mm, Layer};
use synth_ir::{AssemblyDisposition, Board, ProcurementLock};
use synth_place::Placement;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacementCorrection {
    /// Exact LCSC part number, preferred over a footprint rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lcsc_part_number: Option<String>,
    /// Exact KiCad footprint fallback key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footprint: Option<String>,
    pub offset_x_nm: i64,
    pub offset_y_nm: i64,
    pub rotation_degrees: f64,
    pub source: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppliedCorrection {
    pub refdes: String,
    pub matched_by: String,
    pub source: String,
    pub version: String,
    pub offset_x_nm: i64,
    pub offset_y_nm: i64,
    pub rotation_degrees: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CplResult {
    pub csv: String,
    pub corrections: Vec<AppliedCorrection>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ManufacturingError {
    #[error("assignment `{0}` is missing from the board")]
    MissingComponent(String),
    #[error("supplier assignment `{0}` has no exact LCSC part number")]
    MissingSupplierIdentity(String),
    #[error("supplier assignment `{0}` has no footprint")]
    MissingFootprint(String),
    #[error("supplier assignment `{0}` has no placement")]
    MissingPlacement(String),
    #[error("supplier assignment `{0}` is on an unsupported inner layer")]
    InnerLayerPlacement(String),
    #[error("duplicate reference designator `{0}`")]
    DuplicateReference(String),
    #[error("multiple corrections match `{0}` at the same precedence")]
    AmbiguousCorrection(String),
    #[error("invalid correction rotation for `{0}`")]
    InvalidCorrection(String),
}

/// Group exact supplier identities into the official JLCPCB BOM columns.
pub fn build_jlcpcb_bom_csv(
    board: &Board,
    lock: &ProcurementLock,
) -> Result<String, ManufacturingError> {
    let mut grouped: BTreeMap<(String, String, String), Vec<String>> = BTreeMap::new();
    let mut refs = HashSet::new();
    for assignment in &lock.assignments {
        if assignment.variant != lock.variant
            || assignment.disposition != AssemblyDisposition::SupplierAssembly
        {
            continue;
        }
        if !refs.insert(assignment.refdes.clone()) {
            return Err(ManufacturingError::DuplicateReference(
                assignment.refdes.clone(),
            ));
        }
        let component = board
            .components
            .iter()
            .find(|component| component.refdes == assignment.refdes)
            .ok_or_else(|| ManufacturingError::MissingComponent(assignment.refdes.clone()))?;
        let lcsc = assignment
            .lcsc_part_number
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ManufacturingError::MissingSupplierIdentity(assignment.refdes.clone())
            })?;
        let footprint = assignment
            .footprint
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ManufacturingError::MissingFootprint(assignment.refdes.clone()))?;
        let mpn = assignment.manufacturer_part_number.as_deref().unwrap_or("");
        let _ = component; // validates that the lock and board are the same revision.
        grouped
            .entry((mpn.to_string(), footprint.to_string(), lcsc.to_string()))
            .or_default()
            .push(assignment.refdes.clone());
    }
    let mut out = String::from("Comment,Designator,Footprint,LCSC Part #,MPN\n");
    for ((mpn, footprint, lcsc), mut designators) in grouped {
        designators.sort();
        write_csv_field(&mut out, &mpn);
        out.push(',');
        write_csv_field(&mut out, &designators.join(","));
        out.push(',');
        write_csv_field(&mut out, &footprint);
        out.push(',');
        write_csv_field(&mut out, &lcsc);
        out.push(',');
        write_csv_field(&mut out, &mpn);
        out.push('\n');
    }
    Ok(out)
}

/// Build a strict JLCPCB CPL using KiCad footprint-origin coordinates.
pub fn build_jlcpcb_cpl_csv(
    board: &Board,
    placement: &Placement,
    lock: &ProcurementLock,
    corrections: &[PlacementCorrection],
) -> Result<CplResult, ManufacturingError> {
    let placements: HashMap<_, _> = placement
        .components
        .iter()
        .map(|item| (item.id, item))
        .collect();
    let mut refs = HashSet::new();
    let mut applied = Vec::new();
    let mut rows = Vec::new();
    for assignment in &lock.assignments {
        if assignment.variant != lock.variant
            || assignment.disposition != AssemblyDisposition::SupplierAssembly
        {
            continue;
        }
        if !refs.insert(assignment.refdes.clone()) {
            return Err(ManufacturingError::DuplicateReference(
                assignment.refdes.clone(),
            ));
        }
        let component = board
            .components
            .iter()
            .find(|component| component.refdes == assignment.refdes)
            .ok_or_else(|| ManufacturingError::MissingComponent(assignment.refdes.clone()))?;
        let item = placements
            .get(&component.id)
            .ok_or_else(|| ManufacturingError::MissingPlacement(assignment.refdes.clone()))?;
        let layer = match item.layer {
            Layer::Top => "Top",
            Layer::Bottom => "Bottom",
            Layer::Inner1 | Layer::Inner2 | Layer::Inner3 | Layer::Inner4 => {
                return Err(ManufacturingError::InnerLayerPlacement(
                    assignment.refdes.clone(),
                ));
            }
        };
        let footprint = assignment
            .footprint
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ManufacturingError::MissingFootprint(assignment.refdes.clone()))?;
        let correction = select_correction(
            assignment.lcsc_part_number.as_deref(),
            Some(footprint),
            corrections,
            &assignment.refdes,
        )?;
        let origin = synth_place::footprint_origin(component.part.as_ref(), item);
        let (mut x_nm, mut y_nm) = (origin.x_nm, origin.y_nm);
        let mut rotation = f64::from(item.rotation.degrees());
        if let Some(correction) = correction {
            x_nm = x_nm.saturating_add(correction.offset_x_nm);
            y_nm = y_nm.saturating_add(correction.offset_y_nm);
            rotation += correction.rotation_degrees;
            applied.push(correction);
        }
        if !rotation.is_finite() {
            return Err(ManufacturingError::InvalidCorrection(
                assignment.refdes.clone(),
            ));
        }
        // JLC's bottom-side angle is mirrored in board axes; normalize after
        // applying the reviewed correction so repeated exports are stable.
        let rotation = normalize_angle(if layer == "Bottom" {
            360.0 - rotation
        } else {
            rotation
        });
        rows.push((
            assignment.refdes.clone(),
            assignment
                .manufacturer_part_number
                .clone()
                .unwrap_or_default(),
            footprint.to_string(),
            nm_to_mm(x_nm),
            nm_to_mm(y_nm),
            rotation,
            layer.to_string(),
        ));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut csv = String::from("Designator,Val,Package,Mid X,Mid Y,Rotation,Layer\n");
    for (refdes, value, footprint, x, y, rotation, layer) in rows {
        writeln!(
            csv,
            "\"{refdes}\",\"{}\",\"{}\",{x:.3},{y:.3},{rotation:.3},\"{layer}\"",
            escape_csv(&value),
            escape_csv(&footprint)
        )
        .expect("String cannot fail");
    }
    Ok(CplResult {
        csv,
        corrections: applied,
    })
}

fn select_correction(
    lcsc: Option<&str>,
    footprint: Option<&str>,
    corrections: &[PlacementCorrection],
    refdes: &str,
) -> Result<Option<AppliedCorrection>, ManufacturingError> {
    let exact: Vec<_> = corrections
        .iter()
        .filter(|rule| lcsc.is_some_and(|value| rule.lcsc_part_number.as_deref() == Some(value)))
        .collect();
    let footprint_rules: Vec<_> = corrections
        .iter()
        .filter(|rule| {
            exact.is_empty()
                && footprint.is_some_and(|value| rule.footprint.as_deref() == Some(value))
        })
        .collect();
    let (matches, matched_by) = if exact.is_empty() {
        (footprint_rules, "footprint")
    } else {
        (exact, "lcsc_part_number")
    };
    if matches.len() > 1 {
        return Err(ManufacturingError::AmbiguousCorrection(refdes.to_string()));
    }
    Ok(matches.first().map(|rule| AppliedCorrection {
        refdes: refdes.to_string(),
        matched_by: matched_by.to_string(),
        source: rule.source.clone(),
        version: rule.version.clone(),
        offset_x_nm: rule.offset_x_nm,
        offset_y_nm: rule.offset_y_nm,
        rotation_degrees: rule.rotation_degrees,
    }))
}

fn normalize_angle(angle: f64) -> f64 {
    let normalized = angle.rem_euclid(360.0);
    if normalized.abs() < 0.000_5 || (360.0 - normalized).abs() < 0.000_5 {
        0.0
    } else {
        normalized
    }
}

fn escape_csv(value: &str) -> String {
    value.replace('"', "\"\"")
}

fn write_csv_field(out: &mut String, value: &str) {
    out.push('"');
    out.push_str(&escape_csv(value));
    out.push('"');
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    pub schema_version: String,
    pub tool_version: String,
    pub files: BTreeMap<String, String>,
}

/// Hash a validated bundle's bytes in sorted path order.
pub fn build_bundle_manifest(
    files: &BTreeMap<String, Vec<u8>>,
    tool_version: &str,
) -> BundleManifest {
    let hashes = files
        .iter()
        .map(|(path, bytes)| {
            let mut hasher = Sha256::new();
            hasher.update(bytes);
            let digest = hasher.finalize();
            let mut hex = String::with_capacity(digest.len() * 2);
            for byte in digest {
                write!(hex, "{byte:02x}").expect("String cannot fail");
            }
            (path.clone(), format!("sha256:{hex}"))
        })
        .collect();
    BundleManifest {
        schema_version: "synth.manufacturing-manifest.v1".into(),
        tool_version: tool_version.into(),
        files: hashes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use synth_diagnostics::Span;
    use synth_geometry::{Rect, Rotation};
    use synth_ir::{Board, Component, ComponentId, PartAssignment};
    use synth_registry::{Lifecycle, Part, PartId};

    fn fixture() -> (Board, ProcurementLock, Placement) {
        let part = Part {
            id: PartId("r".into()),
            kind: "resistor".into(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: vec![],
            substitutes: vec![],
            mpn: Some("MFG-R".into()),
            lcsc_pn: Some("C1".into()),
            pins: vec![],
            required_decoupling: vec![],
            kicad_symbol: None,
            kicad_footprint: Some("R0603".into()),
            footprint_dimensions: None,
            operating_conditions: None,
            provenance: None,
        };
        let component = Component {
            id: ComponentId(0),
            refdes: "R1".into(),
            kind: "resistor".into(),
            part: Some(part),
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        };
        let board = Board {
            name: "fixture".into(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            schematic_paper: None,
            schematic_overflow: None,
            components: vec![component],
            nets: vec![],
            diff_pairs: vec![],
            notes: vec![],
            legends: false,
            keepouts: vec![],
            stackup: None,
            netclasses: vec![],
            required_processes: std::collections::BTreeSet::new(),
            buses: vec![],
            modules: vec![],
            groups: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        };
        let lock = ProcurementLock {
            schema_version: "synth.procurement-lock.v1".into(),
            design_name: "fixture".into(),
            design_revision: None,
            variant: "default".into(),
            assignments: vec![PartAssignment {
                component_id: ComponentId(0),
                refdes: "R1".into(),
                variant: "default".into(),
                part_id: Some("r".into()),
                manufacturer_part_number: Some("MFG-R".into()),
                lcsc_part_number: Some("C1".into()),
                footprint: Some("R0603".into()),
                disposition: AssemblyDisposition::SupplierAssembly,
                verified: true,
                substitution_of: None,
                change_reason: None,
            }],
        };
        let placement = Placement {
            board_outline: Rect::new(
                synth_geometry::Point::new(0, 0),
                synth_geometry::Point::new(10_000_000, 10_000_000),
            ),
            components: vec![synth_place::ComponentPlacement {
                id: ComponentId(0),
                center: synth_geometry::Point::new(1_000_000, 2_000_000),
                rotation: Rotation::Zero,
                layer: Layer::Top,
            }],
        };
        (board, lock, placement)
    }

    #[test]
    fn correction_precedence_prefers_exact_lcsc() {
        let rules = vec![
            PlacementCorrection {
                lcsc_part_number: None,
                footprint: Some("R0603".into()),
                offset_x_nm: 1,
                offset_y_nm: 2,
                rotation_degrees: 3.0,
                source: "footprint".into(),
                version: "1".into(),
            },
            PlacementCorrection {
                lcsc_part_number: Some("C1".into()),
                footprint: Some("R0603".into()),
                offset_x_nm: 4,
                offset_y_nm: 5,
                rotation_degrees: 6.0,
                source: "exact".into(),
                version: "2".into(),
            },
        ];
        let selected = select_correction(Some("C1"), Some("R0603"), &rules, "R1")
            .unwrap()
            .unwrap();
        assert_eq!(selected.matched_by, "lcsc_part_number");
        assert_eq!(selected.offset_x_nm, 4);
    }

    #[test]
    fn manifest_hashes_are_stable_and_sorted() {
        let files = BTreeMap::from([
            (String::from("z.txt"), b"z".to_vec()),
            (String::from("a.txt"), b"a".to_vec()),
        ]);
        let manifest = build_bundle_manifest(&files, "test");
        assert_eq!(manifest.files.keys().next().unwrap(), "a.txt");
        assert_eq!(manifest.files["a.txt"].len(), 71);
    }

    #[test]
    fn strict_exports_share_exact_assignment_identity() {
        let (board, lock, placement) = fixture();
        let bom = build_jlcpcb_bom_csv(&board, &lock).unwrap();
        assert!(bom.contains("C1") && bom.contains("R1"));
        let cpl = build_jlcpcb_cpl_csv(&board, &placement, &lock, &[]).unwrap();
        assert!(cpl.csv.contains("R1") && cpl.csv.contains("Top"));
        assert!(cpl.corrections.is_empty());
    }
}
