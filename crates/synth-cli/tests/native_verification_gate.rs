// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

const SYNTH: &str = env!("CARGO_BIN_EXE_synth");

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn design() -> PathBuf {
    workspace_root()
        .join("fixtures")
        .join("designs")
        .join("hello.synth")
}

fn scratch(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("synth_native_gate_{}_{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn stub(dir: &Path, script: &str) -> PathBuf {
    let path = dir.join("kicad-cli");
    let body = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = version ]; then echo 10.0.1; exit 0; fi\n\
         {script}\n"
    );
    std::fs::write(&path, body).expect("write stub");
    let mut perms = std::fs::metadata(&path).expect("stat stub").permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
    }
    std::fs::set_permissions(&path, perms).expect("chmod stub");
    path
}

const FIND_OUTPUT: &str = r#"out=""
prev=""
for a in "$@"; do
  if [ "$prev" = "--output" ]; then out="$a"; fi
  prev="$a"
done"#;

struct Outcome {
    code: Option<i32>,
    stderr: String,
    report: Option<serde_json::Value>,
}

impl Outcome {
    fn stage(&self, name: &str) -> serde_json::Value {
        let report = self.report.as_ref().expect("verification report written");
        report["stages"]
            .as_array()
            .expect("stages array")
            .iter()
            .find(|s| s["stage"] == name)
            .unwrap_or_else(|| panic!("no evidence for stage {name}: {report:#}"))
            .clone()
    }

    fn status(&self, stage: &str) -> String {
        self.stage(stage)["status"]
            .as_str()
            .expect("status string")
            .to_string()
    }

    fn reason(&self, stage: &str) -> String {
        self.stage(stage)["reason"]
            .as_str()
            .unwrap_or("<none>")
            .to_string()
    }
}

fn export(label: &str, stub_script: &str, extra: &[&str]) -> Outcome {
    let dir = scratch(label);
    let cli = stub(&dir, stub_script);
    let out = dir.join("out");
    let report_path = dir.join("verification.json");

    let mut command = Command::new(SYNTH);
    command
        .arg("export-kicad")
        .arg(design())
        .arg("--out")
        .arg(&out)
        .arg("--verification-report")
        .arg(&report_path)
        .args(extra)
        .env("KICAD_CLI", &cli)
        .env("SYNTH_KICAD_CLI_TIMEOUT_SECS", "2");

    let output = command.output().expect("run synth export-kicad");
    let report = std::fs::read_to_string(&report_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    Outcome {
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        report,
    }
}

const CLEAN: &str = r#"case "$1 $2" in
  "sch erc"|"pcb drc")
    __FIND_OUTPUT__
    printf '{"kicad_version":"10.0.1","violations":[]}' > "$out"
    exit 0 ;;
esac
exit 0"#;

fn clean_stub() -> String {
    CLEAN.replace("__FIND_OUTPUT__", FIND_OUTPUT)
}

#[test]
fn a_clean_tool_run_passes_and_is_recorded() {
    let run = export("clean", &clean_stub(), &["--validate-erc"]);
    assert_eq!(run.code, Some(0), "stderr:\n{}", run.stderr);
    assert_eq!(run.status("kicad_erc"), "pass");
    assert_eq!(run.status("kicad_drc"), "pass");
    let report = run.report.expect("report");
    assert_eq!(report["release_ready"], true);
    assert_eq!(report["schema_version"], "synth.verification.v1");
}

#[test]
fn reported_violations_fail_the_gate() {
    let script = r#"case "$1 $2" in
  "sch erc")
    __FIND_OUTPUT__
    printf '{"violations":[{"type":"pin_not_driven","severity":"error","description":"Input pin not driven"}]}' > "$out"
    exit 0 ;;
  "pcb drc")
    __FIND_OUTPUT__
    printf '{"violations":[]}' > "$out"
    exit 0 ;;
esac
exit 0"#
        .replace("__FIND_OUTPUT__", FIND_OUTPUT);
    let run = export("violations", &script, &["--validate-erc"]);
    assert_eq!(run.code, Some(1), "stderr:\n{}", run.stderr);
    assert_eq!(run.status("kicad_erc"), "fail");
    assert_eq!(
        run.reason("kicad_erc"),
        "<none>",
        "a real verdict carries no unknown reason"
    );
    assert!(
        run.stderr.contains("pin_not_driven"),
        "the violation must be reported: {}",
        run.stderr
    );
}

#[test]
fn warning_only_violations_still_pass() {
    let script = r#"case "$1 $2" in
  "sch erc")
    __FIND_OUTPUT__
    printf '{"violations":[{"type":"endpoint_off_grid","severity":"warning","description":"off grid"}]}' > "$out"
    exit 0 ;;
  "pcb drc")
    __FIND_OUTPUT__
    printf '{"violations":[]}' > "$out"
    exit 0 ;;
esac
exit 0"#
        .replace("__FIND_OUTPUT__", FIND_OUTPUT);
    let run = export("warnings", &script, &["--validate-erc"]);
    assert_eq!(run.code, Some(0), "stderr:\n{}", run.stderr);
    assert_eq!(run.status("kicad_erc"), "pass");
}

#[test]
fn a_missing_executable_is_unknown_and_blocks_a_requested_check() {
    let dir = scratch("missing");
    let out = dir.join("out");
    let report_path = dir.join("verification.json");
    let output = Command::new(SYNTH)
        .arg("export-kicad")
        .arg(design())
        .arg("--out")
        .arg(&out)
        .arg("--validate-erc")
        .arg("--verification-report")
        .arg(&report_path)
        .env("KICAD_CLI", dir.join("does-not-exist"))
        .output()
        .expect("run synth export-kicad");
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).expect("report written"))
            .expect("report parses");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(1), "stderr:\n{stderr}");
    assert_eq!(report["release_ready"], false);
    let erc = report["stages"]
        .as_array()
        .expect("stages")
        .iter()
        .find(|s| s["stage"] == "kicad_erc")
        .expect("erc evidence")
        .clone();
    assert_eq!(erc["status"], "unknown");
    assert_eq!(erc["reason"], "not_installed");
    assert!(
        erc["command"].as_array().is_some_and(|c| !c.is_empty()),
        "the argv must be recorded for an operator to reproduce"
    );
}

#[test]
fn a_missing_library_is_unknown_with_stderr_captured() {
    let script = r#"echo "Failed to load library 'Device'" >&2
exit 2"#;
    let run = export("missing_library", script, &["--validate-erc"]);
    assert_eq!(run.code, Some(1), "stderr:\n{}", run.stderr);
    assert_eq!(run.status("kicad_erc"), "unknown");
    assert_eq!(run.reason("kicad_erc"), "command_failed");
    let erc = run.stage("kicad_erc");
    assert!(
        erc["stderr"]
            .as_str()
            .is_some_and(|s| s.contains("Failed to load library")),
        "stderr must be kept for diagnosis: {erc:#}"
    );
    assert_eq!(
        erc["tool_version"], "10.0.1",
        "the version must be recorded"
    );
}

#[test]
fn an_unrecognized_report_is_unknown_not_clean() {
    let script = r#"case "$1 $2" in
  "sch erc"|"pcb drc")
    __FIND_OUTPUT__
    printf '{"message":"nothing to do"}' > "$out"
    exit 0 ;;
esac
exit 0"#
        .replace("__FIND_OUTPUT__", FIND_OUTPUT);
    let run = export("unrecognized", &script, &["--validate-erc"]);
    assert_eq!(run.code, Some(1), "stderr:\n{}", run.stderr);
    assert_eq!(run.status("kicad_erc"), "unknown");
    assert_eq!(run.reason("kicad_erc"), "report_unrecognized");
}

#[test]
fn a_malformed_report_is_unknown() {
    let script = r#"case "$1 $2" in
  "sch erc"|"pcb drc")
    __FIND_OUTPUT__
    printf '{"violations": [' > "$out"
    exit 0 ;;
esac
exit 0"#
        .replace("__FIND_OUTPUT__", FIND_OUTPUT);
    let run = export("malformed", &script, &["--validate-erc"]);
    assert_eq!(run.code, Some(1), "stderr:\n{}", run.stderr);
    assert_eq!(run.status("kicad_erc"), "unknown");
    assert_eq!(run.reason("kicad_erc"), "report_malformed");
}

#[test]
fn a_tool_that_writes_no_report_is_unknown() {
    let run = export("no_report", "exit 0", &["--validate-erc"]);
    assert_eq!(run.code, Some(1), "stderr:\n{}", run.stderr);
    assert_eq!(run.status("kicad_erc"), "unknown");
    assert_eq!(run.reason("kicad_erc"), "report_missing");
}

#[test]
fn a_hung_tool_is_unknown_by_timeout() {
    let run = export("timeout", "sleep 60", &["--validate-erc"]);
    assert_eq!(run.code, Some(1), "stderr:\n{}", run.stderr);
    assert_eq!(run.status("kicad_erc"), "unknown");
    assert_eq!(run.reason("kicad_erc"), "timeout");
}

#[test]
fn an_unsupported_version_is_unknown_before_the_tool_runs() {
    let script = "echo 'should not be reached' >&2; exit 0";
    let dir = scratch("old_version");
    let cli = dir.join("kicad-cli");
    std::fs::write(
        &cli,
        format!("#!/bin/sh\nif [ \"$1\" = version ]; then echo 7.0.11; exit 0; fi\n{script}\n"),
    )
    .expect("write stub");
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&cli).expect("stat").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&cli, perms).expect("chmod");
    }
    let report_path = dir.join("verification.json");
    let output = Command::new(SYNTH)
        .arg("export-kicad")
        .arg(design())
        .arg("--out")
        .arg(dir.join("out"))
        .arg("--validate-erc")
        .arg("--verification-report")
        .arg(&report_path)
        .env("KICAD_CLI", &cli)
        .output()
        .expect("run synth export-kicad");
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).expect("report written"))
            .expect("report parses");
    let erc = report["stages"]
        .as_array()
        .expect("stages")
        .iter()
        .find(|s| s["stage"] == "kicad_erc")
        .expect("erc evidence")
        .clone();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(erc["status"], "unknown");
    assert_eq!(erc["reason"], "unsupported_version");
    assert!(
        erc["command"].as_array().is_none_or(Vec::is_empty),
        "the version gate must refuse before invoking the tool: {erc:#}"
    );
}

#[test]
fn force_does_not_convert_an_unavailable_check_into_a_pass() {
    let run = export("force", "exit 2", &["--validate-erc", "--force"]);
    assert_eq!(
        run.code,
        Some(1),
        "--force must not rescue an unavailable check; stderr:\n{}",
        run.stderr
    );
    assert_eq!(run.status("kicad_erc"), "unknown");
    assert!(
        run.stderr.contains("--force does not override this"),
        "the operator must be told --force will not help: {}",
        run.stderr
    );
}

#[test]
fn a_plain_export_still_succeeds_but_records_the_unknown() {
    let dir = scratch("plain");
    let report_path = dir.join("verification.json");
    let output = Command::new(SYNTH)
        .arg("export-kicad")
        .arg(design())
        .arg("--out")
        .arg(dir.join("out"))
        .arg("--verification-report")
        .arg(&report_path)
        .env("KICAD_CLI", dir.join("does-not-exist"))
        .output()
        .expect("run synth export-kicad");
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).expect("report written"))
            .expect("report parses");

    assert_eq!(
        output.status.code(),
        Some(0),
        "a plain export must not require KiCad; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        report["release_ready"], false,
        "but it must not claim to be release ready"
    );
    let drc = report["stages"]
        .as_array()
        .expect("stages")
        .iter()
        .find(|s| s["stage"] == "kicad_drc")
        .expect("drc evidence")
        .clone();
    assert_eq!(drc["status"], "unknown");
    assert_eq!(drc["reason"], "not_installed");
}

mod release_gate {
    use super::*;

    fn check_fab(label: &str, kicad_cli: &Path) -> (Option<i32>, serde_json::Value) {
        let output = Command::new(SYNTH)
            .arg("check")
            .arg(design())
            .arg("--fab")
            .arg("--json")
            .env("KICAD_CLI", kicad_cli)
            .env("SYNTH_KICAD_CLI_TIMEOUT_SECS", "5")
            .output()
            .unwrap_or_else(|e| panic!("run synth check ({label}): {e}"));
        let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "check must emit JSON ({label}): {e}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.code(), report)
    }

    #[test]
    fn an_unavailable_tool_makes_manufacturing_unknown_not_fail() {
        let dir = scratch("gate_missing");
        let (code, report) = check_fab("missing", &dir.join("does-not-exist"));
        let manufacturing = &report["stages"]["manufacturing"];

        assert_eq!(code, Some(1), "an unknown stage must block: {report:#}");
        assert_eq!(
            manufacturing["status"], "unknown",
            "a missing tool is an absence of evidence, not a rejection: {report:#}"
        );
        assert_eq!(
            manufacturing["native"]["release_ready"], false,
            "{report:#}"
        );
        let stages = manufacturing["native"]["stages"]
            .as_array()
            .expect("native stages");
        assert!(
            stages.iter().all(|s| s["status"] == "unknown"),
            "{report:#}"
        );
        assert!(
            stages.iter().all(|s| s["reason"] == "not_installed"),
            "{report:#}"
        );
        assert_ne!(
            report["status"], "pass",
            "the command must never pass over an unknown stage"
        );
    }

    #[test]
    fn a_clean_tool_lets_the_gate_pass() {
        let dir = scratch("gate_clean");
        let script = r#"case "$1 $2" in
  "sch erc"|"pcb drc")
    __FIND_OUTPUT__
    printf '{"kicad_version":"10.0.1","violations":[]}' > "$out"
    exit 0 ;;
  "pcb export") exit 0 ;;
esac
exit 0"#
            .replace("__FIND_OUTPUT__", FIND_OUTPUT);
        let cli = stub(&dir, &script);
        let (code, report) = check_fab("clean", &cli);
        let manufacturing = &report["stages"]["manufacturing"];

        assert_eq!(
            manufacturing["status"], "pass",
            "a clean tool run must pass: {report:#}"
        );
        assert_eq!(report["status"], "pass", "{report:#}");
        assert_eq!(code, Some(0), "{report:#}");
        assert_eq!(manufacturing["native"]["release_ready"], true, "{report:#}");
        assert!(
            manufacturing["artifacts"]
                .as_object()
                .is_some_and(|a| !a.is_empty()),
            "the artifact hashes must still be recorded: {report:#}"
        );
    }

    #[test]
    fn a_broken_tool_makes_manufacturing_unknown() {
        let dir = scratch("gate_broken");
        let script = r#"case "$1 $2" in
  "pcb export") exit 0 ;;
  "sch erc")
    __FIND_OUTPUT__
    printf '{"violations":[]}' > "$out"
    exit 0 ;;
  "pcb drc") echo "Failed to load library" >&2; exit 2 ;;
esac
exit 0"#
            .replace("__FIND_OUTPUT__", FIND_OUTPUT);
        let cli = stub(&dir, &script);
        let (code, report) = check_fab("broken", &cli);
        let manufacturing = &report["stages"]["manufacturing"];

        assert_eq!(code, Some(1), "{report:#}");
        assert_eq!(manufacturing["status"], "unknown", "{report:#}");
        let drc = manufacturing["native"]["stages"]
            .as_array()
            .expect("native stages")
            .iter()
            .find(|s| s["stage"] == "kicad_drc")
            .expect("drc evidence")
            .clone();
        assert_eq!(drc["reason"], "command_failed", "{drc:#}");
        assert!(
            drc["stderr"]
                .as_str()
                .is_some_and(|s| s.contains("Failed to load library")),
            "{drc:#}"
        );
    }
}
