// SPDX-License-Identifier: Apache-2.0

//! `suggest_testbench` drafts a sidecar from the shapes in a compiled board.
//! Every generated value carries its source; nothing is guessed.

use std::path::{Path, PathBuf};

#[path = "support/real_ngspice.rs"]
mod real_ngspice_support;

fn real_ngspice() -> Option<PathBuf> {
    real_ngspice_support::real_ngspice()
}

fn example_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name)
}

fn registry_parts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("registry")
        .join("parts")
        .canonicalize()
        .expect("workspace registry/parts")
}

/// Compile a design from its real path, with imports rooted at its folder.
fn board_from_file(path: &Path) -> synth_ir::Board {
    let source = std::fs::read_to_string(path).expect("read design");
    board(&source)
}

fn board(source: &str) -> synth_ir::Board {
    board_with_registry(source, |_| {})
}

fn board_with_registry(
    source: &str,
    patch: impl FnOnce(&mut synth_registry::Registry),
) -> synth_ir::Board {
    let file = "suggest.synth".to_string();
    let parsed = synth_parser::parse(source, file.clone());
    assert!(
        parsed.diagnostics.is_empty(),
        "parse: {:?}",
        parsed.diagnostics
    );
    let ast = parsed.ast.expect("ast");
    let resolved = synth_ir::resolve_imports(
        &ast,
        &synth_ir::FsImportLoader {
            root: PathBuf::from("."),
        },
        &file,
    );
    assert!(
        resolved
            .diagnostics
            .iter()
            .all(|d| !d.severity.is_blocking()),
        "imports: {:?}",
        resolved.diagnostics
    );
    let mut registry = synth_registry::embedded_registry().clone();
    patch(&mut registry);
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    assert!(
        lowered
            .diagnostics
            .iter()
            .all(|d| !d.severity.is_blocking()),
        "lower: {:?}",
        lowered.diagnostics
    );
    lowered.board.expect("board")
}

fn suggestion(board: &synth_ir::Board) -> (String, Vec<String>) {
    let suggestion = synth_sim::suggest_testbench(board);
    let toml = suggestion.toml.expect("a testbench was suggested");
    (toml, suggestion.not_covered)
}

fn hand_written(example: &str) -> (synth_sim::Testbench, String) {
    let design = example_dir(example).join(format!("{example}.synth"));
    let sidecar = synth_sim::sidecar_path(&design);
    let text = std::fs::read_to_string(&sidecar).expect("hand-written sidecar");
    (
        synth_sim::Testbench::from_path(&sidecar).expect("hand-written sidecar parses"),
        text,
    )
}

fn measure<'a>(testbench: &'a synth_sim::Testbench, name: &str) -> &'a synth_sim::Measure {
    testbench
        .measure
        .iter()
        .find(|measure| measure.name == name)
        .unwrap_or_else(|| panic!("measure `{name}` not found"))
}

/// The generated limits must match the hand-written reference sidecars.
fn assert_same_limits(example: &str, generated: &synth_sim::Testbench) {
    let (reference, _) = hand_written(example);
    for reference in &reference.measure {
        let generated = measure(generated, &reference.name);
        assert_eq!(generated.kind, reference.kind, "{} kind", reference.name);
        assert_eq!(generated.node, reference.node, "{} node", reference.name);
        assert_eq!(
            generated.element, reference.element,
            "{} element",
            reference.name
        );
        assert_eq!(
            generated.at_frequency, reference.at_frequency,
            "{} at_frequency",
            reference.name
        );
        assert_eq!(
            generated.min, reference.min,
            "{} min in the suggested testbench",
            reference.name
        );
        assert_eq!(
            generated.max, reference.max,
            "{} max in the suggested testbench",
            reference.name
        );
    }
    assert_eq!(
        generated.measure.len(),
        reference.measure.len(),
        "suggested shape count"
    );
}

#[test]
fn the_divider_example_gets_its_hand_written_limits() {
    let design = example_dir("divider").join("divider.synth");
    let board = board_from_file(&design);
    let (toml, not_covered) = suggestion(&board);

    let testbench = synth_sim::Testbench::from_toml(&toml).expect("generated sidecar parses");
    assert_eq!(testbench.ground, "GND");
    assert!(testbench.analysis.op);
    assert_eq!(testbench.source.len(), 1);
    assert_eq!(testbench.source[0].net.as_deref(), Some("VCC"));
    assert_eq!(testbench.source[0].dc, Some(12.0));
    assert_same_limits("divider", &testbench);
    assert!(
        not_covered.is_empty(),
        "nothing is left out: {not_covered:?}"
    );

    // Every generated measure names its source.
    assert!(toml.contains("R1"), "{toml}");
    assert!(toml.contains("R2"), "{toml}");
    assert!(toml.contains("value"), "{toml}");
}

#[test]
fn the_led_example_gets_its_hand_written_limits_and_cites_the_datasheet() {
    let design = example_dir("led").join("led.synth");
    let board = board_from_file(&design);
    let (toml, not_covered) = suggestion(&board);

    let testbench = synth_sim::Testbench::from_toml(&toml).expect("generated sidecar parses");
    assert_eq!(testbench.ground, "GND");
    assert!(testbench.analysis.op);
    assert_eq!(testbench.source[0].net.as_deref(), Some("VCC"));
    assert_eq!(testbench.source[0].dc, Some(5.0));
    assert_same_limits("led", &testbench);
    let iled = measure(&testbench, "iled");
    assert_eq!(iled.kind, synth_sim::MeasureKind::Current);
    assert_eq!(iled.element.as_deref(), Some("LED1"));
    assert!(
        not_covered.is_empty(),
        "nothing is left out: {not_covered:?}"
    );

    let datasheet = board
        .components
        .iter()
        .find(|component| component.refdes == "LED1")
        .and_then(|component| component.part.as_ref())
        .and_then(|part| part.provenance.as_ref())
        .and_then(|provenance| provenance.datasheet_url.as_deref())
        .expect("the reference LED carries a datasheet");
    assert!(toml.contains("ltst_c190krkt"), "{toml}");
    assert!(toml.contains("forward_current_ma"), "{toml}");
    assert!(toml.contains(datasheet), "{toml}");
    assert!(toml.contains("# UNREVIEWED"), "{toml}");
    // 25 mA is above the 21 mA band, so the comment must not claim it binds.
    assert!(toml.contains("does not bind"), "{toml}");
    assert!(!toml.contains("caps the upper limit"), "{toml}");
}

#[test]
fn the_rc_example_gets_its_hand_written_limits() {
    let design = example_dir("rc_filter").join("rc_filter.synth");
    let board = board_from_file(&design);
    let (toml, not_covered) = suggestion(&board);

    let testbench = synth_sim::Testbench::from_toml(&toml).expect("generated sidecar parses");
    assert_eq!(testbench.ground, "GND");
    let ac = testbench.analysis.ac.expect("ac analysis");
    assert!(testbench.source[0].ac.is_some_and(|ac| ac > 0.0));
    assert_eq!(testbench.source[0].ac, Some(1.0));
    assert_same_limits("rc_filter", &testbench);
    assert_eq!(
        measure(&testbench, "corner").drop_db,
        None,
        "the 3 dB default is not re-emitted as a guessed value"
    );
    let corner = 1.0 / (2.0 * std::f64::consts::PI * 1e3 * 100e-9);
    assert!(
        ac.start < corner && corner < ac.stop,
        "sweep {}..{} must bracket {corner}",
        ac.start,
        ac.stop
    );
    assert!(
        not_covered.is_empty(),
        "nothing is left out: {not_covered:?}"
    );
    assert!(toml.contains("1/(2*pi*R*C)"), "{toml}");
}

#[test]
fn a_part_without_a_datasheet_gives_no_measure_and_says_why() {
    let board = board(
        r#"
board "no-datasheet" {
  layers 2
  component R1: resistor "r_generic_0805" value "150"
  component LED1: led "led_red_0603"
  power "VCC" 5v
  net "VCC" { R1.p1 }
  net "GND" { LED1.cathode }
  connect R1.p2 -> LED1.anode as "MID"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("LED1")
                && line.contains("led_red_0603")
                && line.contains("no datasheet")),
        "{:?}",
        suggestion.not_covered
    );
}

#[test]
fn a_missing_supply_writes_a_todo_instead_of_a_guess() {
    let board = board(
        r#"
board "mixed-divider" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  component R4: resistor "r_generic_0805" value "10k"
  power "VCC" 12v
  net "VCC" { R1.p1 }
  net "GND" { R2.p2, R4.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  net "VCC2" { R3.p1 }
  connect R3.p2 -> R4.p1 as "MID2"
}
"#,
    );

    let (toml, not_covered) = suggestion(&board);

    assert!(toml.contains("# TODO"), "{toml}");
    assert!(toml.contains("VCC2"), "{toml}");
    assert!(
        not_covered
            .iter()
            .any(|line| line.contains("VCC2") && line.contains("no declared supply")),
        "{not_covered:?}"
    );
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("still parses");
    assert_eq!(testbench.measure.len(), 1, "{toml}");
    measure(&testbench, "vmid");
}

#[test]
fn a_board_with_no_shape_suggests_nothing() {
    let board = board(
        r#"
board "no-shape" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  power "VCC" 12v
  net "VCC" { R1.p1 }
  net "GND" { R1.p2 }
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("R1") && line.contains("recognised shape")),
        "{:?}",
        suggestion.not_covered
    );
}

/// A board name reaches the generated header as a comment, so it must not be
/// able to smuggle a new TOML line into the file.
#[test]
fn a_board_name_cannot_inject_toml() {
    let mut board = board_from_file(&example_dir("divider").join("divider.synth"));
    board.name = "evil\n[[measure]]\nname = \"x\"".to_string();

    let (toml, not_covered) = suggestion(&board);

    let testbench = synth_sim::Testbench::from_toml(&toml).expect("sanitized header parses");
    assert_eq!(testbench.measure.len(), 1);
    assert_eq!(testbench.measure[0].name, "vmid");
    assert!(not_covered.is_empty(), "{not_covered:?}");
}

/// The loader rejects a carriage return in a net name, so the draft must not
/// emit one either.
#[test]
fn a_carriage_return_in_a_net_name_is_not_drafted() {
    let mut board = board_from_file(&example_dir("divider").join("divider.synth"));
    let mid = board
        .nets
        .iter_mut()
        .find(|net| net.name == "MID")
        .expect("MID");
    mid.name = "MI\rD".to_string();

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("newline")),
        "{:?}",
        suggestion.not_covered
    );
}

/// No measure means no file, even when the shape itself was recognised; the
/// stderr line is the only place the missing supply can be reported.
#[test]
fn a_single_shape_without_a_supply_writes_nothing() {
    let source = std::fs::read_to_string(example_dir("led").join("led.synth")).expect("read led");
    let source = source.replace("  power \"VCC\" 5v\n", "");
    assert!(!source.contains("power"), "{source}");
    let board = board(&source);

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("no declared supply")),
        "{:?}",
        suggestion.not_covered
    );
}

const LDO_DIVIDER: &str = r#"
board "ldo-divider" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  net "VIN" { U1.vin }
  net "GND" { U1.gnd, R2.p2 }
  net "VOUT" { U1.vout, R1.p1 }
  connect R1.p2 -> R2.p1 as "MID"
}
"#;

#[test]
fn a_supply_may_come_from_a_power_output_pin() {
    let board = board(LDO_DIVIDER);

    let (toml, not_covered) = suggestion(&board);

    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar parses");
    assert_eq!(testbench.source[0].net.as_deref(), Some("VOUT"));
    assert_eq!(testbench.source[0].dc, Some(3.3));
    assert!(toml.contains("power output that declares 3.3 V"), "{toml}");
    assert!(
        not_covered
            .iter()
            .any(|line| line.contains("U1") && line.contains("recognised shape")),
        "{not_covered:?}"
    );
}

/// A `voltage_max_v` rating is not a nominal supply.
#[test]
fn a_max_rating_is_not_a_supply() {
    let board = board_with_registry(LDO_DIVIDER, |registry| {
        let mut part = registry.lookup("ams1117_3v3").expect("ams1117_3v3").clone();
        let vout = part
            .pins
            .iter_mut()
            .find(|pin| pin.name == "vout")
            .expect("vout");
        vout.voltage_nominal_v = None;
        vout.voltage_max_v = Some(5.0);
        registry.insert(part);
    });

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("VOUT") && line.contains("no declared supply")),
        "{:?}",
        suggestion.not_covered
    );
}

const REGULATOR_LED_BOARD: &str = r#"
board "regulator-led" {
  layers 2
  component U1: regulator "ams1117_5v"
  component D1: led "ltst_c190krkt"
  component R1: resistor "r_generic_0603" value "150"
  component C1: capacitor "c_generic_0805" value "10u"
  component C2: capacitor "c_generic_0805" value "10u"
  connect U1.vout -> R1.p2
  connect R1.p1 -> D1.anode
  connect D1.cathode -> U1.gnd
  connect U1.vout -> C1.p1
  connect U1.gnd -> C1.p2
  connect U1.vout -> C2.p1
  connect U1.gnd -> C2.p2
  net "GND" { U1.gnd D1.cathode C1.p2 C2.p2 }
  net "VOUT" { U1.vout R1.p2 C1.p1 C2.p1 }
}
"#;

/// The regulator pin is replaced by the testbench source, so the draft must
/// ignore the part it cannot model; otherwise the suggested file cannot run.
#[test]
fn a_suggested_sidecar_ignores_the_regulator_that_supplied_the_source() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(REGULATOR_LED_BOARD);
    let (toml, _) = suggestion(&board);
    assert!(toml.contains("ignore = [\"U1\"]"), "{toml}");
    assert!(
        toml.contains(
            "# ignore: U1 (ams1117_5v): replaced by the testbench source on net \"VOUT\"."
        ),
        "{toml}"
    );
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let report = simulate_suggested(&board, &testbench, &ngspice);

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
}

/// A connector is not modelled either; the draft must leave it out so the
/// suggested file runs straight away.
#[test]
fn a_suggested_sidecar_ignores_a_connector_with_no_model() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(
        r#"
board "led_indicator" {
  layers 2
  component J1: connector "jst_ph_2pin"
  component D1: led "ltst_c190krkt"
  component R1: resistor "r_generic_0603" value "150"
  connect J1.p1 -> R1.p1
  connect R1.p2 -> D1.anode
  connect D1.cathode -> J1.p2
  power "VCC" 5v
  net "VCC" { J1.p1 R1.p1 }
  net "GND" { J1.p2 D1.cathode }
}
"#,
    );
    let (toml, _) = suggestion(&board);
    assert!(toml.contains("ignore = [\"J1\"]"), "{toml}");
    assert!(
        toml.contains("# ignore: J1 (jst_ph_2pin): no built-in SPICE model."),
        "{toml}"
    );
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let report = simulate_suggested(&board, &testbench, &ngspice);

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
}

/// A value typo is not a missing model: ignoring the part would hide the
/// broken value, so `--suggest` must leave it in the deck and `sim` must fail
/// `-002`, naming the part and the value.
#[test]
fn a_capacitor_value_typo_is_not_ignored_and_fails_002() {
    let board = board(
        r#"
board "bad-cap" {
  layers 2
  component R1: resistor "r_generic_0805" value "330"
  component LED1: led "ltst_c190krkt"
  component C1: capacitor "c_generic_0805" value "10uu"
  power "VCC" 5v
  net "VCC" { R1.p1, C1.p1 }
  net "GND" { LED1.cathode, C1.p2 }
  connect R1.p2 -> LED1.anode as "MID"
}
"#,
    );
    let (toml, _) = suggestion(&board);
    assert!(!toml.contains("ignore ="), "{toml}");
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let report = synth_sim::simulate(
        &board,
        &testbench,
        &synth_sim::SimOptions {
            ngspice: Some(PathBuf::from("/nonexistent/ngspice")),
            ..synth_sim::SimOptions::default()
        },
    );

    assert!(!report.passed(), "{:?}", report.measures);
    let message = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_002)
        .and_then(|diagnostic| diagnostic.message.as_deref())
        .unwrap_or_else(|| panic!("no -002 in {:?}", report.diagnostics));
    assert!(
        message.contains("C1") && message.contains("10uu"),
        "{message}"
    );
    assert!(!message.contains("ignore"), "{message}");
}

const REGULATOR_INPUT_CAP_BOARD: &str = r#"
board "regulator-input-cap" {
  layers 2
  component U1: regulator "ams1117_5v"
  component C1: capacitor "c_generic_0805" value "10u"
  component R1: resistor "r_generic_0805" value "150"
  component D1: led "ltst_c190krkt"
  power "VIN" 12v
  net "VIN" { U1.vin, C1.p1 }
  net "GND" { U1.gnd, C1.p2, D1.cathode }
  connect U1.vout -> R1.p1 as "VOUT"
  connect R1.p2 -> D1.anode as "MID"
}
"#;

/// The regulator is replaced by the testbench source, which leaves its input
/// cap alone on a dead `VIN` node. The draft must ignore that cap too, with a
/// reason, so the suggested file runs straight away.
#[test]
fn a_suggested_sidecar_ignores_an_input_cap_on_the_ignored_regulator_rail() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(REGULATOR_INPUT_CAP_BOARD);
    let (toml, _) = suggestion(&board);
    assert!(toml.contains("ignore = [\"C1\", \"U1\"]"), "{toml}");
    assert!(
        toml.contains(
            "# ignore: C1 (c_generic_0805): every net it connects to is driven only by ignored parts."
        ),
        "{toml}"
    );
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let report = simulate_suggested(&board, &testbench, &ngspice);

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
}

/// The same dead-rail cap appears when a connector is the ignored part.
#[test]
fn a_suggested_sidecar_ignores_an_input_cap_next_to_an_ignored_connector() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(
        r#"
board "connector-input-cap" {
  layers 2
  component J1: connector "jst_ph_2pin"
  component C1: capacitor "c_generic_0805" value "10u"
  component U1: regulator "ams1117_5v"
  component R1: resistor "r_generic_0805" value "150"
  component D1: led "ltst_c190krkt"
  power "VIN" 12v
  net "VIN" { J1.p1, C1.p1, U1.vin }
  net "GND" { J1.p2, C1.p2, U1.gnd, D1.cathode }
  connect U1.vout -> R1.p1 as "VOUT"
  connect R1.p2 -> D1.anode as "MID"
}
"#,
    );
    let (toml, _) = suggestion(&board);
    assert!(toml.contains("ignore = [\"C1\", \"J1\", \"U1\"]"), "{toml}");
    assert!(
        toml.contains(
            "# ignore: C1 (c_generic_0805): every net it connects to is driven only by ignored parts."
        ),
        "{toml}"
    );
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let report = simulate_suggested(&board, &testbench, &ngspice);

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
}

/// Two caps on one dead rail keep each other "live" in a one-part-at-a-time
/// sweep. Treat the whole net as dead, or the suggested sidecar fails `-008`.
#[test]
fn a_suggested_sidecar_ignores_two_caps_together_on_a_dead_rail() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(
        r#"
board "two-input-caps" {
  layers 2
  component U1: regulator "ams1117_5v"
  component C1: capacitor "c_generic_0805" value "10u"
  component C2: capacitor "c_generic_0603" value "100n"
  component R1: resistor "r_generic_0805" value "150"
  component D1: led "ltst_c190krkt"
  power "VIN" 12v
  net "VIN" { U1.vin, C1.p1, C2.p1 }
  net "GND" { U1.gnd, C1.p2, C2.p2, D1.cathode }
  connect U1.vout -> R1.p1 as "VOUT"
  connect R1.p2 -> D1.anode as "MID"
}
"#,
    );
    let (toml, _) = suggestion(&board);
    assert!(toml.contains("ignore = [\"C1\", \"C2\", \"U1\"]"), "{toml}");
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let report = simulate_suggested(&board, &testbench, &ngspice);

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
}

/// A bad value is a hard `-002`, even on a rail the dead-rail sweep would
/// otherwise drop. Ignoring it would hide the typo behind a PASS.
#[test]
fn a_bad_value_on_a_dead_rail_is_not_ignored_and_fails_002() {
    let board = board(
        r#"
board "dead-rail-bad-cap" {
  layers 2
  component U1: regulator "ams1117_5v"
  component C1: capacitor "c_generic_0805" value "10uu"
  component R1: resistor "r_generic_0805" value "150"
  component D1: led "ltst_c190krkt"
  power "VIN" 12v
  net "VIN" { U1.vin, C1.p1 }
  net "GND" { U1.gnd, C1.p2, D1.cathode }
  connect U1.vout -> R1.p1 as "VOUT"
  connect R1.p2 -> D1.anode as "MID"
}
"#,
    );
    let (toml, _) = suggestion(&board);
    assert!(!toml.contains("\"C1\""), "{toml}");
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let report = synth_sim::simulate(
        &board,
        &testbench,
        &synth_sim::SimOptions {
            ngspice: Some(PathBuf::from("/nonexistent/ngspice")),
            ..synth_sim::SimOptions::default()
        },
    );

    assert!(!report.passed(), "{:?}", report.measures);
    let message = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_002)
        .and_then(|diagnostic| diagnostic.message.as_deref())
        .unwrap_or_else(|| panic!("no -002 in {:?}", report.diagnostics));
    assert!(
        message.contains("C1") && message.contains("10uu"),
        "{message}"
    );
}

/// A modelled LDO stays in the deck when its output rail is declared, so its
/// input rail must be driven too; otherwise ngspice sees a singular n_vin.
#[test]
fn a_suggested_sidecar_sources_a_modelled_regulators_input_rail() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(
        r#"
board "ldo-modelled" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component C1: capacitor "c_generic_0805" value "10u"
  component R1: resistor "r_generic_0805" value "150"
  component D1: led "ltst_c190krkt"
  power "VIN" 12v
  power "VOUT" 3.3v
  net "VIN" { U1.vin, C1.p1 }
  net "GND" { U1.gnd, C1.p2, D1.cathode }
  net "VOUT" { U1.vout, R1.p1 }
  connect R1.p2 -> D1.anode as "MID"
}
"#,
    );
    let (toml, _) = suggestion(&board);
    assert!(toml.contains("net = \"VIN\""), "{toml}");
    assert!(toml.contains("dc = 12.0"), "{toml}");
    assert!(!toml.contains("\"U1\""), "{toml}");
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let report = simulate_suggested(&board, &testbench, &ngspice);

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
}

/// A feedback divider's mid net also drives the regulator's `fb` pin. That is
/// the canonical shape; it must be named, not silently skipped.
#[test]
fn a_feedback_divider_names_the_pin_that_also_drives_its_mid_net() {
    let board = board(
        r#"
board "feedback-divider" {
  layers 2
  component U1: regulator "mp2307_buck"
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  net "VIN" { U1.in }
  net "GND" { U1.gnd, R2.p2 }
  net "VOUT" { R1.p1 }
  connect R1.p2 -> R2.p1 as "MID"
  connect R1.p2 -> U1.fb as "MID"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    let line = suggestion
        .not_covered
        .iter()
        .find(|line| line.contains("divider"))
        .unwrap_or_else(|| panic!("no divider line in {:?}", suggestion.not_covered));
    assert!(line.contains("divider R1/R2"), "{line}");
    assert!(line.contains("mid net also drives U1.fb"), "{line}");
    assert!(
        !suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("R1") && line.contains("not a recognised shape")),
        "{:?}",
        suggestion.not_covered
    );
}

/// Two parallel resistors feeding an LED are not a divider: without a leg to
/// ground there is no divider message to print.
#[test]
fn parallel_resistors_feeding_an_led_are_not_called_a_divider() {
    let board = board(
        r#"
board "parallel-feed" {
  layers 2
  component R1: resistor "r_generic_0805" value "150"
  component R2: resistor "r_generic_0805" value "150"
  component D1: led "ltst_c190krkt"
  power "VCC" 5v
  net "VCC" { R1.p1, R2.p1 }
  net "GND" { D1.cathode }
  connect R1.p2 -> R2.p2 as "MID"
  connect R1.p2 -> D1.anode as "MID"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        !suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("divider")),
        "{:?}",
        suggestion.not_covered
    );
}

/// A passive on a live rail is part of the simulated circuit; it must never
/// be dropped just because some other part on that rail is ignored.
#[test]
fn a_passive_on_a_live_measured_net_is_never_ignored() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(
        r#"
board "divider-with-decoupling" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component C1: capacitor "c_generic_0805" value "10u"
  power "VCC" 12v
  net "VCC" { R1.p1, C1.p1 }
  net "GND" { R2.p2, C1.p2 }
  connect R1.p2 -> R2.p1 as "MID"
}
"#,
    );
    let (toml, _) = suggestion(&board);
    assert!(!toml.contains("ignore ="), "{toml}");
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let report = simulate_suggested(&board, &testbench, &ngspice);

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
}

fn simulate_suggested(
    board: &synth_ir::Board,
    testbench: &synth_sim::Testbench,
    ngspice: &Path,
) -> synth_sim::SimReport {
    synth_sim::simulate(
        board,
        testbench,
        &synth_sim::SimOptions {
            ngspice: Some(ngspice.to_path_buf()),
            design_path: Some(PathBuf::from("suggested.synth")),
            testbench_path: Some(PathBuf::from("suggested.sim.toml")),
            registry_dirs: vec![registry_parts_dir()],
        },
    )
}

fn assert_out_of_limit(report: &synth_sim::SimReport, name: &str, direction: &str) {
    assert!(!report.passed(), "report passed: {:?}", report.measures);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_004)
        .unwrap_or_else(|| panic!("no -004 in {:?}", report.diagnostics));
    let message = diagnostic.message.as_deref().unwrap_or_default();
    assert!(
        message.contains(name) && message.contains(direction),
        "expected {name} {direction}, got: {message}"
    );
}

const LED_BOARD: &str = r#"
board "led-board" {
  layers 2
  component R1: resistor "r_generic_0805" value "330"
  component LED1: led "ltst_c190krkt"
  power "VCC" 5v
  net "VCC" { R1.p1 }
  net "GND" { LED1.cathode }
  connect R1.p2 -> LED1.anode as "MID"
}
"#;

/// A GPIO drives the LED through its series resistor; there is no `power`
/// declaration on the driven net.
const GPIO_LED_BOARD: &str = r#"
board "gpio-led" {
  layers 2
  component U1: mcu "rp2350"
  component R1: resistor "r_generic_0805" value "330"
  component LED1: led "ltst_c190krkt"
  net "VCC" { U1.gpio5, R1.p1 }
  net "GND" { U1.gnd, LED1.cathode }
  connect R1.p2 -> LED1.anode as "MID"
}
"#;

fn fixture_led(registry: &mut synth_registry::Registry) {
    let mut led = registry
        .lookup("ltst_c190krkt")
        .expect("ltst_c190krkt")
        .clone();
    let conditions = led
        .operating_conditions
        .as_mut()
        .expect("operating conditions");
    conditions.forward_voltage_v = Some(2.9);
    conditions.forward_current_ma = Some(5.0);
    registry.insert(led);
}

fn fixture_drive() -> synth_registry::PinDrive {
    synth_registry::PinDrive {
        high_v: 3.3,
        low_v: 0.0,
        max_source_ma: 4.0,
        max_sink_ma: 4.0,
        output_resistance_ohm: Some(25.0),
        datasheet_ref: Some("Table 5-1".to_string()),
    }
}

fn fixture_mcu(registry: &mut synth_registry::Registry, drive: Option<synth_registry::PinDrive>) {
    let mut mcu = registry.lookup("rp2350").expect("rp2350").clone();
    let pin = mcu
        .pins
        .iter_mut()
        .find(|pin| pin.name == "gp5")
        .expect("gp5");
    pin.name = "gpio5".to_string();
    pin.drive = drive;
    registry.insert(mcu);
}

fn gpio_led_board(drive: Option<synth_registry::PinDrive>) -> synth_ir::Board {
    board_with_registry(GPIO_LED_BOARD, |registry| {
        fixture_led(registry);
        fixture_mcu(registry, drive);
    })
}

fn led_board_with(volts: &str, resistor: &str) -> synth_ir::Board {
    board(&format!(
        r#"
board "led-board" {{
  layers 2
  component R1: resistor "r_generic_0805" value "{resistor}"
  component LED1: led "ltst_c190krkt"
  power "VCC" {volts}
  net "VCC" {{ R1.p1 }}
  net "GND" {{ LED1.cathode }}
  connect R1.p2 -> LED1.anode as "MID"
}}
"#
    ))
}

/// The band comes from the design resistor and the registry forward voltage,
/// so a board that is not the reference 150 ohm one still passes.
#[test]
fn a_suggested_led_testbench_passes_a_330_ohm_5v_board() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(LED_BOARD);
    let (toml, not_covered) = suggestion(&board);
    assert!(not_covered.is_empty(), "{not_covered:?}");
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let report = simulate_suggested(&board, &testbench, &ngspice);

    assert!(report.passed(), "{:?}", report.measures);
}

/// A GPIO with drive data becomes a `pin` source at its high level, and the
/// draft adds the pin's current limit as a second measure.
#[test]
fn a_gpio_driven_led_drafts_a_pin_source_and_pin_current_check() {
    let board = gpio_led_board(Some(fixture_drive()));

    let (toml, not_covered) = suggestion(&board);

    assert!(toml.contains("pin = \"U1.gpio5\""), "{toml}");
    assert!(toml.contains("level = \"high\""), "{toml}");
    assert!(toml.contains("U1.gpio5"), "{toml}");
    assert!(toml.contains("high_v"), "{toml}");
    assert!(toml.contains("max_source_ma"), "{toml}");
    assert!(toml.contains("output_resistance_ohm"), "{toml}");
    assert!(toml.contains("Table 5-1"), "{toml}");
    assert!(toml.contains("kind = \"pin_current\""), "{toml}");
    assert!(toml.contains("ignore = [\"U1\"]"), "{toml}");
    assert!(
        !toml.contains("net = \"VCC\"\nkind"),
        "a pin source must not also emit a net source:\n{toml}"
    );

    let testbench = synth_sim::Testbench::from_toml(&toml).expect("generated sidecar parses");
    let source = &testbench.source[0];
    assert_eq!(source.pin.as_deref(), Some("U1.gpio5"));
    assert_eq!(source.level, Some(synth_sim::SourceLevel::High));
    assert_eq!(source.net, None);
    assert_eq!(source.dc, None);
    let pin_current = measure(&testbench, "pin_current");
    assert_eq!(pin_current.kind, synth_sim::MeasureKind::PinCurrent);
    assert_eq!(pin_current.pin.as_deref(), Some("U1.gpio5"));
    assert_eq!(pin_current.min, None);
    assert_eq!(pin_current.max, None);
    assert!(
        not_covered.iter().any(|line| line.contains("U1")),
        "{not_covered:?}"
    );

    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let report = simulate_suggested(&board, &testbench, &ngspice);
    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let pin_current = report
        .measures
        .iter()
        .find(|measure| measure.name == "pin_current")
        .expect("pin_current result");
    assert_eq!(pin_current.max, Some(4.0));
    assert!(
        (0.8..1.8).contains(&pin_current.value),
        "drafted pin current {} should be about 1.3 mA",
        pin_current.value
    );
}

/// 33 ohms draws about 6.6 mA from the same pin, so the drafted testbench
/// fails on its own pin limit.
#[test]
fn a_gpio_driven_led_with_33_ohms_fails_its_pin_current_check() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board_with_registry(
        &GPIO_LED_BOARD.replace("value \"330\"", "value \"33\""),
        |registry| {
            fixture_led(registry);
            fixture_mcu(registry, Some(fixture_drive()));
        },
    );
    let (toml, _) = suggestion(&board);
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("generated sidecar parses");

    let report = simulate_suggested(&board, &testbench, &ngspice);

    assert!(!report.passed(), "{:?}", report.measures);
    let message = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_004
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("pin limit"))
        })
        .and_then(|diagnostic| diagnostic.message.as_deref())
        .unwrap_or_default();
    assert!(message.contains("U1.gpio5"), "{message}");
    assert!(message.contains("max_source_ma"), "{message}");
    assert!(message.contains("Table 5-1"), "{message}");
    assert!(message.contains("above"), "{message}");
}

/// Two LEDs sharing one GPIO net get one `pin_current` check, not two.
#[test]
fn one_pin_supplies_two_leds_but_only_one_pin_current_check() {
    let board = board_with_registry(
        r#"
board "two-led-gpio" {
  layers 2
  component U1: mcu "rp2350"
  component R1: resistor "r_generic_0805" value "330"
  component R2: resistor "r_generic_0805" value "330"
  component LED1: led "ltst_c190krkt"
  component LED2: led "ltst_c190krkt"
  net "VCC" { U1.gpio5, R1.p1, R2.p1 }
  net "GND" { U1.gnd, LED1.cathode, LED2.cathode }
  connect R1.p2 -> LED1.anode as "MID1"
  connect R2.p2 -> LED2.anode as "MID2"
}
"#,
        |registry| {
            fixture_led(registry);
            fixture_mcu(registry, Some(fixture_drive()));
        },
    );

    let (toml, _) = suggestion(&board);
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("generated sidecar parses");

    assert_eq!(
        testbench
            .measure
            .iter()
            .filter(|measure| measure.kind == synth_sim::MeasureKind::PinCurrent)
            .count(),
        1,
        "{toml}"
    );
}

#[test]
fn a_pin_supply_without_drive_data_is_reported_not_defaulted() {
    let board = gpio_led_board(None);

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    let line = suggestion
        .not_covered
        .iter()
        .find(|line| line.contains("pin has no drive data"))
        .unwrap_or_else(|| panic!("no drive-data line in {:?}", suggestion.not_covered));
    assert!(line.contains("U1.gpio5"), "{line}");
}

#[test]
fn a_pin_supply_without_a_datasheet_ref_is_reported_not_defaulted() {
    let mut drive = fixture_drive();
    drive.datasheet_ref = None;
    let board = gpio_led_board(Some(drive));

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    let line = suggestion
        .not_covered
        .iter()
        .find(|line| line.contains("pin has no drive data"))
        .unwrap_or_else(|| panic!("no drive-data line in {:?}", suggestion.not_covered));
    assert!(
        line.contains("U1.gpio5") && line.contains("datasheet_ref"),
        "{line}"
    );
}

/// Two driven pins on the supply net are ambiguous; the draft names both and
/// never guesses one.
#[test]
fn two_driven_pins_on_the_supply_net_are_reported_as_ambiguous() {
    let board = board_with_registry(
        &GPIO_LED_BOARD.replace(
            "net \"VCC\" { U1.gpio5, R1.p1 }",
            "net \"VCC\" { U1.gpio5, U1.gpio6, R1.p1 }",
        ),
        |registry| {
            fixture_led(registry);
            let mut mcu = registry.lookup("rp2350").expect("rp2350").clone();
            for (from, to) in [("gp5", "gpio5"), ("gp6", "gpio6")] {
                let pin = mcu
                    .pins
                    .iter_mut()
                    .find(|pin| pin.name == from)
                    .expect(from);
                pin.name = to.to_string();
                pin.drive = Some(fixture_drive());
            }
            registry.insert(mcu);
        },
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    let line = suggestion
        .not_covered
        .iter()
        .find(|line| line.contains("ambiguous"))
        .unwrap_or_else(|| panic!("no ambiguity line in {:?}", suggestion.not_covered));
    assert!(
        line.contains("U1.gpio5") && line.contains("U1.gpio6"),
        "{line}"
    );
}

/// Two pull-downs on a regulator output are not a divider, so they must not
/// be reported as "divider R1/R2".
#[test]
fn two_pull_downs_on_a_regulator_output_are_not_a_divider() {
    let board = board(
        r#"
board "two-pull-downs" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  net "VIN" { U1.vin }
  net "GND" { U1.gnd, R1.p2, R2.p2 }
  net "VOUT" { U1.vout, R1.p1, R2.p1 }
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(
        !suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("divider")),
        "{:?}",
        suggestion.not_covered
    );
}

#[test]
fn a_suggested_led_testbench_passes_a_1k_12v_board() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let source = LED_BOARD
        .replace("value \"330\"", "value \"1k\"")
        .replace("power \"VCC\" 5v", "power \"VCC\" 12v");
    assert_ne!(source, LED_BOARD);
    let board = board(&source);
    let (toml, not_covered) = suggestion(&board);
    assert!(not_covered.is_empty(), "{not_covered:?}");
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let report = simulate_suggested(&board, &testbench, &ngspice);

    assert!(report.passed(), "{:?}", report.measures);
}

#[test]
fn a_suggested_led_testbench_passes_every_good_board_on_real_ngspice() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    // The registry Vf is the value at `forward_current_ma`; the model's Vf is
    // lower at low current, so the band must come from the diode equation.
    for (volts, resistor) in [
        ("5v", "3.3k"),
        ("5v", "10k"),
        ("3.3v", "1k"),
        ("2.5v", "1k"),
        ("5v", "330"),
        ("12v", "1k"),
    ] {
        let board = led_board_with(volts, resistor);
        let (toml, not_covered) = suggestion(&board);
        assert!(
            not_covered.is_empty(),
            "{volts}/{resistor}: {not_covered:?}"
        );
        let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

        let report = simulate_suggested(&board, &testbench, &ngspice);

        assert!(
            report.passed(),
            "{volts}/{resistor} should pass: {:?}",
            report.measures
        );
    }
}

#[test]
fn an_overdriven_led_drafts_a_band_that_fails_and_says_overdrive() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let source = LED_BOARD
        .replace("value \"330\"", "value \"10\"")
        .replace("power \"VCC\" 5v", "power \"VCC\" 24v");
    assert_ne!(source, LED_BOARD);
    let board = board(&source);
    let suggestion = synth_sim::suggest_testbench(&board);
    let toml = suggestion.toml.expect("a testbench was suggested");
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");
    let iled = measure(&testbench, "iled");
    assert_eq!(iled.min, Some(0.025), "{toml}");
    assert_eq!(iled.max, Some(0.025), "{toml}");
    // The comment must still cite the real ±5% band, not the pinned limit.
    assert!(toml.contains("2.07 A..2.28 A"), "{toml}");
    assert!(
        suggestion
            .warnings
            .iter()
            .any(|line| line.contains("overdrive") && line.contains("LED1")),
        "{:?}",
        suggestion.warnings
    );

    let report = simulate_suggested(&board, &testbench, &ngspice);

    assert_out_of_limit(&report, "iled", "above max");
}

#[test]
fn a_missing_forward_voltage_gives_no_measure_and_says_why() {
    let board = board_with_registry(LED_BOARD, |registry| {
        let mut part = registry
            .lookup("ltst_c190krkt")
            .expect("ltst_c190krkt")
            .clone();
        part.operating_conditions
            .as_mut()
            .expect("operating conditions")
            .forward_voltage_v = None;
        registry.insert(part);
    });

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("LED1")
                && line.contains("ltst_c190krkt")
                && line.contains("forward_voltage_v")),
        "{:?}",
        suggestion.not_covered
    );
}

/// The zero-width overdrive band is a tripwire for the current resistor; after
/// fixing it, the sidecar must be drafted again.
#[test]
fn an_overdrive_draft_says_to_regenerate_after_fixing_the_resistor() {
    let source = LED_BOARD
        .replace("value \"330\"", "value \"10\"")
        .replace("power \"VCC\" 5v", "power \"VCC\" 24v");
    assert_ne!(source, LED_BOARD);
    let board = board(&source);

    let suggestion = synth_sim::suggest_testbench(&board);

    let toml = suggestion.toml.expect("a testbench was suggested");
    assert!(
        toml.contains("# tripwire: regenerate with --suggest --force after fixing the resistor"),
        "{toml}"
    );
}

/// The divider and RC limits are computed from the same values they check, so
/// the generated comment says what the pass actually proves.
#[test]
fn the_divider_and_rc_drafts_say_the_limits_come_from_the_design() {
    const NOTE: &str =
        "#   derived from this design's values; checks wiring and model, not the value choice.";

    let divider = board_from_file(&example_dir("divider").join("divider.synth"));
    let (toml, _) = suggestion(&divider);
    assert!(toml.contains(NOTE), "{toml}");

    let rc = board_from_file(&example_dir("rc_filter").join("rc_filter.synth"));
    let (toml, _) = suggestion(&rc);
    assert!(toml.contains(NOTE), "{toml}");
}

/// The picked supply net may be an internal node of a longer chain; it is not
/// a missing `power` declaration, so no TODO belongs on it.
#[test]
fn a_three_resistor_chain_writes_no_todo_for_its_internal_node() {
    let board = board(
        r#"
board "chain" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  power "VCC" 12v
  net "VCC" { R1.p1 }
  net "GND" { R3.p2 }
  connect R1.p2 -> R2.p1 as "MID1"
  connect R2.p2 -> R3.p1 as "MID2"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        !suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("no declared supply")),
        "{:?}",
        suggestion.not_covered
    );
    assert!(
        suggestion.not_covered.iter().any(|line| line.contains("R2")
            && line.contains("R3")
            && line.contains("MID1")
            && line.contains("internal node")),
        "{:?}",
        suggestion.not_covered
    );
}

/// A connector is not a declared supply: the divider fed from it must be
/// reported, never dropped silently.
#[test]
fn a_divider_fed_from_a_connector_is_reported_not_dropped_silently() {
    let board = board(
        r#"
board "connector-divider" {
  layers 2
  component J1: connector "jst_xh_2pin"
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  net "VIN" { J1.p1, R1.p1 }
  net "GND" { J1.p2, R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("VIN")
                && line.contains("R1")
                && line.contains("R2")
                && line.contains("no declared supply")),
        "{:?}",
        suggestion.not_covered
    );
}

/// Without `max_current_ma` the datasheet forward current is the cap, so an
/// overdriven board still fails its own draft.
#[test]
fn without_max_current_the_forward_current_caps_the_band() {
    let source = LED_BOARD
        .replace("value \"330\"", "value \"10\"")
        .replace("power \"VCC\" 5v", "power \"VCC\" 24v");
    let board = board_with_registry(&source, |registry| {
        let mut part = registry
            .lookup("ltst_c190krkt")
            .expect("ltst_c190krkt")
            .clone();
        part.operating_conditions
            .as_mut()
            .expect("operating conditions")
            .max_current_ma = None;
        registry.insert(part);
    });

    let suggestion = synth_sim::suggest_testbench(&board);
    let toml = suggestion.toml.expect("a testbench was suggested");
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");
    let iled = measure(&testbench, "iled");
    assert_eq!(iled.min, Some(0.02), "{toml}");
    assert_eq!(iled.max, Some(0.02), "{toml}");
    assert!(
        suggestion
            .warnings
            .iter()
            .any(|line| line.contains("overdrive") && line.contains("LED1")),
        "{:?}",
        suggestion.warnings
    );
}

#[test]
fn without_a_forward_current_the_led_is_not_covered() {
    let board = board_with_registry(LED_BOARD, |registry| {
        let mut part = registry
            .lookup("ltst_c190krkt")
            .expect("ltst_c190krkt")
            .clone();
        let conditions = part
            .operating_conditions
            .as_mut()
            .expect("operating conditions");
        conditions.forward_current_ma = None;
        conditions.max_current_ma = None;
        registry.insert(part);
    });

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("LED1") && line.contains("forward_current_ma")),
        "{:?}",
        suggestion.not_covered
    );
}

/// Two LEDs sharing a resistor split its current; a single-LED band would
/// fail a valid board, so neither is drafted.
#[test]
fn two_leds_sharing_a_resistor_are_not_covered() {
    let board = board(
        r#"
board "shared-resistor" {
  layers 2
  component R1: resistor "r_generic_0805" value "150"
  component LED1: led "ltst_c190krkt"
  component LED2: led "ltst_c190krkt"
  power "VCC" 5v
  net "VCC" { R1.p1 }
  net "GND" { LED1.cathode, LED2.cathode }
  connect R1.p2 -> LED1.anode as "MID"
  connect R1.p2 -> LED2.anode as "MID"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    for led in ["LED1", "LED2"] {
        assert!(
            suggestion.not_covered.iter().any(|line| line.contains(led)),
            "{led} is not mentioned: {:?}",
            suggestion.not_covered
        );
    }
}

/// A value that parses but is zero or negative is a different problem from an
/// unparseable one, and the message must say which.
#[test]
fn a_zero_or_negative_series_resistor_says_the_value_must_be_positive() {
    for value in ["0", "-10"] {
        let source = LED_BOARD.replace("value \"330\"", &format!("value \"{value}\""));
        assert_ne!(source, LED_BOARD);
        let board = board(&source);

        let suggestion = synth_sim::suggest_testbench(&board);

        assert!(suggestion.toml.is_none(), "{value}: {:?}", suggestion.toml);
        let line = suggestion
            .not_covered
            .iter()
            .find(|line| line.contains("LED series resistor"))
            .unwrap_or_else(|| panic!("{value}: no resistor line in {:?}", suggestion.not_covered));
        assert!(line.contains("value must be positive"), "{line}");
        assert!(!line.contains("no parseable"), "{line}");
    }
}

/// The ticket rule is "no `reviewed_by` means UNREVIEWED", so a part with no
/// `[provenance]` block at all is marked too.
#[test]
fn parts_without_a_provenance_block_are_marked_unreviewed() {
    let board = board_from_file(&example_dir("divider").join("divider.synth"));
    let (toml, _) = suggestion(&board);

    assert!(toml.contains("# UNREVIEWED: R1 (r_generic_0805)"), "{toml}");
    assert!(toml.contains("# UNREVIEWED: R2 (r_generic_0805)"), "{toml}");
}

/// The datasheet gap is the more specific reason; it wins over the missing
/// supply in the report.
#[test]
fn an_led_without_a_datasheet_reports_that_before_the_missing_supply() {
    let board = board(
        r#"
board "no-datasheet-no-supply" {
  layers 2
  component R1: resistor "r_generic_0805" value "150"
  component LED1: led "led_red_0603"
  net "VCC" { R1.p1 }
  net "GND" { LED1.cathode }
  connect R1.p2 -> LED1.anode as "MID"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("no datasheet")),
        "{:?}",
        suggestion.not_covered
    );
    assert!(
        !suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("no declared supply")),
        "{:?}",
        suggestion.not_covered
    );
}

/// The unparseable value belongs to the bottom resistor here; the message must
/// not blame the top one.
#[test]
fn a_divider_with_one_bad_value_names_that_resistor() {
    let board = board(
        r#"
board "bad-value" {
  layers 2
  component R1: resistor "r_generic_0805" value "4.7k"
  component R2: resistor "r_generic_0805" value "2k2"
  power "VCC" 12v
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    let line = suggestion
        .not_covered
        .iter()
        .find(|line| line.contains("no parseable"))
        .unwrap_or_else(|| panic!("no parseable line in {:?}", suggestion.not_covered));
    assert!(line.contains("R2 (r_generic_0805)"), "{line}");
    assert!(!line.contains("R1"), "{line}");
}

/// Comment numbers are rounded for display; the limits keep 3 significant
/// digits.
#[test]
fn generated_comments_round_only_the_displayed_numbers() {
    let board = board(
        r#"
board "rounding" {
  layers 2
  component R1: resistor "r_generic_0805" value "4.7k"
  component R2: resistor "r_generic_0805" value "2.2k"
  power "VCC" 3.3v
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
}
"#,
    );

    let (toml, _) = suggestion(&board);

    assert!(toml.contains("= 1.052 V"), "{toml}");
    assert!(!toml.contains("1.0521739130434782"), "{toml}");
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");
    let vmid = measure(&testbench, "vmid");
    assert_eq!(vmid.min, Some(1.0));
    assert_eq!(vmid.max, Some(1.1));
}

/// The suggested limits catch the classic divider mistake: R2 ten times too
/// large moves the midpoint far above its band.
#[test]
fn a_suggested_divider_testbench_fails_the_wrong_r2() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let design = example_dir("divider").join("divider.synth");
    let source = std::fs::read_to_string(&design).expect("read design");
    let (toml, _) = suggestion(&board(&source));
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let mutated = source.replace(
        "component R2: resistor \"r_generic_0805\" value \"10k\"",
        "component R2: resistor \"r_generic_0805\" value \"100k\"",
    );
    assert_ne!(mutated, source, "the fixture must have two 10k resistors");
    let report = simulate_suggested(&board(&mutated), &testbench, &ngspice);

    assert_out_of_limit(&report, "vmid", "above max");
}

const RC_5V: &str = r#"
board "rc-5v" {
  layers 2
  component R1: resistor "r_generic_0805" value "1k"
  component C1: capacitor "c_generic_0805" value "100n"
  power "IN" 5v
  net "IN" { R1.p1 }
  net "GND" { C1.p2 }
  connect R1.p2 -> C1.p1 as "OUT"
}
"#;

/// The passband and stopband checks must read an absolute gain: the draft
/// drives the AC sweep at 1 V even though the board declares a 5 V supply.
/// A single pole is -0.043 dB at fc/10 and -20.04 dB at 10*fc.
#[test]
fn a_suggested_rc_testbench_passes_a_5v_board_and_measures_the_slope() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(RC_5V);
    let (toml, not_covered) = suggestion(&board);
    assert!(not_covered.is_empty(), "{not_covered:?}");
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");
    assert_eq!(testbench.source[0].ac, Some(1.0));

    let report = simulate_suggested(&board, &testbench, &ngspice);
    assert!(report.passed(), "{:?}", report.measures);

    let fc = 1.0 / (2.0 * std::f64::consts::PI * 1e3 * 100e-9);
    for name in ["passband", "stopband"] {
        let frequency = measure(&testbench, name)
            .at_frequency
            .expect("an at_frequency measure");
        let expected: f64 = -10.0 * (1.0 + (frequency / fc).powi(2)).log10();
        let measured = report
            .measures
            .iter()
            .find(|measure| measure.name == name)
            .unwrap_or_else(|| panic!("{name} not measured"))
            .value;
        assert!(
            (measured - expected).abs() < 0.01,
            "{name} at {frequency} Hz is {measured} dB, expected {expected} dB"
        );
    }
}

/// Swapping R and C keeps the corner and turns the low-pass into a high-pass;
/// the passband check must catch it.
#[test]
fn a_suggested_rc_testbench_fails_the_swapped_r_and_c() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let design = example_dir("rc_filter").join("rc_filter.synth");
    let source = std::fs::read_to_string(&design).expect("read design");
    let (toml, _) = suggestion(&board(&source));
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let swapped = source
        .replace("net \"IN\" { R1.p1 }", "net \"IN\" { C1.p1 }")
        .replace("net \"GND\" { C1.p2 }", "net \"GND\" { R1.p2 }")
        .replace(
            "connect R1.p2 -> C1.p1 as \"OUT\"",
            "connect C1.p2 -> R1.p1 as \"OUT\"",
        );
    assert_ne!(swapped, source, "the fixture must be the RC low-pass");
    let report = simulate_suggested(&board(&swapped), &testbench, &ngspice);

    assert!(!report.passed(), "{:?}", report.measures);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_004
                && diagnostic.severity.is_blocking()
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("passband"))
        })
        .unwrap_or_else(|| panic!("no passband -004 in {:?}", report.diagnostics));
    assert!(
        diagnostic
            .message
            .as_deref()
            .is_some_and(|message| message.contains("below min")),
        "{diagnostic:?}"
    );
}

/// A three-leg resistor net is divider-like but cannot be drafted; it must be
/// named, not reported as three unclassified parts.
#[test]
fn a_three_leg_divider_names_its_top_net_and_parts() {
    let unpowered = board(
        r#"
board "three-leg" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  net "TOP" { R1.p1 }
  net "GND" { R2.p2, R3.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  connect R1.p2 -> R3.p1 as "MID"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&unpowered);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    let line = suggestion
        .not_covered
        .iter()
        .find(|line| line.contains("divider"))
        .unwrap_or_else(|| panic!("no divider line in {:?}", suggestion.not_covered));
    for needle in ["R1", "R2/R3", "top net", "\"TOP\"", "not a declared supply"] {
        assert!(line.contains(needle), "`{needle}` missing from `{line}`");
    }
    assert!(
        !suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("not a recognised shape")),
        "{:?}",
        suggestion.not_covered
    );

    let powered = board(
        r#"
board "three-leg-powered" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  power "TOP" 12v
  net "TOP" { R1.p1 }
  net "GND" { R2.p2, R3.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  connect R1.p2 -> R3.p1 as "MID"
}
"#,
    );
    let suggestion = synth_sim::suggest_testbench(&powered);
    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    let line = suggestion
        .not_covered
        .iter()
        .find(|line| line.contains("divider"))
        .unwrap_or_else(|| panic!("no divider line in {:?}", suggestion.not_covered));
    assert!(
        line.contains("3 resistor legs") && line.contains("only a two-resistor divider is drafted"),
        "{line}"
    );
}

/// The real blocker for an unpowered three-leg divider is the third leg; the
/// missing supply must not hide it.
#[test]
fn a_three_leg_divider_reports_three_legs_before_the_top_net() {
    let board = board(
        r#"
board "three-leg-unpowered" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  net "TOP" { R1.p1 }
  net "GND" { R2.p2, R3.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  connect R1.p2 -> R3.p1 as "MID"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    let line = suggestion
        .not_covered
        .iter()
        .find(|line| line.contains("divider"))
        .unwrap_or_else(|| panic!("no divider line in {:?}", suggestion.not_covered));
    assert!(
        line.contains("3 resistor legs") && line.contains("top net"),
        "{line}"
    );
}

/// A three-resistor star with no ground leg is not a divider; it must keep the
/// plain "not a recognised shape" report rather than invent a divider shape.
#[test]
fn a_three_resistor_star_without_a_ground_leg_is_not_called_a_divider() {
    let board = board(
        r#"
board "star" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  component R4: resistor "r_generic_0805" value "10k"
  net "A" { R1.p1 }
  net "B" { R2.p1 }
  net "C" { R3.p1 }
  net "X" { R4.p1 }
  net "GND" { R4.p2 }
  connect R1.p2 -> R2.p2 as "MID"
  connect R1.p2 -> R3.p2 as "MID"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    assert!(
        !suggestion
            .not_covered
            .iter()
            .any(|line| line.contains("divider")),
        "{:?}",
        suggestion.not_covered
    );
}

/// A ten-times smaller series resistor overdrives the LED.
#[test]
fn a_suggested_led_testbench_fails_the_wrong_resistor() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let design = example_dir("led").join("led.synth");
    let source = std::fs::read_to_string(&design).expect("read design");
    let (toml, _) = suggestion(&board(&source));
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let mutated = source.replace("value \"150\"", "value \"15\"");
    assert_ne!(
        mutated, source,
        "the fixture must have the 150 ohm resistor"
    );
    let report = simulate_suggested(&board(&mutated), &testbench, &ngspice);

    assert_out_of_limit(&report, "iled", "above max");
}

/// A three-significant-digit limit must not leak binary noise into the TOML.
#[test]
fn a_drafted_limit_has_no_binary_noise() {
    let board = board(
        r#"
board "rc-noise" {
  layers 2
  component R1: resistor "r_generic_0805" value "100"
  component C1: capacitor "c_generic_0805" value "100p"
  power "IN" 5v
  net "IN" { R1.p1 }
  net "GND" { C1.p2 }
  connect R1.p2 -> C1.p1 as "OUT"
}
"#,
    );

    let (toml, _) = suggestion(&board);

    assert!(toml.contains("min = 15600000.0"), "{toml}");
    assert!(toml.contains("max = 16200000.0"), "{toml}");
    assert!(!toml.contains("15599999.999999998"), "{toml}");
}

/// The limits are computed from the full supply value; the source must carry
/// that same value or the simulation is off by the rounding.
#[test]
fn a_drafted_source_keeps_the_declared_supply_value() {
    let board = board(
        r#"
board "precise-supply" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  power "VCC" 1.23456v
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
}
"#,
    );

    let (toml, _) = suggestion(&board);

    assert!(toml.contains("dc = 1.23456"), "{toml}");
    assert!(!toml.contains("dc = 1.235"), "{toml}");
}

/// Synth's mega is `M`; SPICE's `meg` is not accepted in a design value.
#[test]
fn a_meg_value_in_suggest_says_to_use_m_for_mega() {
    let board = board(
        r#"
board "meg-resistor" {
  layers 2
  component R1: resistor "r_generic_0805" value "1meg"
  component R2: resistor "r_generic_0805" value "10k"
  power "VCC" 12v
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    let line = suggestion
        .not_covered
        .iter()
        .find(|line| line.contains("R1"))
        .unwrap_or_else(|| panic!("no R1 line in {:?}", suggestion.not_covered));
    assert!(line.contains("use \"1M\""), "{line}");
    assert!(line.contains("meg"), "{line}");
}

/// The RC capacitor takes the same `meg` hint, and the message names the
/// capacitor rather than blaming the resistor.
#[test]
fn a_meg_capacitor_in_an_rc_says_to_use_m_for_mega() {
    let board = board(
        r#"
board "meg-cap" {
  layers 2
  component R1: resistor "r_generic_0805" value "1k"
  component C1: capacitor "c_generic_0805" value "1meg"
  power "IN" 5v
  net "IN" { R1.p1 }
  net "GND" { C1.p2 }
  connect R1.p2 -> C1.p1 as "OUT"
}
"#,
    );

    let suggestion = synth_sim::suggest_testbench(&board);

    assert!(suggestion.toml.is_none(), "{:?}", suggestion.toml);
    let line = suggestion
        .not_covered
        .iter()
        .find(|line| line.contains("C1"))
        .unwrap_or_else(|| panic!("no C1 line in {:?}", suggestion.not_covered));
    assert!(line.contains("use \"1M\""), "{line}");
    assert!(line.contains("meg"), "{line}");
    assert!(!line.contains("R1"), "{line}");
}

/// The four fault injections from the review: 33 ohm, 2x R and R/2 all move
/// the current outside the band (the reversed LED is a separate test).
#[test]
fn suggested_led_limits_still_catch_33_ohm_2x_and_half_the_resistor() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let design = example_dir("led").join("led.synth");
    let source = std::fs::read_to_string(&design).expect("read design");
    let (toml, _) = suggestion(&board(&source));
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    for (fault, value) in [("33 ohm", "33"), ("2x R", "300"), ("R/2", "75")] {
        let mutated = source.replace("value \"150\"", &format!("value \"{value}\""));
        assert_ne!(
            mutated, source,
            "the fixture must have the 150 ohm resistor"
        );
        let report = simulate_suggested(&board(&mutated), &testbench, &ngspice);

        assert!(
            !report.passed(),
            "{fault} should fail: {:?}",
            report.measures
        );
    }
}

/// A reversed LED draws no forward current, so the suggested minimum fails.
#[test]
fn a_suggested_led_testbench_fails_a_reversed_led() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let design = example_dir("led").join("led.synth");
    let source = std::fs::read_to_string(&design).expect("read design");
    let (toml, _) = suggestion(&board(&source));
    let testbench = synth_sim::Testbench::from_toml(&toml).expect("suggested sidecar");

    let mutated = source
        .replace("net \"GND\" { LED1.cathode }", "net \"GND\" { LED1.anode }")
        .replace(
            "connect R1.p2 -> LED1.anode as \"MID\"",
            "connect R1.p2 -> LED1.cathode as \"MID\"",
        );
    assert_ne!(mutated, source, "the fixture must point the LED at ground");
    let report = simulate_suggested(&board(&mutated), &testbench, &ngspice);

    assert_out_of_limit(&report, "iled", "below min");
}
