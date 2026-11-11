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
    assert_eq!(testbench.source[0].net, "VCC");
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
    assert_eq!(testbench.source[0].net, "VCC");
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
    assert_eq!(testbench.source[0].net, "VOUT");
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
            registry_dirs: Vec::new(),
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
