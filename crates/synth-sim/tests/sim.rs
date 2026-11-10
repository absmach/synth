// SPDX-License-Identifier: Apache-2.0

//! Behaviour of the `simulate` seam: a compiled board plus its testbench
//! sidecar produces a deck, a run and a pass/fail report.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[path = "support/stub_build.rs"]
mod stub_build;

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
    let system = PathBuf::from("/usr/bin/ngspice");
    if system.is_file() {
        return Some(system);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("ngspice"))
        .find(|candidate| candidate.is_file())
}

fn options(stub: &Path) -> synth_sim::SimOptions {
    synth_sim::SimOptions {
        ngspice: Some(stub.to_path_buf()),
        design_path: Some(PathBuf::from("divider.synth")),
        testbench_path: Some(PathBuf::from("divider.sim.toml")),
    }
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
        eprintln!("skipping: ngspice is not installed (sudo apt install ngspice)");
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
        eprintln!("skipping: ngspice is not installed (sudo apt install ngspice)");
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
        eprintln!("skipping: ngspice is not installed (sudo apt install ngspice)");
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
