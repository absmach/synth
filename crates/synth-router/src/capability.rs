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
use crate::tools::ToolPaths;

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
        RouterEngine::Fastroute => fastroute_capability(request),
        // The internal router is compiled in, so there is nothing to
        // discover: it is always runnable, and the version it routes with is
        // this crate's own.
        RouterEngine::Internal => RouterCapability::available(
            RouterEngine::Internal,
            Some(env!("CARGO_PKG_VERSION").to_string()),
            None,
        ),
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
    let tools = &request.tools;
    let Some(jar) = crate::freerouting::resolve_jar(tools) else {
        return RouterCapability::unavailable(
            RouterEngine::Freerouting,
            RouterFailureReason::JarMissing,
            format!(
                "no FreeRouting JAR found; looked at {} and set SYNTH_FREEROUTING_JAR",
                searched_jar_paths(tools).join(", ")
            ),
        );
    };

    let Some(java) = crate::freerouting::resolve_java(tools) else {
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
pub fn searched_jar_paths(tools: &ToolPaths) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(configured) = &tools.freerouting_jar {
        paths.push(configured.display().to_string());
    }
    for base in crate::freerouting::tool_roots(tools) {
        paths.push(
            base.join("freerouting")
                .join(PINNED_FREEROUTING_JAR)
                .display()
                .to_string(),
        );
    }
    paths
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

pub(crate) fn probe_version(binary: &Path, args: &[&str]) -> Option<String> {
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

// ── FastRoute ────────────────────────────────────────────────────────────────

/// Resolve a FastRoute binary: explicit flag/env, the conventional install
/// location, a bundled tool, then `PATH`.
#[must_use]
pub(crate) fn resolve_fastroute(tools: &ToolPaths) -> Option<PathBuf> {
    if let Some(configured) = &tools.fastroute_bin {
        return configured.is_file().then(|| configured.clone());
    }
    let conventional = tools
        .data_dir
        .as_ref()?
        .join("synth")
        .join("fastroute")
        .join("fastroute");
    if conventional.is_file() {
        return Some(conventional);
    }
    if let Some(bundled) = crate::repo_root()
        && let candidate = bundled.join("tools").join("fastroute").join("fastroute")
        && candidate.is_file()
    {
        return Some(candidate);
    }
    which("fastroute")
}

fn fastroute_capability(request: &RouteRequest) -> RouterCapability {
    let Some(binary) = resolve_fastroute(&request.tools) else {
        return RouterCapability::unavailable(
            RouterEngine::Fastroute,
            RouterFailureReason::NotInstalled,
            "no FastRoute binary found; set SYNTH_FASTROUTE_BIN or pass --fastroute-binary",
        );
    };
    // A binary that cannot answer `--version` is not a usable FastRoute:
    // its provenance would be unknown and its DSN/SES contract unverified.
    let Some(version) = probe_version(&binary, &["--version"]) else {
        return RouterCapability::unavailable(
            RouterEngine::Fastroute,
            RouterFailureReason::NotInstalled,
            format!(
                "{} did not answer `--version`; not a usable FastRoute",
                binary.display()
            ),
        );
    };
    RouterCapability::available(RouterEngine::Fastroute, Some(version), None)
}

// ── KiCadRoutingTools ────────────────────────────────────────────────────────

/// The file that must exist for a checkout to be usable.
pub const KRT_ENTRYPOINT: &str = "py_router/route.py";

/// KRT's own capability manifest.
///
/// Checking that `route.py` exists passes for a clone on the wrong branch or
/// one missing the module the chain needs — the run then completes, prints
/// green, and describes an engine the repo does not pin. KRT publishes this
/// manifest precisely so a consumer can assert what a checkout actually
/// provides before it routes. Synth asserts against it at discovery time, so
/// an unusable checkout is reported as unavailable with the missing tokens
/// named rather than as a validation failure after a wasted route.
pub const KRT_CAPABILITIES_SCRIPT: &str = "krt_capabilities.py";

/// The modules and flags the KRT helper drives.
///
/// Keep this list in step with `tools/kicad_routing_tools_route.py`. A token
/// here the helper never uses would reject a checkout that could route the
/// board; a token the helper does use but that is absent here is exactly the
/// hole this check exists to close. KRT's `--require` exits non-zero and
/// names every gap, so the failure message is actionable without Synth
/// interpreting KRT's manifest.
pub const KRT_REQUIRED_CAPABILITIES: &[&str] = &[
    "route.py:--stats",
    "route.py:--json-out",
    "route.py:--write-fill",
    "route.py:--strict-sizes",
    "route.py:--same-net-pad-clearance",
    "route.py:--escalation",
    "route.py:--fab-tier",
    "route.py:--fab-overrides",
    "route.py:--ordering",
    "route.py:--via-cost",
    "route.py:--layers",
    "route.py:--nets",
    "bga_fanout.py:--escape-method",
    "bga_fanout.py:--clearance",
    "bga_fanout.py:--component",
    "bga_fanout.py:--output",
    "bga_fanout.py:--via-size",
    "bga_fanout.py:--via-drill",
    "route_planes.py:--nets",
    "route_planes.py:--plane-layers",
    "qfn_fanout.py:--component",
    "qfn_fanout.py:--output",
    "qfn_fanout.py:--clearance",
    "qfn_fanout.py:--no-fix-drc-settings",
    "check_connected.py",
];

/// Assert that a checkout provides every module and flag Synth drives.
///
/// The check is run with the same interpreter the route will use, so a
/// checkout whose flags change between Python environments cannot pass here
/// and fail there. A missing manifest is reported by name rather than
/// silently skipped: "no way to verify" is not a verification.
fn verify_krt_capabilities(repo: &Path, python: &Path) -> Result<(), String> {
    let manifest = repo.join(KRT_CAPABILITIES_SCRIPT);
    if !manifest.is_file() {
        return Err(format!(
            "{KRT_CAPABILITIES_SCRIPT} is missing from {}; Synth needs it to verify that the \
             checkout provides the flags the router drives",
            repo.display()
        ));
    }
    let mut argv = vec![manifest.display().to_string(), "--require".to_string()];
    argv.extend(
        KRT_REQUIRED_CAPABILITIES
            .iter()
            .map(|token| (*token).to_string()),
    );
    argv.push("--quiet".to_string());
    let invocation = crate::process::run(python, &argv, PROBE_BUDGET, 8192)
        .map_err(|error| format!("could not run {}: {}", manifest.display(), error.detail))?;
    if invocation.succeeded() {
        return Ok(());
    }
    let detail = invocation.combined_output().trim().to_string();
    Err(if detail.is_empty() {
        format!(
            "{KRT_CAPABILITIES_SCRIPT} rejected the checkout with exit {:?}",
            invocation.exit_code
        )
    } else {
        detail
    })
}

fn krt_capability(request: &RouteRequest) -> RouterCapability {
    let tools = &request.tools;
    let Some(repo) = resolve_krt_repo(tools) else {
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

    let Some(python) = resolve_python(tools) else {
        return RouterCapability::unavailable(
            RouterEngine::KiCadRoutingTools,
            RouterFailureReason::PythonMissing,
            "no Python interpreter found; pass --kicad-routing-tools-python",
        );
    };

    // A route.py file is not a pin. Verify the checkout actually speaks the
    // flags the helper will pass before reporting it available; otherwise
    // `synth routers` and the run record would both claim an engine that
    // only partly exists.
    if let Err(detail) = verify_krt_capabilities(&repo, &python) {
        return RouterCapability::unavailable(
            RouterEngine::KiCadRoutingTools,
            RouterFailureReason::CheckoutMissing,
            detail,
        );
    }

    RouterCapability::available(
        RouterEngine::KiCadRoutingTools,
        Some(krt_commit(&repo)),
        probe_version(&python, &["--version"]),
    )
}

/// Resolve the KRT checkout.
///
/// A configured checkout ([`ToolPaths::krt_repo`], from the CLI flag or
/// `KICAD_ROUTING_TOOLS_REPO`) always wins. Otherwise fall back to the
/// location `scripts/setup-routing-engines.sh` installs to, so a machine that
/// ran the setup script uses KiCadRoutingTools without exporting any
/// environment.
pub(crate) fn resolve_krt_repo(tools: &ToolPaths) -> Option<PathBuf> {
    if let Some(configured) = &tools.krt_repo {
        return configured.is_dir().then(|| configured.clone());
    }
    let conventional = tools
        .data_dir
        .as_ref()?
        .join("synth")
        .join("kicad-routing-tools");
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
pub(crate) fn resolve_python(tools: &ToolPaths) -> Option<PathBuf> {
    if let Some(configured) = &tools.krt_python {
        return configured.is_file().then(|| configured.clone());
    }
    let mut absolute: Vec<PathBuf> = Vec::new();
    if let Some(repo) = resolve_krt_repo(tools) {
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

    /// A request whose tool locations are exactly `tools`, ignoring the
    /// environment of the machine running the tests.
    fn isolated_request(engine: RouterEngine, tools: ToolPaths) -> RouteRequest {
        let mut request =
            RouteRequest::new(Path::new("/tmp/board.kicad_pcb"), Path::new("/tmp"), engine);
        request.tools = tools;
        request
    }

    #[test]
    fn an_unconfigured_krt_checkout_reports_checkout_missing() {
        // No configured checkout and no data dir: the conventional install
        // location is not searched, whatever the test machine has installed.
        let request = isolated_request(RouterEngine::KiCadRoutingTools, ToolPaths::default());
        let capability = discover(&request);
        assert!(!capability.available);
        assert_eq!(
            capability.failure().expect("failure").reason,
            RouterFailureReason::CheckoutMissing
        );
    }

    #[test]
    fn an_empty_data_dir_holds_no_conventional_checkout() {
        let data = std::env::temp_dir().join(format!("synth-krt-empty-{}", std::process::id()));
        std::fs::create_dir_all(&data).expect("scratch");
        let request = isolated_request(
            RouterEngine::KiCadRoutingTools,
            ToolPaths {
                data_dir: Some(data.clone()),
                ..ToolPaths::default()
            },
        );
        let capability = discover(&request);
        assert_eq!(
            capability.failure().expect("failure").reason,
            RouterFailureReason::CheckoutMissing
        );
        let _ = std::fs::remove_dir_all(&data);
    }

    /// A scratch interpreter that returns a canned answer, so the capability
    /// check can be tested without whichever Python happens to be installed.
    fn fake_python(dir: &Path, body: &str) -> PathBuf {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let script = dir.join("fake-python");
            std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).expect("write script");
            let mut permissions = std::fs::metadata(&script).expect("metadata").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&script, permissions).expect("chmod");
            script
        }
        #[cfg(not(unix))]
        {
            let _ = (dir, body);
            PathBuf::from("python3")
        }
    }

    #[test]
    fn a_configured_fastroute_binary_wins_over_path_discovery() {
        let dir = std::env::temp_dir().join(format!("synth-fastroute-bin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        let binary = dir.join("fastroute");
        std::fs::write(&binary, b"binary").expect("write binary");
        let tools = ToolPaths {
            fastroute_bin: Some(binary.clone()),
            data_dir: Some(dir.clone()),
            ..ToolPaths::default()
        };
        assert_eq!(resolve_fastroute(&tools), Some(binary));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_fastroute_binary_is_reported_as_unavailable() {
        let tools = ToolPaths {
            fastroute_bin: Some(PathBuf::from("/nonexistent/fastroute")),
            data_dir: Some(PathBuf::from("/nonexistent/data")),
            ..ToolPaths::default()
        };
        let request = isolated_request(RouterEngine::Fastroute, tools);
        let capability = discover(&request);
        assert!(!capability.available);
        assert_eq!(
            capability.failure().expect("failure").reason,
            RouterFailureReason::NotInstalled
        );
    }

    #[test]
    fn krt_required_capabilities_cover_the_helper_contract() {
        assert!(!KRT_REQUIRED_CAPABILITIES.is_empty());
        let mut unique = std::collections::BTreeSet::new();
        for token in KRT_REQUIRED_CAPABILITIES {
            assert!(
                unique.insert(*token),
                "duplicate KRT capability token {token}"
            );
            assert!(
                token.contains(':')
                    || Path::new(token)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("py")),
                "a token must name a module or a module and flag: {token}"
            );
        }
        // The helper's two ends of the chain: routing and escape.
        assert!(KRT_REQUIRED_CAPABILITIES.contains(&"route.py:--json-out"));
        assert!(KRT_REQUIRED_CAPABILITIES.contains(&"check_connected.py"));
    }

    #[test]
    fn a_checkout_without_the_capability_manifest_cannot_be_verified() {
        let dir = std::env::temp_dir().join(format!(
            "krt-no-manifest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        let error = verify_krt_capabilities(&dir, Path::new("/usr/bin/python3"))
            .expect_err("a checkout without a manifest must fail verification");
        assert!(error.contains(KRT_CAPABILITIES_SCRIPT), "{error}");
        assert!(error.contains("missing"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// KRT's manifest prints the exact gaps and exits 3. Synth forwards that
    /// answer rather than replacing it with a generic "checkout unusable".
    #[test]
    fn a_checkout_missing_flags_names_the_gaps() {
        let dir = std::env::temp_dir().join(format!(
            "krt-gaps-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        std::fs::write(dir.join(KRT_CAPABILITIES_SCRIPT), "manifest").expect("manifest");
        let python = fake_python(
            &dir,
            "echo '  - route.py --bogus (flag not supported)'\nexit 3",
        );
        let error = verify_krt_capabilities(&dir, &python)
            .expect_err("a checkout that fails --require must fail verification");
        assert!(error.contains("route.py --bogus"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_checkout_that_satisfies_every_requirement_is_verified() {
        let dir = std::env::temp_dir().join(format!(
            "krt-verified-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        std::fs::write(dir.join(KRT_CAPABILITIES_SCRIPT), "manifest").expect("manifest");
        let python = fake_python(&dir, "exit 0");
        assert!(verify_krt_capabilities(&dir, &python).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_directory_without_the_krt_entrypoint_is_not_a_checkout() {
        let dir = std::env::temp_dir().join(format!("synth_krt_probe_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        // Point discovery at a real directory that is not a KRT checkout.
        let request = isolated_request(
            RouterEngine::KiCadRoutingTools,
            ToolPaths {
                krt_repo: Some(dir.clone()),
                ..ToolPaths::default()
            },
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
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovery_and_the_run_resolve_the_same_configured_java() {
        // Discovery used to ignore the configured Java and report the bundled
        // or PATH one, so `synth routers` could disagree with the run.
        let dir = std::env::temp_dir().join(format!("synth_fr_probe_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        let jar = dir.join("freerouting.jar");
        let java = dir.join("java");
        std::fs::write(&jar, b"jar").expect("jar");
        std::fs::write(&java, b"java").expect("java");
        let missing_java = dir.join("no-java-here");

        let configured = ToolPaths {
            freerouting_jar: Some(jar.clone()),
            freerouting_java: Some(java.clone()),
            ..ToolPaths::default()
        };
        assert_eq!(crate::freerouting::resolve_java(&configured), Some(java));
        let request = isolated_request(RouterEngine::Freerouting, configured);
        assert!(
            discover(&request).available,
            "configured jar and java are used"
        );

        let request = isolated_request(
            RouterEngine::Freerouting,
            ToolPaths {
                freerouting_jar: Some(jar),
                freerouting_java: Some(missing_java),
                ..ToolPaths::default()
            },
        );
        assert_eq!(
            discover(&request).failure().expect("failure").reason,
            RouterFailureReason::JavaMissing,
            "a configured Java that does not exist is not silently replaced"
        );
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
