// SPDX-License-Identifier: Apache-2.0

//! Behaviour of the `simulate` seam: a compiled board plus its testbench
//! sidecar produces a deck, a run and a pass/fail report.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[path = "support/stub_build.rs"]
mod stub_build;

#[path = "support/real_ngspice.rs"]
mod real_ngspice_support;

const DIVIDER: &str = r#"
board "divider" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
}
"#;

const DIVIDER_TB: &str = r#"
ground = "GND"

[[source]]
net = "VCC"
kind = "dc"
dc = 12.0

[analysis]
op = true

[[measure]]
name = "vmid"
kind = "node_voltage"
node = "MID"
min = 5.7
max = 6.3
"#;

const COLLISION: &str = r#"
board "collision" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  component R4: resistor "r_generic_0805" value "10k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  net "mid" { R3.p1, R4.p2 }
  connect R3.p2 -> R4.p1 as "OTHER"
}
"#;

const LED: &str = r#"
board "led" {
  layers 2
  component R1: resistor "r_generic_0805" value "150"
  component LED1: led "led_red_0603"
  net "VCC" { R1.p1 }
  net "GND" { LED1.cathode }
  connect R1.p2 -> LED1.anode as "MID"
}
"#;

/// A board for the registry-patched LED whose pins are named `K` and `A`,
/// with the cathode first.
const K_FIRST_LED: &str = r#"
board "k-first-led" {
  layers 2
  component R1: resistor "r_generic_0805" value "150"
  component LED1: led "led_red_0603"
  net "VCC" { R1.p1 }
  net "GND" { LED1.K }
  connect R1.p2 -> LED1.A as "MID"
}
"#;

const SERIES_TB: &str = r#"
ground = "GND"

[[source]]
net = "VCC"
kind = "dc"
dc = 5.0

[analysis]
op = true

[[measure]]
name = "va"
kind = "node_voltage"
node = "MID"
"#;

const DIODE: &str = r#"
board "diode" {
  layers 2
  component R1: resistor "r_generic_0805" value "430"
  component D1: diode "d_generic"
  net "VCC" { R1.p1 }
  net "GND" { D1.cathode }
  connect R1.p2 -> D1.anode as "MID"
}
"#;

const INDUCTOR: &str = r#"
board "inductor" {
  layers 2
  component L1: inductor "l_generic_0805" value "10uH"
  component R1: resistor "r_generic_0805" value "1k"
  net "VCC" { L1.p1 }
  net "GND" { R1.p2 }
  connect L1.p2 -> R1.p1 as "MID"
}
"#;

/// An IC, a zener and a bridge have no built-in model.
const NO_MODELS: &str = r#"
board "no-models" {
  layers 2
  component U1: mcu "rp2350"
  component D1: diode "d_zener_3v3"
  component D2: diode "d_tvs_5v"
  component B1: diode "d_bridge_rectifier"
  net "VCC" { D1.anode, D2.anode, B1.ac1, B1.pos }
  net "GND" { D1.cathode, D2.cathode, B1.neg, B1.ac2 }
}
"#;

/// The IC has no model; `ignore` is the escape hatch for a known gap.
const IGNORED_IC: &str = r#"
board "ignored-ic" {
  layers 2
  component U1: mcu "rp2350"
  component R1: resistor "r_generic_0805" value "1k"
  component R2: resistor "r_generic_0805" value "1k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
}
"#;

/// Ignoring both ends of a clock net leaves it with no deck node at all.
const IGNORED_CLOCK: &str = r#"
board "ignored-clock" {
  layers 2
  component U1: mcu "rp2350"
  component Y1: crystal "xtal_8mhz"
  component R1: resistor "r_generic_0805" value "1k"
  component R2: resistor "r_generic_0805" value "1k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  net "XIN" { U1.xin, Y1.p1 }
  net "XOUT" { U1.xout, Y1.p2 }
}
"#;

const UNCONNECTED: &str = r#"
board "unconnected" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  net "MID" { R3.p1 }
}
"#;

/// Two capacitors in series give `MID` two connections but no DC path.
const SERIES_CAPACITOR: &str = r#"
board "series-capacitor" {
  layers 2
  component C1: capacitor "c_generic_0805" value "1u"
  component C2: capacitor "c_generic_0805" value "1u"
  net "VCC" { C1.p1 }
  net "GND" { C2.p2 }
  connect C1.p2 -> C2.p1 as "MID"
}
"#;

/// `FLOAT` has one connection; its only part still reaches ground, so this
/// isolates the two-connection rule from the DC-path rule.
const FLOATING: &str = r#"
board "floating" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2, R3.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  net "FLOAT" { R3.p1 }
}
"#;

/// `R1` has an unparseable value, so it has no built-in model.
const NO_MODEL: &str = r#"
board "no-model" {
  layers 2
  component R1: resistor "r_generic_0805" value "nope"
  component R2: resistor "r_generic_0805" value "10k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
}
"#;

/// The AMS1117 part carries a `[spice]` model whose pin order is
/// `vin, gnd, vout`, while the registry pins are `gnd, vout, vin`.
const LDO: &str = r#"
board "ldo" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component R1: resistor "r_generic_0805" value "1k"
  net "VIN" { U1.vin }
  net "GND" { U1.gnd, R1.p2 }
  net "VOUT" { U1.vout, R1.p1 }
}
"#;

const LDO_TB: &str = r#"
ground = "GND"

[[source]]
net = "VIN"
kind = "dc"
dc = 5.0

[analysis]
op = true

[[measure]]
name = "vout"
kind = "node_voltage"
node = "VOUT"
min = 3.25
max = 3.35
"#;

/// Two regulators, each modelled, with conflicting vendor dialects.
const LDO_MIXED: &str = r#"
board "ldo-mixed" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component U2: regulator "ams1117_3v3_lt"
  component R1: resistor "r_generic_0805" value "1k"
  net "VIN" { U1.vin, U2.vin }
  net "GND" { U1.gnd, U2.gnd, R1.p2 }
  net "VOUT" { U1.vout, U2.vout, R1.p1 }
}
"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn board(source: &str) -> synth_ir::Board {
    board_with_registry(source, |_| {})
}

fn board_with_registry(
    source: &str,
    patch: impl FnOnce(&mut synth_registry::Registry),
) -> synth_ir::Board {
    let file = "divider.synth".to_string();
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

fn testbench(source: &str) -> synth_sim::Testbench {
    synth_sim::Testbench::from_toml(source).expect("testbench parses")
}

/// A scratch directory removed when the guard drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("synth_sim_{label}_{}_{nonce}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("stub dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn stub_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
        .join("stub_ngspice.rs")
}

/// Link the stub binary into a scratch directory under `label`, which the
/// stub reads from its own file name. The directory is unique per call so
/// parallel tests never share a stub or its hang markers.
fn stub_ngspice(label: &str) -> (PathBuf, Scratch) {
    let scratch = Scratch::new(label);
    let built = stub_build::build_stub(&stub_source());
    let mut path = scratch.path().join(label);
    if let Some(extension) = built.extension() {
        path.set_extension(extension);
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&built, &path).expect("link stub");
    #[cfg(not(unix))]
    std::fs::copy(&built, &path).expect("copy stub");
    std::fs::write(
        scratch.path().join("fixtures.path"),
        fixtures_dir().display().to_string(),
    )
    .expect("write fixtures.path");
    (path, scratch)
}

fn read_pid(path: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(pid) = text.trim().parse() {
                return pid;
            }
        }
        assert!(
            Instant::now() < deadline,
            "no pid in {} after 2s",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn pid_exists(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn assert_gone(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while pid_exists(pid) {
        assert!(Instant::now() < deadline, "pid {pid} survived the timeout");
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The real engine, when installed. CI installs it; local runs skip.
fn real_ngspice() -> Option<PathBuf> {
    real_ngspice_support::real_ngspice()
}

fn options(stub: &Path) -> synth_sim::SimOptions {
    synth_sim::SimOptions {
        ngspice: Some(stub.to_path_buf()),
        design_path: Some(PathBuf::from("divider.synth")),
        testbench_path: Some(PathBuf::from("divider.sim.toml")),
        registry_dirs: Vec::new(),
    }
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

fn ldo_options(stub: &Path) -> synth_sim::SimOptions {
    synth_sim::SimOptions {
        ngspice: Some(stub.to_path_buf()),
        design_path: Some(PathBuf::from("ldo.synth")),
        testbench_path: Some(PathBuf::from("ldo.sim.toml")),
        registry_dirs: vec![registry_parts_dir()],
    }
}

fn model_file(registry: &Path, name: &str, text: &str) {
    std::fs::write(registry.join(name), text).expect("write model file");
}

fn patched_ams1117(patch: impl FnOnce(&mut synth_registry::SpiceModel)) -> synth_ir::Board {
    board_with_registry(LDO, |registry| {
        let mut part = registry.lookup("ams1117_3v3").expect("ams1117_3v3").clone();
        patch(part.spice.as_mut().expect("[spice] model"));
        registry.insert(part);
    })
}

fn assert_explained(diagnostics: &[synth_diagnostics::Diagnostic]) {
    for diagnostic in diagnostics {
        assert_eq!(
            diagnostic.explanation_url.as_deref(),
            Some(format!("synth.docs/diagnostics/{}", diagnostic.code).as_str()),
            "diagnostic {} has no documentation link",
            diagnostic.code
        );
    }
}

fn assert_located(diagnostics: &[synth_diagnostics::Diagnostic], file: &str) {
    for diagnostic in diagnostics {
        assert_eq!(
            diagnostic
                .location
                .as_ref()
                .map(|location| location.file.as_str()),
            Some(file),
            "diagnostic {} is not located in {file}: {:?}",
            diagnostic.code,
            diagnostic.location
        );
    }
}

#[test]
fn emits_golden_divider_deck() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");
    let expected = std::fs::read_to_string(fixture("divider-op.cir")).expect("golden deck");

    assert_eq!(deck, expected);
}

/// A 2.0 V / 20 mA LED behind 150 Ω from 5 V draws 20 mA. The built-in
/// model is derived from the registry forward voltage and current.
#[test]
fn led_current_matches_the_forward_voltage_model() {
    let board = board(LED);
    let testbench = testbench(SERIES_TB);
    let (stub, _dir) = stub_ngspice("led");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let anode = report.measures[0].value;
    let current = (5.0 - anode) / 150.0;
    let expected = (5.0 - 2.0) / 150.0;
    assert!(
        (current - expected).abs() / expected < 0.02,
        "LED current {current} differs from {expected}"
    );
}

#[test]
fn led_matches_real_ngspice() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(LED);
    let testbench = testbench(SERIES_TB);

    let report = synth_sim::simulate(&board, &testbench, &options(&ngspice));

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let anode = report.measures[0].value;
    assert!(
        (anode - 2.0).abs() < 0.05,
        "LED forward voltage {anode} should be about 2.0 V"
    );
    let current = (5.0 - anode) / 150.0;
    let expected = (5.0 - 2.0) / 150.0;
    assert!(
        (current - expected).abs() / expected < 0.02,
        "LED current {current} differs from {expected}"
    );
}

/// The design point alone cannot catch a wrong IS or N: the model was fitted
/// at 20 mA. At 1 kΩ real ngspice 42 measures 1.9035 V.
#[test]
fn led_matches_real_ngspice_at_an_off_point() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(&LED.replace("value \"150\"", "value \"1k\""));
    let testbench = testbench(SERIES_TB);

    let report = synth_sim::simulate(&board, &testbench, &options(&ngspice));

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let anode = report.measures[0].value;
    assert!(
        (anode - 1.9035).abs() / 1.9035 < 0.03,
        "LED forward voltage at 1 kΩ is {anode}, expected about 1.9035 V"
    );
}

#[test]
fn diode_matches_real_ngspice() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(DIODE);
    let testbench = testbench(SERIES_TB);

    let report = synth_sim::simulate(&board, &testbench, &options(&ngspice));

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let anode = report.measures[0].value;
    assert!(
        (anode - 0.7).abs() < 0.02,
        "diode forward voltage {anode} should be about 0.7 V"
    );
    let current = (5.0 - anode) / 430.0;
    let expected = (5.0 - 0.7) / 430.0;
    assert!(
        (current - expected).abs() / expected < 0.02,
        "diode current {current} differs from {expected}"
    );
}

#[test]
fn emits_golden_led_deck() {
    let board = board(LED);
    let testbench = testbench(SERIES_TB);

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");
    let expected = std::fs::read_to_string(fixture("led-op.cir")).expect("golden deck");

    assert_eq!(deck, expected);
    assert!(deck.contains("DLED1 n_MID 0 DLED1"), "deck:\n{deck}");
    assert!(
        !deck.contains("LLED1"),
        "LED must not emit as an inductor:\n{deck}"
    );
}

/// Some registry LEDs list the cathode first. The diode element must
/// still put the anode net first.
#[test]
fn a_cathode_first_led_emits_anode_to_cathode() {
    let board = board(&LED.replace("led_red_0603", "ltst_c190kgkt"));
    let testbench = testbench(SERIES_TB);

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");

    assert!(deck.contains("DLED1 n_MID 0 DLED1"), "deck:\n{deck}");
}

/// Pin names are the only evidence of polarity. A part that names them `K`
/// and `A` must fail loudly, never emit a possibly reversed diode.
#[test]
fn a_led_without_anode_and_cathode_pin_names_gives_002() {
    let board = board_with_registry(K_FIRST_LED, |registry| {
        let mut led = registry
            .lookup("led_red_0603")
            .expect("led_red_0603")
            .clone();
        led.pins[0].name = "K".to_string();
        led.pins[1].name = "A".to_string();
        registry.insert(led);
    });
    let testbench = testbench(SERIES_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("K-first LED");

    assert_explained(&diagnostics);
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_002
                && diagnostic.message.as_deref().is_some_and(|message| {
                    message.contains("LED1") && message.contains("pins named `anode` and `cathode`")
                })
        }),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn emits_golden_diode_deck() {
    let board = board(DIODE);
    let testbench = testbench(SERIES_TB);

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");
    let expected = std::fs::read_to_string(fixture("diode-op.cir")).expect("golden deck");

    assert_eq!(deck, expected);
}

/// The diode is quoted at 0.7 V / 10 mA, so 430 Ω from 5 V lands on
/// 10 mA and 0.7 V.
#[test]
fn diode_current_matches_the_forward_voltage_model() {
    let board = board(DIODE);
    let testbench = testbench(SERIES_TB);
    let (stub, _dir) = stub_ngspice("diode");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let anode = report.measures[0].value;
    let current = (5.0 - anode) / 430.0;
    let expected = (5.0 - 0.7) / 430.0;
    assert!(
        (current - expected).abs() / expected < 0.02,
        "diode current {current} differs from {expected}"
    );
}

#[test]
fn emits_an_inductor_as_a_henry_value() {
    let board = board(INDUCTOR);
    let testbench = testbench(DIVIDER_TB);

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");

    assert!(deck.contains("LL1 n_VCC n_MID 1e-5"), "deck:\n{deck}");
}

/// The `-002` message must offer the two ways out: a testbench `ignore`
/// entry, or a registry `[spice]` model.
#[test]
fn a_missing_model_error_says_how_to_ignore_or_model_it() {
    let board = board(IGNORED_IC);
    let testbench = testbench(DIVIDER_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("U1 has no model");

    let message = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_002)
        .and_then(|diagnostic| diagnostic.message.as_deref())
        .expect("a -002 for U1");
    assert!(message.contains("ignore"), "{message}");
    assert!(message.contains("[spice]"), "{message}");
}

#[test]
fn unmodelled_parts_are_reported_by_reference_and_part_id() {
    let board = board(NO_MODELS);
    let testbench = testbench(DIVIDER_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("no models");

    assert_explained(&diagnostics);
    for (refdes, part) in [
        ("U1", "rp2350"),
        ("D1", "d_zener_3v3"),
        ("D2", "d_tvs_5v"),
        ("B1", "d_bridge_rectifier"),
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == synth_sim::E_SYNTH_SIM_002
                    && diagnostic
                        .message
                        .as_deref()
                        .is_some_and(|message| message.contains(refdes) && message.contains(part))
            }),
            "{refdes} (`{part}`) missing from {diagnostics:?}"
        );
    }
}

#[test]
fn an_unparseable_or_absent_value_gives_002() {
    let unparseable = board(&DIVIDER.replace("value \"10k\"", "value \"4k7\""));
    let testbench = testbench(DIVIDER_TB);
    let diagnostics = synth_sim::emit_netlist(&unparseable, &testbench).expect_err("4k7");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_002
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("4k7"))
        }),
        "diagnostics: {diagnostics:?}"
    );

    let without_value = board(&DIVIDER.replace(" value \"10k\"", ""));
    let diagnostics = synth_sim::emit_netlist(&without_value, &testbench).expect_err("no value");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_002
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("no value"))
        }),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn an_ignored_part_is_not_reported_as_unmodelled() {
    let board = board(IGNORED_IC);
    let testbench = testbench(&DIVIDER_TB.replacen(
        "ground = \"GND\"\n",
        "ground = \"GND\"\nignore = [\"U1\"]\n",
        1,
    ));
    let (stub, _dir) = stub_ngspice("ok");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let deck = synth_sim::emit_netlist(&board, &testbench).expect("ignored deck");
    assert!(
        !deck.contains("U1"),
        "ignored U1 is still in the deck:\n{deck}"
    );
}

#[test]
fn ignoring_a_part_reruns_the_topology_check() {
    let board = board(LED);
    let testbench = testbench(&SERIES_TB.replacen(
        "ground = \"GND\"\n",
        "ground = \"GND\"\nignore = [\"R1\"]\n",
        1,
    ));

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("R1 removed");

    assert_explained(&diagnostics);
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_008
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("MID"))
        }),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn an_ignored_mcu_and_crystal_are_left_out_of_the_deck() {
    let board = board(IGNORED_CLOCK);
    let testbench = testbench(&DIVIDER_TB.replacen(
        "ground = \"GND\"\n",
        "ground = \"GND\"\nignore = [\"U1\", \"Y1\"]\n",
        1,
    ));

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("ignored clock parts");

    assert!(!deck.contains("U1"), "ignored U1 is in the deck:\n{deck}");
    assert!(!deck.contains("Y1"), "ignored Y1 is in the deck:\n{deck}");
}

#[test]
fn an_unknown_ignore_name_gives_007() {
    let board = board(LED);
    let testbench = testbench(&SERIES_TB.replacen(
        "ground = \"GND\"\n",
        "ground = \"GND\"\nignore = [\"NOPE\"]\n",
        1,
    ));

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("typo");

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_007
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("NOPE"))
        }),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn divider_measures_pass() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("ok");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    assert_explained(&report.diagnostics);
    assert_located(&report.diagnostics, "divider.sim.toml");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_004 && !d.severity.is_blocking()),
        "a passing limit must be an info diagnostic: {:?}",
        report.diagnostics
    );
    assert_eq!(report.measures.len(), 1);
    assert_eq!(report.measures[0].name, "vmid");
    assert!((report.measures[0].value - 6.0).abs() < 1e-9);
    assert!(report.measures[0].passed());
}

#[test]
fn malformed_testbench_gives_007() {
    let missing = synth_sim::Testbench::from_toml("ground = \"GND\"\n\n[analysis]\nop = true\n")
        .expect_err("no sources or measures");
    assert_eq!(missing.code, synth_sim::E_SYNTH_SIM_007);

    let typo = synth_sim::Testbench::from_toml("gronud = \"GND\"\n").expect_err("typo");
    assert_eq!(typo.code, synth_sim::E_SYNTH_SIM_007);
}

#[test]
fn invalid_timeout_gives_007() {
    for timeout in ["0", "-1", "1e20"] {
        let source = DIVIDER_TB.replace(
            "[analysis]",
            &format!("[options]\ntimeout = {timeout}\n\n[analysis]"),
        );
        let error = synth_sim::Testbench::from_toml(&source).expect_err("bad timeout");
        assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007, "timeout {timeout}");
        assert_explained(std::slice::from_ref(&error));
    }
}

#[test]
fn a_sidecar_error_carries_its_path_and_docs_link() {
    let scratch = Scratch::new("sidecar-path");
    let missing = scratch.path().join("missing.sim.toml");
    let error = synth_sim::Testbench::from_path(&missing).expect_err("missing sidecar");
    assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007);
    assert_explained(std::slice::from_ref(&error));
    assert_eq!(
        error
            .location
            .as_ref()
            .map(|location| location.file.as_str()),
        Some(missing.to_str().expect("path is UTF-8"))
    );
}

#[test]
fn sanitized_name_collision_gives_007() {
    let board = board(COLLISION);
    let testbench = testbench(DIVIDER_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("name collision");

    assert_explained(&diagnostics);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_007 && d.severity.is_blocking()),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn an_unconnected_pin_gives_008() {
    let board = board(UNCONNECTED);
    let testbench = testbench(DIVIDER_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("floating pin");

    assert_explained(&diagnostics);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_008 && d.severity.is_blocking()),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn a_series_capacitor_gives_008_naming_the_net() {
    let board = board(SERIES_CAPACITOR);
    let testbench = testbench(DIVIDER_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("no DC path");

    assert_explained(&diagnostics);
    assert!(
        diagnostics.iter().any(|d| {
            d.code == synth_sim::E_SYNTH_SIM_008
                && d.message.as_deref().is_some_and(|m| m.contains("MID"))
        }),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn a_single_connection_net_gives_008_naming_the_net() {
    let board = board(FLOATING);
    let testbench = testbench(DIVIDER_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("floating net");

    assert_explained(&diagnostics);
    assert!(
        diagnostics.iter().any(|d| {
            d.code == synth_sim::E_SYNTH_SIM_008
                && d.message
                    .as_deref()
                    .is_some_and(|m| m.contains("FLOAT") && m.contains("1 connection"))
        }),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn a_missing_model_carries_the_component_span() {
    let board = board(NO_MODEL);
    let testbench = testbench(DIVIDER_TB);
    let options = synth_sim::SimOptions {
        design_path: Some(PathBuf::from("no-model.synth")),
        testbench_path: Some(PathBuf::from("no-model.sim.toml")),
        ..synth_sim::SimOptions::default()
    };

    let report = synth_sim::simulate(&board, &testbench, &options);

    let r1 = board
        .components
        .iter()
        .find(|component| component.refdes == "R1")
        .expect("R1");
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_002)
        .expect("missing model");
    let location = diagnostic.location.as_ref().expect("location");
    assert_eq!(location.file, "no-model.synth");
    assert_eq!(location.span, r1.source_span);
}

#[test]
fn a_floating_net_carries_the_net_span() {
    let board = board(SERIES_CAPACITOR);
    let testbench = testbench(DIVIDER_TB);
    let options = synth_sim::SimOptions {
        design_path: Some(PathBuf::from("series.synth")),
        testbench_path: Some(PathBuf::from("series.sim.toml")),
        ..synth_sim::SimOptions::default()
    };

    let report = synth_sim::simulate(&board, &testbench, &options);

    let mid = board
        .nets
        .iter()
        .find(|net| net.name == "MID")
        .expect("MID");
    let component = mid
        .endpoints
        .first()
        .and_then(|endpoint| {
            board
                .components
                .iter()
                .find(|component| component.id == endpoint.component)
        })
        .expect("MID endpoint component");
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_008
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("MID"))
        })
        .expect("floating MID");
    let location = diagnostic.location.as_ref().expect("location");
    assert_eq!(location.file, "series.synth");
    assert_eq!(location.span, component.source_span);
}

#[test]
fn a_name_collision_is_located_in_the_sidecar() {
    let board = board(COLLISION);
    let testbench = testbench(DIVIDER_TB);
    let options = synth_sim::SimOptions {
        testbench_path: Some(PathBuf::from("collision.sim.toml")),
        ..synth_sim::SimOptions::default()
    };

    let report = synth_sim::simulate(&board, &testbench, &options);

    assert!(
        report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_007
                && diagnostic
                    .location
                    .as_ref()
                    .is_some_and(|location| location.file == "collision.sim.toml")
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn a_megaohm_resistor_emits_an_exponent() {
    let board = board(&DIVIDER.replace("value \"10k\"", "value \"1M\""));
    let testbench = testbench(DIVIDER_TB);

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");

    assert!(
        deck.contains("RR1 n_VCC n_MID 1e6"),
        "deck did not emit 1e6:\n{deck}"
    );
}

/// SPICE reads `meg` as mega, Synth reads `M` as mega. The error must say so.
#[test]
fn a_meg_value_hints_that_synth_uses_m_for_mega() {
    let board = board(&DIVIDER.replace("value \"10k\"", "value \"1meg\""));
    let testbench = testbench(DIVIDER_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("1meg");

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_002
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("Synth uses `M` for mega"))
        }),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn out_of_limit_value_gives_004() {
    let board = board(DIVIDER);
    let testbench = testbench(
        &DIVIDER_TB
            .replace("min = 5.7", "min = 4.0")
            .replace("max = 6.3", "max = 5.5"),
    );
    let (stub, _dir) = stub_ngspice("ok");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert_located(&report.diagnostics, "divider.sim.toml");
    assert!((report.measures[0].value - 6.0).abs() < 1e-9);
    assert!(!report.measures[0].passed());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_004 && d.severity.is_blocking()),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

/// ngspice prints six significant digits, so a limit within the last printed
/// digit must not flip a result that is equal at that precision.
#[test]
fn limit_comparison_uses_the_six_digit_print_precision() {
    let board = board(DIVIDER);
    let (stub, _dir) = stub_ngspice("ok");

    let rounded = testbench(&DIVIDER_TB.replace("min = 5.7", "min = 6.000004"));
    let report = synth_sim::simulate(&board, &rounded, &options(&stub));
    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);

    let outside = testbench(&DIVIDER_TB.replace("min = 5.7", "min = 6.00001"));
    let report = synth_sim::simulate(&board, &outside, &options(&stub));
    assert!(!report.passed(), "diagnostics: {:?}", report.diagnostics);
}

#[test]
fn missing_measurement_value_gives_003() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("missing-meas");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert_located(&report.diagnostics, "divider.sim.toml");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_003 && d.severity.is_blocking()),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn failed_measurement_gives_003() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("failed-meas");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert_located(&report.diagnostics, "divider.sim.toml");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_003
                && d.severity.is_blocking()
                && d.message
                    .as_deref()
                    .is_some_and(|m| m.contains("failed measurement"))),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

/// A flat response never crosses the -3 dB target. The message must name the
/// Synth measure and node, not the generated `synth_corner<N>` vector.
#[test]
fn a_failed_relative_db_names_the_measure_and_the_band_on_real_ngspice() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(DIVIDER);
    let testbench = testbench(
        r#"
ground = "GND"

[[source]]
net = "VCC"
kind = "dc"
dc = 0.0
ac = 1.0

[analysis.ac]
start = 10.0
stop = 100000.0
points_per_decade = 100

[[measure]]
name = "corner"
kind = "relative_db"
node = "MID"
min = 100.0
max = 1000.0
"#,
    );

    let report = synth_sim::simulate(&board, &testbench, &options(&ngspice));

    assert!(!report.passed());
    let message = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_003)
        .and_then(|diagnostic| diagnostic.message.as_deref())
        .expect("a -003 for the failed corner");
    assert!(message.contains("corner"), "{message}");
    assert!(message.contains("MID"), "{message}");
    assert!(
        message.contains("no -3 dB crossing in 10-100000 Hz"),
        "{message}"
    );
    assert!(!message.contains("synth_corner"), "{message}");
}

/// A limit diagnostic points at the `[[measure]]` block that declared it, not
/// at the start of the sidecar.
#[test]
fn a_limit_diagnostic_points_at_its_measure_block() {
    let board = board(DIVIDER);
    let (stub, _dir) = stub_ngspice("ok");
    for (label, min, max, blocking) in [
        ("passing", "5.8", "6.1", false),
        ("failing", "6.2", "6.4", true),
    ] {
        let source = DIVIDER_TB
            .replace("min = 5.7", &format!("min = {min}"))
            .replace("max = 6.3", &format!("max = {max}"));
        let testbench = testbench(&source);

        let report = synth_sim::simulate(&board, &testbench, &options(&stub));

        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.code == synth_sim::E_SYNTH_SIM_004
                    && diagnostic.severity.is_blocking() == blocking
            })
            .unwrap_or_else(|| panic!("{label}: no -004 in {:?}", report.diagnostics));
        let location = diagnostic.location.as_ref().expect("location");
        assert_eq!(location.file, "divider.sim.toml");
        assert!(
            !location.span.is_empty(),
            "{label}: {}",
            location.span.byte_start
        );
        let block = &source[location.span.byte_start as usize..location.span.byte_end as usize];
        assert!(block.starts_with("[[measure]]"), "{label}: {block}");
        assert!(block.contains("name = \"vmid\""), "{label}: {block}");
    }
}

/// `[[ measure ]]` with spaces is valid TOML; the span must still cover the
/// block, and a per-measure `-003` must point at it.
#[test]
fn a_spaced_measure_header_gets_its_span_and_003_points_at_it() {
    // The first, spaced header must own its block; the second header is the
    // exact spelling that the old text scan matched, so a slip would point the
    // first measure at the second block.
    let source = format!(
        "{}\n[[measure]]\nname = \"other\"\nkind = \"node_voltage\"\nnode = \"MID\"\n\
         min = 0.0\nmax = 10.0\n",
        DIVIDER_TB.replace("[[measure]]", "[[ measure ]]")
    );
    let board = board(DIVIDER);
    let testbench = testbench(&source);
    let measure = &testbench.measure[0];
    assert!(!measure.span.is_empty(), "span: {:?}", measure.span);
    let block = &source[measure.span.byte_start as usize..measure.span.byte_end as usize];
    assert!(block.starts_with("[[ measure ]]"), "{block}");
    assert!(block.contains("name = \"vmid\""), "{block}");
    assert!(!block.contains("name = \"other\""), "{block}");

    let (stub, _dir) = stub_ngspice("missing-meas");
    let options = synth_sim::SimOptions {
        testbench_path: Some(PathBuf::from("spaced.sim.toml")),
        ..options(&stub)
    };
    let report = synth_sim::simulate(&board, &testbench, &options);

    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_003
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("vmid"))
        })
        .unwrap_or_else(|| panic!("no per-measure -003 in {:?}", report.diagnostics));
    let location = diagnostic.location.as_ref().expect("location");
    assert_eq!(location.file, "spaced.sim.toml");
    assert_eq!(location.span, measure.span);
}

/// A `-007` about one measure points at that `[[measure]]` block, not 0-0.
#[test]
fn a_per_measure_007_points_at_its_measure_block() {
    let source = DIVIDER_TB.replace("node = \"MID\"", "node = \"MISSING\"");
    assert_ne!(source, DIVIDER_TB);
    let board = board(DIVIDER);
    let testbench = testbench(&source);
    let options = synth_sim::SimOptions {
        design_path: Some(PathBuf::from("spanned.synth")),
        testbench_path: Some(PathBuf::from("spanned.sim.toml")),
        ..synth_sim::SimOptions::default()
    };

    let report = synth_sim::simulate(&board, &testbench, &options);

    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_007
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("MISSING"))
        })
        .unwrap_or_else(|| panic!("no -007 in {:?}", report.diagnostics));
    let location = diagnostic.location.as_ref().expect("location");
    assert_eq!(location.file, "spanned.sim.toml");
    let block = &source[location.span.byte_start as usize..location.span.byte_end as usize];
    assert!(block.starts_with("[[measure]]"), "{block}");
    assert!(block.contains("name = \"vmid\""), "{block}");
}

#[test]
fn missing_done_sentinel_gives_003() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("missing-done");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert_located(&report.diagnostics, "divider.sim.toml");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_003
                && d.severity.is_blocking()
                && d.message
                    .as_deref()
                    .is_some_and(|m| m.contains("SYNTH_DONE"))),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn non_finite_measurement_gives_003() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("nan");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert_located(&report.diagnostics, "divider.sim.toml");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_003
                && d.severity.is_blocking()
                && d.message
                    .as_deref()
                    .is_some_and(|m| m.contains("not finite"))),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn ngspice_error_gives_006() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("error");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert_located(&report.diagnostics, "divider.sim.toml");
    assert!(
        report.diagnostics.iter().any(|d| {
            d.code == synth_sim::E_SYNTH_SIM_006
                && d.message.as_deref().is_some_and(|m| {
                    m.contains("exited with status 1") && m.contains("unknown subckt")
                })
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

/// A singular matrix exits 0 and still prints a value. The warning on stderr
/// is the only evidence the run is wrong.
#[test]
fn singular_matrix_with_exit_zero_gives_006() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("singular");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert_located(&report.diagnostics, "divider.sim.toml");
    assert!(
        report.diagnostics.iter().any(|d| {
            d.code == synth_sim::E_SYNTH_SIM_006
                && d.message
                    .as_deref()
                    .is_some_and(|m| m.contains("singular matrix"))
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

/// A capped stderr can drop the singular warning; a partial log is never a pass.
#[test]
fn truncated_output_gives_006() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("truncated");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert!(
        report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_006
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("truncated log"))
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

/// The singular warning is evidence of a broken run even when the missing
/// SYNTH_DONE would otherwise classify as a measurement failure.
#[test]
fn singular_matrix_without_done_gives_006() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("singular-no-done");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert!(
        report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_006
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("singular matrix"))
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

/// The singular warning is evidence of a broken run even when stderr also
/// says `no such variable`, which the measurement scan would otherwise
/// claim as `-003`.
#[test]
fn singular_matrix_with_a_missing_vector_gives_006() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("singular-missing-value");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert_located(&report.diagnostics, "divider.sim.toml");
    assert!(
        report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_006
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("singular matrix"))
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_003),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn missing_binary_gives_001() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let scratch = Scratch::new("missing-binary");
    let missing = scratch.path().join("ngspice-does-not-exist");

    let report = synth_sim::simulate(&board, &testbench, &options(&missing));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert_located(&report.diagnostics, "divider.sim.toml");
    assert!(
        report.diagnostics.iter().any(|d| {
            d.code == synth_sim::E_SYNTH_SIM_001
                && d.message.as_deref().is_some_and(|m| m.contains("install"))
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn timeout_gives_005_and_kills_the_process_and_grandchild() {
    let board = board(DIVIDER);
    let testbench =
        testbench(&DIVIDER_TB.replace("[analysis]", "[options]\ntimeout = 1.0\n\n[analysis]"));
    let (stub, scratch) = stub_ngspice("hang");

    let started = Instant::now();
    let report = synth_sim::simulate(&board, &testbench, &options(&stub));
    let elapsed = started.elapsed();

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert_located(&report.diagnostics, "divider.sim.toml");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_005),
        "diagnostics: {:?}",
        report.diagnostics
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "timeout took {elapsed:?}, expected about 1s (not timeout + drain grace)"
    );

    let hang = read_pid(&scratch.path().join("hang.pid"));
    #[cfg(unix)]
    {
        let child = read_pid(&scratch.path().join("hang-child.pid"));
        assert_gone(hang);
        assert_gone(child);
    }
    #[cfg(not(unix))]
    let _ = hang;
}

/// The review repro: a wrapper that starts ngspice without `exec` leaves
/// ngspice in the wrapper's process group, not as the direct child.
#[cfg(unix)]
#[test]
fn timeout_kills_a_non_exec_wrapper_and_its_grandchild() {
    use std::os::unix::fs::PermissionsExt as _;

    let board = board(DIVIDER);
    let testbench =
        testbench(&DIVIDER_TB.replace("[analysis]", "[options]\ntimeout = 1.0\n\n[analysis]"));
    let (hang, scratch) = stub_ngspice("hang");
    let wrapper = scratch.path().join("ngspice-wrapper.sh");
    let wrapper_pid_file = scratch.path().join("wrapper.pid");
    let child_pid_file = scratch.path().join("wrapper-child.pid");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\necho $$ > '{}'\n'{}' -b deck.cir &\necho $! > '{}'\nwait\n",
            wrapper_pid_file.display(),
            hang.display(),
            child_pid_file.display()
        ),
    )
    .expect("write wrapper");
    let mut permissions = std::fs::metadata(&wrapper)
        .expect("stat wrapper")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&wrapper, permissions).expect("chmod wrapper");

    let started = Instant::now();
    let report = synth_sim::simulate(&board, &testbench, &options(&wrapper));
    let elapsed = started.elapsed();

    assert!(!report.passed());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_005),
        "diagnostics: {:?}",
        report.diagnostics
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "wrapper timeout took {elapsed:?}, expected about 1s (not timeout + drain grace)"
    );

    let wrapper_pid = read_pid(&wrapper_pid_file);
    let child_pid = read_pid(&child_pid_file);
    let grandchild_pid = read_pid(&scratch.path().join("hang-child.pid"));
    assert_gone(wrapper_pid);
    assert_gone(child_pid);
    assert_gone(grandchild_pid);
}

/// The model path is absolute and quoted; the `X` line follows
/// `pin_order`, not the registry pin order.
#[test]
fn emits_golden_ldo_deck_with_registry_include() {
    let board = board(LDO);
    let testbench = testbench(LDO_TB);
    let dirs = vec![registry_parts_dir()];

    let deck =
        synth_sim::emit_netlist_with_registry_dirs(&board, &testbench, &dirs).expect("deck emits");
    let model = registry_parts_dir().join("spice").join("ams1117_3v3.lib");
    let expected = std::fs::read_to_string(fixture("ldo-op.cir"))
        .expect("golden deck")
        .replace("{{MODEL}}", &model.display().to_string());

    assert_eq!(deck, expected);
    assert!(
        deck.contains("XU1 n_VIN 0 n_VOUT ams1117_3v3 rout=100m"),
        "deck:\n{deck}"
    );
}

#[test]
fn ldo_simulates_through_the_stub() {
    let board = board(LDO);
    let testbench = testbench(LDO_TB);
    let (stub, _dir) = stub_ngspice("ldo");

    let report = synth_sim::simulate(&board, &testbench, &ldo_options(&stub));

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let vout = report.measures[0].value;
    assert!(
        (vout - 3.29967).abs() < 5e-4,
        "vout {vout} should be about 3.29967 V"
    );
}

#[test]
fn ldo_matches_real_ngspice() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = board(LDO);
    let testbench = testbench(LDO_TB);

    let report = synth_sim::simulate(&board, &testbench, &ldo_options(&ngspice));

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let vout = report.measures[0].value;
    assert!(
        (vout - 3.29967).abs() < 5e-4,
        "vout {vout} should be about 3.29967 V"
    );
}

const LDO_MODEL_BODY: &str = "Vref vout int 3.3\nRout int gnd {rout}\n.ends\n";

/// Run the LDO board against a scratch registry model written in `text`.
/// `None` when real ngspice is not installed.
fn ldo_model_report(label: &str, text: &str) -> Option<synth_sim::SimReport> {
    let ngspice = real_ngspice()?;
    let scratch = Scratch::new(label);
    model_file(scratch.path(), label, text);
    let board = patched_ams1117(|spice| spice.file = label.to_string());
    let testbench = testbench(LDO_TB);
    Some(synth_sim::simulate(
        &board,
        &testbench,
        &synth_sim::SimOptions {
            ngspice: Some(ngspice),
            design_path: Some(PathBuf::from("ldo.synth")),
            testbench_path: Some(PathBuf::from("ldo.sim.toml")),
            registry_dirs: vec![scratch.path().to_path_buf()],
        },
    ))
}

fn assert_ldo_vout(report: &synth_sim::SimReport) {
    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let vout = report.measures[0].value;
    assert!(
        (vout - 3.29967).abs() < 5e-4,
        "vout {vout} should be about 3.29967 V"
    );
}

#[test]
fn a_subckt_with_an_inline_semicolon_comment_matches_real_ngspice() {
    let Some(report) = ldo_model_report(
        "semicolon.lib",
        &format!(
            ".subckt ams1117_3v3 vin gnd vout ; comment\n+ params: rout=50m\n{LDO_MODEL_BODY}"
        ),
    ) else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    assert_ldo_vout(&report);
}

#[test]
fn a_subckt_with_an_inline_dollar_comment_matches_real_ngspice() {
    let Some(report) = ldo_model_report(
        "dollar.lib",
        &format!(
            ".subckt ams1117_3v3 vin gnd vout $ comment\n+ params: rout=50m\n{LDO_MODEL_BODY}"
        ),
    ) else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    assert_ldo_vout(&report);
}

/// `$` in column 0 is a comment line in ngspice, so a wrapped `.subckt` line
/// must skip it rather than treat it as the end of the declaration.
#[test]
fn a_subckt_with_a_column_zero_dollar_comment_before_continuation_matches_real_ngspice() {
    let Some(report) = ldo_model_report(
        "dollar_column_zero.lib",
        &format!(
            ".subckt ams1117_3v3 vin gnd\n$ comment\n+ vout params: rout=50m\n{LDO_MODEL_BODY}"
        ),
    ) else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    assert_ldo_vout(&report);
}

#[test]
fn a_subckt_with_a_star_comment_before_continuation_matches_real_ngspice() {
    let Some(report) = ldo_model_report(
        "star_comment.lib",
        &format!(
            ".subckt ams1117_3v3 vin gnd\n* comment\n+ vout params: rout=50m\n{LDO_MODEL_BODY}"
        ),
    ) else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    assert_ldo_vout(&report);
}

#[test]
fn a_subckt_with_a_blank_line_before_continuation_matches_real_ngspice() {
    let Some(report) = ldo_model_report(
        "blank_line.lib",
        &format!(".subckt ams1117_3v3 vin gnd\n\n+ vout params: rout=50m\n{LDO_MODEL_BODY}"),
    ) else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    assert_ldo_vout(&report);
}

#[test]
fn a_bad_spice_pin_order_gives_002_naming_the_part() {
    for pin_order in [
        vec!["vin".to_string(), "gnd".to_string(), "nope".to_string()],
        vec!["vin".to_string(), "gnd".to_string(), "vin".to_string()],
        vec!["vin".to_string(), "gnd".to_string()],
    ] {
        let board = patched_ams1117(|spice| spice.pin_order = pin_order.clone());
        let testbench = testbench(LDO_TB);

        let diagnostics =
            synth_sim::emit_netlist_with_registry_dirs(&board, &testbench, &[registry_parts_dir()])
                .expect_err("bad pin_order");

        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == synth_sim::E_SYNTH_SIM_002
                    && diagnostic.message.as_deref().is_some_and(|message| {
                        message.contains("U1") && message.contains("ams1117_3v3")
                    })
            }),
            "pin_order {pin_order:?}: {diagnostics:?}"
        );
    }
}

#[test]
fn a_missing_spice_subckt_gives_002_naming_the_part() {
    let board = patched_ams1117(|spice| spice.subckt = "no_such_subckt".to_string());
    let testbench = testbench(LDO_TB);

    let diagnostics =
        synth_sim::emit_netlist_with_registry_dirs(&board, &testbench, &[registry_parts_dir()])
            .expect_err("missing subckt");

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_002
                && diagnostic.message.as_deref().is_some_and(|message| {
                    message.contains("U1")
                        && message.contains("ams1117_3v3")
                        && message.contains("no_such_subckt")
                })
        }),
        "diagnostics: {diagnostics:?}"
    );
}

/// `.subckt` terminals must match `pin_order`. ngspice binds the extra nodes
/// silently, so a mismatch otherwise reports a plausible operating point.
#[test]
fn a_subckt_terminal_count_must_match_pin_order() {
    for (label, text) in [
        ("two_terminals.lib", ".subckt ams1117_3v3 vin gnd\n.ends\n"),
        (
            "four_terminals.lib",
            ".subckt ams1117_3v3 vin gnd vout extra\n.ends\n",
        ),
    ] {
        let scratch = Scratch::new(label);
        model_file(scratch.path(), label, text);
        let board = patched_ams1117(|spice| spice.file = label.to_string());
        let testbench = testbench(LDO_TB);

        let diagnostics = synth_sim::emit_netlist_with_registry_dirs(
            &board,
            &testbench,
            &[scratch.path().to_path_buf()],
        )
        .expect_err("terminal count mismatch");

        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == synth_sim::E_SYNTH_SIM_002
                    && diagnostic.message.as_deref().is_some_and(|message| {
                        message.contains("U1") && message.contains("ams1117_3v3")
                    })
            }),
            "{label}: {diagnostics:?}"
        );
    }
}

/// `params:` and `name=value` are not terminals; a wrapped `.subckt` line is
/// still one declaration.
#[test]
fn subckt_params_and_continuation_lines_are_not_terminals() {
    for (label, text) in [
        (
            "params.lib",
            ".subckt ams1117_3v3 vin gnd vout params: rout=50m\nVref vout int 3.3\n\
             Rout int gnd {rout}\n.ends\n",
        ),
        (
            "continuation.lib",
            ".subckt ams1117_3v3 vin gnd\n+ vout params: rout=50m\nVref vout int 3.3\n\
             Rout int gnd {rout}\n.ends\n",
        ),
    ] {
        let scratch = Scratch::new(label);
        model_file(scratch.path(), label, text);
        let board = patched_ams1117(|spice| spice.file = label.to_string());
        let testbench = testbench(LDO_TB);

        synth_sim::emit_netlist_with_registry_dirs(
            &board,
            &testbench,
            &[scratch.path().to_path_buf()],
        )
        .expect("terminal count matches pin_order");
    }
}

/// A missing model file must not fail lowering; only the simulation reports it.
#[test]
fn a_missing_spice_model_file_gives_002_naming_the_part() {
    let board = patched_ams1117(|spice| spice.file = "spice/does_not_exist.lib".to_string());
    let testbench = testbench(LDO_TB);
    let (stub, _dir) = stub_ngspice("ldo");

    let report = synth_sim::simulate(&board, &testbench, &ldo_options(&stub));

    assert!(!report.passed());
    assert!(
        report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_002
                && diagnostic.message.as_deref().is_some_and(|message| {
                    message.contains("U1") && message.contains("ams1117_3v3")
                })
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn mixed_spice_dialects_give_002_naming_both_parts() {
    use synth_registry::SpiceDialect::{Lt, Ps};

    for (first, second) in [(Some(Ps), Some(Lt)), (None, Some(Ps)), (Some(Ps), None)] {
        let board = board_with_registry(LDO_MIXED, |registry| {
            let mut first_part = registry.lookup("ams1117_3v3").expect("ams1117_3v3").clone();
            first_part.spice.as_mut().expect("[spice] model").dialect = first;
            registry.insert(first_part.clone());
            let mut second_part = first_part;
            second_part.id = synth_registry::PartId("ams1117_3v3_lt".to_string());
            second_part.spice.as_mut().expect("[spice] model").dialect = second;
            registry.insert(second_part);
        });
        let testbench = testbench(LDO_TB);

        let diagnostics =
            synth_sim::emit_netlist_with_registry_dirs(&board, &testbench, &[registry_parts_dir()])
                .expect_err("mixed dialects");

        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == synth_sim::E_SYNTH_SIM_002
                    && diagnostic.message.as_deref().is_some_and(|message| {
                        ["U1", "ams1117_3v3", "U2", "ams1117_3v3_lt"]
                            .iter()
                            .all(|needle| message.contains(needle))
                    })
            }),
            "{first:?} + {second:?}: {diagnostics:?}"
        );
        let conflict = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_002)
            .and_then(|diagnostic| diagnostic.message.as_deref())
            .expect("dialect conflict message");
        if first.is_none() || second.is_none() {
            assert!(conflict.contains("uses no `dialect`"), "{conflict}");
            assert!(!conflict.contains("`none`"), "{conflict}");
        }
    }
}

#[test]
fn control_and_shell_model_files_are_rejected() {
    for (label, text) in [
        (
            "control.lib",
            ".subckt ams1117_3v3 vin gnd vout\n.control\nop\n.endc\n.ends\n",
        ),
        (
            "shell.lib",
            ".subckt ams1117_3v3 vin gnd vout\nshell echo escaped\n.ends\n",
        ),
    ] {
        let scratch = Scratch::new(label);
        model_file(scratch.path(), label, text);
        let board = patched_ams1117(|spice| spice.file = label.to_string());
        let testbench = testbench(LDO_TB);

        let diagnostics = synth_sim::emit_netlist_with_registry_dirs(
            &board,
            &testbench,
            &[scratch.path().to_path_buf()],
        )
        .expect_err("forbidden directive");

        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == synth_sim::E_SYNTH_SIM_002
                    && diagnostic
                        .message
                        .as_deref()
                        .is_some_and(|message| message.contains("U1") && message.contains(label))
            }),
            "{label}: {diagnostics:?}"
        );
    }
}

#[test]
fn verilog_a_and_encrypted_models_are_rejected() {
    for (label, text) in [
        (
            "analog.va",
            ".subckt ams1117_3v3 vin gnd vout\nanalog begin\nend\n.ends\n",
        ),
        (
            "encrypted.lib",
            "* $CDNENCSTART\n.subckt ams1117_3v3 vin gnd vout\n.ends\n",
        ),
    ] {
        let scratch = Scratch::new(label);
        model_file(scratch.path(), label, text);
        let board = patched_ams1117(|spice| spice.file = label.to_string());
        let testbench = testbench(LDO_TB);

        let diagnostics = synth_sim::emit_netlist_with_registry_dirs(
            &board,
            &testbench,
            &[scratch.path().to_path_buf()],
        )
        .expect_err("unsupported model format");

        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == synth_sim::E_SYNTH_SIM_002
                    && diagnostic
                        .message
                        .as_deref()
                        .is_some_and(|message| message.contains("U1"))
            }),
            "{label}: {diagnostics:?}"
        );
    }
}

#[test]
fn a_model_file_outside_the_registry_dirs_is_rejected() {
    let scratch = Scratch::new("outside");
    let inside = scratch.path().join("inside");
    let outside = scratch.path().join("outside");
    std::fs::create_dir_all(&inside).expect("inside dir");
    std::fs::create_dir_all(&outside).expect("outside dir");
    let text = ".subckt ams1117_3v3 vin gnd vout\n.ends\n";
    model_file(&outside, "escape.lib", text);
    let absolute = outside.join("escape.lib").display().to_string();

    for file in ["../outside/escape.lib".to_string(), absolute] {
        let board = patched_ams1117(|spice| spice.file = file.clone());
        let testbench = testbench(LDO_TB);

        let diagnostics = synth_sim::emit_netlist_with_registry_dirs(
            &board,
            &testbench,
            std::slice::from_ref(&inside),
        )
        .expect_err("outside the registry");

        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == synth_sim::E_SYNTH_SIM_002
                    && diagnostic.message.as_deref().is_some_and(|message| {
                        message.contains("U1") && message.contains("outside")
                    })
            }),
            "file {file}: {diagnostics:?}"
        );
    }
}

/// Every endpoint of XIN is ignored, so the net is dropped from the deck.
/// A measure on it must be a topology error at the design span, never a
/// `v(n_XIN)` that fails later as `-003` at 0-0.
#[test]
fn a_measure_on_a_fully_ignored_net_gives_008_with_the_net_span() {
    let board = board(IGNORED_CLOCK);
    let testbench = testbench(
        &DIVIDER_TB
            .replacen(
                "ground = \"GND\"\n",
                "ground = \"GND\"\nignore = [\"U1\", \"Y1\"]\n",
                1,
            )
            .replace(
                "max = 6.3\n",
                "max = 6.3\n\n[[measure]]\nname = \"xin\"\nkind = \"node_voltage\"\nnode = \"XIN\"\n",
            ),
    );
    let options = synth_sim::SimOptions {
        design_path: Some(PathBuf::from("ignored.synth")),
        testbench_path: Some(PathBuf::from("ignored.sim.toml")),
        ..synth_sim::SimOptions::default()
    };

    let report = synth_sim::simulate(&board, &testbench, &options);

    let u1 = board
        .components
        .iter()
        .find(|component| component.refdes == "U1")
        .expect("U1");
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_008)
        .expect("ignored-net measure must be -008");
    let location = diagnostic.location.as_ref().expect("location");
    assert_eq!(location.file, "ignored.synth");
    assert_eq!(location.span, u1.source_span);
    assert!(
        diagnostic
            .message
            .as_deref()
            .is_some_and(|message| message.contains("xin") && message.contains("XIN")),
        "{diagnostic:?}"
    );
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_003),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

/// A net driven by a testbench source is a real deck node even when no placed
/// part supplies it.
#[test]
fn a_measure_on_a_sourced_net_is_allowed() {
    let board = board(DIVIDER);
    let testbench = testbench(&DIVIDER_TB.replace(
        "max = 6.3\n",
        "max = 6.3\n\n[[measure]]\nname = \"vcc\"\nkind = \"node_voltage\"\nnode = \"VCC\"\n",
    ));

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("VCC is a node");

    assert!(deck.contains("let m1 = v(n_VCC)"), "deck:\n{deck}");
}

/// ngspice has no vector for node 0, so a ground measure is rejected up front
/// instead of surfacing as a missing-measurement error after the run.
#[test]
fn a_measure_on_ground_is_rejected_with_007() {
    let divider = board(DIVIDER);
    let divider_tb = testbench(&DIVIDER_TB.replace(
        "max = 6.3\n",
        "max = 6.3\n\n[[measure]]\nname = \"vgnd\"\nkind = \"node_voltage\"\nnode = \"GND\"\n",
    ));
    let diagnostics =
        synth_sim::emit_netlist(&divider, &divider_tb).expect_err("v(0) has no vector");
    assert_ground_measure_rejected(&diagnostics);

    let rc = board(RC);
    let rc_tb = testbench(&RC_TB.replace(
        "max = 1620.0\n",
        "max = 1620.0\n\n[[measure]]\nname = \"fgnd\"\nkind = \"relative_db\"\nnode = \"GND\"\n",
    ));
    let diagnostics = synth_sim::emit_netlist(&rc, &rc_tb).expect_err("vdb(0) has no vector");
    assert_ground_measure_rejected(&diagnostics);
}

fn assert_ground_measure_rejected(diagnostics: &[synth_diagnostics::Diagnostic]) {
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_007
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("ground is 0 V by definition"))
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn a_shared_model_file_is_included_once() {
    let board = board_with_registry(LDO_MIXED, |registry| {
        let mut second = registry.lookup("ams1117_3v3").expect("ams1117_3v3").clone();
        second.id = synth_registry::PartId("ams1117_3v3_lt".to_string());
        registry.insert(second);
    });
    let testbench = testbench(LDO_TB);

    let deck =
        synth_sim::emit_netlist_with_registry_dirs(&board, &testbench, &[registry_parts_dir()])
            .expect("deck emits");

    assert_eq!(deck.matches(".include ").count(), 1, "deck:\n{deck}");
    assert!(deck.contains("XU1 "), "deck:\n{deck}");
    assert!(deck.contains("XU2 "), "deck:\n{deck}");
}

/// A nested include could smuggle `.control`/`shell` past the model checks,
/// so v1 rejects the directive itself and never starts ngspice.
#[test]
fn a_model_with_a_nested_include_is_rejected_without_running() {
    let scratch = Scratch::new("nested-include");
    model_file(
        scratch.path(),
        "inner.lib",
        ".subckt ams1117_3v3 vin gnd vout\n.control\nshell echo escaped\n.endc\n.ends\n",
    );
    model_file(
        scratch.path(),
        "outer.lib",
        ".subckt ams1117_3v3 vin gnd vout\n.include \"inner.lib\"\n.ends\n",
    );
    let board = patched_ams1117(|spice| spice.file = "outer.lib".to_string());
    let testbench = testbench(LDO_TB);
    let options = synth_sim::SimOptions {
        // A missing binary proves the emitter failed before any run.
        ngspice: Some(scratch.path().join("must-not-run")),
        design_path: Some(PathBuf::from("ldo.synth")),
        testbench_path: Some(PathBuf::from("ldo.sim.toml")),
        registry_dirs: vec![scratch.path().to_path_buf()],
    };

    let report = synth_sim::simulate(&board, &testbench, &options);

    assert!(
        report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_002
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains(".include"))
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_001),
        "ngspice must not run: {:?}",
        report.diagnostics
    );
}

/// The loader merges user parts over seed parts, so the user registry's copy
/// of a relative model path must win too.
#[test]
fn the_last_registry_dir_wins_for_a_shared_relative_model_path() {
    let seed = Scratch::new("precedence-seed");
    let user = Scratch::new("precedence-user");
    let seed_model = seed.path().join("spice");
    let user_model = user.path().join("spice");
    std::fs::create_dir_all(&seed_model).expect("seed model dir");
    std::fs::create_dir_all(&user_model).expect("user model dir");
    model_file(
        &seed_model,
        "ams1117_3v3.lib",
        ".subckt ams1117_3v3 vin gnd vout params: rout=50m\nVref vout int 3.3\n\
         Rout int gnd {rout}\n.ends\n",
    );
    model_file(
        &user_model,
        "ams1117_3v3.lib",
        ".subckt ams1117_3v3 vin gnd vout params: rout=50m\nVref vout int 3.0\n\
         Rout int gnd {rout}\n.ends\n",
    );
    let board = board(LDO);
    let testbench = testbench(LDO_TB);

    let deck = synth_sim::emit_netlist_with_registry_dirs(
        &board,
        &testbench,
        &[seed.path().to_path_buf(), user.path().to_path_buf()],
    )
    .expect("deck emits");

    let expected = user_model
        .canonicalize()
        .expect("canonical user model")
        .join("ams1117_3v3.lib");
    assert!(
        deck.contains(&format!(".include \"{}\"", expected.display())),
        "deck must include the user copy:\n{deck}"
    );
    let seed_path = seed_model
        .canonicalize()
        .expect("canonical seed model")
        .join("ams1117_3v3.lib");
    assert!(
        !deck.contains(&format!(".include \"{}\"", seed_path.display())),
        "deck must not include the seed copy:\n{deck}"
    );
}

/// `dialect = "ps"` writes `ngbehavior=ps` into `.spiceinit`; the model must
/// still produce the same operating point under real ngspice.
#[test]
fn a_ps_dialect_spice_part_matches_real_ngspice() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let board = patched_ams1117(|spice| {
        spice.dialect = Some(synth_registry::SpiceDialect::Ps);
    });
    let testbench = testbench(LDO_TB);

    let report = synth_sim::simulate(&board, &testbench, &ldo_options(&ngspice));

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let vout = report.measures[0].value;
    assert!(
        (vout - 3.29967).abs() < 5e-4,
        "vout {vout} should be about 3.29967 V"
    );
}

const RC: &str = r#"
board "rc-filter" {
  layers 2
  component R1: resistor "r_generic_0805" value "1k"
  component C1: capacitor "c_generic_0805" value "100n"
  net "IN" { R1.p1 }
  net "GND" { C1.p2 }
  connect R1.p2 -> C1.p1 as "OUT"
}
"#;

const RC_TB: &str = r#"
ground = "GND"

[[source]]
net = "IN"
kind = "dc"
dc = 0.0
ac = 1.0

[analysis.ac]
start = 10.0
stop = 100000.0
points_per_decade = 100

[[measure]]
name = "corner"
kind = "relative_db"
node = "OUT"
min = 1560.0
max = 1620.0
"#;

const CURRENT_TB: &str = r#"
ground = "GND"

[[source]]
net = "VCC"
kind = "dc"
dc = 5.0

[analysis]
op = true

[[measure]]
name = "iled"
kind = "current"
element = "LED1"
min = 0.019
max = 0.021
"#;

/// A source can carry DC and an AC magnitude, and a relative-dB measure is
/// emitted as max, target and crossing in that order.
#[test]
fn emits_an_ac_source_and_a_relative_db_corner_measure() {
    let board = board(RC);
    let testbench = testbench(RC_TB);

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");

    assert!(deck.contains("V1 n_IN 0 DC 0e0 AC 1e0"), "deck:\n{deck}");
    assert!(deck.contains("ac dec 100 1e1 1e5"), "deck:\n{deck}");
    assert!(
        deck.contains("meas ac synth_max0 max vdb(n_OUT)"),
        "deck:\n{deck}"
    );
    assert!(
        deck.contains("let synth_target0 = synth_max0 - 3e0"),
        "deck:\n{deck}"
    );
    assert!(
        deck.contains("meas ac synth_corner0 when vdb(n_OUT) = synth_target0"),
        "deck:\n{deck}"
    );
    assert!(
        deck.contains("echo SYNTH_MEAS corner $&synth_corner0"),
        "deck:\n{deck}"
    );
}

/// A fixed-frequency node voltage reads the AC gain directly, so an absolute
/// dB limit can check the passband or the slope of a filter.
#[test]
fn emits_an_at_frequency_gain_measure() {
    let board = board(RC);
    let testbench = testbench(&RC_TB.replace(
        "max = 1620.0\n",
        "max = 1620.0\n\n[[measure]]\nname = \"passband\"\nkind = \"node_voltage\"\n\
         node = \"OUT\"\nat_frequency = 159.0\nmin = -0.5\nmax = 0.0\n",
    ));

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");

    assert!(
        deck.contains("meas ac synth_gain1 find vdb(n_OUT) at=1.59e2"),
        "deck:\n{deck}"
    );
    assert!(
        deck.contains("echo SYNTH_MEAS passband $&synth_gain1"),
        "deck:\n{deck}"
    );
}

/// `at_frequency` only makes sense for a node voltage during an AC sweep with
/// a non-zero drive, and the value must be a positive frequency.
#[test]
fn an_at_frequency_measure_is_validated() {
    let ac = "[analysis.ac]\nstart = 10\nstop = 1000";
    let op = "[analysis]\nop = true";
    for (label, analysis, measure) in [
        (
            "node_voltage under op",
            op,
            "[[measure]]\nname = \"g\"\nkind = \"node_voltage\"\nnode = \"MID\"\nat_frequency = 100.0",
        ),
        (
            "relative_db",
            ac,
            "[[measure]]\nname = \"f\"\nkind = \"relative_db\"\nnode = \"MID\"\nat_frequency = 100.0",
        ),
        (
            "current",
            op,
            "[[measure]]\nname = \"i\"\nkind = \"current\"\nelement = \"R1\"\nat_frequency = 100.0",
        ),
        (
            "zero frequency",
            ac,
            "[[measure]]\nname = \"g\"\nkind = \"node_voltage\"\nnode = \"MID\"\nat_frequency = 0.0",
        ),
        (
            "negative frequency",
            ac,
            "[[measure]]\nname = \"g\"\nkind = \"node_voltage\"\nnode = \"MID\"\nat_frequency = -1.0",
        ),
    ] {
        let error = synth_sim::Testbench::from_toml(&tb_with(analysis, measure)).expect_err(label);
        assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007, "{label}");
    }

    let no_drive = tb_with(
        ac,
        "[[measure]]\nname = \"g\"\nkind = \"node_voltage\"\nnode = \"MID\"\nat_frequency = 100.0",
    );
    let error = synth_sim::Testbench::from_toml(&no_drive).expect_err("an AC drive is required");
    assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007);
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|message| message.contains("non-zero `ac`")),
        "{error:?}"
    );
}

/// ngspice only reports an out-of-range `at` as `find(AT) : out of interval`.
/// Catch it at load time and name the measure and the sweep instead.
#[test]
fn an_at_frequency_outside_the_sweep_gives_007() {
    let ac = "[analysis.ac]\nstart = 10\nstop = 1000";
    for (label, frequency) in [("above stop", 2000.0), ("below start", 1.0)] {
        let text = tb_with(
            ac,
            &format!(
                "[[measure]]\nname = \"g\"\nkind = \"node_voltage\"\nnode = \"MID\"\n\
                 at_frequency = {frequency}"
            ),
        )
        .replace("dc = 5.0", "dc = 5.0\nac = 1.0");
        let error = synth_sim::Testbench::from_toml(&text).expect_err(label);
        assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007, "{label}");
        assert!(
            error
                .message
                .as_deref()
                .is_some_and(|message| message.contains("`g`")
                    && message.contains("outside the ac sweep 10-1000 Hz")),
            "{label}: {error:?}"
        );
    }
}

/// A current measure names the Synth part; the deck saves and reads the
/// emitted device current vector.
#[test]
fn emits_a_device_current_measure() {
    let board = board(LED);
    let testbench = testbench(CURRENT_TB);

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");

    assert!(deck.contains("save all @DLED1[id]"), "deck:\n{deck}");
    assert!(deck.contains("let m0 = @DLED1[id]"), "deck:\n{deck}");
    assert!(deck.contains("echo SYNTH_MEAS iled $&m0"), "deck:\n{deck}");
}

fn tb_with(analysis: &str, measure: &str) -> String {
    format!(
        "ground = \"GND\"\n\n[[source]]\nnet = \"VCC\"\nkind = \"dc\"\ndc = 5.0\n\n{analysis}\n\n{measure}\n"
    )
}

#[test]
fn a_relative_db_measure_requires_the_ac_analysis() {
    let error = synth_sim::Testbench::from_toml(&tb_with(
        "[analysis]\nop = true",
        "[[measure]]\nname = \"f\"\nkind = \"relative_db\"\nnode = \"MID\"",
    ))
    .expect_err("relative_db needs ac");
    assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007);
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|message| message.contains("ac")),
        "{error:?}"
    );
    let error = synth_sim::Testbench::from_toml(&tb_with(
        "[analysis.ac]\nstart = 10\nstop = 1000",
        "[[measure]]\nname = \"v\"\nkind = \"node_voltage\"\nnode = \"MID\"",
    ))
    .expect_err("node_voltage needs op");
    assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007);
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|message| message.contains("op")),
        "{error:?}"
    );
}

/// Without an AC source there is nothing to sweep; say so before the run
/// instead of reporting a missing measurement afterwards.
#[test]
fn a_relative_db_measure_requires_a_source_with_ac() {
    let error = synth_sim::Testbench::from_toml(&tb_with(
        "[analysis.ac]\nstart = 10\nstop = 1000",
        "[[measure]]\nname = \"f\"\nkind = \"relative_db\"\nnode = \"MID\"",
    ))
    .expect_err("relative_db needs an ac source");
    assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007);
    assert!(
        error.message.as_deref().is_some_and(
            |message| message.contains("relative_db needs a source with a non-zero `ac`")
        ),
        "{error:?}"
    );
}

#[test]
fn a_relative_db_measure_requires_a_non_zero_ac_source() {
    let testbench = tb_with(
        "[analysis.ac]\nstart = 10\nstop = 1000",
        "[[measure]]\nname = \"f\"\nkind = \"relative_db\"\nnode = \"MID\"",
    )
    .replace("dc = 5.0", "dc = 5.0\nac = 0.0");
    let error = synth_sim::Testbench::from_toml(&testbench).expect_err("ac = 0 is no sweep");
    assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007);
    assert!(
        error.message.as_deref().is_some_and(
            |message| message.contains("relative_db needs a source with a non-zero `ac`")
        ),
        "{error:?}"
    );
}

#[test]
fn a_measure_target_must_match_its_kind() {
    let op = "[analysis]\nop = true";
    let ac = "[analysis.ac]\nstart = 10\nstop = 1000";
    for (label, analysis, measure) in [
        (
            "node_voltage with element",
            op,
            "[[measure]]\nname = \"m\"\nkind = \"node_voltage\"\nnode = \"MID\"\nelement = \"R1\"",
        ),
        (
            "node_voltage with drop_db",
            op,
            "[[measure]]\nname = \"m\"\nkind = \"node_voltage\"\nnode = \"MID\"\ndrop_db = 3.0",
        ),
        (
            "node_voltage without node",
            op,
            "[[measure]]\nname = \"m\"\nkind = \"node_voltage\"",
        ),
        (
            "relative_db with element",
            ac,
            "[[measure]]\nname = \"m\"\nkind = \"relative_db\"\nnode = \"MID\"\nelement = \"R1\"",
        ),
        (
            "relative_db with zero drop",
            ac,
            "[[measure]]\nname = \"m\"\nkind = \"relative_db\"\nnode = \"MID\"\ndrop_db = 0.0",
        ),
        (
            "current without element",
            op,
            "[[measure]]\nname = \"m\"\nkind = \"current\"",
        ),
        (
            "current with node",
            op,
            "[[measure]]\nname = \"m\"\nkind = \"current\"\nelement = \"R1\"\nnode = \"MID\"",
        ),
        (
            "current with drop_db",
            op,
            "[[measure]]\nname = \"m\"\nkind = \"current\"\nelement = \"R1\"\ndrop_db = 3.0",
        ),
    ] {
        let error = synth_sim::Testbench::from_toml(&tb_with(analysis, measure)).expect_err(label);
        assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007, "{label}");
    }
}

#[test]
fn ac_analysis_and_source_values_are_validated() {
    for (label, analysis) in [
        ("ac start zero", "[analysis.ac]\nstart = 0\nstop = 1000"),
        (
            "ac stop before start",
            "[analysis.ac]\nstart = 1000\nstop = 100",
        ),
        (
            "ac zero points",
            "[analysis.ac]\nstart = 10\nstop = 1000\npoints_per_decade = 0",
        ),
        (
            "ac too many points",
            "[analysis.ac]\nstart = 10\nstop = 1000\npoints_per_decade = 1001",
        ),
        ("both analyses", "[analysis]\nop = true\nac = {}"),
        ("no analysis", "[analysis]"),
    ] {
        let error = synth_sim::Testbench::from_toml(&tb_with(
            analysis,
            "[[measure]]\nname = \"v\"\nkind = \"node_voltage\"\nnode = \"MID\"",
        ))
        .expect_err(label);
        assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007, "{label}");
    }

    for (label, source) in [
        (
            "no dc or ac",
            "[[source]]\nnet = \"VCC\"\nkind = \"dc\"\n\n[analysis]\nop = true\n\n[[measure]]\nname = \"v\"\nkind = \"node_voltage\"\nnode = \"MID\"",
        ),
        (
            "negative ac",
            "[[source]]\nnet = \"VCC\"\nkind = \"dc\"\nac = -1\n\n[analysis.ac]\nstart = 10\nstop = 1000\n\n[[measure]]\nname = \"f\"\nkind = \"relative_db\"\nnode = \"MID\"",
        ),
    ] {
        let text = format!("ground = \"GND\"\n\n{source}\n");
        let error = synth_sim::Testbench::from_toml(&text)
            .expect_err(label);
        assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007, "{label}");
    }
}

#[test]
fn a_current_measure_rejects_an_ignored_unknown_or_subcircuit_element() {
    let unknown = testbench(&CURRENT_TB.replace("element = \"LED1\"", "element = \"LED2\""));
    let diagnostics = synth_sim::emit_netlist(&board(LED), &unknown).expect_err("unknown refdes");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_007
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("LED2"))
        }),
        "diagnostics: {diagnostics:?}"
    );

    let ignored = testbench(&CURRENT_TB.replacen(
        "ground = \"GND\"\n",
        "ground = \"GND\"\nignore = [\"LED1\"]\n",
        1,
    ));
    let diagnostics = synth_sim::emit_netlist(&board(LED), &ignored).expect_err("ignored element");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_007
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("LED1") && message.contains("ignore"))
        }),
        "diagnostics: {diagnostics:?}"
    );

    let ldo = testbench(&CURRENT_TB.replace("element = \"LED1\"", "element = \"U1\""));
    let diagnostics =
        synth_sim::emit_netlist_with_registry_dirs(&board(LDO), &ldo, &[registry_parts_dir()])
            .expect_err("subcircuit element");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == synth_sim::E_SYNTH_SIM_007
                && diagnostic.message.as_deref().is_some_and(|message| {
                    message.contains("U1") && message.contains("not supported")
                })
        }),
        "diagnostics: {diagnostics:?}"
    );
}

fn rc_with_plot(plot: &str) -> String {
    RC_TB.replacen(
        "ground = \"GND\"\n",
        &format!("ground = \"GND\"\n\n[options]\nplot = \"{plot}\"\n"),
        1,
    )
}

#[test]
fn emits_a_single_quoted_wrdata_line_before_the_done_sentinel() {
    let scratch = Scratch::new("plot-deck");
    let plot = scratch.path().join("nested/wave.dat");
    let testbench = testbench(&rc_with_plot(&plot.display().to_string()));

    let deck = synth_sim::emit_netlist(&board(RC), &testbench).expect("deck emits");

    let line = format!("wrdata '{}' allv", plot.display());
    let position = deck.find(&line).unwrap_or_else(|| {
        panic!("deck has no `{line}`:\n{deck}");
    });
    assert!(
        deck.find("echo SYNTH_MEAS corner").expect("measure") < position,
        "wrdata must follow the measures:\n{deck}"
    );
    assert!(
        position < deck.find("echo SYNTH_DONE").expect("done"),
        "wrdata must precede SYNTH_DONE:\n{deck}"
    );
}

/// The output directory is created before ngspice starts; a missing binary
/// still leaves the resolved relative directory in place.
#[test]
fn simulate_creates_the_plot_directory_before_running_ngspice() {
    let scratch = Scratch::new("plot-dir");
    let sidecar = scratch.path().join("rc.sim.toml");
    std::fs::write(&sidecar, rc_with_plot("deep/nested/wave.dat")).expect("write sidecar");
    let testbench = synth_sim::Testbench::from_path(&sidecar).expect("sidecar parses");
    let options = synth_sim::SimOptions {
        ngspice: Some(scratch.path().join("ngspice-does-not-exist")),
        design_path: Some(PathBuf::from("rc.synth")),
        testbench_path: Some(sidecar),
        registry_dirs: Vec::new(),
    };

    let report = synth_sim::simulate(&board(RC), &testbench, &options);

    assert!(!report.passed());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_001),
        "diagnostics: {:?}",
        report.diagnostics
    );
    assert!(
        scratch.path().join("deep/nested").is_dir(),
        "the plot directory must exist before ngspice runs"
    );
}

#[test]
fn plot_validation_gives_007() {
    for (label, plot) in [
        ("empty", String::new()),
        ("single quote", "it's/wave.dat".to_string()),
        ("newline", "a\nb".to_string()),
    ] {
        let error =
            synth_sim::Testbench::from_toml(&rc_with_plot(&plot)).expect_err("invalid plot path");
        assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007, "{label}");
    }

    let op = tb_with(
        "[analysis]\nop = true",
        "[[measure]]\nname = \"v\"\nkind = \"node_voltage\"\nnode = \"MID\"",
    )
    .replacen(
        "ground = \"GND\"\n",
        "ground = \"GND\"\n\n[options]\nplot = \"wave.dat\"\n",
        1,
    );
    let error = synth_sim::Testbench::from_toml(&op).expect_err("plot needs ac");
    assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007);
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|message| message.contains("ac")),
        "{error:?}"
    );
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
    let file = path.display().to_string();
    let parsed = synth_parser::parse(&source, file.clone());
    assert!(
        parsed.diagnostics.is_empty(),
        "parse: {:?}",
        parsed.diagnostics
    );
    let ast = parsed.ast.expect("ast");
    let root = path.parent().expect("design parent").to_path_buf();
    let resolved = synth_ir::resolve_imports(&ast, &synth_ir::FsImportLoader { root }, &file);
    assert!(
        resolved
            .diagnostics
            .iter()
            .all(|d| !d.severity.is_blocking()),
        "imports: {:?}",
        resolved.diagnostics
    );
    let registry = synth_registry::embedded_registry().clone();
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

fn example_options(design: &Path, testbench: &Path, ngspice: &Path) -> synth_sim::SimOptions {
    synth_sim::SimOptions {
        ngspice: Some(ngspice.to_path_buf()),
        design_path: Some(design.to_path_buf()),
        testbench_path: Some(testbench.to_path_buf()),
        registry_dirs: Vec::new(),
    }
}

/// V_MID = VCC * R2/(R1+R2) = 12 * 10k/(10k+10k) = 6 V.
#[test]
fn divider_example_matches_real_ngspice() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let design = example_dir("divider").join("divider.synth");
    let sidecar = synth_sim::sidecar_path(&design);
    let board = board_from_file(&design);
    let testbench = synth_sim::Testbench::from_path(&sidecar).expect("example sidecar");

    let report = synth_sim::simulate(
        &board,
        &testbench,
        &example_options(&design, &sidecar, &ngspice),
    );

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let vmid = report
        .measures
        .iter()
        .find(|measure| measure.name == "vmid")
        .expect("vmid")
        .value;
    assert!(
        (vmid - 6.0).abs() < 1e-3,
        "divider midpoint {vmid} should be 6 V"
    );
}

/// fc = 1/(2*pi*R*C) = 1/(2*pi*1k*100n) = 1591.55 Hz; the -3.0 dB point of
/// this single pole is 1587.8 Hz, so the measured corner is within 2% of fc.
#[test]
fn rc_filter_example_matches_real_ngspice() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let design = example_dir("rc_filter").join("rc_filter.synth");
    let sidecar = synth_sim::sidecar_path(&design);
    let board = board_from_file(&design);
    let testbench = synth_sim::Testbench::from_path(&sidecar).expect("example sidecar");

    let report = synth_sim::simulate(
        &board,
        &testbench,
        &example_options(&design, &sidecar, &ngspice),
    );

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let corner = report
        .measures
        .iter()
        .find(|measure| measure.name == "corner")
        .expect("corner")
        .value;
    let fc = 1.0 / (2.0 * std::f64::consts::PI * 1e3 * 100e-9);
    assert!(
        (corner - fc).abs() / fc < 0.02,
        "RC corner {corner} Hz differs from fc {fc} Hz by more than 2%"
    );
}

/// I = (Vs - Vf)/R = (5 - 2.0)/150 = 20 mA.
#[test]
fn led_example_matches_real_ngspice() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let design = example_dir("led").join("led.synth");
    let sidecar = synth_sim::sidecar_path(&design);
    let board = board_from_file(&design);
    let testbench = synth_sim::Testbench::from_path(&sidecar).expect("example sidecar");

    let report = synth_sim::simulate(
        &board,
        &testbench,
        &example_options(&design, &sidecar, &ngspice),
    );

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let iled = report
        .measures
        .iter()
        .find(|measure| measure.name == "iled")
        .expect("iled")
        .value;
    let expected = (5.0 - 2.0) / 150.0;
    assert!(
        (iled - expected).abs() / expected < 0.02,
        "LED current {iled} A differs from {expected} A by more than 2%"
    );
}

/// The RC example, copied to scratch with a plot option, writes a real
/// waveform file next to its sidecar.
#[test]
fn a_real_ngspice_run_writes_a_non_empty_plot() {
    let Some(ngspice) = real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let scratch = Scratch::new("rc-plot");
    let design = scratch.path().join("rc_filter.synth");
    std::fs::copy(example_dir("rc_filter").join("rc_filter.synth"), &design).expect("copy design");
    let sidecar_text = std::fs::read_to_string(synth_sim::sidecar_path(
        &example_dir("rc_filter").join("rc_filter.synth"),
    ))
    .expect("example sidecar");
    let sidecar = scratch.path().join("rc_filter.sim.toml");
    std::fs::write(
        &sidecar,
        sidecar_text.replacen(
            "ground = \"GND\"\n",
            "ground = \"GND\"\n\n[options]\nplot = \"wave.dat\"\n",
            1,
        ),
    )
    .expect("write sidecar");
    let board = board_from_file(&design);
    let testbench = synth_sim::Testbench::from_path(&sidecar).expect("copied sidecar");

    let report = synth_sim::simulate(
        &board,
        &testbench,
        &example_options(&design, &sidecar, &ngspice),
    );

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    let plot = scratch.path().join("wave.dat");
    let size = std::fs::metadata(&plot).expect("plot file exists").len();
    assert!(size > 0, "plot file is empty");
}

#[test]
fn simulate_creates_an_absolute_plot_directory_before_the_run() {
    let scratch = Scratch::new("plot-abs-dir");
    let plot = scratch.path().join("absolute/nested/wave.dat");
    let sidecar = scratch.path().join("rc.sim.toml");
    std::fs::write(&sidecar, rc_with_plot(&plot.display().to_string())).expect("write sidecar");
    let testbench = synth_sim::Testbench::from_path(&sidecar).expect("sidecar parses");
    let options = synth_sim::SimOptions {
        ngspice: Some(scratch.path().join("ngspice-does-not-exist")),
        design_path: Some(PathBuf::from("rc.synth")),
        testbench_path: Some(sidecar),
        registry_dirs: Vec::new(),
    };

    let report = synth_sim::simulate(&board(RC), &testbench, &options);

    assert!(!report.passed());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == synth_sim::E_SYNTH_SIM_001),
        "diagnostics: {:?}",
        report.diagnostics
    );
    assert!(
        plot.parent().expect("plot parent").is_dir(),
        "the absolute plot directory must exist before ngspice runs"
    );
}
