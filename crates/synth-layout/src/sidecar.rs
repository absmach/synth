// SPDX-License-Identifier: Apache-2.0

//! Sidecar layout persistence.
//!
//! Allows manual component drag-and-drop overrides from `synth preview`
//! or human design tuning to persist to disk adjacent to `.synth` source files.
//! Version 2 supports provenance (`source: "human_drag" | "agent"`) and priority.
//!
//! # Two namespaces, one schema
//!
//! A component's position is a *different number* depending on what it is
//! a position on, so the overrides cannot be shared between the two
//! consumers:
//!
//! | | coordinates | consumer | canonical file |
//! |---|---|---|---|
//! | [`SidecarKind::Schematic`] | sheet mm, page-local | [`crate::layout_with_sidecar`] | `<design>.schematic.layout.toml` |
//! | [`SidecarKind::Placement`] | board mm | `synth_place::place_with_sidecar` | `<design>.placement.layout.toml` |
//!
//! Both files use the same [`SidecarLayout`] schema — that is the point of
//! the split being by *file*, not by format.
//!
//! Feeding one file to both consumers was a live bug: the placement sidecar
//! was auto-discovered for the schematic too, so board-millimetre overrides
//! were applied to the sheet. A part at board `x = 63.5 mm` landed hundreds
//! of millimetres off its schematic group, which overflowed the page and
//! split the sheet into one near-empty page per group — the exported PDF no
//! longer matched the rendered preview. See [`SidecarKind::resolve`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use synth_ir::ComponentId;

use crate::{Layout, Rotation};

pub const SIDECAR_SCHEMA_VERSION: u32 = 2;

/// Which coordinate space a sidecar's overrides are expressed in.
///
/// Both variants parse the same [`SidecarLayout`] file format; they differ
/// only in which file they look for and which consumer applies it, which is
/// what keeps the two sets of numbers apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarKind {
    /// Sheet-local millimetres on the schematic sheet. Carries
    /// `fit_sheet` and `forced_net_labels`, both schematic-only concepts.
    Schematic,
    /// Board millimetres on the PCB, from `synth_place`.
    Placement,
}

impl SidecarKind {
    /// The canonical filename suffix appended to a `.synth` design's own
    /// filename, e.g. `board.synth` → `board.schematic.layout.toml`.
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Schematic => ".schematic.layout.toml",
            Self::Placement => ".placement.layout.toml",
        }
    }

    /// The deprecated pre-split filename. One file served both consumers,
    /// which is the bug this module documents.
    const LEGACY_SUFFIX: &'static str = ".layout.toml";

    /// The canonical path for this kind, replacing the design's extension
    /// rather than appending to it.
    ///
    /// `board.synth` → `board.schematic.layout.toml`, not
    /// `board.synth.schematic.layout.toml`: `.synth` is the source's
    /// extension, and repeating it reads like a typo. This is also the path
    /// new overrides are *written* to — writers must never target
    /// [`Self::legacy_path`], or the entries would be re-interpreted in the
    /// other coordinate space.
    pub fn canonical_path(self, design: &Path) -> Option<PathBuf> {
        let stem = design.file_stem()?.to_string_lossy();
        let stem = if stem.is_empty() {
            design.file_name()?.to_string_lossy()
        } else {
            stem
        };
        Some(design.with_file_name(format!("{stem}{}", self.suffix())))
    }

    /// The deprecated shared path, whether or not it exists.
    ///
    /// Keeps its historical form — appended to the *full* filename, so
    /// `board.synth` → `board.synth.layout.toml`. Renaming it would strand
    /// every pre-split sidecar where it is, silently dropping overrides.
    pub fn legacy_path(design: &Path) -> Option<PathBuf> {
        let name = design.file_name()?.to_string_lossy();
        Some(design.with_file_name(format!("{name}{}", Self::LEGACY_SUFFIX)))
    }

    /// Resolve the sidecar to read for this kind, or `None` when the design
    /// has no persisted overrides.
    ///
    /// [`Self::Placement`] still falls back to the deprecated shared file, so
    /// a board whose footprints were dragged before the split keeps its
    /// positions. Placement is the fallback's correct home: the placement
    /// sidecar is what `synth_place_with_hints`, `synth_route`, and
    /// `synth_drc_report` always documented.
    ///
    /// [`Self::Schematic`] deliberately does **not** fall back. Reading the
    /// legacy file here is precisely the coordinate-space bug, and it fails
    /// silently and badly (board millimetres on the sheet, page overflow,
    /// spurious multi-sheet split), so a legacy file is reported instead —
    /// see [`Self::legacy_needs_migration`].
    pub fn resolve(self, design: &Path) -> Option<PathBuf> {
        let canonical = self.canonical_path(design).filter(|p| p.exists());
        match self {
            Self::Schematic => canonical,
            Self::Placement => {
                canonical.or_else(|| Self::legacy_path(design).filter(|p| p.exists()))
            }
        }
    }

    /// Whether a deprecated shared sidecar exists that the schematic will
    /// not read on its own.
    ///
    /// [`Self::Schematic`] returns `true` whenever one exists *and carries
    /// component overrides*, because that is exactly when schematic positions
    /// may be sitting in a file the schematic will never load. Nothing in the
    /// file records which coordinate space its numbers are in — that
    /// ambiguity is the whole reason for the split — so the only safe action
    /// is to ask the user to split the file. An empty legacy file (a
    /// placeholder that was never written) is not worth reporting.
    pub fn legacy_needs_migration(self, design: &Path) -> bool {
        self == Self::Schematic
            && Self::legacy_path(design).is_some_and(|p| {
                SidecarLayout::load_from_file(&p).is_some_and(|s| !s.components.is_empty())
            })
    }

    /// Resolve from a design path held as a string, which is how the MCP
    /// layer carries `file_path`.
    pub fn resolve_str(self, design: &str) -> Option<PathBuf> {
        self.resolve(Path::new(design))
    }

    /// Canonical path from a design path held as a string.
    pub fn canonical_path_str(self, design: &str) -> Option<PathBuf> {
        self.canonical_path(Path::new(design))
    }

    /// Deprecated shared path from a design path held as a string.
    pub fn legacy_path_str(self, design: &str) -> Option<PathBuf> {
        Self::legacy_path(Path::new(design))
    }

    /// A user-facing migration hint for [`Self::legacy_needs_migration`].
    ///
    /// Advisory, not an error: the legacy file keeps working for placement,
    /// which is what most designs used it for. It is reported because any
    /// schematic positions in it are being dropped, and the file cannot be
    /// read as a schematic sidecar without guessing.
    ///
    /// # Panics
    ///
    /// Never: every `expect` is guarded by [`Self::legacy_needs_migration`],
    /// which itself returns `false` for a design with no file name.
    pub fn migration_notice(self, design: &Path) -> Option<String> {
        if !self.legacy_needs_migration(design) {
            return None;
        }
        let legacy = Self::legacy_path(design).expect("checked by legacy_needs_migration");
        let schematic = self.canonical_path(design).expect("design has a file name");
        let placement = SidecarKind::Placement
            .canonical_path(design)
            .expect("design has a file name");
        Some(format!(
            "{} is a pre-split sidecar. It is still read for PCB placement, but the \
             schematic ignores it: nothing in the file says whether its coordinates are \
             board mm or sheet mm, so applying them to the sheet is not safe. Rename it \
             to {}, or split its entries between {} (sheet mm) and {} (board mm).",
            legacy.display(),
            placement.display(),
            schematic.display(),
            placement.display(),
        ))
    }
}

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
    /// Force the sheet to the smallest size that fits the drawing, with the
    /// drawing centred on it.
    /// Applied *after* the auto-layout, because the pipeline re-derives
    /// `sheet_size` from scratch (`grow_sheet_to_fit`) and would otherwise
    /// discard a persisted fit.
    #[serde(default)]
    pub fit_sheet: bool,
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
    /// Force every net named in [`Self::forced_net_labels`] to render as
    /// label stubs. Applied *after* routing: `route_and_label` recomputes
    /// wires and labels for the whole board, so a forced label set before it
    /// would be overwritten. Unknown net names are ignored (a stale sidecar
    /// entry after a rename must not fail a layout).
    pub fn apply_forced_labels(&self, board: &synth_ir::Board, layout: &mut Layout) {
        if self.forced_net_labels.is_empty() {
            return;
        }
        let by_name: HashMap<&str, synth_ir::NetId> =
            board.nets.iter().map(|n| (n.name.as_str(), n.id)).collect();
        for forced in &self.forced_net_labels {
            if let Some(&net) = by_name.get(forced.net.as_str()) {
                crate::force_net_label(board, layout, net);
            }
        }
    }

    /// Shrink the sheet to the smallest size that fits the drawing and
    /// centre the drawing on it, when [`Self::fit_sheet`] is set. Applied
    /// after routing/annotation so bounds include wires and text.
    ///
    /// Component overrides in this file stay in the auto-layout's
    /// (un-centred) coordinates: they are overlaid first and the centring
    /// shift moves them together with everything else.
    pub fn apply_sheet_fit(&self, board: &synth_ir::Board, layout: &mut Layout) {
        if !self.fit_sheet {
            return;
        }
        if let Some((min_x, max_x, min_y, max_y)) = crate::drawing_bounds(board, layout) {
            layout.sheet_size = crate::fit_sheet_size_any(min_x, max_x, min_y, max_y);
            crate::centre_on_sheet(board, layout);
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
            groups: Vec::new(),
            legends: false,
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

#[cfg(test)]
mod forced_label_tests {
    use super::*;
    use synth_diagnostics::Span;
    use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, NetId, PinId};

    fn two_net_board() -> Board {
        let comp = |i: u32, refdes: &str| Component {
            id: ComponentId(i),
            refdes: refdes.to_string(),
            kind: "resistor".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        };
        Board {
            groups: Vec::new(),
            legends: false,
            name: "t".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: vec![comp(0, "R1"), comp(1, "R2")],
            nets: vec![Net {
                id: NetId(0),
                name: "SIG".to_string(),
                endpoints: vec![
                    NetEndpoint {
                        component: ComponentId(0),
                        pin: PinId(0),
                        source_span: Span::new(0, 0),
                    },
                    NetEndpoint {
                        component: ComponentId(1),
                        pin: PinId(0),
                        source_span: Span::new(0, 0),
                    },
                ],
                netclass: None,
                voltage: None,
            }],
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

    fn layout_with_wire_on_net_zero() -> (Board, crate::Layout) {
        let board = two_net_board();
        let mut layout = crate::Layout {
            components: vec![
                crate::ComponentPlacement {
                    id: ComponentId(0),
                    center_mm: (10.0, 10.0),
                    rotation: Rotation::Zero,
                },
                crate::ComponentPlacement {
                    id: ComponentId(1),
                    center_mm: (40.0, 10.0),
                    rotation: Rotation::Zero,
                },
            ],
            wires: Vec::new(),
            junctions: Vec::new(),
            power_flags: Vec::new(),
            net_labels: Vec::new(),
            hierarchical_labels: Vec::new(),
            annotations: Vec::new(),
            group_boxes: Vec::new(),
            sheet_size: crate::SheetSize::A4,
        };
        layout.wires.push(crate::WirePath {
            net: NetId(0),
            points: vec![(10.0, 10.0), (40.0, 10.0)],
            junctions: Vec::new(),
        });
        (board, layout)
    }

    #[test]
    fn forced_label_replaces_wire_and_persists() {
        let (board, mut layout) = layout_with_wire_on_net_zero();
        let mut sidecar = SidecarLayout::default();
        sidecar.forced_net_labels.push(ForcedNetLabel {
            net: "SIG".to_string(),
            source: OverrideSource::Agent,
        });

        // Re-running routing (a structural op) reintroduces the wire...
        crate::route_and_label(&board, &mut layout);
        // ...and applying forced labels afterwards must remove it again.
        sidecar.apply_forced_labels(&board, &mut layout);

        assert!(
            layout.wires.iter().all(|w| w.net != NetId(0)),
            "net 0 should be forced to labels, not wires; wires={:?} labels={:?}",
            layout.wires.iter().map(|w| w.net).collect::<Vec<_>>(),
            layout.net_labels.iter().map(|l| l.net).collect::<Vec<_>>()
        );
        assert_eq!(layout.net_labels.len(), 2, "one label per endpoint");
        assert!(layout.wires.iter().all(|w| w.net != NetId(0)));
    }

    #[test]
    fn forced_label_round_trips_through_toml() {
        let mut sidecar = SidecarLayout::default();
        sidecar.forced_net_labels.push(ForcedNetLabel {
            net: "SIG".to_string(),
            source: OverrideSource::Agent,
        });
        let dir = std::env::temp_dir().join(format!("synth_fnl_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("x.layout.toml");
        sidecar.save_to_file(&path).unwrap();
        let reloaded = SidecarLayout::load_from_file(&path).expect("reload");
        assert_eq!(reloaded.forced_net_labels.len(), 1);
        assert_eq!(reloaded.forced_net_labels[0].net, "SIG");
        let _ = std::fs::remove_file(&path);
    }

    // ----- Two namespaces: the coordinate-space regression -------------------
    //
    // Before the split one `<design>.layout.toml` fed both the sheet layout
    // and the PCB placer. A footprint dragged to board (63.5, 12) was then
    // read as sheet position 63.5 mm, which threw the part out of its group,
    // overflowed A2, and split the export into one near-empty sheet per
    // group. These tests pin both halves of the fix.

    #[test]
    fn canonical_names_replace_the_extension_and_differ_per_kind() {
        let design = Path::new("/p/board.synth");
        assert_eq!(
            SidecarKind::Schematic.canonical_path(design).unwrap(),
            PathBuf::from("/p/board.schematic.layout.toml"),
        );
        assert_eq!(
            SidecarKind::Placement.canonical_path(design).unwrap(),
            PathBuf::from("/p/board.placement.layout.toml"),
        );
        // The legacy name appends to the full filename and must not move,
        // or every pre-split sidecar is stranded in place.
        assert_eq!(
            SidecarKind::legacy_path(design).unwrap(),
            PathBuf::from("/p/board.synth.layout.toml"),
        );
    }

    #[test]
    fn schematic_resolution_refuses_the_deprecated_shared_file() {
        let dir = std::env::temp_dir().join(format!("synth_ns_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let design = dir.join("board.synth");

        // A pre-split sidecar, board millimetres, alone on disk.
        let mut legacy = SidecarLayout {
            schema_version: SIDECAR_SCHEMA_VERSION,
            ..Default::default()
        };
        legacy.merge_override(
            "L1".into(),
            SidecarPlacement {
                x: 63.5,
                y: 12.0,
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
        legacy
            .save_to_file(&SidecarKind::legacy_path(&design).unwrap())
            .unwrap();

        // Placement keeps reading it, so existing board drags survive.
        assert_eq!(
            SidecarKind::Placement.resolve(&design),
            SidecarKind::legacy_path(&design),
            "placement must fall back to the deprecated file"
        );
        // The schematic must not: applying board millimetres to the sheet is
        // the bug, and it fails silently.
        assert_eq!(
            SidecarKind::Schematic.resolve(&design),
            None,
            "schematic must never read the deprecated shared file"
        );
        assert!(SidecarKind::Schematic.legacy_needs_migration(&design));
        let notice = SidecarKind::Schematic
            .migration_notice(&design)
            .expect("a notice when one is needed");
        assert!(notice.contains("board.schematic.layout.toml"), "{notice}");
        assert!(notice.contains("board.placement.layout.toml"), "{notice}");

        // Once the canonical schematic file exists it wins, and the notice
        // stops firing even though the legacy file is still there.
        SidecarLayout::default()
            .save_to_file(&SidecarKind::Schematic.canonical_path(&design).unwrap())
            .unwrap();
        assert_eq!(
            SidecarKind::Schematic.resolve(&design),
            SidecarKind::Schematic.canonical_path(&design),
            "canonical schematic file must take precedence"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_kind_reads_only_its_own_canonical_file() {
        let dir = std::env::temp_dir().join(format!("synth_ns2_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let design = dir.join("board.synth");

        // Both files exist, each with a single component in its own space.
        for (kind, refdes, x) in [
            (SidecarKind::Schematic, "R1", 210.82_f64),
            (SidecarKind::Placement, "L1", 63.5_f64),
        ] {
            let mut s = SidecarLayout {
                schema_version: SIDECAR_SCHEMA_VERSION,
                ..Default::default()
            };
            s.merge_override(
                refdes.into(),
                SidecarPlacement {
                    x,
                    y: 12.0,
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
            s.save_to_file(&kind.canonical_path(&design).unwrap())
                .unwrap();
        }

        // Resolution never crosses over, so neither consumer can see the
        // other's coordinates.
        assert_eq!(
            SidecarKind::Schematic
                .resolve(&design)
                .unwrap()
                .file_name()
                .unwrap(),
            "board.schematic.layout.toml"
        );
        assert_eq!(
            SidecarKind::Placement
                .resolve(&design)
                .unwrap()
                .file_name()
                .unwrap(),
            "board.placement.layout.toml"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_forced_net_name_is_ignored() {
        let (board, mut layout) = layout_with_wire_on_net_zero();
        let mut sidecar = SidecarLayout::default();
        sidecar.forced_net_labels.push(ForcedNetLabel {
            net: "RENAMED_AWAY".to_string(),
            source: OverrideSource::Agent,
        });
        // Must not panic, and must leave the real net's wire alone.
        sidecar.apply_forced_labels(&board, &mut layout);
        assert!(layout.wires.iter().any(|w| w.net == NetId(0)));
    }
}
