// SPDX-License-Identifier: Apache-2.0

//! Manufacturer DRC profile — per-fab rule constants.
//!
//! Profiles live as TOML files under `profiles/<name>.toml`
//! (slice 2 ships the JLC/PCBWay/OSHPark catalog). For slice
//! 1A we ship a single hard-coded default + the loader so the
//! `synth drc` CLI works without a profile file argument.
//!
//! All rule values are in millimetres at the TOML boundary;
//! the IR converts to nanometers on load.

use serde::{Deserialize, Serialize};
use synth_geometry::mm_to_nm;
use thiserror::Error;

/// A complete manufacturer DRC profile. Slice 1A carried the
/// three trace / clearance rule values; slice 1B adds the
/// drill / annular / drill-to-copper trio so the via-aware
/// rules have constants to compare against. Mask sliver and
/// silk overlap (slice 1C) will add two more fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManufacturerProfile {
    /// Display name. Embedded in `DrcReport` so downstream
    /// consumers can verify which profile produced a report.
    pub name: String,
    /// Minimum copper trace width, in nanometers.
    pub min_trace_width_nm: i64,
    /// Minimum copper-to-copper clearance between distinct
    /// nets, in nanometers.
    pub min_copper_clearance_nm: i64,
    /// Minimum distance from any copper to the Edge.Cuts
    /// board outline, in nanometers.
    pub min_copper_to_edge_nm: i64,
    /// Minimum drilled hole diameter (vias + through-hole pad
    /// drills), in nanometers. JLC's standard tier is 0.3 mm;
    /// going below that incurs the "min hole size 0.2 mm"
    /// surcharge.
    pub min_drill_diameter_nm: i64,
    /// Minimum annular ring width — copper pad radius minus
    /// drill radius. JLC's standard tier is 0.13 mm for vias
    /// and 0.25 mm for PTH; the profile carries the smaller
    /// value (vias are the common case).
    pub min_annular_ring_nm: i64,
    /// Minimum clearance from any drilled hole's edge to copper
    /// belonging to a different net, in nanometers.
    pub min_drill_to_copper_nm: i64,
}

impl ManufacturerProfile {
    /// JLCPCB's standard hobbyist tier (no surcharge):
    /// 0.127 mm trace, 0.127 mm clearance, 0.3 mm board edge,
    /// 0.3 mm drill, 0.13 mm annular ring, 0.2 mm drill-to-
    /// copper. V1's default; the `synth drc` CLI uses this
    /// when no `--profile <path>` flag is supplied.
    #[must_use]
    pub fn jlc_standard() -> Self {
        Self {
            name: "jlc-standard".to_string(),
            min_trace_width_nm: mm_to_nm(0.127),
            min_copper_clearance_nm: mm_to_nm(0.127),
            min_copper_to_edge_nm: mm_to_nm(0.3),
            min_drill_diameter_nm: mm_to_nm(0.3),
            min_annular_ring_nm: mm_to_nm(0.13),
            min_drill_to_copper_nm: mm_to_nm(0.2),
        }
    }

    /// PCBWay standard 2-layer / 4-layer tier:
    /// 0.15 mm trace, 0.15 mm clearance, 0.3 mm board edge,
    /// 0.3 mm drill, 0.15 mm annular ring, 0.2 mm drill-to-copper.
    #[must_use]
    pub fn pcbway_standard() -> Self {
        Self {
            name: "pcbway-standard".to_string(),
            min_trace_width_nm: mm_to_nm(0.15),
            min_copper_clearance_nm: mm_to_nm(0.15),
            min_copper_to_edge_nm: mm_to_nm(0.3),
            min_drill_diameter_nm: mm_to_nm(0.3),
            min_annular_ring_nm: mm_to_nm(0.15),
            min_drill_to_copper_nm: mm_to_nm(0.2),
        }
    }

    /// OSHPark 4-layer tier:
    /// 0.125 mm trace, 0.125 mm clearance, 0.3 mm board edge,
    /// 0.25 mm drill, 0.125 mm annular ring, 0.2 mm drill-to-copper.
    #[must_use]
    pub fn oshpark_4layer() -> Self {
        Self {
            name: "oshpark-4layer".to_string(),
            min_trace_width_nm: mm_to_nm(0.125),
            min_copper_clearance_nm: mm_to_nm(0.125),
            min_copper_to_edge_nm: mm_to_nm(0.3),
            min_drill_diameter_nm: mm_to_nm(0.25),
            min_annular_ring_nm: mm_to_nm(0.125),
            min_drill_to_copper_nm: mm_to_nm(0.2),
        }
    }

    /// Select a built-in profile by string name ("jlc", "pcbway", "oshpark").
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.contains("pcbway") {
            Self::pcbway_standard()
        } else if lower.contains("oshpark") {
            Self::oshpark_4layer()
        } else {
            Self::jlc_standard()
        }
    }

    /// Load a profile from a TOML file at `path`. The TOML's
    /// numeric fields use millimetres (`min_trace_width_mm`
    /// rather than `_nm`) — match KiCad's convention so
    /// hand-edited profiles are immediately legible.
    pub fn from_toml_file(path: &std::path::Path) -> Result<Self, ProfileError> {
        let text = std::fs::read_to_string(path).map_err(|source| ProfileError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml_str(&text)
    }

    /// Parse a profile from a TOML string. Public for
    /// in-memory tests and the WASM build (no `std::fs`).
    pub fn from_toml_str(text: &str) -> Result<Self, ProfileError> {
        let raw: TomlProfile = toml::from_str(text).map_err(ProfileError::Parse)?;
        Ok(Self {
            name: raw.name,
            min_trace_width_nm: mm_to_nm(raw.min_trace_width_mm),
            min_copper_clearance_nm: mm_to_nm(raw.min_copper_clearance_mm),
            min_copper_to_edge_nm: mm_to_nm(raw.min_copper_to_edge_mm),
            min_drill_diameter_nm: mm_to_nm(raw.min_drill_diameter_mm),
            min_annular_ring_nm: mm_to_nm(raw.min_annular_ring_mm),
            min_drill_to_copper_nm: mm_to_nm(raw.min_drill_to_copper_mm),
        })
    }
}

#[derive(Debug, Deserialize)]
struct TomlProfile {
    name: String,
    min_trace_width_mm: f64,
    min_copper_clearance_mm: f64,
    min_copper_to_edge_mm: f64,
    min_drill_diameter_mm: f64,
    min_annular_ring_mm: f64,
    min_drill_to_copper_mm: f64,
}

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("could not read profile {path:?}: {source}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse profile TOML: {0}")]
    Parse(toml::de::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jlc_standard_constants_match_jlc_docs() {
        let p = ManufacturerProfile::jlc_standard();
        assert_eq!(p.min_trace_width_nm, mm_to_nm(0.127));
        assert_eq!(p.min_copper_clearance_nm, mm_to_nm(0.127));
        assert_eq!(p.min_copper_to_edge_nm, mm_to_nm(0.3));
        assert_eq!(p.min_drill_diameter_nm, mm_to_nm(0.3));
        assert_eq!(p.min_annular_ring_nm, mm_to_nm(0.13));
        assert_eq!(p.min_drill_to_copper_nm, mm_to_nm(0.2));
    }

    #[test]
    fn toml_roundtrips_to_nanometres() {
        let text = r#"
name = "test"
min_trace_width_mm = 0.15
min_copper_clearance_mm = 0.2
min_copper_to_edge_mm = 0.5
min_drill_diameter_mm = 0.4
min_annular_ring_mm = 0.15
min_drill_to_copper_mm = 0.25
"#;
        let p = ManufacturerProfile::from_toml_str(text).expect("parse");
        assert_eq!(p.name, "test");
        assert_eq!(p.min_trace_width_nm, mm_to_nm(0.15));
        assert_eq!(p.min_copper_clearance_nm, mm_to_nm(0.2));
        assert_eq!(p.min_copper_to_edge_nm, mm_to_nm(0.5));
        assert_eq!(p.min_drill_diameter_nm, mm_to_nm(0.4));
        assert_eq!(p.min_annular_ring_nm, mm_to_nm(0.15));
        assert_eq!(p.min_drill_to_copper_nm, mm_to_nm(0.25));
    }
}
