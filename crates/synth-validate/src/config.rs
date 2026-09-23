// SPDX-License-Identifier: Apache-2.0

//! ERC configuration: the configurable pin-type conflict table and
//! the thresholds the deeper checks use.
//!
//! Everything here has a working default, so `run_erc` needs no
//! configuration. A project can override any of it from an
//! `<design>.synth.erc.toml` sidecar (auto-loaded by `synth validate`)
//! or by calling [`run_erc_with_config`](crate::run_erc_with_config)
//! directly:
//!
//! ```toml
//! # board.synth.erc.toml
//! [pin_conflicts]
//! # severity for a pin-type pair not listed below
//! default = "warning"
//!
//! [pin_conflicts.pairs]
//! # "type_a:type_b" -> severity; order in the key does not matter
//! "output:output" = "error"
//! "open_drain_low:output" = "warning"
//! "bidirectional:bidirectional" = "info"
//!
//! # thresholds
//! led_max_current_ma = 20.0
//! pullup_margin_v = 0.5
//! power_budget_headroom_pct = 0.0
//! ```
//!
//! The table is *symmetric*: `"output:output"` and its reverse are the
//! same entry, and keys are normalized so entry order never matters.
//! Keys are validated on load, so a misspelt pin type is an error
//! rather than a silently dead entry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use synth_diagnostics::Severity;
use synth_registry::ElectricalType;

/// File-name suffix for the auto-discovered per-design config, e.g.
/// `board.synth` → `board.synth.erc.toml`.
pub const ERC_CONFIG_SUFFIX: &str = ".erc.toml";

/// The project-level ERC configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ErcConfig {
    /// Pin-type conflict severities (`E-SYNTH-CONNECT-007`).
    pub pin_conflicts: PinConflictTable,
    /// `E-SYNTH-LED-001`: current above this through an LED is an error.
    pub led_max_current_ma: f64,
    /// `E-SYNTH-POWER-008`: how much above a device's operating voltage a
    /// pull-up rail may sit before the mismatch is an error.
    pub pullup_margin_v: f64,
    /// `E-SYNTH-POWER-010`: fraction of a regulator's maximum current
    /// to keep free as headroom (0.0 = exactly at the limit is fine).
    pub power_budget_headroom_pct: f64,
    /// `E-SYNTH-CONNECT-009`: severity for an open-drain net with no
    /// pull-up resistor.
    pub open_drain_no_pullup: Severity,
    /// `E-SYNTH-ESD-001`: require ESD/reverse-polarity protection on
    /// external connector nets.
    pub require_connector_protection: bool,
}

impl Default for ErcConfig {
    fn default() -> Self {
        Self {
            pin_conflicts: PinConflictTable::kicad_default(),
            led_max_current_ma: 20.0,
            pullup_margin_v: 0.5,
            power_budget_headroom_pct: 0.0,
            open_drain_no_pullup: Severity::Warning,
            require_connector_protection: true,
        }
    }
}

impl ErcConfig {
    /// Parse a config from TOML text. Unknown keys are rejected so a
    /// typo cannot silently disable a check.
    ///
    /// # Errors
    /// Returns the underlying `toml` error (unknown key, bad type).
    pub fn from_toml_str(text: &str) -> Result<Self, String> {
        // Unknown keys at the top level and inside `[pin_conflicts]`
        // are both errors: a mis-typed threshold must not be silently
        // ignored.
        let config: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        config.pin_conflicts.validate()?;
        Ok(config)
    }

    /// Load a config from `path`, or `None` when the file is absent.
    ///
    /// # Errors
    /// Returns an error when the file exists but cannot be read or parsed.
    pub fn load_from_file(path: &Path) -> Result<Option<Self>, String> {
        if !path.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        Self::from_toml_str(&text)
            .map(Some)
            .map_err(|e| format!("parsing {}: {e}", path.display()))
    }

    /// The config path auto-discovered for a design file:
    /// `board.synth` → `board.synth.erc.toml`.
    ///
    /// Appends to the full file name (rather than replacing the
    /// extension) so `ic.synth` and `ic.synthj` cannot collide.
    #[must_use]
    pub fn sidecar_path_for(design: &Path) -> PathBuf {
        let mut name = design.file_name().unwrap_or_default().to_os_string();
        name.push(ERC_CONFIG_SUFFIX);
        design.with_file_name(name)
    }

    /// Load the auto-discovered config for `design`, if the sidecar exists.
    ///
    /// # Errors
    /// Returns an error when the sidecar exists but is malformed.
    pub fn load_for_design(design: &Path) -> Result<Option<Self>, String> {
        Self::load_from_file(&Self::sidecar_path_for(design))
    }
}

/// Severity of a conflict between two pin electrical types, keyed by
/// the unordered pair. Absent pairs fall back to `default`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PinConflictTable {
    /// Severity for any pair not listed in `pairs`. `None` (the
    /// default) means "no conflict".
    pub default: Option<Severity>,
    /// Explicit overrides, keyed `"type_a:type_b"` with the two type
    /// names normalized (sorted), so lookups are order-independent.
    pub pairs: BTreeMap<String, Severity>,
}

impl Default for PinConflictTable {
    fn default() -> Self {
        Self::kicad_default()
    }
}

impl PinConflictTable {
    /// The built-in table, modelled on KiCad's pin/pin ERC matrix, with
    /// the pairs already owned by a dedicated rule left out (see
    /// [`Self::owned_by_dedicated_rule`]).
    #[must_use]
    pub fn kicad_default() -> Self {
        let mut pairs = BTreeMap::new();
        let mut put = |a: ElectricalType, b: ElectricalType, s: Severity| {
            pairs.insert(pair_key(a, b), s);
        };
        // Two push-pull drivers fighting: an error (KiCad "Pins are
        // connected to each other").
        put(
            ElectricalType::Output,
            ElectricalType::Output,
            Severity::Error,
        );
        // Push-pull driver vs. bidirectional / tri-state: a warning, as
        // in KiCad (the bidir pin may be an input).
        put(
            ElectricalType::Output,
            ElectricalType::Bidirectional,
            Severity::Warning,
        );
        put(
            ElectricalType::Output,
            ElectricalType::ThreeStatable,
            Severity::Warning,
        );
        // Two bidirectional or two tri-state pins are the *normal* case
        // for any shared bus (I²C, a bidirectional GPIO link), so those
        // pairs are deliberately absent from the table.
        // Push-pull output against an open-drain pin: the open-drain's
        // pull-up fights the output, so KiCad warns.
        put(
            ElectricalType::Output,
            ElectricalType::OpenDrainLow,
            Severity::Warning,
        );
        put(
            ElectricalType::Output,
            ElectricalType::OpenDrainHigh,
            Severity::Warning,
        );
        // Both open-drain: a legal wired-AND as long as both are the
        // same flavour.
        put(
            ElectricalType::OpenDrainLow,
            ElectricalType::OpenDrainHigh,
            Severity::Warning,
        );
        // A power output driving a logic output, or a logic output
        // driving a power output, is a rail short.
        put(
            ElectricalType::PowerOutput,
            ElectricalType::Output,
            Severity::Error,
        );
        put(
            ElectricalType::PowerOutput,
            ElectricalType::Bidirectional,
            Severity::Error,
        );
        put(
            ElectricalType::PowerOutput,
            ElectricalType::OpenDrainLow,
            Severity::Error,
        );
        put(
            ElectricalType::PowerOutput,
            ElectricalType::OpenDrainHigh,
            Severity::Error,
        );
        Self {
            default: None,
            pairs,
        }
    }

    /// Severity for the pair `(a, b)`, order-independent. `None` when
    /// the pair is unlisted and there is no configured default.
    #[must_use]
    pub fn severity(&self, a: ElectricalType, b: ElectricalType) -> Option<Severity> {
        self.pairs.get(&pair_key(a, b)).copied().or(self.default)
    }

    /// Check every configured pair key names two known pin types, so a
    /// typo cannot leave a dead entry in the table.
    ///
    /// # Errors
    /// Returns a message naming the offending key.
    pub fn validate(&self) -> Result<(), String> {
        for key in self.pairs.keys() {
            let Some((a, b)) = key.split_once(':') else {
                return Err(format!(
                    "invalid pin_conflicts key `{key}`: expected `<type>:<type>`"
                ));
            };
            for name in [a, b] {
                if parse_type(name).is_none() {
                    return Err(format!(
                        "unknown pin type `{name}` in pin_conflicts key `{key}`"
                    ));
                }
            }
        }
        Ok(())
    }

    /// Override the severity of a pair (used by tests and by
    /// programmatic callers; file-based config goes through serde).
    pub fn set(&mut self, a: ElectricalType, b: ElectricalType, severity: Severity) {
        self.pairs.insert(pair_key(a, b), severity);
    }

    /// True for pairs a dedicated rule already reports with a better
    /// message and a patch, so the generic table must stay silent to
    /// avoid double-reporting:
    ///
    /// - `power_output` ↔ `power_output` → `E-SYNTH-POWER-002`
    /// - `output` ↔ `output` → `E-SYNTH-CONNECT-004`
    /// - `power_output`/`power_input` ↔ `power_input`/`ground` → `E-SYNTH-POWER-003`
    /// - anything ↔ `do_not_connect` → `E-SYNTH-CONNECT-003`
    #[must_use]
    pub fn owned_by_dedicated_rule(a: ElectricalType, b: ElectricalType) -> bool {
        use ElectricalType as E;
        let pair = unordered(a, b);
        matches!(
            pair,
            (E::PowerOutput, E::PowerOutput) | (E::Output, E::Output) | (E::DoNotConnect, _)
        ) || (pair.0 == E::PowerInput && matches!(pair.1, E::GroundReference | E::PowerOutput))
            || (pair.1 == E::PowerInput && matches!(pair.0, E::GroundReference | E::PowerOutput))
    }
}

/// Parse a `snake_case` pin-type name, the inverse of [`type_name`].
#[must_use]
pub fn parse_type(name: &str) -> Option<ElectricalType> {
    ALL_TYPES.iter().copied().find(|t| type_name(*t) == name)
}

/// Every known electrical type, for name lookups and validation.
const ALL_TYPES: [ElectricalType; 17] = [
    ElectricalType::Passive,
    ElectricalType::PowerInput,
    ElectricalType::PowerOutput,
    ElectricalType::GroundReference,
    ElectricalType::Bidirectional,
    ElectricalType::Input,
    ElectricalType::Output,
    ElectricalType::ThreeStatable,
    ElectricalType::OpenDrainLow,
    ElectricalType::OpenDrainHigh,
    ElectricalType::Analog,
    ElectricalType::Rf,
    ElectricalType::DifferentialPositive,
    ElectricalType::DifferentialNegative,
    ElectricalType::Clock,
    ElectricalType::DoNotConnect,
    ElectricalType::Unclassified,
];

/// Normalize a two-type key so it is order-independent.
fn pair_key(a: ElectricalType, b: ElectricalType) -> String {
    let (a, b) = unordered(a, b);
    format!("{}:{}", type_name(a), type_name(b))
}

/// Order a pair deterministically by name.
fn unordered(a: ElectricalType, b: ElectricalType) -> (ElectricalType, ElectricalType) {
    if type_name(a) <= type_name(b) {
        (a, b)
    } else {
        (b, a)
    }
}

/// The `snake_case` name of an electrical type, matching the registry
/// spelling (`"output"`, `"open_drain_low"`, …).
#[must_use]
pub fn type_name(t: ElectricalType) -> &'static str {
    match t {
        ElectricalType::Passive => "passive",
        ElectricalType::PowerInput => "power_input",
        ElectricalType::PowerOutput => "power_output",
        ElectricalType::GroundReference => "ground_reference",
        ElectricalType::Bidirectional => "bidirectional",
        ElectricalType::Input => "input",
        ElectricalType::Output => "output",
        ElectricalType::ThreeStatable => "three_statable",
        ElectricalType::OpenDrainLow => "open_drain_low",
        ElectricalType::OpenDrainHigh => "open_drain_high",
        ElectricalType::Analog => "analog",
        ElectricalType::Rf => "rf",
        ElectricalType::DifferentialPositive => "differential_positive",
        ElectricalType::DifferentialNegative => "differential_negative",
        ElectricalType::Clock => "clock",
        ElectricalType::DoNotConnect => "do_not_connect",
        ElectricalType::Unclassified => "unclassified",
        // `ElectricalType` is #[non_exhaustive]: a future variant gets
        // a stable placeholder name rather than failing to compile.
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_table_matches_kicad_shape() {
        let t = PinConflictTable::kicad_default();
        assert_eq!(
            t.severity(ElectricalType::Output, ElectricalType::Output),
            Some(Severity::Error)
        );
        // Symmetric and order-independent.
        assert_eq!(
            t.severity(ElectricalType::Output, ElectricalType::OpenDrainLow),
            t.severity(ElectricalType::OpenDrainLow, ElectricalType::Output)
        );
        // Unlisted pair: no conflict by default.
        assert_eq!(
            t.severity(ElectricalType::Input, ElectricalType::Output),
            None
        );
    }

    #[test]
    fn table_is_overridable_from_toml() {
        let cfg = ErcConfig::from_toml_str(
            r#"
            [pin_conflicts]
            default = "warning"

            [pin_conflicts.pairs]
            "output:output" = "info"
            "#,
        )
        .unwrap();
        assert_eq!(
            cfg.pin_conflicts
                .severity(ElectricalType::Output, ElectricalType::Output),
            Some(Severity::Info),
            "explicit entry wins"
        );
        assert_eq!(
            cfg.pin_conflicts
                .severity(ElectricalType::Input, ElectricalType::Input),
            Some(Severity::Warning),
            "default applies to unlisted pairs"
        );
    }

    #[test]
    fn unknown_config_key_is_rejected() {
        assert!(ErcConfig::from_toml_str("led_max_current_ma = 10.0\nnope = 1\n").is_err());
        assert!(ErcConfig::from_toml_str("[pin_conflicts]\nnot_a_pair = \"error\"\n").is_err());
    }

    #[test]
    fn unknown_pin_type_in_a_pair_key_is_rejected() {
        let err = ErcConfig::from_toml_str("[pin_conflicts.pairs]\n\"outpt:output\" = \"error\"\n")
            .unwrap_err();
        assert!(err.contains("outpt"), "{err}");
        assert!(
            ErcConfig::from_toml_str("[pin_conflicts.pairs]\n\"output\" = \"error\"\n").is_err()
        );
    }

    #[test]
    fn thresholds_override() {
        let cfg = ErcConfig::from_toml_str("led_max_current_ma = 5.0\n").unwrap();
        assert!((cfg.led_max_current_ma - 5.0).abs() < f64::EPSILON);
        assert!(
            cfg.require_connector_protection,
            "untouched fields keep defaults"
        );
    }

    #[test]
    fn dedicated_rule_pairs_are_excluded() {
        use ElectricalType as E;
        assert!(PinConflictTable::owned_by_dedicated_rule(
            E::Output,
            E::Output
        ));
        assert!(PinConflictTable::owned_by_dedicated_rule(
            E::PowerOutput,
            E::PowerOutput
        ));
        assert!(PinConflictTable::owned_by_dedicated_rule(
            E::DoNotConnect,
            E::Output
        ));
        assert!(!PinConflictTable::owned_by_dedicated_rule(
            E::Output,
            E::Bidirectional
        ));
    }

    #[test]
    fn config_loads_from_a_sidecar_file() {
        let dir = std::env::temp_dir().join(format!("synth-erc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let design = dir.join("board.synth");
        let sidecar = ErcConfig::sidecar_path_for(&design);
        std::fs::write(&sidecar, "[pin_conflicts]\ndefault = \"info\"\n").unwrap();
        let loaded = ErcConfig::load_for_design(&design)
            .expect("loads")
            .expect("present");
        assert_eq!(
            loaded
                .pin_conflicts
                .severity(ElectricalType::Input, ElectricalType::Input),
            Some(Severity::Info)
        );
        // Absent sidecar is not an error, just the default.
        let other = dir.join("other.synth");
        assert!(ErcConfig::load_for_design(&other).expect("ok").is_none());
        // Malformed sidecar is an error, not a silent default.
        std::fs::write(&sidecar, "led_max_current_ma = \"nope\"\n").unwrap();
        assert!(ErcConfig::load_for_design(&design).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sidecar_path_appends_suffix() {
        assert_eq!(
            ErcConfig::sidecar_path_for(Path::new("/d/board.synth")),
            PathBuf::from("/d/board.synth.erc.toml")
        );
    }
}
