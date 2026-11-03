// SPDX-License-Identifier: Apache-2.0

//! End-to-end coverage of the external-routing pipeline, driven by stub
//! routers.
//!
//! The tests here never invoke FreeRouting or KiCadRoutingTools. They stand
//! in for them with shell scripts that reproduce the behaviours the
//! pipeline has to survive — a clean run, a partial route, a crash, a hang,
//! malformed output, and a router that exits `0` while writing no board.
//! That is deliberate: those are the cases that decide whether a board can
//! reach a fabricator, and they have to be reproducible in CI on a machine
//! with neither engine installed.
//!
//! The stubs stand in for the *engine*, never for the pipeline. The stub
//! receives the same argv the real helper would and writes a real
//! `.kicad_pcb`, so everything downstream — the topology comparison, the
//! union-find connectivity pass, the KiCad DRC gate, the artifact naming,
//! the run record — runs exactly as it does in production.
//!
//! The one thing that cannot be stubbed is KiCad itself. Where `kicad-cli`
//! is absent, the DRC gate reports itself unavailable and the run lands in
//! review rather than in `Routed`, which is the behaviour under test
//! anyway.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use synth_router::{
    contract::{FabricationPolicy, RouteRequest, RouterEngine, RouterLimits},
    result::RouteState,
};

/// Serialises the tests that touch the process environment.
///
/// The adapters resolve the engine through environment variables and the
/// stub engines are installed on `PATH`, so these tests are inherently
/// process-global. Cargo runs tests on parallel threads within one process,
/// so without this lock one test's stub `python3` would satisfy another
/// test's engine lookup.
fn serialised() -> MutexGuard<'static, ()> {
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A scratch directory removed when the guard drops.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("synth_router_it_{label}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir");
        Self { path }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.file(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent dir");
        }
        std::fs::write(&path, contents).expect("write fixture");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Clear every router environment variable, returning a guard that puts
/// them back.
fn without_router_environment() -> EngineEnvRestorer {
    EngineEnvRestorer::take()
}

struct EngineEnvRestorer {
    _lock: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    /// Empty data/home tree that hides a real KiCadRoutingTools install from
    /// the conventional-location fallback while the guard is held.
    empty_data: PathBuf,
}

impl EngineEnvRestorer {
    fn take() -> Self {
        let lock = serialised();
        let mut saved: Vec<(&'static str, Option<std::ffi::OsString>)> = Vec::new();
        for key in [
            "SYNTH_FREEROUTING_JAR",
            "SYNTH_FREEROUTING_JAVA",
            "KICAD_ROUTING_TOOLS_REPO",
            "SYNTH_KRT_PYTHON",
        ] {
            saved.push((key, std::env::var_os(key)));
            std::env::remove_var(key);
        }
        // The resolver falls back to `$XDG_DATA_HOME|$HOME/.local/share/synth/
        // kicad-routing-tools`; point both at an empty tree so a checkout that
        // happens to be installed on the machine running the tests does not
        // turn an "unavailable" expectation into a real run.
        let empty_data =
            std::env::temp_dir().join(format!("synth_router_empty_data_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&empty_data);
        let _ = std::fs::create_dir_all(&empty_data);
        for key in ["XDG_DATA_HOME", "HOME"] {
            saved.push((key, std::env::var_os(key)));
            std::env::set_var(key, &empty_data);
        }
        Self {
            _lock: lock,
            saved,
            empty_data,
        }
    }
}

impl Drop for EngineEnvRestorer {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        let _ = std::fs::remove_dir_all(&self.empty_data);
    }
}

fn kicad_cli_available() -> bool {
    which("kicad-cli").is_some()
}

fn which(binary: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}

// ── Board fixtures ───────────────────────────────────────────────────────────

/// A two-pad net routed end to end: the fully-routed board.
///
/// Written by hand so the reader's tests do not depend on the exporter's
/// output being correct.
const ROUTED_BOARD: &str = r#"(kicad_pcb
  (version 20260206)
  (generator "synth-eda")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "SIG")
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu") (at 10 20)
    (property "Reference" "R1" (at 0 -1) (layer "F.SilkS"))
    (pad "1" smd roundrect (at -0.825 0) (size 0.8 0.95)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "SIG")))
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu") (at 20 20)
    (property "Reference" "R2" (at 0 -1) (layer "F.SilkS"))
    (pad "1" smd roundrect (at 0.825 0) (size 0.8 0.95)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "SIG")))
  (segment (start 9.175 20) (end 20.825 20) (width 0.25) (layer "F.Cu") (net 1))
  (gr_line (start 0 0) (end 30 0) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 30 0) (end 30 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 30 30) (end 0 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 30) (end 0 0) (layer "Edge.Cuts") (width 0.1))
)
"#;

/// The same board with the connecting track missing: a partial route.
///
/// This is the case a router exit code cannot distinguish from success,
/// and it is the reason connectivity is re-derived from copper.
const PARTIAL_BOARD: &str = r#"(kicad_pcb
  (version 20260206)
  (generator "synth-eda")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "SIG")
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu") (at 10 20)
    (property "Reference" "R1" (at 0 -1) (layer "F.SilkS"))
    (pad "1" smd roundrect (at -0.825 0) (size 0.8 0.95)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "SIG")))
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu") (at 20 20)
    (property "Reference" "R2" (at 0 -1) (layer "F.SilkS"))
    (pad "1" smd roundrect (at 0.825 0) (size 0.8 0.95)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "SIG")))
  (gr_line (start 0 0) (end 30 0) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 30 0) (end 30 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 30 30) (end 0 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 30) (end 0 0) (layer "Edge.Cuts") (width 0.1))
)
"#;

/// The routed board with a footprint removed: a topology regression.
///
/// Copper that routes perfectly between the pads of a board that has lost a
/// part is still the wrong board, and only a comparison against the
/// baseline catches it.
const MISSING_FOOTPRINT_BOARD: &str = r#"(kicad_pcb
  (version 20260206)
  (generator "synth-eda")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "SIG")
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu") (at 10 20)
    (property "Reference" "R1" (at 0 -1) (layer "F.SilkS"))
    (pad "1" smd roundrect (at -0.825 0) (size 0.8 0.95)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "SIG")))
  (segment (start 9.175 20) (end 20.825 20) (width 0.25) (layer "F.Cu") (net 1))
  (gr_line (start 0 0) (end 30 0) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 30 0) (end 30 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 30 30) (end 0 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 30) (end 0 0) (layer "Edge.Cuts") (width 0.1))
)
"#;

/// A board carrying copper below the fabrication floor.
const UNDERSIZED_BOARD: &str = r#"(kicad_pcb
  (version 20260206)
  (generator "synth-eda")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "SIG")
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu") (at 10 20)
    (property "Reference" "R1" (at 0 -1) (layer "F.SilkS"))
    (pad "1" smd roundrect (at -0.825 0) (size 0.8 0.95)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "SIG")))
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu") (at 20 20)
    (property "Reference" "R2" (at 0 -1) (layer "F.SilkS"))
    (pad "1" smd roundrect (at 0.825 0) (size 0.8 0.95)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "SIG")))
  (segment (start 9.175 20) (end 20.825 20) (width 0.05) (layer "F.Cu") (net 1))
  (gr_line (start 0 0) (end 30 0) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 30 0) (end 30 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 30 30) (end 0 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 30) (end 0 0) (layer "Edge.Cuts") (width 0.1))
)
"#;

// ── Stub routers ─────────────────────────────────────────────────────────────

/// What a stub engine should do when invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    /// Write the given board and exit 0.
    Write(&'static str),
    /// Exit 0 and write nothing — the signature "success with no board".
    Silent,
    /// Exit non-zero without writing a board.
    Crash,
    /// Exit 0 after writing something that is not a board.
    Malformed,
    /// Sleep far longer than any budget the test sets.
    Hang,
}

struct StubRouter {
    /// Owns the stub script's directory so it is removed with the test.
    _scratch: Scratch,
    command: PathBuf,
}

impl StubRouter {
    /// Build a stub that behaves as requested.
    ///
    /// The script is installed as `python3` on `PATH` for the duration of
    /// the run, because the FreeRouting adapter invokes its helper through
    /// the interpreter. That keeps the production argv — helper, baseline,
    /// candidate, `--jar`, `--java`, … — exactly as it ships.
    fn new(label: &str, behaviour: Behaviour) -> Self {
        let scratch = Scratch::new(label);
        let command = scratch.file("bin/python3");
        std::fs::create_dir_all(command.parent().expect("bin dir")).expect("bin dir");

        let body = script_body(behaviour);
        std::fs::write(&command, body).expect("write stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&command).expect("stat").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&command, permissions).expect("chmod");
        }
        Self {
            _scratch: scratch,
            command,
        }
    }

    /// Install the stub for the duration of one run.
    ///
    /// Takes the serialisation lock and restores `PATH` and every router
    /// environment variable on drop, so a run cannot leak its stub into the
    /// next test.
    fn activate(&self, jar: &Path, java: &Path) -> EngineEnv {
        // The lock is taken once and moved into the guard: taking it again
        // inside `install` would self-deadlock on a non-reentrant mutex.
        EngineEnv::install(serialised(), &self.command, jar, java)
    }
}

/// The environment a stub engine runs under, restored on drop.
struct EngineEnv {
    _lock: MutexGuard<'static, ()>,
    path: Option<std::ffi::OsString>,
    keys: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl EngineEnv {
    fn install(lock: MutexGuard<'static, ()>, stub: &Path, jar: &Path, java: &Path) -> Self {
        let previous_path = std::env::var_os("PATH");
        let bin = stub.parent().expect("stub bin dir");
        let joined = match &previous_path {
            Some(existing) => {
                use std::os::unix::ffi::OsStrExt as _;
                format!("{}:{}", bin.display(), existing.as_bytes().escape_ascii())
            }
            None => bin.display().to_string(),
        };
        std::env::set_var("PATH", joined);

        let mut keys = Vec::new();
        for (key, value) in [
            ("SYNTH_FREEROUTING_JAR", Some(jar.to_path_buf())),
            ("SYNTH_FREEROUTING_JAVA", Some(java.to_path_buf())),
            // Cleared so a developer's own kicad-cli cannot bypass the stub.
            ("KICAD_CLI", None),
        ] {
            let saved = std::env::var_os(key);
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            keys.push((key, saved));
        }

        Self {
            _lock: lock,
            path: previous_path,
            keys,
        }
    }
}

impl Drop for EngineEnv {
    fn drop(&mut self) {
        for (key, value) in &self.keys {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        match &self.path {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }
}

/// A shell stub that reads the argv a real helper would and acts on it.
///
/// Written as a script rather than compiled so the behaviour under test is
/// readable in one place: the arguments are the production contract, and a
/// stub that hard-coded its own would not test it.
fn script_body(behaviour: Behaviour) -> String {
    let action = match behaviour {
        Behaviour::Write(_) => "route",
        Behaviour::Silent => "silent",
        Behaviour::Crash => "crash",
        Behaviour::Malformed => "malformed",
        Behaviour::Hang => "hang",
    };
    // Trailing newlines are trimmed because the heredoc supplies its own:
    // the installed board must be byte-identical to what the router wrote.
    let board = match behaviour {
        Behaviour::Write(board) => board.trim_end(),
        _ => "",
    };
    format!(
        r#"#!/bin/sh
# Stub external router. Parses the argv the production adapter builds:
#   <helper> <baseline> <candidate> --jar <jar> --java <java> ...
# Flag values are skipped, so the last positional really is the board this
# engine must write — and the board it is handed is the baseline, never the
# delivery path.
POSITIONAL=""
REPORT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --report) REPORT="$2"; shift 2 ;;
    --ses-output|--stats|--passes|--threads|--jar|--java|--repo|--python|--escalation|--fab-tier|--fab-overrides|--same-net-pad-clearance)
      shift 2
      ;;
    --*) shift ;;
    *) POSITIONAL="$1"; shift ;;
  esac
done

echo "stub-router argv: $*" >&2

if [ -n "$REPORT" ]; then
  printf '{{"router":"stub","jar":"stub-1.0.jar","java_version":"stub-jvm","imported_segments":1,"imported_vias":0,"notes":[]}}' > "$REPORT"
fi

case "{action}" in
  route)
    cat > "$POSITIONAL" <<'STUB_BOARD_EOF'
{board}
STUB_BOARD_EOF
    exit 0
    ;;
  silent)
    exit 0
    ;;
  crash)
    echo "stub router: segmentation fault (core dumped)" >&2
    exit 139
    ;;
  malformed)
    printf '(kicad_pcb (version 20260206) (footprint' > "$POSITIONAL"
    exit 0
    ;;
  hang)
    sleep 300
    exit 0
    ;;
esac
"#
    )
}

/// A request wired to a stub engine, with the baseline already preserved.
fn request_for(scratch: &Scratch, timeout_secs: u64) -> RouteRequest {
    let board = scratch.write("demo.kicad_pcb", PARTIAL_BOARD);
    let mut request = RouteRequest::new(&board, &scratch.path, RouterEngine::Freerouting);
    request.stem = "demo".to_string();
    request.limits = RouterLimits::default().with_wall_clock(timeout_secs);
    request.policy = FabricationPolicy::default();
    request.input_hash = "test-input-hash".to_string();
    request.profile_name = "jlc-standard".to_string();
    request.preserve_baseline().expect("baseline preserved");
    request
}

/// Stub files standing in for the JAR and JVM the adapter insists exist.
///
/// What is under test is the pipeline's reaction to the engine's
/// behaviour, so the "engine" itself is a file that exists and does
/// nothing. The adapter still has to find it before it will start the
/// helper, which is the part being exercised.
fn stub_engine_files(scratch: &Scratch) -> (PathBuf, PathBuf) {
    let jar = scratch.write("tools/freerouting/freerouting-stub.jar", "stub jar");
    let java = scratch.write("tools/jre25/bin/java", "stub java");
    (jar, java)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn a_clean_route_that_validates_reports_routed() {
    let scratch = Scratch::new("clean");
    let router = StubRouter::new("clean_router", Behaviour::Write(ROUTED_BOARD));
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let request = request_for(&scratch, 120);
    let report = synth_router::route(&request);
    assert_eq!(
        report.engine,
        RouterEngine::Freerouting,
        "the report must name the engine that ran"
    );
    assert_eq!(
        report.provenance.engine_version.as_deref(),
        Some("stub-1.0.jar"),
        "the engine's own version has to reach provenance"
    );
    assert!(
        report.provenance.runtime_version.is_some(),
        "the runtime version must be recorded too"
    );
    assert!(
        !report.provenance.command.is_empty(),
        "the exact argv has to be reproducible"
    );
    assert!(report.provenance.settings.contains_key("passes"));
    assert_eq!(
        report.provenance.input_hash, "test-input-hash",
        "the router input hash identifies the board the engine was given"
    );
    assert_eq!(report.statistics.segments, 1);
    assert_eq!(report.statistics.connected_nets, 1);
    assert_eq!(report.statistics.open_nets, 0);

    // A validated candidate is installed over the delivery path; the
    // baseline survives as the record of what was routed.
    assert_board(&scratch.file("demo.kicad_pcb"), ROUTED_BOARD);
    assert_board(&scratch.file("demo.synth.kicad_pcb"), PARTIAL_BOARD);
    assert!(
        !scratch.file("demo.freerouting.kicad_pcb").exists(),
        "the candidate is renamed into place, not left behind"
    );

    assert!(scratch.file("demo.routing.json").is_file());
    assert!(scratch.file("demo.freerouting.log").is_file());

    // The run record is the answer every layer reads, so it must say the
    // same thing the returned report says.
    let reread = synth_router::RouteReport::read_from(&scratch.file("demo.routing.json"))
        .expect("run record parses");
    assert_eq!(reread.state, report.state);
    assert_eq!(reread.state.as_str(), report.state.as_str());

    // Whether this reaches `routed` depends on KiCad being installed:
    // an unperformed DRC is review, never a pass.
    if kicad_cli_available() {
        assert_eq!(report.state, RouteState::Routed, "{:?}", report.validation);
        assert!(report.is_fabrication_ready());
    } else {
        assert_eq!(report.state, RouteState::ReviewRequired);
        assert!(!report.is_fabrication_ready());
    }
}

#[test]
fn a_partial_route_is_never_reported_as_a_success() {
    // The engine exits 0 and writes a board. The board is routable-looking
    // and is not routed. Only the connectivity pass catches this.
    let scratch = Scratch::new("partial");
    let router = StubRouter::new("partial_router", Behaviour::Write(PARTIAL_BOARD));
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let request = request_for(&scratch, 120);
    let report = synth_router::route(&request);
    assert_eq!(report.state, RouteState::ValidationFailed);
    assert!(!report.is_fabrication_ready());
    assert_eq!(report.statistics.open_nets, 1);
    assert_eq!(report.statistics.connected_nets, 0);

    let validation = report.validation.expect("validation ran");
    assert!(
        validation
            .connectivity
            .open
            .iter()
            .any(|open| open.net == "SIG" && open.pad_count == 2),
        "{:?}",
        validation.connectivity.open
    );
    assert!(
        validation
            .blocking_reasons
            .iter()
            .any(|reason| reason.contains("SIG")),
        "{:?}",
        validation.blocking_reasons
    );

    // The un-routed board stays the deliverable: an unvalidated candidate
    // is never promoted.
    assert_eq!(
        std::fs::read_to_string(scratch.file("demo.kicad_pcb")).unwrap(),
        PARTIAL_BOARD
    );
    assert!(
        scratch.file("demo.freerouting.kicad_pcb").is_file(),
        "a rejected candidate is retained for review rather than deleted"
    );
}

#[test]
fn a_router_that_drops_a_footprint_fails_topology() {
    // Every net on this board is routed; the board is still wrong because a
    // part is missing. Connectivity alone would call it clean.
    let scratch = Scratch::new("topology");
    let router = StubRouter::new("topology_router", Behaviour::Write(MISSING_FOOTPRINT_BOARD));
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let request = request_for(&scratch, 120);
    let report = synth_router::route(&request);
    assert_eq!(report.state, RouteState::ValidationFailed);
    let validation = report.validation.expect("validation ran");
    assert!(
        validation
            .topology
            .missing_footprints
            .contains(&"R2".to_string()),
        "{:?}",
        validation.topology
    );
    assert!(
        validation
            .blocking_reasons
            .iter()
            .any(|reason| reason.contains("R2")),
        "{:?}",
        validation.blocking_reasons
    );
}

#[test]
fn copper_below_the_fabrication_floor_is_rejected() {
    let scratch = Scratch::new("undersized");
    let router = StubRouter::new("undersized_router", Behaviour::Write(UNDERSIZED_BOARD));
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let request = request_for(&scratch, 120);
    let report = synth_router::route(&request);
    assert_eq!(report.state, RouteState::ValidationFailed);
    let validation = report.validation.expect("validation ran");
    assert!(
        !validation.connectivity.undersized_tracks.is_empty(),
        "{:?}",
        validation.connectivity
    );
    assert!(
        validation
            .blocking_reasons
            .iter()
            .any(|reason| reason.contains("fabrication minimum width")),
        "{:?}",
        validation.blocking_reasons
    );
}

#[test]
fn an_engine_that_exits_zero_without_writing_a_board_is_a_failure() {
    // FreeRouting's characteristic failure: a clean exit after routing
    // nothing at all.
    let scratch = Scratch::new("silent");
    let router = StubRouter::new("silent_router", Behaviour::Silent);
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let request = request_for(&scratch, 120);
    let report = synth_router::route(&request);
    assert_ne!(report.state, RouteState::Routed);
    assert!(!report.is_fabrication_ready());
    assert_eq!(
        report.failure_reason(),
        Some(synth_router::RouterFailureReason::NoOutput)
    );
    assert_board(&scratch.file("demo.kicad_pcb"), PARTIAL_BOARD);
}

#[test]
fn a_crashed_engine_leaves_the_baseline_and_the_log() {
    let scratch = Scratch::new("crash");
    let router = StubRouter::new("crash_router", Behaviour::Crash);
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let request = request_for(&scratch, 120);
    let report = synth_router::route(&request);
    assert_eq!(
        report.failure_reason(),
        Some(synth_router::RouterFailureReason::Crashed)
    );
    assert_eq!(
        report.failed_stage(),
        Some(synth_router::RouterStage::Route)
    );
    assert!(
        scratch.file("demo.freerouting.log").is_file(),
        "a crashed run must leave something to diagnose it"
    );
    assert_board(&scratch.file("demo.kicad_pcb"), PARTIAL_BOARD);
    assert_board(&scratch.file("demo.synth.kicad_pcb"), PARTIAL_BOARD);
}

#[test]
fn a_hung_engine_is_killed_at_the_budget() {
    let scratch = Scratch::new("hang");
    let router = StubRouter::new("hang_router", Behaviour::Hang);
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let started = std::time::Instant::now();
    let request = request_for(&scratch, 1);
    let report = synth_router::route(&request);
    let elapsed = started.elapsed();
    assert_eq!(
        report.failure_reason(),
        Some(synth_router::RouterFailureReason::Timeout)
    );
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "the budget must be enforced, not waited out: {elapsed:?}"
    );
    assert_board(&scratch.file("demo.synth.kicad_pcb"), PARTIAL_BOARD);
}

#[test]
fn an_unreadable_candidate_is_reported_as_an_unperformed_check() {
    // A malformed board must not degrade into an empty one: an empty board
    // passes every "is anything missing?" question.
    let scratch = Scratch::new("malformed");
    let router = StubRouter::new("malformed_router", Behaviour::Malformed);
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let request = request_for(&scratch, 120);
    let report = synth_router::route(&request);
    assert_ne!(report.state, RouteState::Routed);
    let validation = report.validation.expect("validation was attempted");
    assert!(
        !validation.unavailable_checks.is_empty(),
        "an unreadable board is a check that could not run: {validation:?}"
    );
    assert_board(&scratch.file("demo.kicad_pcb"), PARTIAL_BOARD);
}

#[test]
fn a_run_can_be_retried_after_a_failure_and_succeed() {
    // Recovery has to be idempotent: the same revision, re-run, produces the
    // routed board without any manual cleanup.
    let scratch = Scratch::new("recovery");
    let board = scratch.write("demo.kicad_pcb", PARTIAL_BOARD);
    let mut request = RouteRequest::new(&board, &scratch.path, RouterEngine::Freerouting);
    request.stem = "demo".to_string();
    request.limits = RouterLimits::default().with_wall_clock(120);
    request.profile_name = "jlc-standard".to_string();
    request.preserve_baseline().expect("baseline");

    let (jar, java) = stub_engine_files(&scratch);
    let failing = StubRouter::new("recovery_fail", Behaviour::Crash);
    {
        let _env = failing.activate(&jar, &java);
        let first = synth_router::route(&request);
        assert!(!first.is_fabrication_ready());
        assert_board(&scratch.file("demo.kicad_pcb"), PARTIAL_BOARD);
    }

    let succeeding = StubRouter::new("recovery_ok", Behaviour::Write(ROUTED_BOARD));
    {
        let _env = succeeding.activate(&jar, &java);
        let second = synth_router::route(&request);
        if kicad_cli_available() {
            assert_eq!(second.state, RouteState::Routed, "{second:?}");
        } else {
            assert_eq!(second.state, RouteState::ReviewRequired, "{second:?}");
        }
    }

    assert_board(&scratch.file("demo.kicad_pcb"), ROUTED_BOARD);
    assert_board(&scratch.file("demo.synth.kicad_pcb"), PARTIAL_BOARD);
}

/// The routed board with a ground pour that the router left unfilled.
fn routed_board_with_zone() -> &'static str {
    static BOARD: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BOARD.get_or_init(build_routed_board_with_zone)
}

fn build_routed_board_with_zone() -> String {
    let zone = r#"  (zone (net 1) (net_name "SIG") (layer "B.Cu") (uuid "zone-1")
    (fill yes (thermal_gap 0.2) (thermal_bridge_width 0.25))
    (polygon (pts (xy 1 1) (xy 29 1) (xy 29 29) (xy 1 29))))
"#;
    let body = ROUTED_BOARD.trim_end().trim_end_matches(')');
    format!("{body}{zone})\n")
}

/// A `kicad-cli` that reports a clean DRC and, when asked to save the board,
/// saves one whose zone is filled -- or fails, or is never expected to be
/// asked. Installed beside the stub engine so it shadows any real KiCad for
/// the one test that holds the environment lock.
fn install_kicad_stub(router: &StubRouter, save: &str) -> PathBuf {
    let marker = router.command.with_file_name("saved-a-board");
    let script = format!(
        r#"#!/bin/sh
[ "$1" = version ] && {{ echo 10.0.0; exit 0; }}
OUT=""; SAVE=0; BOARD=""
while [ $# -gt 0 ]; do
  case "$1" in
    --output) OUT="$2"; shift 2 ;;
    --save-board) SAVE=1; shift ;;
    --format) shift 2 ;;
    --*) shift ;;
    *) BOARD="$1"; shift ;;
  esac
done
printf '{{"violations": [], "unconnected_items": []}}' > "$OUT"
if [ "$SAVE" = 1 ]; then
  touch "{marker}"
  {save}
fi
exit 0
"#,
        marker = marker.display()
    );
    let path = router.command.with_file_name("kicad-cli");
    std::fs::write(&path, script).expect("write kicad stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    marker
}

const FILLED_SAVE: &str = r#"cat > "$BOARD" <<'SAVED_EOF'
(kicad_pcb (zone (uuid "zone-1")
  (filled_polygon (layer "B.Cu") (pts (xy 1 1) (xy 29 1) (xy 29 29) (xy 1 29)))))
SAVED_EOF"#;

#[test]
fn a_validated_board_is_delivered_with_its_zones_filled() {
    let scratch = Scratch::new("zonefill");
    let with_zone = routed_board_with_zone();
    let router = StubRouter::new("zonefill_router", Behaviour::Write(with_zone));
    let marker = install_kicad_stub(&router, FILLED_SAVE);
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let report = synth_router::route(&request_for(&scratch, 120));
    assert_eq!(report.state, RouteState::Routed, "{:?}", report.validation);
    assert!(marker.exists(), "the zones were never filled");

    let delivered = std::fs::read_to_string(scratch.file("demo.kicad_pcb")).expect("delivered");
    assert_eq!(delivered.matches("(filled_polygon").count(), 1);
    // Only the fill is added: the router's net table and copper are as written.
    assert!(delivered.contains(r#"(net 1 "SIG")"#));
    assert!(delivered.contains("(segment (start 9.175 20)"));
    assert_board(&scratch.file("demo.synth.kicad_pcb"), PARTIAL_BOARD);
}

#[test]
fn a_board_with_only_some_zones_filled_is_refilled_in_full() {
    let scratch = Scratch::new("zonefill_partial");
    // zone-1 carries a stale fill from before the final routing; zone-2 has none.
    let half_filled = routed_board_with_zone()
        .replace(
            "(polygon (pts (xy 1 1)",
            "(filled_polygon (layer \"B.Cu\") (pts (xy 5 5) (xy 6 5) (xy 6 6)))\n    (polygon (pts (xy 1 1)",
        )
        .trim_end()
        .trim_end_matches(')')
        .to_string()
        + "  (zone (net 1) (net_name \"SIG\") (layer \"B.Cu\") (uuid \"zone-2\")\n    (fill yes)\n    (polygon (pts (xy 1 1) (xy 9 1) (xy 9 9))))\n)\n";
    assert!(
        half_filled.contains("(xy 5 5)"),
        "the stale fill was not injected"
    );
    let router = StubRouter::new(
        "zonefill_partial_router",
        Behaviour::Write(Box::leak(half_filled.into_boxed_str())),
    );
    let marker = install_kicad_stub(
        &router,
        r#"cat > "$BOARD" <<'SAVED_EOF'
(kicad_pcb
  (zone (uuid "zone-1")
    (filled_polygon (layer "B.Cu") (pts (xy 1 1) (xy 29 1) (xy 29 29) (xy 1 29))))
  (zone (uuid "zone-2")
    (filled_polygon (layer "B.Cu") (pts (xy 2 2) (xy 8 2) (xy 8 8)))))
SAVED_EOF"#,
    );
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let report = synth_router::route(&request_for(&scratch, 120));
    assert_eq!(report.state, RouteState::Routed, "{:?}", report.validation);
    assert!(marker.exists(), "a half-filled board was not refilled");

    let delivered = std::fs::read_to_string(scratch.file("demo.kicad_pcb")).expect("delivered");
    assert_eq!(
        delivered.matches("(filled_polygon").count(),
        2,
        "{delivered}"
    );
    assert!(!delivered.contains("(xy 5 5)"), "stale copper survived");
}

/// The island repair needs the filled board, so a fill that fails means pour
/// islands cannot be checked. That is a required check that did not run: the
/// candidate is review-required and the un-routed baseline stays in place,
/// never a routed board delivered unchecked.
#[test]
fn a_fill_that_fails_blocks_the_board_because_islands_cannot_be_checked() {
    let scratch = Scratch::new("zonefill_fail");
    let with_zone = routed_board_with_zone();
    let router = StubRouter::new("zonefill_fail_router", Behaviour::Write(with_zone));
    let marker = install_kicad_stub(&router, "exit 1");
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let report = synth_router::route(&request_for(&scratch, 120));
    assert_eq!(
        report.state,
        RouteState::ReviewRequired,
        "{:?}",
        report.validation
    );
    let validation = report.validation.expect("validation ran");
    assert!(
        validation
            .unavailable_checks
            .iter()
            .any(|check| check.contains("pour island repair did not run")),
        "{:?}",
        validation.unavailable_checks
    );
    assert!(marker.exists());
    assert_board(&scratch.file("demo.kicad_pcb"), PARTIAL_BOARD);
}

#[test]
fn a_garbage_save_is_rejected_and_the_board_is_not_delivered() {
    let scratch = Scratch::new("zonefill_garbage");
    let with_zone = routed_board_with_zone();
    let router = StubRouter::new("zonefill_garbage_router", Behaviour::Write(with_zone));
    let marker = install_kicad_stub(&router, "echo '(not a board' > \"$BOARD\"");
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let report = synth_router::route(&request_for(&scratch, 120));
    assert_eq!(
        report.state,
        RouteState::ReviewRequired,
        "{:?}",
        report.validation
    );
    assert!(marker.exists());
    assert_board(&scratch.file("demo.kicad_pcb"), PARTIAL_BOARD);
    // The rejected save leaves no staged file beside the delivered path.
    let staged: Vec<_> = std::fs::read_dir(&scratch.path)
        .expect("scratch")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("staged") || name.to_ascii_lowercase().ends_with(".tmp"))
        .collect();
    assert!(staged.is_empty(), "staged files were left: {staged:?}");
}

/// Assert that `path` holds `expected`.
///
/// Compared as text: whether the delivered board is byte-identical to what
/// the router wrote is part of the contract, because the candidate that
/// gets installed has to be the one that was validated.
fn assert_board(path: &Path, expected: &str) {
    let actual = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()));
    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "{} does not hold the expected board",
        path.display()
    );
}

#[test]
fn the_input_hash_identifies_the_board_the_engine_was_given() {
    // The router reads the baseline, never the delivery path. Reading the
    // delivery path would let a previous run's copper masquerade as this
    // run's output.
    let scratch = Scratch::new("input_hash");
    let router = StubRouter::new("hash_router", Behaviour::Write(ROUTED_BOARD));
    let (jar, java) = stub_engine_files(&scratch);
    let _env = router.activate(&jar, &java);

    let mut request = request_for(&scratch, 120);
    request
        .input_hash
        .clone_from(&sha256_of(&request.baseline_path()));
    let baseline_hash = request.input_hash.clone();
    let report = synth_router::route(&request);
    assert_eq!(report.provenance.input_hash, baseline_hash);
    assert_ne!(
        baseline_hash,
        sha256_of(&scratch.file("demo.kicad_pcb")),
        "the recorded input hash must be the baseline, not the routed result"
    );
}

fn sha256_of(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    match std::fs::read(path) {
        Ok(bytes) => {
            let mut out = String::with_capacity(64);
            for byte in Sha256::digest(&bytes) {
                let _ = write!(out, "{byte:02x}");
            }
            out
        }
        Err(_) => "<unreadable>".to_string(),
    }
}

#[test]
fn kicad_routing_tools_reports_a_capability_error_when_no_checkout_is_configured() {
    // The optional engine must fail with a capability error naming what to
    // install — never fall back to FreeRouting, and never report a pass.
    let _restorer = without_router_environment();

    let scratch = Scratch::new("krt_absent");
    let board = scratch.write("demo.kicad_pcb", PARTIAL_BOARD);
    let request = RouteRequest::new(&board, &scratch.path, RouterEngine::KiCadRoutingTools);
    let report = synth_router::route(&request);

    assert_eq!(report.state, RouteState::RouterUnavailable);
    assert_eq!(
        report.failure_reason(),
        Some(synth_router::RouterFailureReason::CheckoutMissing)
    );
    assert_eq!(report.engine, RouterEngine::KiCadRoutingTools);
    let failure = report.failure.expect("a failure is recorded");
    assert!(
        failure.remediation.contains("kicad-routing-tools-repo"),
        "{}",
        failure.remediation
    );
    assert!(
        report.provenance.command.is_empty(),
        "no engine was started, so no argv should be claimed"
    );
}

#[test]
fn a_directory_without_the_krt_entrypoint_is_not_a_checkout() {
    // Serialised: the checkout is configured through the environment, so
    // this test cannot run beside one that clears it.
    let _restorer = without_router_environment();
    let scratch = Scratch::new("krt_bogus");
    std::fs::create_dir_all(scratch.file("fake/py_router")).expect("fake checkout");
    std::env::set_var("KICAD_ROUTING_TOOLS_REPO", scratch.file("fake"));

    let board = scratch.write("demo.kicad_pcb", PARTIAL_BOARD);
    let request = RouteRequest::new(&board, &scratch.path, RouterEngine::KiCadRoutingTools);
    let report = synth_router::route(&request);

    assert_eq!(report.state, RouteState::RouterUnavailable);
    let failure = report.failure.expect("a failure is recorded");
    assert!(
        failure.detail.contains("py_router/route.py"),
        "{}",
        failure.detail
    );
}

#[test]
fn every_engine_reports_its_availability_without_being_run() {
    // Discovery must be cheap and side-effect free: it starts nothing and
    // writes nothing, so it is safe to call before every routing attempt.
    let _restorer = without_router_environment();
    let scratch = Scratch::new("discovery");
    let capabilities = synth_router::capability::discover_all_in_out_dir(&scratch.path);
    assert_eq!(capabilities.len(), RouterEngine::all().len());
    assert_eq!(capabilities[0].engine, RouterEngine::default());
    for capability in &capabilities {
        assert_eq!(capability.available, capability.failure.is_none());
        if !capability.available {
            let failure = capability.failure().expect("unavailable carries a reason");
            assert!(failure.reason.is_unavailable());
            assert!(!failure.remediation.is_empty());
        }
    }
}

#[test]
fn a_second_run_against_the_same_candidate_is_refused_rather_than_racing() {
    let _env = without_router_environment();
    // Two routers writing one candidate is silent corruption: both succeed
    // and the last rename wins. The refusal is the fix.
    let scratch = Scratch::new("duplicate");
    let board = scratch.write("demo.kicad_pcb", PARTIAL_BOARD);
    let mut request = RouteRequest::new(&board, &scratch.path, RouterEngine::Freerouting);
    request.stem = "demo".to_string();
    request.preserve_baseline().expect("baseline");

    let guard =
        synth_router::process::RunGuard::acquire(request.candidate_path().display().to_string())
            .expect("first run holds the lock");
    let report = synth_router::route(&request);

    assert_eq!(report.state, RouteState::ValidationFailed);
    let failure = report.failure.expect("a failure is recorded");
    assert!(
        failure.detail.contains("already in progress"),
        "{}",
        failure.detail
    );

    drop(guard);
}

/// The CLI binary, when the workspace builds it alongside this test.
fn synth_cli() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("SYNTH_BIN") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    // `cargo test -p synth-router` does not build the CLI, so the binary is
    // looked up rather than assumed. The workspace-wide run does build it,
    // which is where this actually executes.
    let mut path = std::env::current_exe().ok()?;
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    let candidate = path.join("synth");
    candidate.is_file().then_some(candidate)
}

#[test]
fn the_cli_lists_every_engine_with_the_same_identifiers() {
    // Two surfaces must not disagree about which engines exist. The
    // identifier a caller passes to `synth_route` has to be the one
    // `synth routers` reports, or discovery is useless.
    let Some(synth) = synth_cli() else {
        return;
    };
    let output = Command::new(&synth)
        .arg("routers")
        .output()
        .expect("run synth routers");
    assert!(output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for engine in RouterEngine::all() {
        assert!(
            combined.contains(engine.as_str()),
            "`synth routers` must mention {}: {combined}",
            engine.as_str()
        );
    }
}

#[test]
fn an_unknown_engine_name_is_refused_rather_than_defaulted() {
    // The single most dangerous behaviour this contract prevents: an
    // unrecognised name silently routing with the default engine.
    assert_eq!(RouterEngine::parse("ngspice"), None);
    assert_eq!(RouterEngine::parse(""), None);
    assert_eq!(
        RouterEngine::parse("kicad-routing-tools"),
        Some(RouterEngine::KiCadRoutingTools)
    );
}
