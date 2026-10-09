// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

const SYNTH: &str = env!("CARGO_BIN_EXE_synth");

#[derive(Debug, Deserialize)]
struct Matrix {
    schema_version: String,
    board: Vec<BoardSpec>,
}

#[derive(Debug, Deserialize)]
struct BoardSpec {
    id: String,
    path: String,
    class: String,
    status: String,
    expect: String,
    /// Expected result of the manufacturing stage, which asks a different
    /// question from the source/DRC gate: a board can be electrically sound
    /// and still be unable to produce a fab package. Only consulted when the
    /// run includes `--fab`.
    #[serde(default)]
    expect_fab: Option<String>,
    #[serde(default)]
    expect_source_codes: Vec<String>,
    #[serde(default)]
    rationale: String,
}

#[derive(Debug, Serialize)]
struct BoardResult {
    id: String,
    class: String,
    declared_status: String,
    expected: String,
    actual: String,
    /// The source/DRC verdict on its own. Under `--fab` this is what `expect`
    /// is compared against, because `actual` also folds in manufacturing.
    gate: String,
    source: String,
    drc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    manufacturing: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    manufacturing_reason: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    manufacturing_stderr: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_codes: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    deterministic_artifacts: BTreeMap<String, String>,
    agrees: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    disagreements: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Results {
    schema_version: String,
    matrix_schema_version: String,
    kicad_version: String,
    registry_manifest_sha256: String,
    /// Which engine laid the copper these results describe.
    router: String,
    fab: bool,
    boards: Vec<BoardResult>,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load_matrix() -> Matrix {
    let path = workspace_root().join("qualification").join("matrix.toml");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn kicad_version() -> String {
    let binary = std::env::var("KICAD_CLI").unwrap_or_else(|_| "kicad-cli".to_string());
    Command::new(binary)
        .arg("version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .map(|l| l.trim().to_string())
        })
        .unwrap_or_else(|| "not installed".to_string())
}

/// SHA-256 over the registry manifest, so a part edit that changes a board's
/// result is attributable rather than mysterious.
fn registry_manifest_sha256() -> String {
    use std::fmt::Write as _;
    let output = Command::new(SYNTH)
        .arg("registry")
        .arg("manifest")
        .current_dir(workspace_root())
        .output()
        .expect("run synth registry manifest");
    if !output.status.success() {
        return "unavailable".to_string();
    }
    let mut out = String::with_capacity(64);
    for byte in <sha2::Sha256 as sha2::Digest>::digest(&output.stdout) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn stage_status(report: &serde_json::Value, stage: &str) -> String {
    report["stages"][stage]["status"]
        .as_str()
        .unwrap_or("absent")
        .to_string()
}

/// The source/DRC verdict, which is what `synth check` reports on its own.
///
/// `synth check` computes its overall status as `source && drc && (!fab ||
/// manufacturing)`, so under `--fab` the overall status answers a different
/// question from the one `expect` asks. Both stages must pass here, and
/// `unknown` is not a pass: a DRC that could not be performed has not cleared
/// the board.
fn gate_verdict(report: &serde_json::Value) -> String {
    let passed = stage_status(report, "source") == "pass" && stage_status(report, "drc") == "pass";
    if passed { "pass" } else { "fail" }.to_string()
}

fn source_codes(report: &serde_json::Value) -> Vec<String> {
    report["stages"]["source"]["result"]["diagnostics"]
        .as_array()
        .map(|d| {
            let mut codes: Vec<String> = d
                .iter()
                .filter(|x| x["severity"] == "error" || x["severity"] == "fatal")
                .filter_map(|x| x["code"].as_str().map(str::to_string))
                .collect();
            codes.sort();
            codes.dedup();
            codes
        })
        .unwrap_or_default()
}

/// Artifact hashes for the files that are byte-deterministic.
///
/// `kicad-cli` stamps per-run timestamps into Gerber headers, so those are
/// recorded by `synth check` but deliberately not compared here: pinning them
/// would make the matrix fail at random. The schematic, PCB and BOM are
/// IR-driven and stable, so they are the drift detector.
fn deterministic_artifacts(report: &serde_json::Value) -> BTreeMap<String, String> {
    const STABLE: [&str; 3] = [".kicad_sch", ".kicad_pcb", "bom.csv"];
    report["stages"]["manufacturing"]["artifacts"]
        .as_object()
        .map(|a| {
            a.iter()
                .filter(|(path, _)| STABLE.iter().any(|s| path.ends_with(s)))
                .filter_map(|(path, hash)| hash.as_str().map(|h| (path.clone(), h.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Why the manufacturing stage failed.
///
/// `synth check` reports the stage's status but not the export's stderr, and
/// an export that bails early — on a part with no real footprint, say — never
/// reaches the evidence report either. So on a failure the export is re-run
/// directly and its complaint recorded, which is the difference between a
/// result a reviewer can act on and one that only says "fail".
fn diagnose_manufacturing(root: &Path, board: &Path) -> Vec<String> {
    let out = root
        .join("target")
        .join("qualification")
        .join("diagnose")
        .join(board.file_stem().unwrap_or_default());
    let mut command = Command::new(SYNTH);
    command
        .arg("export-kicad")
        .arg(board)
        .arg("--out")
        .arg(&out)
        .arg("--gerbers")
        .arg("--drill")
        .arg("--validate-erc");
    // The same engine the stage used. Without this the re-run falls back to
    // the default, fails for want of a router, and reports that instead of
    // whatever actually went wrong — which is worse than reporting nothing,
    // because it reads like a diagnosis.
    if let Some(router) = std::env::var_os("SYNTH_QUALIFY_ROUTER") {
        command.arg("--router").arg(router);
    }
    let output = command.current_dir(root).output();
    let Ok(output) = output else {
        return vec!["could not re-run export-kicad".to_string()];
    };
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            // The structural gate prints a header and then one indented line
            // per offending part. Keeping only the header told us a board was
            // refused but never which part did it.
            t.starts_with("error")
                || t.starts_with("warning: UNTRUSTED")
                || t.contains("refusing")
                || t.contains("E-SYNTH-QUAL-")
                || (l.starts_with("  ") && t.contains(": "))
        })
        .map(|l| l.trim().chars().take(300).collect::<String>())
        .take(12)
        .collect()
}

fn run_board(spec: &BoardSpec, fab: bool) -> BoardResult {
    let root = workspace_root();
    let board = root.join(&spec.path);
    assert!(
        board.is_file(),
        "{}: {} is not a file",
        spec.id,
        board.display()
    );

    let mut command = Command::new(SYNTH);
    command.arg("check").arg(&board).arg("--json");
    if fab {
        command.arg("--fab");
    }
    // Which engine routes the board. `synth check` defaults to freerouting,
    // which wants a JAR and a JVM; CI installs KiCadRoutingTools and selects
    // it here. This is not a detail: without an engine the DRC stage cannot
    // produce counts at all and reports `unknown`, which is not a pass, so
    // every board's gate fails for a reason that is not about the board.
    if let Some(router) = std::env::var_os("SYNTH_QUALIFY_ROUTER") {
        command.arg("--router").arg(router);
    }
    // A dense board's DRC runs a full place-and-route: the six-layer dual-USB
    // board takes about 50s on a release build, well past the 30s default, and
    // a stage that overruns reports `unknown` rather than what it found.
    // Honour an inherited value so CI can raise it further.
    if std::env::var_os("SYNTH_CHECK_DRC_TIMEOUT_SECS").is_none() {
        command.env("SYNTH_CHECK_DRC_TIMEOUT_SECS", "600");
    }
    let output = command
        .current_dir(&root)
        .output()
        .unwrap_or_else(|e| panic!("{}: run synth check: {e}", spec.id));

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "{}: check must emit JSON: {e}\nstderr:\n{}",
            spec.id,
            String::from_utf8_lossy(&output.stderr)
        )
    });

    let actual = report["status"].as_str().unwrap_or("absent").to_string();
    // `expect` describes the source/DRC gate; `expect_fab` describes the
    // manufacturing stage. Comparing `expect` against the command's overall
    // status would conflate them under `--fab`, where a board declaring
    // `expect = "pass"` and `expect_fab = "fail"` would contradict itself and
    // report a disagreement it had already declared.
    let gate = gate_verdict(&report);
    let codes = source_codes(&report);
    let mut disagreements = Vec::new();

    // "unestablished" means this board has not yet been observed in the
    // reference environment, which is CI with KiCad installed. Several rules —
    // E-SYNTH-PIN-001 among them — only run when footprints are readable, so a
    // result from a machine without KiCad is not a baseline. The observed value
    // is recorded either way; it is just not compared, because asserting a
    // guess is how a gate teaches people to ignore it.
    if spec.expect != "unestablished" && gate != spec.expect {
        disagreements.push(format!(
            "expected the source/DRC gate to {}, it reported {gate}",
            spec.expect
        ));
    }
    for code in &spec.expect_source_codes {
        if !codes.contains(code) {
            disagreements.push(format!(
                "expected source diagnostic {code}, which was not reported"
            ));
        }
    }

    let manufacturing = fab.then(|| stage_status(&report, "manufacturing"));
    let mut manufacturing_stderr = Vec::new();
    if let Some(actual_fab) = manufacturing.as_deref() {
        let expected_fab = spec.expect_fab.as_deref().unwrap_or(&spec.expect);
        // "unestablished" means no baseline exists yet for this board's
        // manufacturing stage. The observed value is still recorded, so the
        // next run's artifact says what to write down; it is just not compared,
        // because comparing against a guess is how a gate teaches people to
        // ignore it.
        if expected_fab != "unestablished" && actual_fab != expected_fab {
            disagreements.push(format!(
                "expected the manufacturing stage to {expected_fab}, it reported {actual_fab}"
            ));
        }
        if actual_fab != "pass" {
            manufacturing_stderr = diagnose_manufacturing(&root, &board);
        }
    }

    BoardResult {
        id: spec.id.clone(),
        class: spec.class.clone(),
        declared_status: spec.status.clone(),
        expected: spec.expect.clone(),
        source: stage_status(&report, "source"),
        drc: stage_status(&report, "drc"),
        manufacturing_reason: report["stages"]["manufacturing"]["reason"]
            .as_str()
            .map(str::to_string),
        manufacturing_stderr,
        manufacturing,
        source_codes: codes,
        deterministic_artifacts: deterministic_artifacts(&report),
        agrees: disagreements.is_empty(),
        actual,
        gate,
        disagreements,
    }
}

/// The release-blocking matrix.
///
/// Ignored by default: each board runs a full place-and-route, which takes
/// minutes, and the normal test shards should stay fast. CI runs it with
/// `--ignored` in its own job. Use a release build — a debug DRC on a real
/// board takes about ten times as long and overruns the stage budget.
#[test]
#[ignore = "release-gate matrix; run with --ignored in the qualification job"]
fn reference_boards_match_their_declared_qualification() {
    let matrix = load_matrix();
    assert_eq!(
        matrix.schema_version, "synth.qualification.v1",
        "unknown matrix schema"
    );
    assert!(
        matrix.board.len() >= 8,
        "the corpus must cover classes A-D with positive and negative cases, found {}",
        matrix.board.len()
    );

    let fab = std::env::var("SYNTH_QUALIFY_FAB").is_ok();
    let boards: Vec<BoardResult> = matrix
        .board
        .iter()
        .map(|spec| run_board(spec, fab))
        .collect();

    let results = Results {
        schema_version: "synth.qualification.results.v1".to_string(),
        matrix_schema_version: matrix.schema_version.clone(),
        kicad_version: kicad_version(),
        registry_manifest_sha256: registry_manifest_sha256(),
        router: std::env::var("SYNTH_QUALIFY_ROUTER")
            .unwrap_or_else(|_| "freerouting (default)".to_string()),
        fab,
        boards,
    };

    let out_dir = workspace_root().join("target").join("qualification");
    std::fs::create_dir_all(&out_dir).expect("create results dir");
    let out_path = out_dir.join("results.json");
    std::fs::write(
        &out_path,
        serde_json::to_string_pretty(&results).expect("serialize results"),
    )
    .expect("write results");
    eprintln!("qualification results: {}", out_path.display());

    for board in &results.boards {
        let classes = ["A", "B", "C", "D"];
        assert!(
            classes.contains(&board.class.as_str()),
            "{}: class {} is outside A-D",
            board.id,
            board.class
        );
        let statuses = ["qualified", "experimental", "unsupported"];
        assert!(
            statuses.contains(&board.declared_status.as_str()),
            "{}: status {} is not one of {statuses:?}",
            board.id,
            board.declared_status
        );
    }

    // A green job means "nothing drifted", not "these boards can be built".
    // Say so in the log, because the declared gaps are the whole reason the
    // class claims are not yet qualified, and a check that reads as success
    // while nine boards cannot produce a fab package is a check people learn
    // to ignore.
    if fab {
        let blocked: Vec<&BoardResult> = results
            .boards
            .iter()
            .filter(|b| b.manufacturing.as_deref().is_some_and(|m| m != "pass"))
            .filter(|b| b.gate == "pass")
            .collect();
        if !blocked.is_empty() {
            eprintln!(
                "declared gap: {} electrically sound board(s) cannot produce a fab package:",
                blocked.len()
            );
            for board in &blocked {
                eprintln!(
                    "  {} (class {}): manufacturing {}",
                    board.id,
                    board.class,
                    board.manufacturing.as_deref().unwrap_or("absent")
                );
            }
            eprintln!("see docs/qualification-handoff.md for what this blocks");
        }
    }

    let drifted: Vec<&BoardResult> = results.boards.iter().filter(|b| !b.agrees).collect();
    assert!(
        drifted.is_empty(),
        "the qualification matrix no longer matches reality. Every line below is \
         either a regression or a launch claim that grew without being declared; \
         update qualification/matrix.toml only once you know which.\n{}\nFull results: {}",
        drifted
            .iter()
            .map(|b| format!(
                "  {} (class {}, declared {}): {}",
                b.id,
                b.class,
                b.declared_status,
                b.disagreements.join("; ")
            ))
            .collect::<Vec<_>>()
            .join("\n"),
        out_path.display()
    );
}

/// Cheap enough for the normal shards: the corpus itself has to stay coherent
/// even when nobody runs the boards.
#[test]
fn the_matrix_is_well_formed() {
    let matrix = load_matrix();
    let root = workspace_root();
    let mut seen = std::collections::BTreeSet::new();

    for spec in &matrix.board {
        assert!(
            seen.insert(spec.id.clone()),
            "duplicate board id {}",
            spec.id
        );
        assert!(
            root.join(&spec.path).is_file(),
            "{}: {} does not exist",
            spec.id,
            spec.path
        );
        assert!(
            ["pass", "fail", "unestablished"].contains(&spec.expect.as_str()),
            "{}: expect must be pass, fail or unestablished, found {}",
            spec.id,
            spec.expect
        );
        assert!(
            !spec.rationale.trim().is_empty(),
            "{}: every board needs a rationale, so a later reader knows why it is in the corpus",
            spec.id
        );
        // Declared `unsupported` is what makes a board a negative test, not
        // `expect = "fail"`. A positive board's gate can fail at DRC with the
        // source stage clean, and then there are no source diagnostics to name.
        if spec.status == "unsupported" {
            assert!(
                !spec.expect_source_codes.is_empty(),
                "{}: a negative test must name the diagnostics it expects, or it \
                 passes for any reason at all",
                spec.id
            );
        }
        if spec.status == "unsupported" {
            assert_eq!(
                spec.expect, "fail",
                "{}: an unsupported board must be a negative test",
                spec.id
            );
        }
        if let Some(fab) = spec.expect_fab.as_deref() {
            assert!(
                ["pass", "fail", "unknown", "unestablished"].contains(&fab),
                "{}: expect_fab must be pass, fail, unknown or unestablished, found {fab}",
                spec.id
            );
            // The manufacturing stage runs whenever the *source* stage passes,
            // not when the whole gate does, so a board whose DRC fails still
            // produces a manufacturing verdict. Only a board that fails at
            // source never reaches it.
            if spec.expect == "fail" && !spec.expect_source_codes.is_empty() {
                assert_eq!(
                    fab, "unknown",
                    "{}: a board that fails the source gate never reaches manufacturing, so expect_fab can only be unknown",
                    spec.id
                );
            }
            if fab == "pass" {
                assert_eq!(
                    spec.expect, "pass",
                    "{}: manufacturing cannot pass unless the gate does",
                    spec.id
                );
            }
        }
    }

    for class in ["A", "B", "C", "D"] {
        assert!(
            matrix.board.iter().any(|b| b.class == class),
            "class {class} has no board in the corpus"
        );
    }
    assert!(
        matrix.board.iter().any(|b| b.expect == "pass"),
        "the corpus needs at least one positive case"
    );
    assert!(
        matrix.board.iter().any(|b| b.expect == "fail"),
        "the corpus needs at least one negative case"
    );
}
