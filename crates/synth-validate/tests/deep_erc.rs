// SPDX-License-Identifier: Apache-2.0

//! Phase 6 deeper-ERC tests: the configurable pin-conflict table, the
//! voltage-domain rules, the protection checks, and the naming hygiene
//! rules. Each test drives the real pipeline (parse → lower → run_erc)
//! so registry data is exercised, not stubs.

use std::path::{Path, PathBuf};

use synth_diagnostics::{Diagnostic, Severity};
use synth_validate::{ErcConfig, PinConflictTable};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn validate_with(src: &str, config: &ErcConfig) -> Vec<Diagnostic> {
    let filename = "deep_erc.synth".to_string();
    let parse = synth_parser::parse(src, filename.clone());
    let ast = parse.ast.as_ref().expect("source must parse");
    let registry_dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry must load");
    let lowered = synth_ir::lower(ast, &registry, &filename);
    let mut all = parse.diagnostics;
    all.extend(lowered.diagnostics);
    if let Some(board) = lowered.board.as_ref() {
        all.extend(synth_validate::run_erc_with_config(
            board, &filename, config,
        ));
    }
    all
}

fn validate(src: &str) -> Vec<Diagnostic> {
    validate_with(src, &ErcConfig::default())
}

fn codes<'a>(diags: &'a [Diagnostic], code: &str) -> Vec<&'a Diagnostic> {
    diags.iter().filter(|d| d.code == code).collect()
}

// ---------------------------------------------------------------------------
// E-SYNTH-CONNECT-007 — configurable pin-type conflict table
// ---------------------------------------------------------------------------

const CONFLICT_SRC: &str = r#"board "t" {
  layers 2
  component U1: ic "lm555_timer"
  component C1: capacitor "c_generic_0603"
  connect U1.out -> U1.disch
  connect U1.vcc -> C1.p1
  connect U1.gnd -> C1.p2
  connect U1.trig -> C1.p1
  connect U1.rst -> C1.p1
  connect U1.thr -> C1.p1
  connect U1.ctrl -> C1.p2
}"#;

#[test]
fn pin_conflict_fires_with_default_table() {
    let diags = validate(CONFLICT_SRC);
    let hits = codes(&diags, "E-SYNTH-CONNECT-007");
    assert_eq!(hits.len(), 1, "one finding per net/type-pair: {hits:?}");
    assert_eq!(hits[0].severity, Severity::Warning);
}

#[test]
fn pin_conflict_severity_is_configurable() {
    let mut config = ErcConfig::default();
    config.pin_conflicts.set(
        synth_registry::ElectricalType::OpenDrainLow,
        synth_registry::ElectricalType::Output,
        Severity::Error,
    );
    let diags = validate_with(CONFLICT_SRC, &config);
    let hits = codes(&diags, "E-SYNTH-CONNECT-007");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].severity, Severity::Error, "override applied");
}

#[test]
fn pin_conflict_can_be_silenced() {
    let mut config = ErcConfig::default();
    // Remove the pair from the table entirely.
    config.pin_conflicts.pairs.remove("open_drain_low:output");
    let diags = validate_with(CONFLICT_SRC, &config);
    assert!(
        codes(&diags, "E-SYNTH-CONNECT-007").is_empty(),
        "removed pair must not fire"
    );
}

#[test]
fn dedicated_rule_pairs_are_not_double_reported() {
    // Two push-pull outputs: E-SYNTH-CONNECT-004 owns it, so the table
    // must stay silent even though `output:output` is "error" in it.
    let src = r#"board "t" {
      layers 2
      component U1: ic "sn74hc595_shift"
      component U2: ic "sn74hc595_shift"
      connect U1.qa -> U2.qa
    }"#;
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-CONNECT-007").is_empty(),
        "output/output belongs to CONNECT-004"
    );
}

#[test]
fn shared_bidirectional_bus_is_not_a_pin_conflict() {
    // Two bidirectional pins on one net is the normal I²C case, not a
    // conflict — the table must leave it alone.
    let src = r#"board "t" {
      layers 2
      component U2: mcu "atmega328p"
      component U3: sensor "bmp280_pressure"
      connect U2.pc4_sda -> U3.sda
      connect U2.pc5_scl -> U3.scl
      connect U2.vcc -> U3.vdd
    }"#;
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-CONNECT-007").is_empty(),
        "a bidirectional bus is not a pin-type conflict"
    );
}

// ---------------------------------------------------------------------------
// E-SYNTH-POWER-008/009/010 — voltage domains
// ---------------------------------------------------------------------------

#[test]
fn pullup_above_device_supply_is_an_error() {
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component U2: regulator "ams1117_5v"
      component U3: sensor "bmp280_pressure"
      component R1: resistor "r_generic_0603" value "4.7k"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      component C4: capacitor "c_generic_0805" value "10u"
      connect U1.vout -> U3.vdd
      connect U3.sda -> R1.p1
      connect R1.p2 -> U2.vout
      connect U1.vout -> U3.scl
    }"#;
    let diags = validate(src);
    assert_eq!(codes(&diags, "E-SYNTH-POWER-008").len(), 1);
}

#[test]
fn pullup_to_the_devices_own_rail_is_clean() {
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component U3: sensor "bmp280_pressure"
      component R1: resistor "r_generic_0603" value "4.7k"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      component C4: capacitor "c_generic_0805" value "10u"
      connect U1.vout -> U3.vdd
      connect U3.sda -> R1.p1
      connect R1.p2 -> U1.vout
      connect U1.vout -> U3.scl
    }"#;
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-POWER-008").is_empty(),
        "3.3V pull-up on a 3.3V device is fine"
    );
}

#[test]
fn regulator_input_below_minimum_is_an_error() {
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component J1: connector "header_1x4"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      component C4: capacitor "c_generic_0805" value "10u"
      power "VIN33" 3.3v
      connect J1.p1 -> U1.vin as "VIN33"
      connect J1.p2 -> U1.gnd
      connect U1.vin -> C1.p1
      connect U1.gnd -> C1.p2
      connect U1.vin -> C2.p1
      connect U1.gnd -> C2.p2
      connect U1.vout -> C3.p1
      connect U1.gnd -> C3.p2
      connect U1.vout -> C4.p1
      connect U1.gnd -> C4.p2
    }"#;
    let diags = validate(src);
    let hits = codes(&diags, "E-SYNTH-POWER-009");
    assert_eq!(hits.len(), 1);
    let found = hits[0].found.clone().unwrap_or_default();
    assert!(found.contains("3.3"), "must quote the actual rail: {found}");
}

#[test]
fn regulator_input_in_range_is_clean() {
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component J1: connector "header_1x4"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      component C4: capacitor "c_generic_0805" value "10u"
      power "VIN5" 5v
      connect J1.p1 -> U1.vin as "VIN5"
      connect J1.p2 -> U1.gnd
      connect U1.vin -> C1.p1
      connect U1.gnd -> C1.p2
      connect U1.vin -> C2.p1
      connect U1.gnd -> C2.p2
      connect U1.vout -> C3.p1
      connect U1.gnd -> C3.p2
      connect U1.vout -> C4.p1
      connect U1.gnd -> C4.p2
    }"#;
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-POWER-009").is_empty(),
        "5V is in range"
    );
}

#[test]
fn power_budget_over_limit_is_an_error() {
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component U2: mcu "atmega328p"
      component U3: mcu "atmega328p"
      component U4: mcu "atmega328p"
      component U5: mcu "atmega328p"
      component U6: mcu "atmega328p"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component J1: connector "header_1x4"
      power "+3V3" 3.3v
      connect J1.p1 -> U1.vin
      connect J1.p2 -> U1.gnd
      connect U1.vin -> C1.p1
      connect U1.gnd -> C1.p2
      connect U1.vout -> C2.p1 as "+3V3"
      connect U1.gnd -> C2.p2
      connect U1.vout -> U2.vcc
      connect U1.vout -> U3.vcc
      connect U1.vout -> U4.vcc
      connect U1.vout -> U5.vcc
      connect U1.vout -> U6.vcc
    }"#;
    let diags = validate(src);
    let hits = codes(&diags, "E-SYNTH-POWER-010");
    assert_eq!(hits.len(), 1);
    let found = hits[0].found.clone().unwrap_or_default();
    assert!(found.contains("1000mA"), "sums the loads: {found}");
}

#[test]
fn power_budget_headroom_is_configurable() {
    // 3 MCUs = 600mA against an 800mA regulator: fine at 0% headroom,
    // over budget at 50% (600 > 800 × 0.5).
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component U2: mcu "atmega328p"
      component U3: mcu "atmega328p"
      component U7: mcu "atmega328p"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component J1: connector "header_1x4"
      power "+3V3" 3.3v
      connect J1.p1 -> U1.vin
      connect J1.p2 -> U1.gnd
      connect U1.vin -> C1.p1
      connect U1.gnd -> C1.p2
      connect U1.vout -> C2.p1 as "+3V3"
      connect U1.gnd -> C2.p2
      connect U1.vout -> U2.vcc
      connect U1.vout -> U3.vcc
      connect U1.vout -> U7.vcc
    }"#;
    assert!(codes(&validate(src), "E-SYNTH-POWER-010").is_empty());
    let strict = ErcConfig::from_toml_str("power_budget_headroom_pct = 50.0\n").unwrap();
    assert_eq!(
        codes(&validate_with(src, &strict), "E-SYNTH-POWER-010").len(),
        1,
        "50% headroom makes a 600mA load over budget"
    );
}

#[test]
fn gpio_driven_led_with_a_resistor_is_not_flagged() {
    // The LED is fed by an MCU pin through a resistor, not by a rail.
    // That is a perfectly ordinary LED driver, so no finding.
    let src = r#"board "t" {
      layers 2
      component U2: mcu "atmega328p"
      component D1: led "led_red_0603"
      component R4: resistor "r_generic_0603" value "330"
      connect U2.pd0_rx -> R4.p1
      connect R4.p2 -> D1.anode
      connect D1.cathode -> U2.gnd
    }"#;
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-LED-001").is_empty(),
        "a resistor-fed LED is not a missing-resistor case"
    );
}

// ---------------------------------------------------------------------------
// E-SYNTH-CONNECT-008/009, E-SYNTH-ESD-001, E-SYNTH-LED-001
// ---------------------------------------------------------------------------

#[test]
fn floating_input_on_a_used_part_warns_only_for_non_required_pins() {
    let src = r#"board "t" {
      layers 2
      component U1: ic "lm555_timer"
      component C1: capacitor "c_generic_0603"
      connect U1.vcc -> C1.p1
      connect U1.gnd -> C1.p2
    }"#;
    let diags = validate(src);
    // lm555 declares vcc/gnd/trig/rst/out/thr/disch required; only `ctrl`
    // is optional, so this rule reports exactly that one and the required
    // pins stay with E-SYNTH-CONNECT-001.
    let hits = codes(&diags, "E-SYNTH-CONNECT-008");
    assert_eq!(
        hits.len(),
        1,
        "only the non-required input floats: {hits:?}"
    );
    let found = hits[0].found.clone().unwrap_or_default();
    assert!(found.contains("ctrl"), "{found}");
    assert!(
        !codes(&diags, "E-SYNTH-CONNECT-001").is_empty(),
        "required floating pins are E-SYNTH-CONNECT-001's job"
    );
}

#[test]
fn unconnected_part_is_not_flagged_as_floating_input() {
    // A wholly unused part is E-SYNTH-CONNECT-006's story, not this rule.
    let src = r#"board "t" {
      layers 2
      component U1: ic "lm555_timer"
      component R1: resistor "r_generic_0603"
      connect R1.p1 -> R1.p2
    }"#;
    let diags = validate(src);
    assert!(codes(&diags, "E-SYNTH-CONNECT-008").is_empty());
}

#[test]
fn open_drain_without_pullup_warns() {
    let src = r#"board "t" {
      layers 2
      component U1: ic "lm555_timer"
      component C1: capacitor "c_generic_0603"
      component C2: capacitor "c_generic_0603"
      connect U1.vcc -> C1.p1
      connect U1.gnd -> C1.p2
      connect U1.disch -> C2.p1
      connect C2.p2 -> U1.gnd
    }"#;
    let diags = validate(src);
    assert_eq!(codes(&diags, "E-SYNTH-CONNECT-009").len(), 1);
}

#[test]
fn i2c_open_drain_is_left_to_the_i2c_rule() {
    // atecc608's SDA is open-drain with an i2c capability: E-SYNTH-I2C-002
    // owns the pull-up requirement, so this rule must stay silent.
    let src = r#"board "t" {
      layers 2
      component U1: secure_element "atecc608"
      component U2: mcu "rp2350"
      connect U1.sda -> U2.gp0
      connect U1.scl -> U2.gp1
      connect U1.vcc -> U2.gp2
      connect U1.gnd -> U2.gp3
    }"#;
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-CONNECT-009").is_empty(),
        "I2C pull-ups belong to E-SYNTH-I2C-002"
    );
}

#[test]
fn unprotected_usb_signal_warns() {
    let src = r#"board "t" {
      layers 2
      component J1: connector "usb_c_receptacle"
      component R1: resistor "r_generic_0603" value "1k"
      connect J1.dp -> R1.p1
      connect R1.p2 -> J1.gnd
    }"#;
    let diags = validate(src);
    assert!(
        !codes(&diags, "E-SYNTH-ESD-001").is_empty(),
        "usb D+ with no TVS must warn"
    );
}

#[test]
fn plain_header_is_not_treated_as_an_external_dc_input() {
    // A debug header feeding a rail must not demand a protection device.
    let src = r#"board "t" {
      layers 2
      component J1: connector "header_1x4"
      component U1: regulator "ams1117_3v3"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      component C4: capacitor "c_generic_0805" value "10u"
      connect J1.p1 -> U1.vin
      connect J1.p2 -> U1.gnd
      connect U1.vin -> C1.p1
      connect U1.gnd -> C1.p2
      connect U1.vin -> C2.p1
      connect U1.gnd -> C2.p2
      connect U1.vout -> C3.p1
      connect U1.gnd -> C3.p2
      connect U1.vout -> C4.p1
      connect U1.gnd -> C4.p2
    }"#;
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-ESD-001").is_empty(),
        "a generic header is not a DC input"
    );
}

#[test]
fn esd_check_can_be_disabled() {
    let src = r#"board "t" {
      layers 2
      component J1: connector "usb_c_receptacle"
      component R1: resistor "r_generic_0603" value "1k"
      connect J1.dp -> R1.p1
      connect R1.p2 -> J1.gnd
    }"#;
    let config = ErcConfig::from_toml_str("require_connector_protection = false\n").unwrap();
    let diags = validate_with(src, &config);
    assert!(codes(&diags, "E-SYNTH-ESD-001").is_empty());
}

#[test]
fn led_over_current_is_an_error() {
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_5v"
      component D1: led "led_red_0603"
      component R1: resistor "r_generic_0603" value "33"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      connect U1.vout -> R1.p2
      connect R1.p1 -> D1.anode
      connect D1.cathode -> U1.gnd
      connect U1.vout -> C1.p1
      connect U1.gnd -> C1.p2
      connect U1.vout -> C2.p1
      connect U1.gnd -> C2.p2
    }"#;
    let diags = validate(src);
    let hits = codes(&diags, "E-SYNTH-LED-001");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].severity, Severity::Error);
}

#[test]
fn led_with_a_proper_resistor_is_clean() {
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_5v"
      component D1: led "led_red_0603"
      component R1: resistor "r_generic_0603" value "220"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      connect U1.vout -> R1.p2
      connect R1.p1 -> D1.anode
      connect D1.cathode -> U1.gnd
      connect U1.vout -> C1.p1
      connect U1.gnd -> C1.p2
      connect U1.vout -> C2.p1
      connect U1.gnd -> C2.p2
    }"#;
    // (5 − 1.9) / 220 ≈ 14mA, and 0.04W — inside both budgets.
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-LED-001").is_empty(),
        "14mA through 220Ω is fine"
    );
}

// ---------------------------------------------------------------------------
// E-SYNTH-NAME-007/008/009/010 — naming hygiene
// ---------------------------------------------------------------------------

#[test]
fn case_only_net_collision_is_an_error() {
    let src = r#"board "t" {
      layers 2
      component R1: resistor "r_generic_0603"
      component R2: resistor "r_generic_0603"
      net "SDA" { R1.p1 }
      connect R2.p1 -> R2.p2 as "Sda"
    }"#;
    let diags = validate(src);
    assert_eq!(codes(&diags, "E-SYNTH-NAME-007").len(), 1);
}

#[test]
fn declared_label_used_once_warns_and_unnamed_net_uses_connect_002() {
    // Declared, one endpoint → NAME-008 (not CONNECT-002).
    let declared = r#"board "t" {
      layers 2
      component R1: resistor "r_generic_0603"
      net "ONLY_ONCE" { R1.p1 }
    }"#;
    let diags = validate(declared);
    assert_eq!(codes(&diags, "E-SYNTH-NAME-008").len(), 1);
    assert!(
        codes(&diags, "E-SYNTH-CONNECT-002").is_empty(),
        "exactly one rule owns the declared-label case"
    );

    // Unnamed, one endpoint → CONNECT-002 (not NAME-008).
    let unnamed = r#"board "t" {
      layers 2
      component R1: resistor "r_generic_0603"
      connect R1.p1 -> R1.p1
    }"#;
    let diags = validate(unnamed);
    assert_eq!(codes(&diags, "E-SYNTH-CONNECT-002").len(), 1);
    assert!(codes(&diags, "E-SYNTH-NAME-008").is_empty());
}

#[test]
fn ground_pin_on_a_rail_is_an_error() {
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component U2: sensor "bmp280_pressure"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      component C4: capacitor "c_generic_0805" value "10u"
      connect U1.vout -> U2.gnd
      connect U1.vout -> C1.p1
      connect U1.gnd -> C1.p2
      connect U1.vout -> C2.p1
      connect U1.gnd -> C2.p2
      connect U1.vout -> C3.p1
      connect U1.gnd -> C3.p2
      connect U1.vout -> C4.p1
      connect U1.gnd -> C4.p2
    }"#;
    let diags = validate(src);
    assert_eq!(codes(&diags, "E-SYNTH-NAME-009").len(), 1);
}

#[test]
fn ordinary_ground_net_is_not_flagged() {
    // A regulator's `gnd` pin tied to a bypass cap is a normal ground,
    // even though the net has no ground name yet.
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      component C4: capacitor "c_generic_0805" value "10u"
      connect U1.vin -> C1.p1
      connect U1.gnd -> C1.p2
      connect U1.vin -> C2.p1
      connect U1.gnd -> C2.p2
      connect U1.vout -> C3.p1
      connect U1.gnd -> C3.p2
      connect U1.vout -> C4.p1
      connect U1.gnd -> C4.p2
    }"#;
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-NAME-009").is_empty(),
        "an auto-named gnd-to-cap net is an ordinary ground"
    );
}

/// NAME-010 needs a per-unit-power symbol, which the shipped registry
/// does not have yet, so the board is built from a synthetic part.
#[test]
fn multi_unit_rails_split_is_an_error() {
    use synth_diagnostics::Span;
    use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, NetId, PinId};
    use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin as RegPin, PinNumber};

    fn pin(name: &str, et: ElectricalType, unit: Option<&str>) -> RegPin {
        RegPin {
            name: name.to_string(),
            number: PinNumber(name.to_string()),
            electrical_type: et,
            capabilities: Vec::new(),
            required: false,
            unit: unit.map(str::to_string),
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }
    }

    let part = Part {
        id: PartId("dual_opamp_units".into()),
        kind: "opamp".into(),
        description: None,
        version: 0,
        lifecycle: Lifecycle::Active,
        signed_by: Vec::new(),
        substitutes: Vec::new(),
        mpn: None,
        lcsc_pn: None,
        pins: vec![
            pin("vcc_a", ElectricalType::PowerInput, Some("A")),
            pin("vcc_b", ElectricalType::PowerInput, Some("B")),
            pin("out_a", ElectricalType::Output, Some("A")),
            pin("out_b", ElectricalType::Output, Some("B")),
        ],
        required_decoupling: Vec::new(),
        kicad_symbol: None,
        kicad_footprint: None,
        footprint_dimensions: None,
        operating_conditions: None,
        provenance: None,
    };
    let board = Board {
        name: "t".into(),
        layers: 2,
        manufacturer: None,
        revision: None,
        company: None,
        components: vec![Component {
            id: ComponentId(0),
            refdes: "U1".into(),
            kind: "opamp".into(),
            part: Some(part),
            value: None,
            dnp: false,
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        }],
        nets: vec![
            Net {
                id: NetId(0),
                name: "+3V3".into(),
                endpoints: vec![NetEndpoint {
                    component: ComponentId(0),
                    pin: PinId(0),
                    source_span: Span::new(0, 0),
                }],
                netclass: None,
                voltage: None,
            },
            Net {
                id: NetId(1),
                name: "+5V".into(),
                endpoints: vec![NetEndpoint {
                    component: ComponentId(0),
                    pin: PinId(1),
                    source_span: Span::new(0, 0),
                }],
                netclass: None,
                voltage: None,
            },
        ],
        diff_pairs: Vec::new(),
        notes: Vec::new(),
        keepouts: Vec::new(),
        netclasses: vec![],
        buses: vec![],
        modules: vec![],
        source_span: Span::new(0, 0),
    };
    let diags = synth_validate::run_erc(&board, "t.synth");
    let hits = codes(&diags, "E-SYNTH-NAME-010");
    assert_eq!(hits.len(), 1, "{hits:?}");

    // Tying both units to one rail clears it.
    let mut fixed = board.clone();
    fixed.nets[1].endpoints[0].pin = PinId(0);
    let diags = synth_validate::run_erc(&fixed, "t.synth");
    assert!(codes(&diags, "E-SYNTH-NAME-010").is_empty());
}

// ---------------------------------------------------------------------------
// Configuration surface
// ---------------------------------------------------------------------------

#[test]
fn default_config_is_the_kicad_shaped_table() {
    let config = ErcConfig::default();
    assert_eq!(
        config.pin_conflicts.severity(
            synth_registry::ElectricalType::Output,
            synth_registry::ElectricalType::Output
        ),
        Some(Severity::Error)
    );
    assert_eq!(PinConflictTable::kicad_default(), config.pin_conflicts);
}

// ---------------------------------------------------------------------------
// E-SYNTH-PINMUX-001 — one pin asked to carry two functions
// ---------------------------------------------------------------------------

#[test]
fn pin_mux_conflict_fires_for_two_functions_on_one_pin() {
    // Both connects share `U1.gp0`, so the two function nets merge onto
    // one pin: I²C SCL and UART TX cannot both live there.
    let diags = validate(
        r#"board "t" {
  layers 2
  component U1: mcu "rp2350"
  connect U1.gp0 -> "I2C1_SCL"
  connect U1.gp0 -> "UART1_TX"
}"#,
    );
    let hits = codes(&diags, "E-SYNTH-PINMUX-001");
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].severity, Severity::Error);
    // The mux rule owns the function-name case, so the generic
    // conflicting-names rule must stay quiet (no duplicate finding).
    assert!(
        codes(&diags, "E-SYNTH-NAME-005").is_empty(),
        "NAME-005 must not double-report a function conflict"
    );
}

#[test]
fn non_function_name_conflict_still_reports_name_005() {
    // Power rails are not functions: the generic rule keeps owning them.
    let diags = validate(
        r#"board "t" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component C3: capacitor "c_generic_0603"
  component C4: capacitor "c_generic_0603"
  connect U1.vout -> C3.p1 as "+3V3"
  connect C3.p1 -> C4.p1 as "+5V"
}"#,
    );
    assert!(!codes(&diags, "E-SYNTH-NAME-005").is_empty());
    assert!(codes(&diags, "E-SYNTH-PINMUX-001").is_empty());
}

// ---------------------------------------------------------------------------
// E-SYNTH-PINMUX-002 — a function routed to a pin that cannot carry it
// ---------------------------------------------------------------------------

#[test]
fn pin_function_support_flags_incapable_muxed_pin() {
    // Both pins are muxable (no dedicated I²C peripheral), so the
    // capability-consistency rule cannot see the mistake; the net name
    // says I²C SCL, and gp0 does not list `i2c_scl`.
    let diags = validate(
        r#"board "t" {
  layers 2
  component U1: mcu "rp2350"
  connect U1.gp0 -> "I2C1_SCL"
  connect U1.gp1 -> "I2C1_SCL"
}"#,
    );
    let hits = codes(&diags, "E-SYNTH-PINMUX-002");
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].severity, Severity::Error);
    // A dedicated-peer net is the protocol rule's case, not ours.
    assert!(
        codes(&diags, "E-SYNTH-I2C-001").is_empty(),
        "I2C-001 must not fire without a dedicated peripheral pin"
    );
}

#[test]
fn pin_function_support_is_quiet_when_the_pin_can_carry_it() {
    // Both pins list `i2c_scl`, so the net is fine.
    let diags = validate(
        r#"board "t" {
  layers 2
  component U1: mcu "rp2350"
  component U2: mcu "rp2350"
  connect U1.gp1 -> "I2C1_SCL"
  connect U2.gp1 -> "I2C1_SCL"
}"#,
    );
    assert!(codes(&diags, "E-SYNTH-PINMUX-002").is_empty(), "{diags:?}");
}

#[test]
fn pin_function_support_ignores_passives_and_unrelated_names() {
    // A pull-up resistor has no capabilities and must not be judged; a
    // net whose name implies no function is never checked.
    let diags = validate(
        r#"board "t" {
  layers 2
  component U1: mcu "rp2350"
  component R1: resistor "r_generic_0603"
  connect U1.gp1 -> "I2C1_SCL"
  connect R1.p1 -> "I2C1_SCL"
  connect U1.gp2 -> "STATUS_LED"
}"#,
    );
    assert!(codes(&diags, "E-SYNTH-PINMUX-002").is_empty(), "{diags:?}");
}
