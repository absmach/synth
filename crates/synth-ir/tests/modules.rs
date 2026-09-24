// SPDX-License-Identifier: Apache-2.0

//! Module instantiation, interface bundles, and buses (§Phase 3).
//!
//! `fixtures/ir/module_divider.synth` covers the happy path as a
//! snapshot; these tests pin the behaviour that snapshot cannot
//! express — that two instances *share* a bound net rather than
//! duplicating it, and the seven `E-SYNTH-MODULE-*` diagnostics.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn by_refdes<'a>(board: &'a synth_ir::Board, refdes: &str) -> Option<&'a synth_ir::Component> {
    board.components.iter().find(|c| c.refdes == refdes)
}

fn registry() -> synth_registry::Registry {
    synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load")
}

/// Parse + lower `src`, returning the board (if any) and every
/// diagnostic code, in order.
fn lower_src(src: &str) -> (Option<synth_ir::Board>, Vec<String>) {
    let parse = synth_parser::parse(src, "modules.synth");
    assert!(
        !parse.has_errors(),
        "parser diagnostics: {:?}",
        parse
            .diagnostics
            .iter()
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );
    let ast = parse.ast.expect("clean parse yields an ast");
    let lowered = synth_ir::lower(&ast, &registry(), "modules.synth");
    (
        lowered.board,
        lowered.diagnostics.iter().map(|d| d.code.clone()).collect(),
    )
}

const TWO_CHANNELS: &str = r#"board "b" {
    layers 2
    module "Ch" (a: input, b: output) {
        component R1: resistor "r_generic_0603"
        component R2: resistor "r_generic_0603"
        connect a -> R1.p1
        connect R1.p2 -> R2.p1
        connect R2.p2 -> b
    }
    use "Ch" as X1 {
        a -> "SHARED"
        b -> "OUT_1"
    }
    use "Ch" as X2 {
        a -> "SHARED"
        b -> "OUT_2"
    }
}"#;

#[test]
fn instances_share_a_bound_net_by_name() {
    let (board, codes) = lower_src(TWO_CHANNELS);
    assert!(codes.is_empty(), "unexpected diagnostics: {codes:?}");
    let board = board.expect("board");

    let refs: Vec<&str> = board.components.iter().map(|c| c.refdes.as_str()).collect();
    assert_eq!(refs, vec!["X1_R1", "X1_R2", "X2_R1", "X2_R2"]);

    // "SHARED" must be ONE net carrying the `a` pin of both instances,
    // not two nets that merely happen to share a name.
    let shared: Vec<&synth_ir::Net> = board.nets.iter().filter(|n| n.name == "SHARED").collect();
    assert_eq!(shared.len(), 1, "SHARED must be a single merged net");
    let owners: Vec<String> = shared[0]
        .endpoints
        .iter()
        .map(|ep| board.component(ep.component).unwrap().refdes.clone())
        .collect();
    assert!(owners.contains(&"X1_R1".to_string()), "{owners:?}");
    assert!(owners.contains(&"X2_R1".to_string()), "{owners:?}");

    // Instance-local nets stay distinct.
    assert!(board.nets.iter().any(|n| n.name == "OUT_1"));
    assert!(board.nets.iter().any(|n| n.name == "OUT_2"));
}

#[test]
fn interface_bundle_binds_every_member_in_one_clause() {
    let src = r#"board "b" {
        layers 2
        module "Leaf" (i2c: I2C) {
            component U1: sensor "bmp280_pressure"
            connect U1.sda -> i2c.sda
            connect U1.scl -> i2c.scl
            connect U1.vdd -> "3V3"
            connect U1.gnd -> "GND"
        }
        interface "I2C" (sda: i2c_sda, scl: i2c_scl)
        bus "BUS0" (sda, scl)
        use "Leaf" as L1 {
            i2c -> "BUS0"
        }
    }"#;
    let (board, codes) = lower_src(src);
    let board = board.expect("board");
    assert!(
        !codes.iter().any(|c| c.starts_with("E-SYNTH-MODULE")),
        "module diagnostics: {codes:?}"
    );
    // One `i2c -> "BUS0"` binding must have created both member nets.
    assert!(board.nets.iter().any(|n| n.name == "BUS0.sda"), "BUS0.sda");
    assert!(board.nets.iter().any(|n| n.name == "BUS0.scl"), "BUS0.scl");
    assert_eq!(board.buses.len(), 1);
    assert_eq!(board.buses[0].members, vec!["sda", "scl"]);
    assert_eq!(board.modules[0].ports.len(), 1);
}

#[test]
fn param_override_beats_default() {
    let src = r#"board "b" {
        layers 2
        module "One" (a: input, b: output) {
            param r: resistance = 4.7kohm
            component R1: resistor "r_generic_0603" value $r
            connect a -> R1.p1
            connect R1.p2 -> b
        }
        use "One" as P1 {
            a -> "IN"
            b -> "OUT"
        }
        use "One" as P2 (r = 22kohm) {
            a -> "IN"
            b -> "OUT2"
        }
    }"#;
    let (board, codes) = lower_src(src);
    assert!(codes.is_empty(), "{codes:?}");
    let board = board.expect("board");
    let value = |refdes: &str| by_refdes(&board, refdes).and_then(|c| c.value.clone());
    assert_eq!(value("P1_R1").as_deref(), Some("4.7kohm"));
    assert_eq!(value("P2_R1").as_deref(), Some("22kohm"));
}

#[test]
fn unknown_module_is_reported() {
    let (_, codes) = lower_src(r#"board "b" { layers 2 use "Nope" as X { } }"#);
    assert!(
        codes.contains(&"E-SYNTH-MODULE-001".to_string()),
        "{codes:?}"
    );
}

#[test]
fn unknown_parameter_is_reported() {
    let src = r#"board "b" {
        layers 2
        module "M" (a: input) {
            component R1: resistor "r_generic_0603"
            connect a -> R1.p1
        }
        use "M" as X (nope = 1kohm) { a -> "IN" }
    }"#;
    let (_, codes) = lower_src(src);
    assert!(
        codes.contains(&"E-SYNTH-MODULE-002".to_string()),
        "{codes:?}"
    );
}

#[test]
fn unknown_port_is_reported() {
    let src = r#"board "b" {
        layers 2
        module "M" (a: input) {
            component R1: resistor "r_generic_0603"
            connect a -> R1.p1
        }
        use "M" as X { a -> "IN", b -> "OUT" }
    }"#;
    let (_, codes) = lower_src(src);
    assert!(
        codes.contains(&"E-SYNTH-MODULE-003".to_string()),
        "{codes:?}"
    );
}

#[test]
fn unbound_port_is_reported() {
    let src = r#"board "b" {
        layers 2
        module "M" (a: input, b: output) {
            component R1: resistor "r_generic_0603"
            connect a -> R1.p1
            connect R1.p2 -> b
        }
        use "M" as X { a -> "IN" }
    }"#;
    let (_, codes) = lower_src(src);
    assert!(
        codes.contains(&"E-SYNTH-MODULE-004".to_string()),
        "{codes:?}"
    );
}

#[test]
fn nested_module_instantiation_is_reported() {
    let src = r#"board "b" {
        layers 2
        module "Inner" (a: input) {
            component R1: resistor "r_generic_0603"
            connect a -> R1.p1
        }
        module "Outer" (a: input) {
            component R2: resistor "r_generic_0603"
            connect a -> R2.p1
            use "Inner" as N1 { a -> "NESTED" }
        }
        use "Outer" as O1 { a -> "IN" }
    }"#;
    let (_, codes) = lower_src(src);
    assert!(
        codes.contains(&"E-SYNTH-MODULE-005".to_string()),
        "{codes:?}"
    );
}

#[test]
fn unknown_bus_in_bind_is_reported() {
    let src = r#"board "b" {
        layers 2
        interface "I2C" (sda: i2c_sda)
        component J1: connector "jst_ph_2pin"
        bind "NOPE" : I2C { sda -> J1.p1 }
    }"#;
    let (_, codes) = lower_src(src);
    assert!(
        codes.contains(&"E-SYNTH-MODULE-006".to_string()),
        "{codes:?}"
    );
}

#[test]
fn duplicate_instance_label_is_reported() {
    let src = r#"board "b" {
        layers 2
        module "M" (a: input) {
            component R1: resistor "r_generic_0603"
            connect a -> R1.p1
        }
        use "M" as X { a -> "IN" }
        use "M" as X { a -> "IN2" }
    }"#;
    let (_, codes) = lower_src(src);
    assert!(
        codes.contains(&"E-SYNTH-MODULE-007".to_string()),
        "{codes:?}"
    );
}

#[test]
fn instance_membership_is_a_sheet() {
    let (board, codes) = lower_src(TWO_CHANNELS);
    assert!(codes.is_empty(), "{codes:?}");
    let board = board.expect("board");
    // Instance identity is carried by the component's sheet, so §P26
    // can split one instance per page when the design overflows and
    // the placer can cluster an instance's parts together.
    let x2 = by_refdes(&board, "X2_R1").expect("X2_R1");
    assert_eq!(x2.sheet.as_deref(), Some("X2"));
    let x1 = by_refdes(&board, "X1_R1").expect("X1_R1");
    assert_eq!(x1.sheet.as_deref(), Some("X1"));
}
