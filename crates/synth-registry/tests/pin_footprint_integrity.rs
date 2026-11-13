// SPDX-License-Identifier: Apache-2.0

//! #43: the registry qualification must catch structural pin/footprint
//! defects, not only missing provenance. Each test mutates one fact and
//! asserts the part is blocked or explicitly unproven.

use std::collections::{BTreeMap, BTreeSet};

use synth_registry::qualify::{
    qualify_part, CheckStatus, FootprintFacts, PadFacts, PadSide, PartQualification, SymbolFacts,
};

const BASE: &str = r#"
id = "subject"
kind = "mcu"
kicad_symbol = "Lib:Subject"
kicad_footprint = "Lib:Subject"

[footprint_dimensions]
width_mm = 4.0
height_mm = 4.0
courtyard_margin_mm = 0.25

[[pins]]
name = "vdd"
number = "1"
electrical_type = "power_input"

[[pins]]
name = "gnd"
number = "2"
electrical_type = "power_input"

[[pins]]
name = "sda"
number = "3"
electrical_type = "bidirectional"

[[pins]]
name = "scl"
number = "4"
electrical_type = "input"

[provenance]
reviewed_by = "a reviewer"
reviewed_at = "2026-11-13"
datasheet_url = "https://example.invalid/subject.pdf"
"#;

struct Pads(Vec<PadFacts>);
impl FootprintFacts for Pads {
    fn pads(&self, _lib_id: &str) -> Option<Vec<PadFacts>> {
        Some(self.0.clone())
    }
}

struct Symbol {
    numbers: BTreeSet<String>,
    names: BTreeMap<String, BTreeSet<String>>,
}
impl SymbolFacts for Symbol {
    fn pin_numbers(&self, _lib_id: &str) -> Option<BTreeSet<String>> {
        Some(self.numbers.clone())
    }
    fn pin_names(&self, _lib_id: &str) -> Option<BTreeMap<String, BTreeSet<String>>> {
        Some(self.names.clone())
    }
}

fn pad(number: &str, cx: f64, side: PadSide) -> PadFacts {
    PadFacts {
        number: number.to_string(),
        center_mm: (cx, 0.0),
        size_mm: (0.5, 0.5),
        side,
        is_npth: false,
    }
}

fn good_pads() -> Pads {
    Pads(vec![
        pad("1", -1.5, PadSide::Front),
        pad("2", -0.5, PadSide::Front),
        pad("3", 0.5, PadSide::Front),
        pad("4", 1.5, PadSide::Front),
    ])
}

fn good_symbol() -> Symbol {
    let names = BTreeMap::from([
        ("1".to_string(), ["VDD".to_string()].into_iter().collect()),
        ("2".to_string(), ["GND".to_string()].into_iter().collect()),
        ("3".to_string(), ["SDA".to_string()].into_iter().collect()),
        ("4".to_string(), ["SCL".to_string()].into_iter().collect()),
    ]);
    Symbol {
        numbers: names.keys().cloned().collect(),
        names,
    }
}

fn base_part() -> synth_registry::Part {
    let registry = synth_registry::load_from_strs(&[("subject.synth.toml", BASE)])
        .expect("the base part loads");
    registry.lookup("subject").expect("subject").clone()
}

fn codes(q: &PartQualification) -> Vec<&str> {
    q.findings().map(|f| f.code.as_str()).collect()
}

#[test]
fn the_unmutated_part_qualifies() {
    let q = qualify_part(&base_part(), &good_pads(), &good_symbol());
    assert_eq!(q.status, CheckStatus::Pass, "{:#?}", q.checks);
    assert!(codes(&q).is_empty(), "{:?}", codes(&q));
}

#[test]
fn a_missing_footprint_pad_blocks() {
    let pads = Pads(good_pads().0[..3].to_vec());
    let q = qualify_part(&base_part(), &pads, &good_symbol());
    assert!(codes(&q).contains(&"E-SYNTH-QUAL-001"), "{:#?}", q.checks);
}

#[test]
fn a_swapped_symbol_pin_name_blocks() {
    let mut symbol = good_symbol();
    symbol
        .names
        .insert("3".to_string(), ["SCL".to_string()].into_iter().collect());
    symbol
        .names
        .insert("4".to_string(), ["SDA".to_string()].into_iter().collect());
    let q = qualify_part(&base_part(), &good_pads(), &symbol);
    assert!(codes(&q).contains(&"E-SYNTH-QUAL-005"), "{:#?}", q.checks);
}

#[test]
fn a_mirrored_footprint_blocks() {
    let pads = Pads(
        good_pads()
            .0
            .into_iter()
            .map(|mut p| {
                p.side = PadSide::Back;
                p
            })
            .collect(),
    );
    let q = qualify_part(&base_part(), &pads, &good_symbol());
    assert!(codes(&q).contains(&"E-SYNTH-QUAL-012"), "{:#?}", q.checks);
}

#[test]
fn a_unit_typo_in_the_package_dimensions_blocks() {
    let mut part = base_part();
    part.footprint_dimensions.as_mut().unwrap().width_mm = 400.0;
    let q = qualify_part(&part, &good_pads(), &good_symbol());
    assert!(codes(&q).contains(&"E-SYNTH-QUAL-006"), "{:#?}", q.checks);
}

#[test]
fn stale_review_metadata_blocks() {
    let mut part = base_part();
    part.provenance.as_mut().unwrap().reviewed_at = Some("not-a-date".to_string());
    let q = qualify_part(&part, &good_pads(), &good_symbol());
    assert!(codes(&q).contains(&"E-SYNTH-QUAL-015"), "{:#?}", q.checks);
}

// ---------------------------------------------------------------------------
// Class fixtures from #43: RF, connector, and the exposed thermal pad.
// Each class gets its own part and footprint facts, because the mutation
// that matters is different per package family.
// ---------------------------------------------------------------------------

/// A two-pin RF part (a balun-like series element): two pads, two symbol
/// pins, and the declared pin map agreeing with both.
const RF: &str = r#"
id = "rf_balun"
kind = "rf"
kicad_symbol = "Lib:Balun"
kicad_footprint = "Lib:Balun-2Pin"

[footprint_dimensions]
width_mm = 2.0
height_mm = 1.2
courtyard_margin_mm = 0.25

[[pins]]
name = "primary"
number = "1"
electrical_type = "passive"

[[pins]]
name = "secondary"
number = "2"
electrical_type = "passive"

[provenance]
reviewed_by = "a reviewer"
reviewed_at = "2026-11-13"
datasheet_url = "https://example.invalid/balun.pdf"
"#;

fn rf_part() -> synth_registry::Part {
    let registry =
        synth_registry::load_from_strs(&[("rf_balun.synth.toml", RF)]).expect("the rf part loads");
    registry.lookup("rf_balun").expect("rf_balun").clone()
}

fn rf_pads() -> Pads {
    Pads(vec![
        pad("1", -0.6, PadSide::Front),
        pad("2", 0.6, PadSide::Front),
    ])
}

fn rf_symbol() -> Symbol {
    let names = BTreeMap::from([
        (
            "1".to_string(),
            ["PRIMARY".to_string()].into_iter().collect(),
        ),
        (
            "2".to_string(),
            ["SECONDARY".to_string()].into_iter().collect(),
        ),
    ]);
    Symbol {
        numbers: names.keys().cloned().collect(),
        names,
    }
}

/// A 1x4 header: four pads, four symbol pins.
const HEADER: &str = r#"
id = "qual_header"
kind = "connector"
kicad_symbol = "Lib:Conn_01x04"
kicad_footprint = "Lib:Header-1x04"

[footprint_dimensions]
width_mm = 10.0
height_mm = 2.6
courtyard_margin_mm = 0.25

[[pins]]
name = "p1"
number = "1"
electrical_type = "passive"

[[pins]]
name = "p2"
number = "2"
electrical_type = "passive"

[[pins]]
name = "p3"
number = "3"
electrical_type = "passive"

[[pins]]
name = "p4"
number = "4"
electrical_type = "passive"

[provenance]
reviewed_by = "a reviewer"
reviewed_at = "2026-11-13"
datasheet_url = "https://example.invalid/header.pdf"
"#;

fn header_part() -> synth_registry::Part {
    let registry = synth_registry::load_from_strs(&[("qual_header.synth.toml", HEADER)])
        .expect("the header part loads");
    registry.lookup("qual_header").expect("qual_header").clone()
}

fn header_pads() -> Pads {
    Pads(vec![
        pad("1", -3.81, PadSide::Front),
        pad("2", -1.27, PadSide::Front),
        pad("3", 1.27, PadSide::Front),
        pad("4", 3.81, PadSide::Front),
    ])
}

fn header_symbol() -> Symbol {
    let names = ["1", "2", "3", "4"]
        .into_iter()
        .map(|n| (n.to_string(), [format!("Pin_{n}")].into_iter().collect()))
        .collect();
    Symbol {
        numbers: ["1", "2", "3", "4"]
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        names,
    }
}

#[test]
fn the_rf_class_fixture_qualifies_unmutated() {
    let q = qualify_part(&rf_part(), &rf_pads(), &rf_symbol());
    assert_eq!(q.status, CheckStatus::Pass, "{:#?}", q.checks);
    assert!(codes(&q).is_empty(), "{:?}", codes(&q));
}

#[test]
fn an_rf_part_with_more_pins_than_its_two_pin_footprint_blocks() {
    // The pad-count disagreement that matters for a 2-pin RF footprint:
    // the part claims a third pad the package does not have.
    let mut part = rf_part();
    part.pins.push(synth_registry::Pin {
        name: "case".to_string(),
        number: synth_registry::PinNumber("3".to_string()),
        electrical_type: synth_registry::ElectricalType::Passive,
        capabilities: Vec::new(),
        required: false,
        footprint_only: false,
        unit: None,
        voltage_max_v: None,
        voltage_min_v: None,
        voltage_nominal_v: None,
        drive: None,
    });
    let q = qualify_part(&part, &rf_pads(), &rf_symbol());
    assert!(codes(&q).contains(&"E-SYNTH-QUAL-001"), "{:#?}", q.checks);
}

#[test]
fn the_connector_class_fixture_qualifies_unmutated() {
    let q = qualify_part(&header_part(), &header_pads(), &header_symbol());
    assert_eq!(q.status, CheckStatus::Pass, "{:#?}", q.checks);
    assert!(codes(&q).is_empty(), "{:?}", codes(&q));
}

#[test]
fn a_header_declaring_one_more_pin_than_its_pads_blocks() {
    // One extra pin in the pin map, one pad short in the footprint.
    let mut part = header_part();
    part.pins.push(synth_registry::Pin {
        name: "p5".to_string(),
        number: synth_registry::PinNumber("5".to_string()),
        electrical_type: synth_registry::ElectricalType::Passive,
        capabilities: Vec::new(),
        required: false,
        footprint_only: false,
        unit: None,
        voltage_max_v: None,
        voltage_min_v: None,
        voltage_nominal_v: None,
        drive: None,
    });
    let q = qualify_part(&part, &header_pads(), &header_symbol());
    assert!(codes(&q).contains(&"E-SYNTH-QUAL-001"), "{:#?}", q.checks);
    let f = q
        .findings()
        .find(|f| f.code == "E-SYNTH-QUAL-001")
        .expect("an unmatched-pin finding");
    assert!(f.found.contains('5'), "the extra pin is named: {f:?}");
}

#[test]
fn an_undeclared_exposed_pad_blocks() {
    // The footprint has a copper thermal pad under the body that no pin
    // declares — the direction the pad-pin check exists for.
    let pads = Pads(vec![
        pad("1", -1.5, PadSide::Front),
        pad("2", -0.5, PadSide::Front),
        pad("3", 0.5, PadSide::Front),
        pad("4", 1.5, PadSide::Front),
        PadFacts {
            number: "9".to_string(),
            center_mm: (0.0, 0.0),
            size_mm: (2.5, 2.5),
            side: PadSide::Front,
            is_npth: false,
        },
    ]);
    let q = qualify_part(&base_part(), &pads, &good_symbol());
    assert!(codes(&q).contains(&"E-SYNTH-QUAL-002"), "{:#?}", q.checks);
    let f = q
        .findings()
        .find(|f| f.code == "E-SYNTH-QUAL-002")
        .expect("an orphan-pad finding");
    assert!(
        f.found.contains('9'),
        "the exposed pad is named so the fix is obvious: {f:?}"
    );
}
