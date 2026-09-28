// SPDX-License-Identifier: Apache-2.0

//! Compiler-owned mechanical board-family profiles.
//!
//! These profiles describe fixed-form-factor outlines, not manufacturer
//! fabrication rules.  Keeping them beside the placer means the UI and agent
//! can request a known mechanical envelope without inventing dimensions.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BoardFamilyProfile {
    pub name: &'static str,
    pub description: &'static str,
    pub layers: &'static [u8],
    pub width_mm: f64,
    pub height_mm: f64,
    pub edge_clearance_mm: f64,
}

const TWO_LAYER: &[u8] = &[2];
const TWO_OR_FOUR_LAYER: &[u8] = &[2, 4];
const FOUR_LAYER: &[u8] = &[4];

/// The shipped, deterministic board-family catalog.
pub const PROFILES: &[BoardFamilyProfile] = &[
    BoardFamilyProfile {
        name: "arduino-uno-shield",
        description: "Arduino Uno shield mechanical envelope",
        layers: TWO_LAYER,
        width_mm: 68.6,
        height_mm: 53.3,
        edge_clearance_mm: 2.0,
    },
    BoardFamilyProfile {
        name: "raspberry-pi-hat",
        description: "Raspberry Pi HAT mechanical envelope",
        layers: TWO_OR_FOUR_LAYER,
        width_mm: 65.0,
        height_mm: 56.0,
        edge_clearance_mm: 2.0,
    },
    BoardFamilyProfile {
        name: "featherwing",
        description: "Adafruit FeatherWing mechanical envelope",
        layers: TWO_LAYER,
        width_mm: 51.0,
        height_mm: 23.0,
        edge_clearance_mm: 1.5,
    },
    BoardFamilyProfile {
        name: "standard-4-layer-100x100",
        description: "Generic 100 mm square four-layer board envelope",
        layers: FOUR_LAYER,
        width_mm: 100.0,
        height_mm: 100.0,
        edge_clearance_mm: 2.0,
    },
];

pub fn get(name: &str) -> Option<&'static BoardFamilyProfile> {
    PROFILES.iter().find(|profile| profile.name == name)
}

pub fn supports_layers(profile: &BoardFamilyProfile, layers: u32) -> bool {
    profile.layers.contains(&(layers as u8))
}

#[cfg(test)]
mod tests {
    use super::{get, PROFILES};

    #[test]
    fn catalog_contains_mechanical_envelopes_with_valid_dimensions() {
        assert!(get("arduino-uno-shield").is_some());
        assert!(PROFILES.iter().all(|profile| {
            profile.width_mm > 0.0
                && profile.height_mm > 0.0
                && profile.edge_clearance_mm > 0.0
                && !profile.layers.is_empty()
        }));
    }
}
