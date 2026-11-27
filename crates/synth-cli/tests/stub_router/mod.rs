// SPDX-License-Identifier: Apache-2.0

//! A stub external router for the CLI integration tests.
//!
//! The release gate is fail-closed on routing: an export with no
//! independently validated route is not fabricable. That means a test that
//! wants to assert anything about the *rest* of the gate — ERC evidence, DRC
//! evidence, the release manifest — has to supply a route, or it will only
//! ever see the routing gate refuse.
//!
//! Installing a stub keeps those tests honest: the pipeline runs its real
//! export, real adapter invocation, and real independent checks, and only
//! the engine's copper is a stand-in.

#![cfg(unix)]
#![allow(dead_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A stub router, described as the environment a `synth` run needs to use it.
///
/// The adapters read `PATH` to find the interpreter that runs the routing
/// helper, and the router environment variables to find the engine. Those go
/// on each `synth` command through [`StubRouter::envs`], never into this test
/// process, so tests holding different stubs can run side by side.
pub struct StubRouter {
    bin_dir: PathBuf,
    envs: Vec<(&'static str, OsString)>,
}

/// Install the stub router for the duration of one test.
///
/// `out_dir` receives the stub JAR and Java stand-ins: the adapter insists
/// both exist before it will start the helper, so they have to be present.
pub fn install(out_dir: &Path) -> StubRouter {
    let bin_dir = out_dir.join("stub-router-bin");
    std::fs::create_dir_all(&bin_dir).expect("stub bin dir");

    // The stub is installed as `python3`, so the adapter invokes it exactly as
    // it invokes the real helper: as the interpreter. Its shebang is rewritten
    // to the absolute path of a real interpreter found *before* this directory
    // goes on PATH, so it cannot resolve back to itself.
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub_router.py");
    let real_python = real_interpreter();
    let body = std::fs::read_to_string(&source).expect("read stub router");
    let interpreter = bin_dir.join("python3");
    std::fs::write(
        &interpreter,
        body.replacen(
            "#!/usr/bin/python3",
            &format!("#!{}", real_python.display()),
            1,
        )
        .replace("@@REAL_PYTHON@@", &real_python.to_string_lossy()),
    )
    .expect("write stub router");
    make_executable(&interpreter);

    let jar = out_dir.join("tools/freerouting/freerouting-stub.jar");
    if let Some(parent) = jar.parent() {
        std::fs::create_dir_all(parent).expect("tools dir");
    }
    std::fs::write(&jar, b"stub jar").expect("write stub jar");
    let java = out_dir.join("tools/jre25/bin/java");
    if let Some(parent) = java.parent() {
        std::fs::create_dir_all(parent).expect("jre dir");
    }
    std::fs::write(&java, b"stub java").expect("write stub java");

    // KiCadRoutingTools is the default engine, so give it a checkout to be
    // available at. The stub interpreter runs in place of both engines'
    // helpers, so an empty `py_router/route.py` is enough to pass the
    // entrypoint check; `SYNTH_KRT_PYTHON` points the adapter at the stub.
    let krt_repo = out_dir.join("krt-stub-repo");
    std::fs::create_dir_all(krt_repo.join("py_router")).expect("krt checkout dir");
    std::fs::write(krt_repo.join("py_router").join("route.py"), b"# stub\n")
        .expect("write krt entrypoint");
    // Capability discovery asks KRT's own manifest for the flags the helper
    // drives. The stub interpreter delegates this probe to the real Python,
    // so an empty manifest is a truthful "this checkout supports everything"
    // for the purposes of a test that supplies its own router.
    std::fs::write(krt_repo.join("krt_capabilities.py"), b"# stub\n")
        .expect("write krt capability manifest");
    // The capability probe imports KRT's compiled `grid_router` module from
    // `rust_router/`; an empty module stands in for the build.
    std::fs::create_dir_all(krt_repo.join("rust_router")).expect("rust_router dir");
    std::fs::write(
        krt_repo.join("rust_router").join("grid_router.py"),
        b"# stub\n",
    )
    .expect("write grid_router stub");

    // The stub's directory goes first on the child's PATH, ahead of this
    // process's own PATH.
    let path = std::env::join_paths(
        std::iter::once(bin_dir.clone()).chain(
            std::env::var_os("PATH")
                .map_or_default(|existing| std::env::split_paths(&existing).collect::<Vec<_>>()),
        ),
    )
    .expect("PATH entries contain no separator");

    let envs = vec![
        ("PATH", path),
        ("SYNTH_FREEROUTING_JAR", jar.into_os_string()),
        ("SYNTH_FREEROUTING_JAVA", java.into_os_string()),
        // Keep the KiCadRoutingTools checkout the run path resolves equal to
        // the one capability discovery reports, so the default engine is the
        // stub whichever surface is asked.
        ("KICAD_ROUTING_TOOLS_REPO", krt_repo.into_os_string()),
        ("SYNTH_KRT_PYTHON", interpreter.into_os_string()),
    ];

    StubRouter { bin_dir, envs }
}

/// Locate a real Python interpreter, ignoring this helper's own directory.
///
/// Searches this process's `PATH`, which never holds the stub's directory, so
/// the hit is never the stub itself.
fn real_interpreter() -> PathBuf {
    for name in ["python3", "python"] {
        let found = Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {name}"))
            .output()
            .expect("probe PATH");
        if found.status.success() {
            let path = String::from_utf8_lossy(&found.stdout).trim().to_string();
            if !path.is_empty() {
                return PathBuf::from(path);
            }
        }
    }
    panic!("no python3 on PATH to run the stub router");
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path).expect("stat stub").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("chmod stub");
}

impl StubRouter {
    /// The directory the stub interpreter lives in.
    pub fn bin_dir(&self) -> &Path {
        &self.bin_dir
    }

    /// The environment a `synth` command needs to route with this stub:
    /// `command.envs(router.envs())`.
    pub fn envs(&self) -> impl Iterator<Item = (&'static str, &OsString)> + '_ {
        self.envs.iter().map(|(key, value)| (*key, value))
    }
}

/// Run `synth` with the stub router installed.
///
/// Convenience for the many gate tests whose only difference is the stub
/// `kicad-cli` they need.
pub fn synth_with_stub_router(synth: &str, scratch: &Path, args: &[&str]) -> std::process::Output {
    let router = install(scratch);
    Command::new(synth)
        .args(args)
        .envs(router.envs())
        .output()
        .expect("run synth")
}
