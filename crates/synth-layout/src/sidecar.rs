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
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub rotation: u32,
    #[serde(default = "default_source")]
    pub source: OverrideSource,
    #[serde(default = "default_priority")]
    pub priority: OverridePriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
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
        layout: &mut Layout,
        refdes_to_id: &HashMap<String, ComponentId>,
    ) {
        for (refdes, override_pos) in &self.components {
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
                timestamp: None,
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
                timestamp: None,
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
                timestamp: None,
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
}
