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
    /// Dielectric heights of parallel run two traces must share before
    /// `W-SYNTH-DRC-013` calls it edge coupling. A process-neutral default;
    /// tighter than this and ordinary boards warn.
    #[serde(default = "default_crosstalk_run_multiple")]
    pub crosstalk_run_multiple: i64,
    /// Assumed temperature rise for the ampacity curve (`E-SYNTH-DRC-015`),
    /// in milli-degrees Celsius (`10000` is 10 °C).
    #[serde(default = "default_ampacity_delta_t_milli_c")]
    pub ampacity_delta_t_milli_c: i64,
    /// IR-drop budget for a declared-current net (`W-SYNTH-DRC-017`), in
    /// millivolts. A net whose class declares a plane is carried by the plane
    /// and is not checked.
    #[serde(default = "default_ir_drop_budget_mv")]
    pub ir_drop_budget_mv: i64,
    /// Whether the process fills and caps a via drilled inside an
    /// SMD pad.
    ///
    /// A capability rather than a rule floor. At fine pitch
    /// via-in-pad is the only escape the arithmetic leaves, and on
    /// a process that cannot fill and cap it the same copper is a
    /// solder-bridging defect. A board declares that it needs
    /// via-in-pad; this says whether the tier offers it.
    #[serde(default)]
    pub via_in_pad: bool,
}

const fn default_crosstalk_run_multiple() -> i64 {
    10
}

const fn default_ampacity_delta_t_milli_c() -> i64 {
    10_000
}

const fn default_ir_drop_budget_mv() -> i64 {
    50
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
            crosstalk_run_multiple: default_crosstalk_run_multiple(),
            ampacity_delta_t_milli_c: default_ampacity_delta_t_milli_c(),
            ir_drop_budget_mv: default_ir_drop_budget_mv(),
            via_in_pad: false,
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
            crosstalk_run_multiple: default_crosstalk_run_multiple(),
            ampacity_delta_t_milli_c: default_ampacity_delta_t_milli_c(),
            ir_drop_budget_mv: default_ir_drop_budget_mv(),
            via_in_pad: false,
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
            crosstalk_run_multiple: default_crosstalk_run_multiple(),
            ampacity_delta_t_milli_c: default_ampacity_delta_t_milli_c(),
            ir_drop_budget_mv: default_ir_drop_budget_mv(),
            via_in_pad: false,
        }
    }

    /// JLCPCB's standard tier with the resin-filled-via option
    /// taken: the same geometric floors, plus a process that fills
    /// and caps a via inside an SMD pad.
    ///
    /// The floors are deliberately identical to
    /// [`Self::jlc_standard`]. Filling a via buys the capability
    /// and not a finer drill or a narrower track, so a board that
    /// names this tier to reach via-in-pad is not also granted
    /// geometry it never paid for.
    #[must_use]
    pub fn jlc_filled_via() -> Self {
        Self {
            name: "jlc-filled-via".to_string(),
            via_in_pad: true,
            ..Self::jlc_standard()
        }
    }

    /// JLCPCB's advanced tier: the filled-via process plus the 0.2 mm
    /// minimum via hole, which JLCPCB offers on four- and six-layer builds
    /// at a surcharge.
    ///
    /// Only the drill moves. The trace, clearance, edge and annular-ring
    /// floors stay where [`Self::jlc_standard`] put them, for the same
    /// reason [`Self::jlc_filled_via`] does not widen them: a board that
    /// names this tier to reach a finer hole is not also granted geometry it
    /// never paid for.
    ///
    /// What needs it: a module whose stock footprint drills its own thermal
    /// vias finer than the standard floor. Every KiCad ESP32-WROOM-32
    /// variant does — twelve 0.2 mm holes in the ground paddle, and because
    /// they sit inside pad 39 they also need the fill, which is why this
    /// builds on the filled-via tier rather than on the plain one. Their
    /// pads are 0.6 mm across, so the 0.13 mm ring floor is already clear
    /// and does not have to move with the drill.
    #[must_use]
    pub fn jlc_advanced() -> Self {
        Self {
            name: "jlc-advanced".to_string(),
            min_drill_diameter_nm: mm_to_nm(0.2),
            ..Self::jlc_filled_via()
        }
    }

    /// Select a built-in profile by string name.
    ///
    /// The fabricator tiers match on a substring, because a board writes a
    /// vendor's name loosely ("pcbway", "PCBWay standard") and the tier is the
    /// same either way. `jlc-filled-via` and `jlc-advanced` do not: each is
    /// one named process option rather than a vendor, so each is matched
    /// exactly. A substring would hand a surcharged process to any future
    /// profile whose name happened to contain the word, and a board gets the
    /// tier it named or the conservative floor, never a better one by
    /// accident. A name this does not recognise falls back to
    /// `jlc-standard`, the most conservative floor.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower == Self::jlc_advanced().name {
            Self::jlc_advanced()
        } else if lower == Self::jlc_filled_via().name {
            Self::jlc_filled_via()
        } else if lower.contains("pcbway") {
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
            crosstalk_run_multiple: raw.crosstalk_run_multiple,
            ampacity_delta_t_milli_c: raw.ampacity_delta_t_milli_c,
            ir_drop_budget_mv: raw.ir_drop_budget_mv,
            via_in_pad: raw.via_in_pad,
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
    #[serde(default = "default_crosstalk_run_multiple")]
    crosstalk_run_multiple: i64,
    #[serde(default = "default_ampacity_delta_t_milli_c")]
    ampacity_delta_t_milli_c: i64,
    #[serde(default = "default_ir_drop_budget_mv")]
    ir_drop_budget_mv: i64,
    /// Absent from every profile written before the capability
    /// existed, and absence means the process does not offer it.
    #[serde(default)]
    via_in_pad: bool,
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
    fn jlc_advanced_moves_the_drill_and_nothing_else() {
        let standard = ManufacturerProfile::jlc_standard();
        let advanced = ManufacturerProfile::jlc_advanced();

        // The one thing it buys, plus the fill those holes need because they
        // sit inside a pad.
        assert_eq!(advanced.min_drill_diameter_nm, mm_to_nm(0.2));
        assert!(advanced.via_in_pad);

        // Everything else stays on the conservative floor. A finer hole is
        // not a licence to draw a narrower track.
        assert_eq!(advanced.min_trace_width_nm, standard.min_trace_width_nm);
        assert_eq!(
            advanced.min_copper_clearance_nm,
            standard.min_copper_clearance_nm
        );
        assert_eq!(
            advanced.min_copper_to_edge_nm,
            standard.min_copper_to_edge_nm
        );
        assert_eq!(advanced.min_annular_ring_nm, standard.min_annular_ring_nm);
        assert_eq!(
            advanced.min_drill_to_copper_nm,
            standard.min_drill_to_copper_nm
        );
    }

    #[test]
    fn the_esp32_module_paddle_vias_fit_the_advanced_tier_and_not_the_standard_one() {
        // Every KiCad ESP32-WROOM-32 variant drills twelve 0.2 mm holes in
        // the ground paddle, on 0.6 mm pads.
        let paddle_drill = mm_to_nm(0.2);
        let paddle_ring = mm_to_nm((0.6 - 0.2) / 2.0);

        let standard = ManufacturerProfile::jlc_standard();
        assert!(
            paddle_drill < standard.min_drill_diameter_nm,
            "the standard floor is what the board was violating"
        );

        let advanced = ManufacturerProfile::jlc_advanced();
        assert!(paddle_drill >= advanced.min_drill_diameter_nm);
        assert!(
            paddle_ring >= advanced.min_annular_ring_nm,
            "the ring was never the problem, so it did not move"
        );
    }

    #[test]
    fn a_surcharged_tier_is_never_reached_by_a_loose_name() {
        // Exact names get their tier.
        assert_eq!(
            ManufacturerProfile::from_name("jlc-advanced").name,
            "jlc-advanced"
        );
        assert_eq!(
            ManufacturerProfile::from_name("JLC-Advanced").name,
            "jlc-advanced"
        );

        // A board writing the vendor loosely gets the conservative floor,
        // not the process it did not order.
        for loose in ["jlcpcb", "JLCPCB advanced", "jlc", "jlcpcb-advanced"] {
            let p = ManufacturerProfile::from_name(loose);
            assert_eq!(p.name, "jlc-standard", "{loose} should not buy a tier");
            assert!(!p.via_in_pad);
            assert_eq!(p.min_drill_diameter_nm, mm_to_nm(0.3));
        }
    }

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

    /// Via-in-pad is a paid process option, so no standard tier may
    /// grant it by default: a board that has not asked for it and a
    /// fab that has not sold it must both read as "no".
    #[test]
    fn the_standard_tiers_do_not_offer_via_in_pad() {
        for profile in [
            ManufacturerProfile::jlc_standard(),
            ManufacturerProfile::pcbway_standard(),
            ManufacturerProfile::oshpark_4layer(),
        ] {
            assert!(
                !profile.via_in_pad,
                "{} must not offer via-in-pad",
                profile.name
            );
        }
    }

    /// Filling a via buys the capability and nothing else. If this
    /// tier ever drifts to a finer floor, a board could reach the
    /// geometry by asking for the fill, which is not what it paid
    /// for.
    #[test]
    fn the_filled_via_tier_offers_it_on_the_standard_floors() {
        let filled = ManufacturerProfile::jlc_filled_via();
        let standard = ManufacturerProfile::jlc_standard();

        assert!(filled.via_in_pad);
        assert_eq!(filled.min_trace_width_nm, standard.min_trace_width_nm);
        assert_eq!(
            filled.min_copper_clearance_nm,
            standard.min_copper_clearance_nm
        );
        assert_eq!(filled.min_copper_to_edge_nm, standard.min_copper_to_edge_nm);
        assert_eq!(filled.min_drill_diameter_nm, standard.min_drill_diameter_nm);
        assert_eq!(filled.min_annular_ring_nm, standard.min_annular_ring_nm);
        assert_eq!(
            filled.min_drill_to_copper_nm,
            standard.min_drill_to_copper_nm
        );
    }

    #[test]
    fn from_name_resolves_the_filled_via_tier() {
        assert!(ManufacturerProfile::from_name("jlc-filled-via").via_in_pad);
        assert!(!ManufacturerProfile::from_name("jlcpcb").via_in_pad);
    }

    /// The filled-via tier is matched exactly, not as a substring. A
    /// substring would hand the capability to any future profile whose name
    /// happened to contain the word — a process permission is not something
    /// to grant by accident of spelling.
    #[test]
    fn a_name_that_merely_contains_filled_is_not_the_filled_via_tier() {
        for name in [
            "jlc-unfilled-via",
            "acme-filled-resin",
            "filled",
            "JLC-FILLED-VIA-draft",
        ] {
            assert!(
                !ManufacturerProfile::from_name(name).via_in_pad,
                "`{name}` must not resolve to the filled-via tier"
            );
        }
        // Spelling the tier exactly still works, in any case.
        assert!(ManufacturerProfile::from_name("JLC-Filled-Via").via_in_pad);
    }

    /// The vendor tiers keep their loose match: a board writes a fabricator's
    /// name as prose and the floor is the same either way.
    #[test]
    fn a_vendor_tier_still_matches_loosely() {
        assert_eq!(
            ManufacturerProfile::from_name("PCBWay standard").min_trace_width_nm,
            ManufacturerProfile::pcbway_standard().min_trace_width_nm
        );
        assert_eq!(
            ManufacturerProfile::from_name("oshpark 4-layer").min_trace_width_nm,
            ManufacturerProfile::oshpark_4layer().min_trace_width_nm
        );
    }

    /// Every profile TOML written before the capability existed
    /// omits the key, and absence must read as "the process does
    /// not offer it" rather than as a parse failure.
    #[test]
    fn a_profile_toml_without_the_key_does_not_offer_via_in_pad() {
        let text = r#"
name = "legacy"
min_trace_width_mm = 0.15
min_copper_clearance_mm = 0.2
min_copper_to_edge_mm = 0.5
min_drill_diameter_mm = 0.4
min_annular_ring_mm = 0.15
min_drill_to_copper_mm = 0.25
"#;
        let p = ManufacturerProfile::from_toml_str(text).expect("parse");
        assert!(!p.via_in_pad);
    }

    #[test]
    fn a_profile_toml_can_declare_the_capability() {
        let text = r#"
name = "house-fab"
min_trace_width_mm = 0.15
min_copper_clearance_mm = 0.2
min_copper_to_edge_mm = 0.5
min_drill_diameter_mm = 0.4
min_annular_ring_mm = 0.15
min_drill_to_copper_mm = 0.25
via_in_pad = true
"#;
        let p = ManufacturerProfile::from_toml_str(text).expect("parse");
        assert!(p.via_in_pad);
    }
}
