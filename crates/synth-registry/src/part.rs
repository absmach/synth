// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::capability::{ElectricalType, PinCapability};

/// Stable identifier for a registry part. Lowercase, no spaces, matches
/// the filename: `rp2350.synth.toml` → `PartId("rp2350")`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PartId(pub String);

impl PartId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PartId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Physical pin number (string, not int, because real-world parts have
/// pin numbers like `A1`, `B7`, `EP` for exposed pads).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PinNumber(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    #[default]
    Active,
    Nrnd,
    Obsolete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub id: PartId,
    pub kind: String,
    #[serde(default)]
    pub description: Option<String>,

    #[serde(default)]
    pub version: u32,

    #[serde(default)]
    pub lifecycle: Lifecycle,

    #[serde(default)]
    pub signed_by: Vec<String>,

    #[serde(default)]
    pub substitutes: Vec<PartId>,

    /// Manufacturer Part Number (e.g. `"RP2350A"`).
    #[serde(default)]
    pub mpn: Option<String>,

    /// LCSC part number for JLCPCB assembly ordering (e.g. `"C2040"`).
    #[serde(default, alias = "lcsc_id")]
    pub lcsc_pn: Option<String>,

    #[serde(default)]
    pub pins: Vec<Pin>,

    #[serde(default)]
    pub required_decoupling: Vec<RequiredDecoupling>,

    /// Reference to a KiCad stock symbol library entry, e.g.
    /// `"Device:R"`, `"Regulator_Linear:AMS1117-3.3"`. When present,
    /// the KiCad exporter emits this as the `(lib_id ...)` value on
    /// instances and **does not** synthesize a rectangle body —
    /// KiCad's bundled global library resolves the reference.
    /// When `None`, the exporter falls back to a synthesized
    /// rectangle so long-tail parts still open in KiCad.
    #[serde(default)]
    pub kicad_symbol: Option<String>,

    /// Reference to a KiCad stock footprint library entry, e.g.
    /// `"Resistor_SMD:R_0603_1608Metric"`. Used by the PCB exporter
    /// (Phase 7+) and embedded in the schematic's `Footprint`
    /// property so KiCad's 3D viewer can resolve the matching
    /// `.step` model.
    #[serde(default)]
    pub kicad_footprint: Option<String>,

    /// Physical footprint dimensions for fast spatial layout / continuous placement without parsing KiCad geometry.
    #[serde(default)]
    pub footprint_dimensions: Option<FootprintDimensions>,

    /// Electrical operating conditions and power limits.
    #[serde(default)]
    pub operating_conditions: Option<OperatingConditions>,

    /// Curation/trust metadata (Phase 15, R15.3). Absent for legacy
    /// seed parts — treated as `source = "seed"` for back-compat, which
    /// is considered verified (proof-read by the core team). Generated,
    /// imported, or authored parts (Tier 2) normally carry an empty
    /// `reviewed_by`, which triggers `W-SYNTH-PART-UNVERIFIED` at
    /// export unless `--allow-unverified-parts` is passed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

/// Trust/curation metadata for a registry part (Phase 15, R15.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Provenance {
    /// How the part entered the registry.
    /// `seed` = shipped by core (verified); `generated` = via
    /// `synth part import lcsc`; `imported` = via `synth part import
    /// kicad`; `authored` = via `synth_author_part`.
    #[serde(default)]
    pub source: ProvenanceSource,

    /// Tool/version that produced the entry, e.g.
    /// `"synth-part-import-lcsc 0.1"`.
    #[serde(default)]
    pub generator: Option<String>,

    /// Datasheet URL; also populates the exported symbol `Datasheet`
    /// property (`symbol_lib.rs`).
    #[serde(default)]
    pub datasheet_url: Option<String>,

    /// Reviewer identity. Empty ⇒ unverified (emits
    /// `W-SYNTH-PART-UNVERIFIED` at export).
    #[serde(default)]
    pub reviewed_by: Option<String>,

    /// ISO-8601 review date.
    #[serde(default)]
    pub reviewed_at: Option<String>,

    /// Upstream LCSC part number when the part came from EasyEDA/LCSC.
    #[serde(default)]
    pub upstream_lcsc_pn: Option<String>,
}

/// Origin of a registry part (Phase 15, R15.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceSource {
    /// Shipped by the core team; proof-read and (for Tier 1) signed.
    #[default]
    Seed,
    /// Produced by `synth part import lcsc`.
    Generated,
    /// Produced by `synth part import kicad`.
    Imported,
    /// Produced by `synth_author_part` (datasheet-driven).
    Authored,
}

impl Part {
    /// A part is unverified when it carries provenance (i.e. it is a
    /// Tier-2 user/generated entry) but has no `reviewed_by` reviewer.
    /// Legacy seed parts without a `[provenance]` section are trusted.
    pub fn is_unverified(&self) -> bool {
        match &self.provenance {
            None => false,
            Some(p) => p.reviewed_by.as_deref().unwrap_or("").is_empty(),
        }
    }
}

#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FootprintDimensions {
    pub width_mm: f64,
    pub height_mm: f64,
    #[serde(default)]
    pub courtyard_margin_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperatingConditions {
    #[serde(default)]
    pub min_voltage_v: Option<f64>,
    #[serde(default)]
    pub max_voltage_v: Option<f64>,
    #[serde(default)]
    pub max_current_ma: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    /// Logical pin name as used in SynthSpec endpoints (`U1.spi0`).
    pub name: String,

    /// Physical pin number as printed in the datasheet.
    pub number: PinNumber,

    pub electrical_type: ElectricalType,

    #[serde(default)]
    pub capabilities: Vec<PinCapability>,

    /// `true` if a connection to this pin is required for the part to
    /// function. Floating required pins emit `E-SYNTH-CONNECT-001`.
    #[serde(default)]
    pub required: bool,

    /// Unit or gate identifier for multi-unit components (e.g. `"A"`, `"B"` for dual op-amps).
    #[serde(default)]
    pub unit: Option<String>,

    /// Maximum safe voltage input/output for this pin (e.g., `5.0` for 5V tolerant pins).
    #[serde(default)]
    pub voltage_max_v: Option<f64>,

    /// Minimum voltage for this pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_min_v: Option<f64>,

    /// Nominal operating voltage for this pin (e.g., `3.3` or `5.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_nominal_v: Option<f64>,
}

impl Pin {
    pub fn nominal_voltage_v(&self) -> Option<f64> {
        self.voltage_nominal_v.or(self.voltage_max_v)
    }
}

/// A decoupling requirement attached to a power pin or power rail.
///
/// Phase 3 records the requirement structurally but does not yet
/// enforce it (decoupling checks are a Phase 3.5 rule); the field
/// exists so that authored parts can carry the data forward.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequiredDecoupling {
    pub net: String,
    pub value: String,
    pub count: u32,
    #[serde(default)]
    pub max_distance_mm: Option<f64>,
}

impl Part {
    pub fn find_pin(&self, name: &str) -> Option<&Pin> {
        self.pins.iter().find(|p| p.name == name)
    }

    pub fn is_level_shifter(&self) -> bool {
        let k = self.kind.to_lowercase();
        k == "level_shifter"
            || k == "translator"
            || k == "isolator"
            || k == "optocoupler"
            || k == "buffer"
            || k == "transceiver"
    }
}
