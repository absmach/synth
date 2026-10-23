// SPDX-License-Identifier: Apache-2.0

//! Capability discovery: can this engine actually run here?
//!
//! Discovery exists so an absent router is reported *before* any work
//! starts, with a specific reason, rather than surfacing later as an
//! opaque I/O error from deep inside an adapter. The three ways a router
//! can be unusable — no JAR, no Java, no checkout — each get their own
//! reason and their own remediation, because they have different fixes.
//!
//! It also pins the versions. A FreeRouting JAR swapped under the same
//! path produces different copper, so a run record without the version is
//! not reproducible.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::contract::{RouteRequest, RouterEngine};
use crate::failure::{RouterFailure, RouterFailureReason, RouterStage};

/// FreeRouting JAR filename the project pins by default.
pub const PINNED_FREEROUTING_JAR: &str = "freerouting-2.4.1.jar";

/// Directory name of the bundled JRE, tried before falling back to `java`.
pub const BUNDLED_JRE_DIR: &str = "tools/jre25";

/// How long a version probe may take before the engine is treated as
/// unusable. A JVM that needs longer to print `-version` will not route
/// faster.
const PROBE_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

/// What one engine can do on this machine, and why not if it cannot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouterCapability {
    pub engine: RouterEngine,
    /// True when every prerequisite for a run is present.
    pub available: bool,
    /// Version string, when it could be determined. `None` on an
    /// unavailable engine is expected; `None` on an available one means
    /// the probe failed, which the run record still carries so a
    /// consumer can tell "unversioned router" from "no router".
    #[serde(default)]
    pub version: Option<String>,
    /// Runtime that would execute the engine.
    #[serde(default)]
    pub runtime_version: Option<String>,
    /// Human-readable explanation, present when unavailable.
    #[serde(default)]
    pub detail: Option<String>,
    /// Structured failure, when unavailable.
    #[serde(default)]
    pub failure: Option<RouterFailure>,
}

impl RouterCapability {
    /// A capability that is ready to run.
    #[must_use]
    pub fn available(
        engine: RouterEngine,
        version: Option<String>,
        runtime_version: Option<String>,
    ) -> Self {
        Self {
            engine,
            available: true,
            version,
            runtime_version,
            detail: None,
            failure: None,
        }
    }

    /// A capability that cannot run, and why.
    #[must_use]
    pub fn unavailable(
        engine: RouterEngine,
        reason: RouterFailureReason,
        detail: impl Into<String>,
    ) -> Self {
        let failure = RouterFailure::for_stage(engine, RouterStage::Capability, reason, detail);
        Self {
            engine,
            available: false,
            version: None,
            runtime_version: None,
            detail: Some(failure.detail.clone()),
            failure: Some(failure),
        }
    }

    /// The structured failure, when this engine cannot run.
    #[must_use]
    pub fn failure(&self) -> Option<&RouterFailure> {
        self.failure.as_ref()
    }
}

/// Determine whether `request.engine` can run, without starting it.
///
/// An available engine carries no failure; an unavailable one does, and
/// [`RouterCapability::failure`] returns it. Discovery is cheap and
/// side-effect free: it stats files and probes versions, and never writes.
#[must_use]
pub fn discover(request: &RouteRequest) -> RouterCapability {
    match request.engine {
        RouterEngine::Freerouting => freerouting_capability(request),
        RouterEngine::KiCadRoutingTools => krt_capability(request),
    }
}

/// Report every engine's availability, for `synth routers` and for UI
/// pickers that need to grey out what is not installed.
///
/// FreeRouting is listed first because it is the default path.
#[must_use]
pub fn discover_all(request: &RouteRequest) -> Vec<RouterCapability> {
    RouterEngine::all()
        .iter()
        .map(|engine| {
            let mut scoped = request.clone();
            scoped.engine = *engine;
            discover(&scoped)
        })
        .collect()
}

/// Report availability of every engine for a directory, without a request.
#[must_use]
pub fn discover_all_in_out_dir(out_dir: &Path) -> Vec<RouterCapability> {
    let template = RouteRequest::new(
        Path::new("board.kicad_pcb"),
        out_dir,
        RouterEngine::Freerouting,
    );
    discover_all(&template)
}

// ── FreeRouting ──────────────────────────────────────────────────────────────

fn freerouting_capability(request: &RouteRequest) -> RouterCapability {
    let Some(jar) = resolve_jar(request) else {
        return RouterCapability::unavailable(
            RouterEngine::Freerouting,
            RouterFailureReason::JarMissing,
            format!(
                "no FreeRouting JAR found; looked at {} and set SYNTH_FREEROUTING_JAR",
                searched_jar_paths().join(", ")
            ),
        );
    };

    let Some(java) = resolve_java() else {
        return RouterCapability::unavailable(
            RouterEngine::Freerouting,
            RouterFailureReason::JavaMissing,
            format!("no Java runtime found; looked at {BUNDLED_JRE_DIR}/bin/java and `java`"),
        );
    };

    let version = jar_version(&jar).or_else(|| Some(jar_stem(&jar)));
    let runtime_version = probe_version(&java, &["-version"]);
    RouterCapability::available(RouterEngine::Freerouting, version, runtime_version)
}

/// Candidate JAR locations, most specific first.
///
/// An explicit configuration always wins; the pinned JAR name is searched
/// before a loose glob so a stray newer JAR in the directory cannot
/// silently change which engine version runs.
#[must_use]
pub fn searched_jar_paths() -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(configured) = configured_jar() {
        paths.push(configured.display().to_string());
    }
    for base in tool_roots() {
        paths.push(
            base.join("freerouting")
                .join(PINNED_FREEROUTING_JAR)
                .display()
                .to_string(),
        );
    }
    paths
}

fn configured_jar() -> Option<PathBuf> {
    std::env::var_os("SYNTH_FREEROUTING_JAR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn resolve_jar(_request: &RouteRequest) -> Option<PathBuf> {
    if let Some(configured) = configured_jar() {
        return configured.is_file().then_some(configured);
    }
    tool_roots()
        .into_iter()
        .map(|base| base.join("freerouting").join(PINNED_FREEROUTING_JAR))
        .find(|p| p.is_file())
}

/// Directories that may hold `tools/freerouting/` and the bundled JRE.
fn tool_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(configured) = std::env::var("SYNTH_TOOLS_DIR") {
        let path = PathBuf::from(configured);
        if !path.as_os_str().is_empty() {
            roots.push(path);
        }
    }
    if let Some(root) = crate::repo_root() {
        roots.push(root.join("tools"));
        if let Some(parent) = root.parent() {
            roots.push(parent.join("tools"));
        }
    }
    roots
}

fn resolve_java() -> Option<PathBuf> {
    for root in tool_roots() {
        let bundled = root.join("jre25").join("bin").join("java");
        if bundled.is_file() {
            return Some(bundled);
        }
    }
    // Fall back to PATH: a JVM that is not in the tools tree but is on
    // PATH is still usable, and requiring a bundled copy would make the
    // default path fail on any machine that installed Java normally.
    which("java")
}

/// Locate an executable on `PATH`.
///
/// Deliberately a plain `PATH` scan rather than a dependency: this runs
/// once per run, on the failure path for `java`, and adding a process
/// resolution crate to answer "is java installed" is not a trade worth
/// making.
#[must_use]
pub fn which(binary: &str) -> Option<PathBuf> {
    if binary.contains(std::path::MAIN_SEPARATOR) {
        let path = PathBuf::from(binary);
        return path.is_file().then_some(path);
    }
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}

/// The pinned JAR's file stem, used as a version fallback when the JAR
/// cannot be interrogated.
fn jar_stem(jar: &Path) -> String {
    jar.file_stem().map_or_else(
        || "unknown".to_string(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// Read the JAR's `Implementation-Version`, falling back to its filename.
///
/// The filename is a real signal here — the pinned JAR is named for its
/// version — but the manifest is authoritative when both are available.
fn jar_version(jar: &Path) -> Option<String> {
    let bytes = std::fs::read(jar).ok()?;
    // The manifest is stored uncompressed near the end of the archive.
    // Scanning for the entry header is enough: a full unzip would pull in
    // an archive dependency for one field.
    let text = String::from_utf8_lossy(&bytes);
    let marker = "Implementation-Version:";
    let start = text.rfind(marker)? + marker.len();
    let rest = &text[start..];
    let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
    let version = rest[..end].trim();
    (!version.is_empty() && version != "unknown").then(|| version.to_string())
}

/// Probe a runtime's version line, reading either stream.
///
/// `java -version` writes to stderr while `python --version` writes to
/// stdout, so both are consulted rather than guessing.
#[must_use]
pub fn probe_runtime_version(binary: &Path) -> Option<String> {
    probe_version(binary, &["-version"])
}

fn probe_version(binary: &Path, args: &[&str]) -> Option<String> {
    let owned: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    let invocation = crate::process::run(binary, &owned, PROBE_BUDGET, 4096).ok()?;
    if !invocation.succeeded() {
        return None;
    }
    let mut text = invocation.stderr.clone();
    text.push_str(&invocation.stdout);
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

// ── KiCadRoutingTools ────────────────────────────────────────────────────────

/// The file that must exist for a checkout to be usable.
pub const KRT_ENTRYPOINT: &str = "py_router/route.py";

fn krt_capability(_request: &RouteRequest) -> RouterCapability {
    let Some(repo) = resolve_krt_repo() else {
        return RouterCapability::unavailable(
            RouterEngine::KiCadRoutingTools,
            RouterFailureReason::CheckoutMissing,
            "no KiCadRoutingTools checkout configured; pass --kicad-routing-tools-repo \
             or set KICAD_ROUTING_TOOLS_REPO",
        );
    };

    let entrypoint = repo.join(KRT_ENTRYPOINT);
    if !entrypoint.is_file() {
        return RouterCapability::unavailable(
            RouterEngine::KiCadRoutingTools,
            RouterFailureReason::CheckoutMissing,
            format!(
                "{} is not a KiCadRoutingTools checkout: {} is missing",
                repo.display(),
                KRT_ENTRYPOINT
            ),
        );
    }

    if crate::repo_tool(crate::kicad_routing_tools::HELPER).is_none() {
        return RouterCapability::unavailable(
            RouterEngine::KiCadRoutingTools,
            RouterFailureReason::NotInstalled,
            format!(
                "{} was not found in the repository's tools/ directory",
                crate::kicad_routing_tools::HELPER
            ),
        );
    }

    let Some(python) = resolve_python() else {
        return RouterCapability::unavailable(
            RouterEngine::KiCadRoutingTools,
            RouterFailureReason::PythonMissing,
            "no Python interpreter found; pass --kicad-routing-tools-python",
        );
    };

    RouterCapability::available(
        RouterEngine::KiCadRoutingTools,
        Some(krt_commit(&repo)),
        probe_version(&python, &["--version"]),
    )
}

/// Resolve the KRT checkout.
///
/// An explicit `KICAD_ROUTING_TOOLS_REPO` always wins. Otherwise fall back to
/// the location `scripts/setup-routing-engines.sh` installs to, so a machine
/// that ran the setup script uses KiCadRoutingTools without exporting any
/// environment. The CLI flag is threaded through the request by the caller
/// that owns it, so the flag takes precedence by arriving as an already
/// resolved path.
pub(crate) fn resolve_krt_repo() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("KICAD_ROUTING_TOOLS_REPO")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
    {
        return configured.is_dir().then_some(configured);
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })?;
    let conventional = data.join("synth").join("kicad-routing-tools");
    conventional.is_dir().then_some(conventional)
}

/// Resolve a Python interpreter that KiCadRoutingTools can run under.
///
/// KRT's `route.py` imports KiCad's `pcbnew`, which the first `python3` on
/// `PATH` usually cannot (a version-manager shim, an unprovisioned system
/// interpreter). Prefer the checkout's own venv, then a system interpreter
/// that actually imports `pcbnew`.
///
/// Only **absolute**, well-known interpreters are executed to test the import;
/// a `PATH` entry is never run, so discovery has no side effect on whatever
/// `python3` a caller happens to have first on their path. The final fallback
/// returns a `PATH` interpreter without executing it, so a machine with no
/// pcbnew-capable Python still reports "available" here and fails later with
/// KRT's own, clearer message.
pub(crate) fn resolve_python() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("SYNTH_KRT_PYTHON")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
    {
        return configured.is_file().then_some(configured);
    }
    let mut absolute: Vec<PathBuf> = Vec::new();
    if let Some(repo) = resolve_krt_repo() {
        absolute.push(repo.join(".venv").join("bin").join("python"));
    }
    absolute.push(PathBuf::from("/usr/bin/python3"));
    absolute.push(PathBuf::from("/usr/local/bin/python3"));
    for candidate in &absolute {
        if candidate.is_file() && imports_pcbnew(candidate) {
            return Some(candidate.clone());
        }
    }
    which("python3").or_else(|| which("python"))
}

/// Whether `python` can `import pcbnew`.
fn imports_pcbnew(python: &Path) -> bool {
    std::process::Command::new(python)
        .args(["-c", "import pcbnew"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The checkout's commit, from `git rev-parse --short HEAD`.
///
/// Recorded because a KRT route is only reproducible against a known
/// commit; "some checkout" is not provenance.
/// The checkout's commit, recorded so a run is reproducible.
///
/// Returns `"unknown"` rather than omitting the field: a route against an
/// unidentified checkout is still worth keeping, as long as the run record
/// says plainly that the revision is not known.
#[must_use]
pub fn checkout_commit(repo: &Path) -> String {
    krt_commit(repo)
}

/// The version of a KiCadRoutingTools checkout.
///
/// Prefers the git commit, but falls back to the checkout's `VERSION` file.
/// A system-wide install is normally owned by root (`/opt/...`) and git
/// refuses to read it — "detected dubious ownership" — so keying the version
/// to git alone reports `unknown` for exactly the deployments most likely to
/// be shared. The `VERSION` file needs no ownership check and always names the
/// release KRT's own build_router.py fetched.
fn krt_commit(repo: &Path) -> String {
    if let Some(commit) = git_commit(repo) {
        return commit;
    }
    version_file(repo).unwrap_or_else(|| "unknown".to_string())
}

fn git_commit(repo: &Path) -> Option<String> {
    let git = which("git")?;
    let invocation = crate::process::run(
        &git,
        &[
            "-C".to_string(),
            repo.display().to_string(),
            "rev-parse".to_string(),
            "--short".to_string(),
            "HEAD".to_string(),
        ],
        PROBE_BUDGET,
        4096,
    )
    .ok()?;
    if !invocation.succeeded() {
        return None;
    }
    let commit = invocation.stdout.trim().to_string();
    (!commit.is_empty()).then_some(commit)
}

fn version_file(repo: &Path) -> Option<String> {
    let text = std::fs::read_to_string(repo.join("VERSION")).ok()?;
    let version = text.trim().lines().next()?.trim().to_string();
    (!version.is_empty()).then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkout_git_refuses_still_yields_its_release_version() {
        // A system-wide install (/opt/...) is normally owned by root and git
        // refuses to read it ("dubious ownership"), so a version keyed to git
        // alone reads `unknown` for exactly the deployments most likely to be
        // shared. The VERSION file KRT writes during setup names the release.
        let dir = std::env::temp_dir().join(format!(
            "krt-version-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        std::fs::write(dir.join("VERSION"), "0.22.1\n").expect("version file");
        assert_eq!(checkout_commit(&dir), "0.22.1");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_checkout_with_no_version_marker_is_reported_as_unknown() {
        let dir = std::env::temp_dir().join(format!("krt-noversion-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        assert_eq!(checkout_commit(&dir), "unknown");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_freerouting_jar_names_the_paths_it_searched() {
        // "install the router" is not actionable; the searched paths are.
        let request = RouteRequest::new(
            Path::new("/tmp/board.kicad_pcb"),
            Path::new("/tmp"),
            RouterEngine::Freerouting,
        );
        let capability = discover(&request);
        // Discovery is environment-sensitive, so assert on the shape of the
        // answer rather than on whether a JAR happens to be installed.
        if !capability.available {
            let failure = capability.failure().expect("unavailable carries a failure");
            assert_eq!(failure.reason, RouterFailureReason::JarMissing);
            assert!(failure.detail.contains("JAR"), "{}", failure.detail);
            assert!(!failure.remediation.is_empty());
        }
    }

    #[test]
    fn an_unconfigured_krt_checkout_reports_checkout_missing() {
        // Discovery falls back to the conventional install dir when the env
        // var is unset, so point both XDG_DATA_HOME and HOME at an empty
        // scratch tree to make "no checkout anywhere" true regardless of what
        // is installed on the machine running the tests.
        let data = std::env::temp_dir().join(format!("synth-krt-empty-{}", std::process::id()));
        std::fs::create_dir_all(&data).expect("scratch");
        let previous_repo = std::env::var_os("KICAD_ROUTING_TOOLS_REPO");
        let previous_xdg = std::env::var_os("XDG_DATA_HOME");
        let previous_home = std::env::var_os("HOME");
        std::env::remove_var("KICAD_ROUTING_TOOLS_REPO");
        std::env::set_var("XDG_DATA_HOME", &data);
        std::env::set_var("HOME", &data);

        let request = RouteRequest::new(
            Path::new("/tmp/board.kicad_pcb"),
            Path::new("/tmp"),
            RouterEngine::KiCadRoutingTools,
        );
        let capability = discover(&request);
        assert!(!capability.available);
        assert_eq!(
            capability.failure().expect("failure").reason,
            RouterFailureReason::CheckoutMissing
        );

        for (name, value) in [
            ("KICAD_ROUTING_TOOLS_REPO", previous_repo),
            ("XDG_DATA_HOME", previous_xdg),
            ("HOME", previous_home),
        ] {
            match value {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_directory_without_the_krt_entrypoint_is_not_a_checkout() {
        let dir = std::env::temp_dir().join(format!("synth_krt_probe_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        // Point discovery at a real directory that is not a KRT checkout.
        let previous = std::env::var_os("KICAD_ROUTING_TOOLS_REPO");
        std::env::set_var("KICAD_ROUTING_TOOLS_REPO", &dir);

        let request = RouteRequest::new(
            Path::new("/tmp/board.kicad_pcb"),
            Path::new("/tmp"),
            RouterEngine::KiCadRoutingTools,
        );
        let capability = discover(&request);
        assert!(!capability.available);
        let failure = capability.failure().expect("failure");
        assert_eq!(failure.reason, RouterFailureReason::CheckoutMissing);
        assert!(
            failure.detail.contains(KRT_ENTRYPOINT),
            "{}",
            failure.detail
        );

        match previous {
            Some(value) => std::env::set_var("KICAD_ROUTING_TOOLS_REPO", value),
            None => std::env::remove_var("KICAD_ROUTING_TOOLS_REPO"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovery_reports_every_engine_rather_than_only_the_selected_one() {
        let all = discover_all_in_out_dir(Path::new("/tmp"));
        assert_eq!(all.len(), RouterEngine::all().len());
        assert_eq!(all[0].engine, RouterEngine::default());
    }

    #[test]
    fn an_available_capability_carries_no_failure() {
        let capability = RouterCapability::available(
            RouterEngine::Freerouting,
            Some("2.4.1".to_string()),
            Some("openjdk 21".to_string()),
        );
        assert!(capability.available);
        assert!(capability.failure().is_none());
        assert!(capability.detail.is_none());
    }
}
