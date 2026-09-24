// SPDX-License-Identifier: Apache-2.0

//! Sidecar layout persistence (`<design>.synth.layout.toml`).
//!
//! Allows manual component drag-and-drop overrides from `synth preview`
//! or human design tuning to persist to disk adjacent to `.synth` source files.
//! Version 2 supports provenance (`source: "human_drag" | "agent"`) and priority.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use synth_ir::ComponentId;

use crate::{Layout, Rotation};

pub const SIDECAR_SCHEMA_VERSION: u32 = 2;

fn default_schema_version() -> u32 {
    SIDECAR_SCHEMA_VERSION
}

fn default_source() -> OverrideSource {
    OverrideSource::HumanDrag
}

fn default_priority() -> OverridePriority {
    OverridePriority::Soft
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SidecarLayout {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub components: HashMap<String, SidecarPlacement>,
    #[serde(default)]
    pub forced_net_labels: Vec<ForcedNetLabel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SidecarPlacement {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub rotation: u32,
    /// Sheet the dragged component was on (§P26 multi-sheet).
    /// Coordinates are sheet-local: when set and the component has
    /// since moved to another sheet, the override is stale and is
    /// skipped rather than misplacing the part. Unset (legacy files)
    /// applies sheet-blind as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    #[serde(default = "default_source")]
    pub source: OverrideSource,
    #[serde(default = "default_priority")]
    pub priority: OverridePriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Optional anchor. When set, position is anchor center plus dx/dy in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_to: Option<String>,
    #[serde(default)]
    pub dx: f64,
    #[serde(default)]
    pub dy: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideSource {
    Agent,
    HumanDrag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverridePriority {
    Soft,
    Hard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForcedNetLabel {
    pub net: String,
    pub source: OverrideSource,
}

impl SidecarLayout {
    pub fn load_from_file(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        toml::from_str(&content).ok()
    }

    pub fn save_to_file(&self, path: &Path) -> std::io::Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, content)
    }

    pub fn merge_override(&mut self, refdes: String, placement: SidecarPlacement) {
        if let Some(existing) = self.components.get(&refdes) {
            if existing.source == OverrideSource::HumanDrag
                && placement.source == OverrideSource::Agent
                && existing.priority >= placement.priority
            {
                return;
            }
        }
        self.components.insert(refdes, placement);
    }

    pub fn apply_to_layout(
        &self,
        board: &synth_ir::Board,
        layout: &mut Layout,
        refdes_to_id: &HashMap<String, ComponentId>,
    ) {
        let sheet_of = |refdes: &str| -> Option<&str> {
            let id = refdes_to_id.get(refdes)?;
            board.component(*id)?.sheet.as_deref()
        };
        // Resolve absolute entries first; TOML map order is not significant.
        for (refdes, override_pos) in self
            .components
            .iter()
            .filter(|(_, p)| p.relative_to.is_none())
        {
            // Stale-sheet guard (§P26): coordinates are sheet-local,
            // so an override recorded on another sheet must not move
            // this component.
            if let Some(want) = override_pos.sheet.as_deref() {
                if sheet_of(refdes) != Some(want) {
                    continue;
                }
            }
            if let Some(&comp_id) = refdes_to_id.get(refdes) {
                if let Some(placement) = layout.components.iter_mut().find(|p| p.id == comp_id) {
                    placement.center_mm = (override_pos.x, override_pos.y);
                    placement.rotation = match override_pos.rotation {
                        90 => Rotation::Ninety,
                        180 => Rotation::OneEighty,
                        270 => Rotation::TwoSeventy,
                        _ => Rotation::Zero,
                    };
                }
            }
        }

        // Then resolve relative entries. The bounded pass supports chains and
        // leaves cyclic or missing anchors at the automatic placement.
        for _ in 0..self.components.len() {
            let mut changed = false;
            for (refdes, override_pos) in self
                .components
                .iter()
                .filter(|(_, p)| p.relative_to.is_some())
            {
                if let Some(want) = override_pos.sheet.as_deref() {
                    if sheet_of(refdes) != Some(want) {
                        continue;
                    }
                }
                let Some(anchor) = override_pos.relative_to.as_ref() else {
                    continue;
                };
                let Some(&comp_id) = refdes_to_id.get(refdes) else {
                    continue;
                };
                let Some(&anchor_id) = refdes_to_id.get(anchor) else {
                    continue;
                };
                let Some(anchor_center) = layout
                    .components
                    .iter()
                    .find(|p| p.id == anchor_id)
                    .map(|p| p.center_mm)
                else {
                    continue;
                };
                if let Some(placement) = layout.components.iter_mut().find(|p| p.id == comp_id) {
                    let center = (
                        anchor_center.0 + override_pos.dx,
                        anchor_center.1 + override_pos.dy,
                    );
                    if placement.center_mm != center {
                        changed = true;
                    }
                    placement.center_mm = center;
                    placement.rotation = match override_pos.rotation {
                        90 => Rotation::Ninety,
                        180 => Rotation::OneEighty,
                        270 => Rotation::TwoSeventy,
                        _ => Rotation::Zero,
                    };
                }
            }
            if !changed {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_agent_override_does_not_overwrite_human_drag() {
        let mut sidecar = SidecarLayout::default();
        sidecar.merge_override(
            "U1".into(),
            SidecarPlacement {
                x: 18.0,
                y: 20.0,
                rotation: 0,
                source: OverrideSource::HumanDrag,
                priority: OverridePriority::Hard,
                sheet: None,
                timestamp: None,
                relative_to: None,
                dx: 0.0,
                dy: 0.0,
            },
        );
        sidecar.merge_override(
            "U1".into(),
            SidecarPlacement {
                x: 5.0,
                y: 5.0,
                rotation: 0,
                source: OverrideSource::Agent,
                priority: OverridePriority::Hard,
                sheet: None,
                timestamp: None,
                relative_to: None,
                dx: 0.0,
                dy: 0.0,
            },
        );
        assert_eq!(sidecar.components["U1"].x, 18.0);
        assert_eq!(sidecar.components["U1"].source, OverrideSource::HumanDrag);
    }

    #[test]
    fn write_agent_override_allowed_when_no_human_override_exists() {
        let mut sidecar = SidecarLayout::default();
        sidecar.merge_override(
            "C1".into(),
            SidecarPlacement {
                x: 12.0,
                y: 8.0,
                rotation: 0,
                source: OverrideSource::Agent,
                priority: OverridePriority::Soft,
                sheet: None,
                timestamp: None,
                relative_to: None,
                dx: 0.0,
                dy: 0.0,
            },
        );
        assert_eq!(sidecar.components["C1"].x, 12.0);
    }

    #[test]
    fn sidecar_v1_loads_without_error() {
        let toml = r"[components.U1]
x = 18.0
y = 20.0
rotation = 0
";
        let sidecar: SidecarLayout = toml::from_str(toml).unwrap();
        assert_eq!(sidecar.components["U1"].source, OverrideSource::HumanDrag);
    }

    #[test]
    fn relative_sidecar_entry_loads_without_absolute_coordinates() {
        let toml = r#"[components.C1]
relative_to = "U1"
dx = 2.5
dy = -1.0
rotation = 90
source = "agent"
priority = "hard"
"#;
        let sidecar: SidecarLayout = toml::from_str(toml).unwrap();
        let c1 = &sidecar.components["C1"];
        assert_eq!(c1.relative_to.as_deref(), Some("U1"));
        assert_eq!((c1.x, c1.y, c1.dx, c1.dy), (0.0, 0.0, 2.5, -1.0));
    }

    fn test_board(pairs: &[(&str, Option<&str>)]) -> crate::Board {
        use synth_diagnostics::Span;
        use synth_ir::{Board, Component, ComponentId};
        Board {
            name: "t".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: pairs
                .iter()
                .enumerate()
                .map(|(i, (refdes, sheet))| Component {
                    id: ComponentId(i as u32),
                    refdes: (*refdes).to_string(),
                    kind: "resistor".to_string(),
                    part: None,
                    value: None,
                    dnp: false,
                    properties: std::collections::BTreeMap::new(),
                    placement_hint: None,
                    group: None,
                    sheet: sheet.map(str::to_string),
                    source_span: Span::new(0, 0),
                })
                .collect(),
            nets: Vec::new(),
            diff_pairs: Vec::new(),
            notes: Vec::new(),
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    fn base_layout(ids: &[(u32, f64, f64)]) -> Layout {
        Layout {
            components: ids
                .iter()
                .map(|&(id, x, y)| crate::ComponentPlacement {
                    id: ComponentId(id),
                    center_mm: (x, y),
                    rotation: Rotation::Zero,
                })
                .collect(),
            wires: Vec::new(),
            junctions: Vec::new(),
            power_flags: Vec::new(),
            net_labels: Vec::new(),
            hierarchical_labels: Vec::new(),
            annotations: Vec::new(),
            group_boxes: Vec::new(),
            sheet_size: crate::SheetSize::A4,
        }
    }

    fn override_at(sheet: Option<&str>, x: f64, y: f64) -> SidecarPlacement {
        SidecarPlacement {
            x,
            y,
            rotation: 0,
            sheet: sheet.map(str::to_string),
            source: OverrideSource::HumanDrag,
            priority: OverridePriority::Hard,
            timestamp: None,
            relative_to: None,
            dx: 0.0,
            dy: 0.0,
        }
    }

    #[test]
    fn override_applies_when_sheet_matches() {
        let board = test_board(&[("R1", Some("Power"))]);
        let mut layout = base_layout(&[(0, 10.0, 10.0)]);
        let mut sidecar = SidecarLayout::default();
        sidecar.merge_override("R1".into(), override_at(Some("Power"), 42.0, 43.0));
        let ids: HashMap<String, ComponentId> = HashMap::from([("R1".to_string(), ComponentId(0))]);
        sidecar.apply_to_layout(&board, &mut layout, &ids);
        assert_eq!(layout.components[0].center_mm, (42.0, 43.0));
    }

    #[test]
    fn stale_sheet_override_is_skipped() {
        // Recorded on "Power", but the component now lives on "Input":
        // sheet-local coordinates would misplace it, so skip.
        let board = test_board(&[("R1", Some("Input"))]);
        let mut layout = base_layout(&[(0, 10.0, 10.0)]);
        let mut sidecar = SidecarLayout::default();
        sidecar.merge_override("R1".into(), override_at(Some("Power"), 42.0, 43.0));
        let ids: HashMap<String, ComponentId> = HashMap::from([("R1".to_string(), ComponentId(0))]);
        sidecar.apply_to_layout(&board, &mut layout, &ids);
        assert_eq!(
            layout.components[0].center_mm,
            (10.0, 10.0),
            "override skipped"
        );
    }

    #[test]
    fn sheet_blind_override_still_applies() {
        // Legacy files carry no sheet: apply as before.
        let board = test_board(&[("R1", Some("Power"))]);
        let mut layout = base_layout(&[(0, 10.0, 10.0)]);
        let mut sidecar = SidecarLayout::default();
        sidecar.merge_override("R1".into(), override_at(None, 42.0, 43.0));
        let ids: HashMap<String, ComponentId> = HashMap::from([("R1".to_string(), ComponentId(0))]);
        sidecar.apply_to_layout(&board, &mut layout, &ids);
        assert_eq!(layout.components[0].center_mm, (42.0, 43.0));
    }
}
