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
    let registry_dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry must load");
    validate_in(src, &registry, config)
}

fn validate_in(
    src: &str,
    registry: &synth_registry::Registry,
    config: &ErcConfig,
) -> Vec<Diagnostic> {
    let filename = "deep_erc.synth".to_string();
    let parse = synth_parser::parse(src, filename.clone());
    let ast = parse.ast.as_ref().expect("source must parse");
    let lowered = synth_ir::lower(ast, registry, &filename);
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

/// A part whose sense pin is rated far above its own supply, defined here
/// rather than taken from the registry so the rule is tested on its own.
const HIGH_SIDE_MONITOR: &str = r#"
id = "test_high_side_monitor"
kind = "sensor"
description = "Three-channel high-side monitor: 3.3 V supply, 26 V sense inputs"

[operating_conditions]
min_voltage_v = 2.7
max_voltage_v = 5.5

[[pins]]
name = "vs"
number = "1"
electrical_type = "power_input"

[[pins]]
name = "gnd"
number = "2"
electrical_type = "ground_reference"

[[pins]]
name = "in_p"
number = "3"
electrical_type = "input"
voltage_max_v = 26.0

[[pins]]
name = "in_n"
number = "4"
electrical_type = "input"
voltage_max_v = 26.0
"#;

fn validate_with_monitor(src: &str) -> Vec<Diagnostic> {
    let mut registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let extra =
        synth_registry::load_from_strs(&[("test_high_side_monitor.synth.toml", HIGH_SIDE_MONITOR)])
            .unwrap();
    for (_, part) in extra.iter() {
        registry.insert(part.clone());
    }
    validate_in(src, &registry, &ErcConfig::default())
}

/// A pin that declares what it tolerates outranks its device's supply.
///
/// A high-side current monitor runs from 3.3 V and specifies 26 V common mode
/// on its sense inputs. Comparing the measured rail against the device supply
/// condemns the part for doing its job.
#[test]
fn a_pin_rated_above_its_device_supply_tolerates_the_rail() {
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component U2: regulator "ams1117_5v"
      component U3: sensor "test_high_side_monitor"
      component R1: resistor "r_generic_0603" value "0.01"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      component C4: capacitor "c_generic_0805" value "10u"
      connect U1.vout -> U3.vs
      connect U3.gnd -> U1.gnd as "GND"
      // The 5 V rail reaches a sense input through the shunt. The pin is
      // rated to 26 V, so this is the measurement working, not an overstress.
      connect U2.vout -> R1.p1
      connect R1.p1 -> U3.in_p
      connect R1.p2 -> U3.in_n
    }"#;
    let diags = validate_with_monitor(src);
    assert!(
        codes(&diags, "E-SYNTH-POWER-008").is_empty(),
        "a 26 V-rated sense pin tolerates a 5 V rail: {:?}",
        codes(&diags, "E-SYNTH-POWER-008")
    );
}

/// And a pin that says nothing still falls back to its device's supply, so
/// the exemption is a statement by the part rather than a hole in the rule.
#[test]
fn a_pin_with_no_rating_still_falls_back_to_the_device_supply() {
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
      connect U1.vout -> U3.scl
      connect U3.sda -> R1.p1
      connect R1.p2 -> U2.vout
    }"#;
    let diags = validate(src);
    assert_eq!(codes(&diags, "E-SYNTH-POWER-008").len(), 1);
}

/// A rail-monitoring divider is not a bus pull-up either.
///
/// Reading a 5 V rail on a 3.3 V ADC pin through a divider is the ordinary
/// way to do it, and the top resistor alone looks exactly like a pull-up to a
/// rail above the device's supply. What the pin sees is the divided voltage.
#[test]
fn a_rail_sense_divider_onto_a_lower_voltage_pin_is_clean() {
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component U2: regulator "ams1117_5v"
      component U3: sensor "bmp280_pressure"
      component R1: resistor "r_generic_0603" value "22k"
      component R2: resistor "r_generic_0603" value "10k"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      component C4: capacitor "c_generic_0805" value "10u"
      connect U1.vout -> U3.vdd
      connect U1.vout -> U3.scl
      // 22k/10k from the 5 V rail puts 1.56 V on the pin, not 5 V.
      connect U2.vout -> R1.p1
      connect R1.p2 -> U3.sda
      connect U3.sda -> R2.p1
      connect R2.p2 -> U1.gnd as "GND"
    }"#;
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-POWER-008").is_empty(),
        "a 22k/10k divider from 5 V presents 1.56 V, which a 3.3 V part is fine with: {:?}",
        codes(&diags, "E-SYNTH-POWER-008")
    );
}

/// And the rule still bites when the divider really does deliver too much.
/// This is what keeps the exemption from being a hole: the ratio is computed,
/// not waved through because two resistors were found.
#[test]
fn a_divider_that_still_delivers_too_much_is_an_error() {
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component U2: regulator "ams1117_5v"
      component U3: sensor "bmp280_pressure"
      component R1: resistor "r_generic_0603" value "1k"
      component R2: resistor "r_generic_0603" value "100k"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      component C4: capacitor "c_generic_0805" value "10u"
      connect U1.vout -> U3.vdd
      connect U1.vout -> U3.scl
      // 1k/100k from 5 V is 4.95 V at the pin: a divider in name only.
      connect U2.vout -> R1.p1
      connect R1.p2 -> U3.sda
      connect U3.sda -> R2.p1
      connect R2.p2 -> U1.gnd as "GND"
    }"#;
    let diags = validate(src);
    assert_eq!(
        codes(&diags, "E-SYNTH-POWER-008").len(),
        1,
        "4.95 V on a 3.3 V part is still an overstress"
    );
}

/// An adjustable regulator's feedback divider is not a bus pull-up.
///
/// The divider from the output rail down to `fb` is how the part is told what
/// to produce, so the two voltages this rule compares are the divider's own
/// two ends: a 5 V buck with a 1.2 V reference reads as "a 5.0 V pull-up on a
/// 1.2 V device", and the rule condemns the circuit that sets the very rail
/// it is measuring against.
///
/// Reduced from the Class-C multi-rail board, where it was the only error.
/// It needs the whole input chain to fire — a bare buck and divider does not
/// resolve the rail's voltage — which is why the fixture is this long.
#[test]
fn a_regulators_feedback_divider_is_not_a_pullup() {
    let src = r##"board "multirail_compute" {
    layers 4
    manufacturer "jlcpcb"
    netclass "V5" {
      trace_width 0.8mm
      clearance 0.25mm
      plane
    }
    group "POWER IN" color "#c2410c" {
      component J1: connector "jst_ph_2pin" 
      component F1: fuse      "polyfuse"
      component D1: diode     "d_schottky"
      component D2: diode     "tvs_array_5v"
      component C1: capacitor "c_electrolytic_100uf" value "100uF"
      component TP1: testpoint "testpoint_pad_1_5" 
      connect J1.p1 -> D1.anode
      connect D1.cathode -> F1.p1
      connect F1.p2 -> D2.ch1 as "V12"
      connect F1.p2 -> C1.p1
      connect F1.p2 -> TP1.p1
      connect J1.p2 -> D2.gnd as "GND"
      connect J1.p2 -> C1.p2
      connect D2.vcc -> D2.ch1
    }
    group "5V BUCK" color "#b91c1c" {
      component U1: regulator "mp2307_buck" 
      component L1: inductor  "l_generic_1210" value "10uH"
      component C2: capacitor "c_generic_0805" value "22uF"
      component C3: capacitor "c_generic_0805" value "22uF"
      component R1: resistor  "r_generic_0603" value "31.6k"
      component R2: resistor  "r_generic_0603" value "10k"
      component C4: capacitor "c_generic_0603" value "100nF"
      component NTC1: resistor "r_thermistor_ntc_10k" value "10k"
      component TP2: testpoint "testpoint_pad_1_5" 
      connect F1.p2 -> U1.in
      connect F1.p2 -> C2.p1
      connect C2.p2 -> U1.gnd
      connect U1.en -> F1.p2
      connect U1.bs -> C4.p1
      connect C4.p2 -> U1.sw
      connect U1.sw -> L1.p1
      connect L1.p2 -> C3.p1 as "V5" class "V5"
      connect L1.p2 -> R1.p1
      connect R1.p2 -> U1.fb
      connect U1.fb -> R2.p1
      connect R2.p2 -> U1.gnd
      connect C3.p2 -> U1.gnd
      connect L1.p2 -> TP2.p1
      connect U1.ep -> U1.gnd
      connect NTC1.p1 -> L1.p2
      connect NTC1.p2 -> U1.gnd
    }
}"##;
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-POWER-008").is_empty(),
        "a feedback divider is how an adjustable rail is set, not a pull-up: {:?}",
        codes(&diags, "E-SYNTH-POWER-008")
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
fn a_tvs_clamped_to_a_rail_is_not_counted_as_a_load() {
    // The TVS array records 1000 mA — its surge/clamp rating, not a steady
    // draw. Counting it charges the regulator a full amp for a part that idles
    // at microamps, so a correctly protected board looks over budget and the
    // "fix" would be to drop the protection. Here U2 draws 200 mA and the rail
    // is well inside the 800 mA regulator once the TVS is not a load.
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component U2: mcu "atmega328p"
      component D1: diode "tvs_array_5v"
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
      connect U1.vout -> D1.vcc as "+3V3"
      connect U1.gnd -> D1.gnd
    }"#;
    assert!(
        codes(&validate(src), "E-SYNTH-POWER-010").is_empty(),
        "a TVS clamp on the rail is not a steady load"
    );
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

// ---------------------------------------------------------------------------
// RS-485 lines: E-SYNTH-ESD-001 and E-SYNTH-RS485-001
// ---------------------------------------------------------------------------

/// A half-duplex RS-485 transceiver with its bus pins tagged. Its
/// description mentions the on-chip ESD rating, as real transceivers do:
/// that is not an external TVS and must not silence the warning.
const RS485_XCVR: &str = r#"id = "rs485_xcvr"
kind = "ic"
version = 0
lifecycle = "active"
description = "Test RS-485 transceiver, 18 kV IEC 61000-4-2 contact ESD"

[[pins]]
name = "a"
number = "1"
electrical_type = "bidirectional"
capabilities = ["rs485_a"]

[[pins]]
name = "b"
number = "2"
electrical_type = "bidirectional"
capabilities = ["rs485_b"]

[[pins]]
name = "gnd"
number = "3"
electrical_type = "ground_reference"
"#;

fn validate_rs485(src: &str) -> Vec<Diagnostic> {
    let mut registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let extra = synth_registry::load_from_strs(&[("rs485_xcvr.synth.toml", RS485_XCVR)]).unwrap();
    for (_, part) in extra.iter() {
        registry.insert(part.clone());
    }
    validate_in(src, &registry, &ErcConfig::default())
}

#[test]
fn an_unprotected_rs485_line_to_a_connector_warns() {
    let src = r#"board "t" {
      layers 2
      component U1: ic "rs485_xcvr"
      component J1: connector "header_1x3"
      connect U1.a -> J1.p1
      connect U1.b -> J1.p2
      connect U1.gnd -> J1.p3
    }"#;
    let diags = validate_rs485(src);
    assert_eq!(
        codes(&diags, "E-SYNTH-ESD-001").len(),
        2,
        "A and B each reach the connector with no TVS: {diags:#?}"
    );
}

#[test]
fn a_tvs_on_each_rs485_line_silences_the_warning() {
    let src = r#"board "t" {
      layers 2
      component U1: ic "rs485_xcvr"
      component J1: connector "header_1x3"
      component D1: diode "d_tvs_5v"
      component D2: diode "d_tvs_5v"
      connect U1.a -> J1.p1
      connect U1.b -> J1.p2
      connect U1.gnd -> J1.p3
      connect U1.a -> D1.anode
      connect U1.b -> D2.anode
      connect D1.cathode -> U1.gnd
      connect D2.cathode -> U1.gnd
    }"#;
    let diags = validate_rs485(src);
    assert!(codes(&diags, "E-SYNTH-ESD-001").is_empty(), "{diags:#?}");
}

#[test]
fn a_termination_resistor_hard_wired_across_a_and_b_warns() {
    let src = r#"board "t" {
      layers 2
      component U1: ic "rs485_xcvr"
      component R1: resistor "r_generic_0603" value "120"
      connect U1.a -> R1.p1
      connect U1.b -> R1.p2
    }"#;
    let diags = validate_rs485(src);
    let found = codes(&diags, "E-SYNTH-RS485-001");
    assert_eq!(found.len(), 1, "{diags:#?}");
    assert_eq!(found[0].severity, Severity::Warning);
}

#[test]
fn a_resistor_from_one_rs485_line_to_ground_is_not_termination() {
    let src = r#"board "t" {
      layers 2
      component U1: ic "rs485_xcvr"
      component R1: resistor "r_generic_0603" value "680"
      connect U1.a -> R1.p1
      connect R1.p2 -> U1.gnd
    }"#;
    let diags = validate_rs485(src);
    assert!(codes(&diags, "E-SYNTH-RS485-001").is_empty(), "{diags:#?}");
}

#[test]
fn a_termination_resistor_behind_a_jumper_does_not_warn() {
    let src = r#"board "t" {
      layers 2
      component U1: ic "rs485_xcvr"
      component R1: resistor "r_generic_0603" value "120"
      component J2: connector "header_1x3"
      connect U1.a -> R1.p1
      connect R1.p2 -> J2.p1
      connect J2.p2 -> U1.b
    }"#;
    let diags = validate_rs485(src);
    assert!(codes(&diags, "E-SYNTH-RS485-001").is_empty(), "{diags:#?}");
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
fn declared_names_that_look_like_the_placeholder_are_still_checked() {
    // `net_7` written by the author is a declared name, not the `net_<n>`
    // lowering gives an unnamed net (absmach/synth#197).
    let collision = r#"board "t" {
      layers 2
      component R1: resistor "r_generic_0603"
      component R2: resistor "r_generic_0603"
      net "net_7" { R1.p1 }
      connect R2.p1 -> R2.p2 as "NET_7"
    }"#;
    assert_eq!(codes(&validate(collision), "E-SYNTH-NAME-007").len(), 1);

    let once = r#"board "t" {
      layers 2
      component R1: resistor "r_generic_0603"
      net "net_7" { R1.p1 }
    }"#;
    assert_eq!(codes(&validate(once), "E-SYNTH-NAME-008").len(), 1);
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
// A long, explicit `Board` fixture: it is almost entirely the literal, and
// adding a field to `Board` pushes it one line past the default threshold.
#[allow(clippy::too_many_lines)]
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
        schematic_overflow: None,
        schematic_paper: None,
        groups: Vec::new(),
        legends: false,
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
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        }],
        nets: vec![
            Net {
                id: NetId(0),
                name: "+3V3".into(),
                auto_named: false,
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
                auto_named: false,
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
        stackup: None,
        netclasses: vec![],
        buses: vec![],
        modules: vec![],
        variants: vec![],
        required_processes: std::collections::BTreeSet::new(),
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

// ---------------------------------------------------------------------------
// Phase 7 — variants and structured values
// ---------------------------------------------------------------------------

#[test]
fn variant_with_unknown_refdes_is_reported() {
    let diags = validate(
        r#"board "t" {
  layers 2
  component U1: mcu "rp2350"
  variant "lite" { dnp U9 }
}"#,
    );
    assert_eq!(codes(&diags, "E-SYNTH-VARIANT-002").len(), 1, "{diags:?}");
}

#[test]
fn duplicate_variant_name_is_reported() {
    let diags = validate(
        r#"board "t" {
  layers 2
  component U1: mcu "rp2350"
  variant "lite" { dnp U1 }
  variant "lite" { dnp U1 }
}"#,
    );
    assert_eq!(codes(&diags, "E-SYNTH-VARIANT-001").len(), 1, "{diags:?}");
}

#[test]
fn class_two_ceramic_at_high_bias_warns() {
    let diags = validate(
        r#"board "t" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component C1: capacitor "c_generic_0603" dielectric "X7R" voltage "6.3V"
  power "+5V" 5v
  connect U1.vout -> C1.p1 as "+5V"
  connect C1.p2 -> "GND"
  connect U1.gnd -> "GND"
  connect U1.vin -> "VIN"
}"#,
    );
    let hits = codes(&diags, "E-SYNTH-CAP-001");
    assert_eq!(hits.len(), 1, "{diags:?}");
    assert_eq!(hits[0].severity, Severity::Warning);
}

#[test]
fn class_one_ceramic_and_low_bias_stay_quiet() {
    let diags = validate(
        r#"board "t" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component C1: capacitor "c_generic_0603" dielectric "C0G" voltage "6.3V"
  component C2: capacitor "c_generic_0603" dielectric "X7R" voltage "50V"
  power "+5V" 5v
  connect U1.vout -> C1.p1 as "+5V"
  connect C1.p2 -> "GND"
  connect U1.vout -> C2.p1 as "+5V"
  connect C2.p2 -> "GND"
  connect U1.gnd -> "GND"
  connect U1.vin -> "VIN"
}"#,
    );
    assert!(codes(&diags, "E-SYNTH-CAP-001").is_empty(), "{diags:?}");
}

#[test]
fn derating_needs_both_dielectric_and_rating() {
    // Without the structured values the rule declines rather than guessing.
    let diags = validate(
        r#"board "t" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component C1: capacitor "c_generic_0603"
  power "+5V" 5v
  connect U1.vout -> C1.p1 as "+5V"
  connect C1.p2 -> "GND"
  connect U1.gnd -> "GND"
  connect U1.vin -> "VIN"
}"#,
    );
    assert!(codes(&diags, "E-SYNTH-CAP-001").is_empty(), "{diags:?}");
}

/// A pin's rating is about the pin, so it is compared against what reaches
/// the pin.
///
/// The exemption used to test the *rail* against `voltage_max_v`, which undid
/// the divider the rule had just computed: a 12 V rail divided to 4 V on an
/// input rated 5 V was still reported, because 12 V exceeds 5 V, while the
/// pin was inside its rating the whole time. The rating sits above the
/// divided output and below the rail, so only the right comparison clears it.
const TWELVE_VOLT_SUPPLY: &str = r#"
id = "test_12v_supply"
kind = "regulator"
description = "12 V rail, declared here so the rail sits above the pin rating"

[operating_conditions]
min_voltage_v = 14.0
max_voltage_v = 40.0
max_current_ma = 1000.0

[[pins]]
name = "gnd"
number = "1"
electrical_type = "power_input"

[[pins]]
name = "vout"
number = "2"
electrical_type = "power_output"
voltage_nominal_v = 12.0

[[pins]]
name = "vin"
number = "3"
electrical_type = "power_input"
"#;

const FIVE_VOLT_TOLERANT: &str = r#"
id = "test_five_volt_tolerant"
kind = "sensor"
description = "3.3 V part with a 5 V-tolerant sense input"

[operating_conditions]
min_voltage_v = 3.0
max_voltage_v = 3.6

[[pins]]
name = "vdd"
number = "1"
electrical_type = "power_input"

[[pins]]
name = "gnd"
number = "2"
electrical_type = "ground_reference"

[[pins]]
name = "sense"
number = "3"
electrical_type = "input"
voltage_max_v = 4.5
"#;

#[test]
fn a_divided_rail_is_judged_against_the_pin_rating_not_the_rail() {
    let mut registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let extra = synth_registry::load_from_strs(&[
        ("test_five_volt_tolerant.synth.toml", FIVE_VOLT_TOLERANT),
        ("test_12v_supply.synth.toml", TWELVE_VOLT_SUPPLY),
    ])
    .unwrap();
    for (_, part) in extra.iter() {
        registry.insert(part.clone());
    }

    // 12 V through 100k/50k puts 4.0 V on the pin: above the 3.3 V supply,
    // so the device-supply check would report it; below the pin's 5 V
    // rating, so the pin's own rating clears it; and below the 12 V rail,
    // which is what the old comparison looked at and refused to clear.
    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component U2: regulator "test_12v_supply"
      component U3: sensor "test_five_volt_tolerant"
      component R1: resistor "r_generic_0603" value "100k"
      component R2: resistor "r_generic_0603" value "50k"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      connect U1.vout -> U3.vdd
      connect U2.vout -> R1.p1
      connect R1.p2 -> U3.sense
      connect U3.sense -> R2.p1
      connect R2.p2 -> U1.gnd as "GND"
    }"#;
    let diags = validate_in(src, &registry, &ErcConfig::default());
    assert!(
        codes(&diags, "E-SYNTH-POWER-008").is_empty(),
        "4.0 V is inside the pin's declared 4.5 V rating: {:?}",
        codes(&diags, "E-SYNTH-POWER-008")
    );
}

/// A resistor network with more than two branches is not a divider.
///
/// Keeping the last resistor found on each side turned any network with a
/// branch to a rail and a branch to ground into a two-resistor divider, and
/// the arithmetic that followed described a circuit the board does not have.
/// That suppresses a real overvoltage rather than reporting it, so the
/// exemption now needs exactly one readable resistor to each side.
///
/// Here a second 100k to the 12 V rail puts the true node at 6.0 V, not the
/// 4.0 V the two-resistor reading would have claimed, and 6.0 V is over the
/// pin's 4.5 V rating. The conservative rail check has to stand.
#[test]
fn a_three_branch_resistor_network_is_not_exempted_as_a_divider() {
    let mut registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let extra = synth_registry::load_from_strs(&[
        ("test_five_volt_tolerant.synth.toml", FIVE_VOLT_TOLERANT),
        ("test_12v_supply.synth.toml", TWELVE_VOLT_SUPPLY),
    ])
    .unwrap();
    for (_, part) in extra.iter() {
        registry.insert(part.clone());
    }

    let src = r#"board "t" {
      layers 2
      component U1: regulator "ams1117_3v3"
      component U2: regulator "test_12v_supply"
      component U3: sensor "test_five_volt_tolerant"
      component R1: resistor "r_generic_0603" value "100k"
      component R2: resistor "r_generic_0603" value "50k"
      component R3: resistor "r_generic_0603" value "100k"
      component C1: capacitor "c_generic_0805" value "10u"
      component C2: capacitor "c_generic_0805" value "10u"
      component C3: capacitor "c_generic_0805" value "10u"
      connect U1.vout -> U3.vdd
      connect U2.vout -> R1.p1
      connect R1.p2 -> U3.sense
      connect U3.sense -> R2.p1
      connect R2.p2 -> U1.gnd as "GND"
      // A second branch to the same rail: two resistors up, one down.
      connect U2.vout -> R3.p1
      connect R3.p2 -> U3.sense
    }"#;
    let diags = validate_in(src, &registry, &ErcConfig::default());
    assert_eq!(
        codes(&diags, "E-SYNTH-POWER-008").len(),
        1,
        "two resistors to the rail is not a divider this can reduce to two \
         values, so the rail voltage stands: {:?}",
        codes(&diags, "E-SYNTH-POWER-008")
    );
}

/// The CH340G's `v3` is not a rail with a budget.
///
/// `v3` is the internal LDO's decoupling tap, and the part's 20 mA
/// `max_current_ma` is its own draw rather than anything it can deliver.
/// Once the pin was corrected to `power_output` to match the stock symbol,
/// `PowerBudgetRule` read it as a 20 mA regulator and charged it for whatever
/// sat on the net, so `pass__power_004_sufficient_decoupling` reported two
/// E-SYNTH-POWER-010 overruns on a board that is fine.
#[test]
fn the_ch340g_auxiliary_output_is_not_a_regulator_rail() {
    let src = r#"board "power_anomaly_pass" {
      layers 2
      component U1: ic "ch340g"
      component U2: ic "ch340g"
      component J1: connector "header_1x4"
      component C1: capacitor "c_generic_0603" value "100nF"
      component C2: capacitor "c_generic_0603" value "100nF"

      connect J1.p1 -> U1.vcc
      connect J1.p1 -> U2.vcc
      connect J1.p1 -> C1.p1
      connect J1.p1 -> C2.p1

      connect J1.p2 -> U1.gnd
      connect J1.p2 -> U2.gnd
      connect J1.p2 -> C1.p2
      connect J1.p2 -> C2.p2

      connect U1.v3 -> J1.p3
      connect U2.v3 -> J1.p4
    }"#;
    let diags = validate(src);
    assert!(
        codes(&diags, "E-SYNTH-POWER-010").is_empty(),
        "an internal regulator's decoupling pin supplies no rail: {:?}",
        codes(&diags, "E-SYNTH-POWER-010")
    );
}
