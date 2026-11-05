// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

const SYNTH: &str = env!("CARGO_BIN_EXE_synth");

#[derive(Debug, Deserialize)]
struct Matrix {
    schema_version: String,
    baseline: Baseline,
    board: Vec<BoardSpec>,
}

/// What the whole corpus was measured against, pinned so a change to it fails
/// the pull request that made it.
#[derive(Debug, Deserialize)]
struct Baseline {
    /// SHA-256 of `synth registry manifest`, as [`registry_manifest_sha256`]
    /// computes it. Every board compiles against the registry, so a part edit
    /// can move any verdict or artifact below; it has to be declared here.
    registry_manifest_sha256: String,
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
    /// SHA-256 over the board's inputs, as computed by [`fixture_sha256`].
    /// The verdicts above are only meaningful for the fixture they were
    /// measured on, so a board whose source changes must be re-measured.
    #[serde(default)]
    fixture_sha256: String,
    /// Expected SHA-256 of each pinned artifact the manufacturing stage
    /// writes, keyed by its path in the export (see [`PINNED_ARTIFACTS`]).
    /// Compared exactly under `--fab`, in both directions.
    #[serde(default)]
    expect_artifacts: BTreeMap<String, String>,
    /// Expected normalized hash of each board file, plus the Gerbers and drill
    /// files as one entry each (see [`normalized_artifacts`]). Compared exactly
    /// under `--fab`, in both directions.
    #[serde(default)]
    expect_geometry: BTreeMap<String, String>,
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
    fixture_sha256: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    deterministic_artifacts: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    normalized_artifacts: BTreeMap<String, String>,
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

/// The files a board is compiled from: the `.synth` source and the layout
/// sidecars beside it (`<stem>.placement.layout.toml`,
/// `<stem>.schematic.layout.toml`), which move parts and so move results.
fn fixture_files(board: &Path) -> Vec<PathBuf> {
    let stem = board
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let sidecar_prefix = format!("{stem}.");
    let mut files = vec![board.to_path_buf()];
    if let Some(dir) = board.parent().and_then(|d| std::fs::read_dir(d).ok()) {
        files.extend(dir.filter_map(Result::ok).map(|e| e.path()).filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&sidecar_prefix) && n.ends_with(".layout.toml"))
        }));
    }
    files.sort();
    files
}

/// SHA-256 over a board's inputs, pinned per row as `fixture_sha256`.
///
/// This is the half of drift detection that can be pinned. The outputs cannot
/// (see [`deterministic_artifacts`]), but the inputs are ours: each file is
/// hashed under its name and length, so renaming, adding or deleting a sidecar
/// moves the hash as surely as editing one.
fn fixture_sha256(board: &Path) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    let mut hasher = sha2::Sha256::new();
    for file in fixture_files(board) {
        let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    let mut out = String::with_capacity(64);
    for byte in hasher.finalize() {
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

/// The artifacts whose hashes are pinned per board as `expect_artifacts`.
///
/// The schematic and the BOM are written by this compiler from the board and
/// the registry alone, so they are byte-stable: CI runs on different commits
/// produce the same hashes. A change to either is a change to what the board
/// is, and fails the fab job until someone declares it.
///
/// The bytes of a `.kicad_pcb` and of the Gerbers are not reproducible: the
/// routing engine mints UUIDs and writes tracks in its own order, and
/// `kicad-cli` stamps every Gerber with the time. What they describe is, so
/// those are pinned as `expect_geometry` once normalized (see
/// [`normalized_artifacts`]).
const PINNED_ARTIFACTS: [&str; 2] = [".kicad_sch", "bom.csv"];

fn is_pinned_artifact(path: &str) -> bool {
    PINNED_ARTIFACTS.iter().any(|s| path.ends_with(s))
}

/// Keys of `expect_geometry` that name a whole directory of the export rather
/// than a board file.
const FAB_OUTPUT_DIRS: [&str; 2] = ["gerbers", "drill"];

fn is_geometry_key(key: &str) -> bool {
    key.ends_with(".kicad_pcb") || FAB_OUTPUT_DIRS.contains(&key)
}

/// A board file reduced to what it describes, one top-level item per line.
///
/// UUIDs are minted per run. Tracks come out in whatever order the engine
/// finished them, and either way round, so items are sorted and each track's
/// ends are put in a fixed order. Zone fills are dropped: the same pour can list
/// an extra vertex along a straight edge depending on the order the tracks were
/// knocked out of it, and a fill is derived from the outline, tracks, pads and
/// clearances that remain in the hash.
fn normalized_pcb(text: &str) -> String {
    let mut items: Vec<String> =
        top_level_items(&without_forms(text, &["uuid", "tstamp", "filled_polygon"]))
            .into_iter()
            .map(|item| track_ends_in_order(&item))
            .collect();
    items.sort();
    items.join("\n")
}

/// S-expression text with whitespace collapsed and the named forms removed.
/// String literals pass through untouched.
fn without_forms(text: &str, dropped: &[&str]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    // The depth at which the form being skipped opened, while skipping one.
    let mut skipping: Option<usize> = None;
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        if in_string {
            if skipping.is_none() {
                out.push(c);
            }
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                if skipping.is_none() {
                    out.push(c);
                }
            }
            '(' => {
                depth += 1;
                if skipping.is_none() {
                    let rest = &text[i + 1..];
                    let name = rest
                        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
                        .next()
                        .unwrap_or_default();
                    if dropped.contains(&name) {
                        skipping = Some(depth);
                    } else {
                        out.push(c);
                    }
                }
            }
            ')' => {
                if skipping == Some(depth) {
                    skipping = None;
                } else if skipping.is_none() {
                    while out.ends_with(' ') {
                        out.pop();
                    }
                    out.push(c);
                }
                depth = depth.saturating_sub(1);
            }
            c if c.is_whitespace() => {
                if skipping.is_none() && !out.ends_with([' ', '(']) {
                    out.push(' ');
                }
            }
            c => {
                if skipping.is_none() {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// The direct children of the root form, as written. Expects the output of
/// [`without_forms`].
fn top_level_items(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' => {
                depth += 1;
                if depth == 2 {
                    start = i;
                }
            }
            ')' => {
                if depth == 2 {
                    items.push(text[start..=i].to_string());
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    items
}

/// A track or arc with its `start` and `end` swapped if need be, so the same
/// copper reads the same whichever way the engine drew it.
fn track_ends_in_order(item: &str) -> String {
    if !(item.starts_with("(segment ") || item.starts_with("(arc ")) {
        return item.to_string();
    }
    let point = |tag: &str| -> Option<(usize, usize)> {
        let at = item.find(tag)?;
        let close = at + item[at..].find(')')?;
        Some((at + tag.len(), close))
    };
    let (Some(start), Some(end)) = (point("(start "), point("(end ")) else {
        return item.to_string();
    };
    let (a, b) = (&item[start.0..start.1], &item[end.0..end.1]);
    if a <= b {
        return item.to_string();
    }
    format!(
        "{}{b}{}{a}{}",
        &item[..start.0],
        &item[start.1..end.0],
        &item[end.1..]
    )
}

/// A Gerber, job or drill file reduced to the set of things it puts on the
/// layer.
///
/// The header's time and tool version go, so a run or a KiCad patch release does
/// not read as a change of content. The lines are compared as a set because the
/// file follows the board's track order, and a draw's `D01`/`D02` is dropped
/// because it follows the track's direction. Copper regions are the same zone
/// fills [`normalized_pcb`] drops, for the same reason.
fn normalized_fab_output(name: &str, text: &str) -> String {
    const VOLATILE: [&str; 5] = [
        "CreationDate",
        "GenerationSoftware",
        "\"Version\"",
        "Created by",
        "DRILL file",
    ];
    let copper = name.ends_with("_Cu.gbr");
    let mut in_region = false;
    let mut lines = BTreeSet::new();
    for line in text.lines() {
        if copper && line == "G36*" {
            in_region = true;
        }
        if !in_region && !VOLATILE.iter().any(|v| line.contains(v)) {
            let line = line
                .strip_suffix("D01*")
                .or_else(|| line.strip_suffix("D02*"))
                .unwrap_or(line);
            lines.insert(line);
        }
        if line == "G37*" {
            in_region = false;
        }
    }
    lines.into_iter().collect::<Vec<_>>().join("\n")
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(64);
    for byte in <sha2::Sha256 as sha2::Digest>::digest(bytes) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The files directly inside `dir`, sorted by name.
fn sorted_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// Hashes of the physical result in an export directory, normalized so they
/// hold across runs: one per `.kicad_pcb`, and one each over the Gerbers and the
/// drill files.
///
/// Every board file is hashed, not just `<board>.kicad_pcb`. A board whose
/// routing fails validation exports the unrouted board under that name, and the
/// engine's copper survives only in the intermediates beside it.
fn normalized_artifacts(export: &Path) -> BTreeMap<String, String> {
    let read = |path: &Path| {
        String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).into_owned()
    };
    let name = |path: &Path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let mut hashes = BTreeMap::new();
    for file in sorted_files(export) {
        if name(&file).ends_with(".kicad_pcb") {
            hashes.insert(
                name(&file),
                sha256_hex(normalized_pcb(&read(&file)).as_bytes()),
            );
        }
    }
    for dir in FAB_OUTPUT_DIRS {
        let mut content = String::new();
        for file in sorted_files(&export.join(dir)) {
            // The PDF drill maps restate the drill files, with a timestamp in
            // the binary.
            if file.extension().is_some_and(|e| e == "pdf") {
                continue;
            }
            content.push_str(&name(&file));
            content.push('\n');
            content.push_str(&normalized_fab_output(&name(&file), &read(&file)));
            content.push('\n');
        }
        if !content.is_empty() {
            hashes.insert(dir.to_string(), sha256_hex(content.as_bytes()));
        }
    }
    hashes
}

/// Hashes of the schematic, board and BOM the manufacturing stage wrote.
///
/// The pinned ones (see [`PINNED_ARTIFACTS`]) are compared against
/// `expect_artifacts`. The raw `.kicad_pcb` hash is recorded for attribution;
/// [`normalized_artifacts`] is what guards the board.
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

    // What CI uploads, so a failure can be inspected without re-running it. A
    // previous run's files must not pass for this one's.
    let kept = root
        .join("target")
        .join("qualification")
        .join("boards")
        .join(&spec.id);
    let _ = std::fs::remove_dir_all(&kept);
    std::fs::create_dir_all(&kept)
        .unwrap_or_else(|e| panic!("{}: create {}: {e}", spec.id, kept.display()));

    let mut command = Command::new(SYNTH);
    command.arg("check").arg(&board).arg("--json");
    if fab {
        command.arg("--fab").arg("--artifacts-dir").arg(&kept);
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
    std::fs::write(kept.join("check.json"), &output.stdout)
        .unwrap_or_else(|e| panic!("{}: write the check report: {e}", spec.id));

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

    let fixture = fixture_sha256(&board);
    if fixture != spec.fixture_sha256 {
        disagreements.push(format!(
            "the board's inputs changed since its verdicts were measured \
             (fixture_sha256 is now {fixture})"
        ));
    }

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
    // The declared codes are the board's whole error-level source output, not
    // a minimum. A subset check would let an `unsupported` board acquire a new
    // defect and still pass, so long as the ones already written down kept
    // firing — which is the direction this matrix exists to catch. Codes that
    // appear and codes that vanish are both reported, because either means the
    // compiler now says something different about a board nobody re-declared.
    //
    // Only error-level codes are compared. `source_codes` already filters to
    // those, so a new warning does not fail the gate.
    let expected_codes: BTreeSet<&String> = spec.expect_source_codes.iter().collect();
    let observed_codes: BTreeSet<&String> = codes.iter().collect();
    for code in expected_codes.difference(&observed_codes) {
        disagreements.push(format!(
            "expected source diagnostic {code}, which was not reported"
        ));
    }
    for code in observed_codes.difference(&expected_codes) {
        disagreements.push(format!(
            "reported source diagnostic {code}, which the matrix does not declare"
        ));
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

    let artifacts = deterministic_artifacts(&report);
    // Only a fab run writes artifacts, so only a fab run can compare them. Both
    // directions are drift: a pinned file that changed or vanished, and a
    // pinned kind of file the board now produces that nobody declared.
    let normalized = normalized_artifacts(&kept.join("release"));
    if fab {
        disagreements.extend(artifact_drift(&spec.expect_artifacts, &artifacts));
        disagreements.extend(geometry_drift(&spec.expect_geometry, &normalized));
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
        fixture_sha256: fixture,
        deterministic_artifacts: artifacts,
        normalized_artifacts: normalized,
        agrees: disagreements.is_empty(),
        actual,
        gate,
        disagreements,
    }
}

/// How the pinned artifacts a run produced differ from the ones declared.
fn artifact_drift(
    expected: &BTreeMap<String, String>,
    observed: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut drift = Vec::new();
    for (path, hash) in expected {
        match observed.get(path) {
            Some(actual) if actual == hash => {}
            Some(actual) => drift.push(format!(
                "artifact {path} changed (expect_artifacts has {hash}, the run wrote {actual})"
            )),
            None => drift.push(format!(
                "artifact {path} is pinned in expect_artifacts but the run did not write it"
            )),
        }
    }
    for (path, actual) in observed {
        if is_pinned_artifact(path) && !expected.contains_key(path) {
            drift.push(format!(
                "artifact {path} was written but is not pinned (its hash is {actual})"
            ));
        }
    }
    drift
}

/// How the normalized board and fabrication hashes a run produced differ from
/// the ones declared. A board file that moves here has different copper,
/// footprints or outline, not merely different bytes.
fn geometry_drift(
    expected: &BTreeMap<String, String>,
    observed: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut drift = Vec::new();
    for (key, hash) in expected {
        match observed.get(key) {
            Some(actual) if actual == hash => {}
            Some(actual) => drift.push(format!(
                "geometry of {key} changed (expect_geometry has {hash}, the run produced {actual})"
            )),
            None => drift.push(format!(
                "{key} is pinned in expect_geometry but the run did not write it"
            )),
        }
    }
    for (key, actual) in observed {
        if !expected.contains_key(key) {
            drift.push(format!(
                "geometry of {key} is not pinned (its normalized hash is {actual})"
            ));
        }
    }
    drift
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
    // The row to write once a geometry change is understood to be intended.
    for board in drifted
        .iter()
        .filter(|b| fab && !b.normalized_artifacts.is_empty())
    {
        let entries: Vec<String> = board
            .normalized_artifacts
            .iter()
            .map(|(key, hash)| format!("\"{key}\" = \"{hash}\""))
            .collect();
        eprintln!(
            "{}: expect_geometry = {{ {} }}",
            board.id,
            entries.join(", ")
        );
    }
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
        // An edit to a board's source or layout sidecars makes every verdict
        // on its row a claim about a different board. Checked here, in the
        // normal shards, so an accidental fixture change fails on the pull
        // request that made it rather than waiting for the release gate.
        let fixture = fixture_sha256(&root.join(&spec.path));
        assert_eq!(
            spec.fixture_sha256, fixture,
            "{}: the board's inputs no longer match the ones its verdicts were \
             measured on. Re-run the matrix, update expect/expect_fab/\
             expect_source_codes if they moved, then set fixture_sha256 = \"{fixture}\"",
            spec.id
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
        for (path, hash) in &spec.expect_artifacts {
            assert!(
                is_pinned_artifact(path),
                "{}: expect_artifacts pins {path}, which is not one of {PINNED_ARTIFACTS:?}",
                spec.id
            );
            assert!(
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "{}: expect_artifacts has {hash} for {path}, which is not a SHA-256",
                spec.id
            );
        }
        for (key, hash) in &spec.expect_geometry {
            assert!(
                is_geometry_key(key),
                "{}: expect_geometry pins {key}, which is neither a .kicad_pcb nor one of \
                 {FAB_OUTPUT_DIRS:?}",
                spec.id
            );
            assert!(
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "{}: expect_geometry has {hash} for {key}, which is not a SHA-256",
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
    // Positive and negative are claims about why a board is in the corpus, so
    // they are read off `status`, not off `expect`. `expect` is a measurement:
    // a positive board whose copper is complete can still fail at DRC, and
    // requiring some board to declare `expect = "pass"` would force the matrix
    // to claim a pass nobody had observed — which is the one thing a gate must
    // never be made to do. Today every board in the corpus declares
    // `expect = "fail"`, and the corpus is still well formed.
    assert!(
        matrix.board.iter().any(|b| b.status != "unsupported"),
        "the corpus needs at least one positive case"
    );
    assert!(
        matrix.board.iter().any(|b| b.status == "unsupported"),
        "the corpus needs at least one negative case"
    );
}

/// A registry edit can move any board's verdicts and artifacts, so it is
/// pinned and checked here, in the normal shards, where the pull request that
/// made the edit is the one that fails.
#[test]
fn the_registry_is_the_one_the_matrix_was_measured_against() {
    let matrix = load_matrix();
    let registry = registry_manifest_sha256();
    assert_eq!(
        matrix.baseline.registry_manifest_sha256, registry,
        "the registry no longer matches the one the matrix was measured against. \
         Re-run the matrix, update any verdicts or expect_artifacts that moved, \
         then set [baseline] registry_manifest_sha256 = \"{registry}\""
    );
}

#[test]
fn a_changed_pinned_artifact_is_drift() {
    let pinned = BTreeMap::from([("b.kicad_sch".to_string(), "a".repeat(64))]);
    let observed = BTreeMap::from([("b.kicad_sch".to_string(), "b".repeat(64))]);
    let drift = artifact_drift(&pinned, &observed);
    assert_eq!(drift.len(), 1, "{drift:?}");
    assert!(drift[0].contains("changed"), "{drift:?}");
}

#[test]
fn a_missing_or_undeclared_pinned_artifact_is_drift() {
    let pinned = BTreeMap::from([("bom.csv".to_string(), "a".repeat(64))]);
    let observed = BTreeMap::from([("b.kicad_sch".to_string(), "b".repeat(64))]);
    let drift = artifact_drift(&pinned, &observed);
    assert_eq!(drift.len(), 2, "{drift:?}");
    assert!(
        drift.iter().any(|d| d.contains("did not write")),
        "{drift:?}"
    );
    assert!(drift.iter().any(|d| d.contains("not pinned")), "{drift:?}");
}

#[test]
fn an_unpinned_board_file_is_not_drift() {
    // A routed board's raw bytes move with the external engine, so they are
    // recorded and not compared here. `geometry_drift` is what guards the board.
    let observed = BTreeMap::from([
        ("b.kicad_sch".to_string(), "a".repeat(64)),
        ("b.kicad_pcb".to_string(), "b".repeat(64)),
    ]);
    let pinned = BTreeMap::from([("b.kicad_sch".to_string(), "a".repeat(64))]);
    assert!(artifact_drift(&pinned, &observed).is_empty());
}

const ROUTED: &str = r#"(kicad_pcb
	(version 20260206)
	(segment
		(start 1 2)
		(end 3 4)
		(width 0.2)
		(layer "F.Cu")
		(net "A")
		(uuid "11111111-1111-1111-1111-111111111111")
	)
	(via
		(at 5 6)
		(uuid "22222222-2222-2222-2222-222222222222")
	)
	(zone
		(net "GND")
		(polygon
			(pts (xy 0 0) (xy 9 0) (xy 9 9))
		)
		(filled_polygon
			(layer "F.Cu")
			(pts (xy 1 1) (xy 8 1) (xy 8 8))
		)
		(uuid "33333333-3333-3333-3333-333333333333")
	)
)"#;

#[test]
fn board_churn_is_not_a_geometry_change() {
    // The same board as `ROUTED`, as another run writes it: fresh UUIDs, items
    // in another order, the track drawn the other way, and the fill listing an
    // extra vertex along a straight edge.
    let rewritten = r#"(kicad_pcb
	(version 20260206)
	(zone
		(net "GND")
		(polygon
			(pts (xy 0 0) (xy 9 0) (xy 9 9))
		)
		(filled_polygon
			(layer "F.Cu")
			(pts (xy 1 1) (xy 4 1) (xy 8 1) (xy 8 8))
		)
		(uuid "66666666-6666-6666-6666-666666666666")
	)
	(via
		(at 5 6)
		(uuid "55555555-5555-5555-5555-555555555555")
	)
	(segment
		(start 3 4)
		(end 1 2)
		(width 0.2)
		(layer "F.Cu")
		(net "A")
		(uuid "44444444-4444-4444-4444-444444444444")
	)
)"#;
    assert_eq!(normalized_pcb(ROUTED), normalized_pcb(rewritten));
}

#[test]
fn moved_copper_is_a_geometry_change() {
    let base = normalized_pcb(ROUTED);
    for (from, to) in [
        // A track end, a via, a track's net and width, a pour's outline.
        ("(end 3 4)", "(end 3 5)"),
        ("(at 5 6)", "(at 5 7)"),
        ("(net \"A\")", "(net \"B\")"),
        ("(width 0.2)", "(width 0.3)"),
        ("(xy 9 9)", "(xy 9 8)"),
    ] {
        assert!(ROUTED.contains(from), "{from} is not in the fixture");
        assert_ne!(
            base,
            normalized_pcb(&ROUTED.replace(from, to)),
            "{from} -> {to} went unnoticed"
        );
    }
}

const GERBER: &str = "%TF.GenerationSoftware,KiCad,Pcbnew,10.0.7*%
%TF.CreationDate,2026-11-05T15:22:45+03:00*%
G04 Created by KiCad (PCBNEW 10.0.7) date 2026-11-05 15:22:45*
D10*
X1000000Y-1000000D02*
X2000000Y-1000000D01*
X5000000Y-5000000D03*
G36*
X0Y0D02*
X100Y0D01*
G37*
M02*
";

#[test]
fn gerber_churn_is_not_a_content_change() {
    // Another run: a later stamp, a newer patch release, the track drawn the
    // other way, an aperture selected again, a fill with an extra vertex.
    let rerun = "%TF.GenerationSoftware,KiCad,Pcbnew,10.0.8*%
%TF.CreationDate,2026-11-06T09:00:00+03:00*%
G04 Created by KiCad (PCBNEW 10.0.8) date 2026-11-06 09:00:00*
D10*
X2000000Y-1000000D02*
D10*
X1000000Y-1000000D01*
X5000000Y-5000000D03*
G36*
X0Y0D02*
X50Y0D01*
X100Y0D01*
G37*
M02*
";
    assert_eq!(
        normalized_fab_output("b-F_Cu.gbr", GERBER),
        normalized_fab_output("b-F_Cu.gbr", rerun)
    );
}

#[test]
fn moved_gerber_content_is_a_content_change() {
    let base = normalized_fab_output("b-F_Cu.gbr", GERBER);
    for (from, to) in [
        ("X2000000Y-1000000D01*", "X2000000Y-1000001D01*"),
        ("X5000000Y-5000000D03*", "X5000000Y-5000001D03*"),
    ] {
        assert_ne!(
            base,
            normalized_fab_output("b-F_Cu.gbr", &GERBER.replace(from, to)),
            "{from} -> {to} went unnoticed"
        );
    }
    // A region off a copper layer is footprint artwork rather than a zone fill.
    assert_ne!(
        normalized_fab_output("b-F_Mask.gbr", GERBER),
        normalized_fab_output("b-F_Mask.gbr", &GERBER.replace("X100Y0D01*", "X101Y0D01*"))
    );
}

#[test]
fn geometry_drift_runs_in_both_directions() {
    let pinned = BTreeMap::from([
        ("b.kicad_pcb".to_string(), "a".repeat(64)),
        ("drill".to_string(), "c".repeat(64)),
    ]);
    assert!(geometry_drift(&pinned, &pinned).is_empty());

    let observed = BTreeMap::from([
        ("b.kicad_pcb".to_string(), "b".repeat(64)),
        ("gerbers".to_string(), "d".repeat(64)),
    ]);
    let drift = geometry_drift(&pinned, &observed);
    assert_eq!(drift.len(), 3, "{drift:?}");
    assert!(drift.iter().any(|d| d.contains("changed")), "{drift:?}");
    assert!(
        drift.iter().any(|d| d.contains("did not write")),
        "{drift:?}"
    );
    assert!(drift.iter().any(|d| d.contains("not pinned")), "{drift:?}");
}
