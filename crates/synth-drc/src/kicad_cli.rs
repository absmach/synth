// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use synth_diagnostics::UnknownReason;

pub const MIN_SUPPORTED_MAJOR: u32 = 8;

pub const DEFAULT_TIMEOUT_SECS: u64 = 300;

const VERSION_PROBE_TIMEOUT_SECS: u64 = 30;

pub const PIPE_DRAIN_GRACE: Duration = Duration::from_secs(5);

pub fn binary() -> String {
    std::env::var("KICAD_CLI").unwrap_or_else(|_| "kicad-cli".to_string())
}

pub fn timeout() -> Duration {
    let secs = std::env::var("SYNTH_KICAD_CLI_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

#[derive(Debug, Clone)]
pub struct Invocation {
    pub command: Vec<String>,
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

impl Invocation {
    pub fn succeeded(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }

    pub fn failure_reason(&self) -> Option<UnknownReason> {
        if self.timed_out {
            Some(UnknownReason::Timeout)
        } else if self.exit_code == Some(0) {
            None
        } else {
            Some(UnknownReason::CommandFailed)
        }
    }

    pub fn failure_detail(&self, budget: Duration) -> String {
        if self.timed_out {
            format!(
                "`{}` did not finish within {}s and was terminated",
                self.command.join(" "),
                budget.as_secs()
            )
        } else {
            match self.exit_code {
                Some(code) => format!("`{}` exited with code {code}", self.command.join(" ")),
                None => format!("`{}` was terminated by a signal", self.command.join(" ")),
            }
        }
    }
}

#[derive(Debug)]
pub struct SpawnFailure {
    pub command: Vec<String>,
    pub reason: UnknownReason,
    pub detail: String,
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

fn collect(rx: &Receiver<Vec<u8>>, deadline: Instant) -> Vec<u8> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    rx.recv_timeout(remaining).unwrap_or_default()
}

pub fn run(args: &[String], budget: Duration) -> Result<Invocation, SpawnFailure> {
    run_binary(&binary(), args, budget)
}

pub fn run_binary(
    bin: &str,
    args: &[String],
    budget: Duration,
) -> Result<Invocation, SpawnFailure> {
    let mut command = Vec::with_capacity(args.len() + 1);
    command.push(bin.to_string());
    command.extend(args.iter().cloned());

    let mut child = match Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(source) => {
            let reason = if source.kind() == std::io::ErrorKind::NotFound {
                UnknownReason::NotInstalled
            } else {
                UnknownReason::SpawnFailed
            };
            return Err(SpawnFailure {
                detail: format!("could not start `{bin}`: {source}"),
                command,
                reason,
            });
        }
    };

    let stdout_reader = drain(child.stdout.take());
    let stderr_reader = drain(child.stderr.take());

    let deadline = Instant::now() + budget;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(source) => {
                let _ = child.kill();
                return Err(SpawnFailure {
                    detail: format!("could not wait on `{bin}`: {source}"),
                    command,
                    reason: UnknownReason::SpawnFailed,
                });
            }
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let drain_deadline = Instant::now() + PIPE_DRAIN_GRACE;
    let stdout = collect(&stdout_reader, drain_deadline);
    let stderr = collect(&stderr_reader, drain_deadline);

    Ok(Invocation {
        command,
        stdout,
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        exit_code: if timed_out {
            None
        } else {
            status.and_then(|s| s.code())
        },
        timed_out,
    })
}

pub fn version() -> Option<String> {
    static VERSION: OnceLock<Option<String>> = OnceLock::new();
    VERSION.get_or_init(probe_version).clone()
}

fn probe_version() -> Option<String> {
    let run = run(
        &["version".to_string()],
        Duration::from_secs(VERSION_PROBE_TIMEOUT_SECS),
    )
    .ok()?;
    if !run.succeeded() {
        return None;
    }
    let text = String::from_utf8_lossy(&run.stdout);
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

pub fn major_version(version: &str) -> Option<u32> {
    let digits: String = version
        .trim()
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

pub fn version_rejection(version: Option<&str>) -> Option<(UnknownReason, String)> {
    let version = version?;
    let major = major_version(version)?;
    if major >= MIN_SUPPORTED_MAJOR {
        return None;
    }
    Some((
        UnknownReason::UnsupportedVersion,
        format!(
            "kicad-cli {version} is older than the minimum supported major \
             version {MIN_SUPPORTED_MAJOR}; its reports have no JSON schema Synth can read"
        ),
    ))
}

#[derive(Debug)]
pub struct ScratchFile {
    path: std::path::PathBuf,
}

impl ScratchFile {
    pub fn reserve(prefix: &str, extension: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("{prefix}_{}_{seq}.{extension}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn arg(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for ScratchFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_version_parses_kicad_formats() {
        assert_eq!(major_version("10.0.1"), Some(10));
        assert_eq!(major_version("8.0.4-unknown-abc123"), Some(8));
        assert_eq!(major_version("  9.0.0\n"), Some(9));
        assert_eq!(major_version("v10.0"), Some(10));
        assert_eq!(major_version("not a version"), None);
    }

    #[test]
    fn versions_below_the_minimum_are_rejected() {
        let (reason, detail) = version_rejection(Some("7.0.11")).expect("7.x is unsupported");
        assert_eq!(reason, UnknownReason::UnsupportedVersion);
        assert!(detail.contains("7.0.11"), "{detail}");
    }

    #[test]
    fn supported_and_future_versions_are_accepted() {
        assert!(version_rejection(Some("8.0.4")).is_none());
        assert!(version_rejection(Some("10.0.1")).is_none());
        assert!(version_rejection(Some("14.2.0")).is_none());
    }

    #[test]
    fn an_unqueryable_version_is_not_a_rejection() {
        assert!(version_rejection(None).is_none());
        assert!(version_rejection(Some("mystery build")).is_none());
    }

    #[test]
    fn scratch_file_paths_are_unique_and_self_cleaning() {
        let a = ScratchFile::reserve("synth_test_scratch", "json");
        let b = ScratchFile::reserve("synth_test_scratch", "json");
        assert_ne!(a.path(), b.path());

        let path = a.path().to_path_buf();
        std::fs::write(&path, b"{}").expect("write scratch");
        assert!(path.exists());
        drop(a);
        assert!(
            !path.exists(),
            "scratch file must not outlive its guard: {}",
            path.display()
        );
    }

    #[test]
    fn reserving_clears_a_colliding_leftover() {
        let first = ScratchFile::reserve("synth_test_collide", "json");
        let path = first.path().to_path_buf();
        std::fs::write(&path, b"stale").expect("write stale report");
        std::mem::forget(first);
        assert!(path.exists());

        let stale = std::fs::read(&path).unwrap();
        assert_eq!(stale, b"stale");
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    mod runner {
        use super::*;

        fn sh(script: &str, budget_ms: u64) -> Result<Invocation, SpawnFailure> {
            run_binary(
                "/bin/sh",
                &["-c".to_string(), script.to_string()],
                Duration::from_millis(budget_ms),
            )
        }

        #[test]
        fn a_missing_executable_is_not_installed() {
            let err = run_binary(
                "synth-definitely-not-a-real-kicad-cli",
                &[],
                Duration::from_secs(5),
            )
            .expect_err("a missing binary cannot run");
            assert_eq!(err.reason, UnknownReason::NotInstalled);
            assert_eq!(err.command[0], "synth-definitely-not-a-real-kicad-cli");
        }

        #[test]
        fn a_nonzero_exit_is_command_failed_and_keeps_stderr() {
            let run = sh("echo 'library not found' >&2; exit 3", 5_000).expect("stub ran");
            assert!(!run.succeeded());
            assert_eq!(run.exit_code, Some(3));
            assert_eq!(run.failure_reason(), Some(UnknownReason::CommandFailed));
            assert!(run.stderr.contains("library not found"), "{}", run.stderr);
            let detail = run.failure_detail(Duration::from_secs(5));
            assert!(detail.contains("code 3"), "{detail}");
        }

        #[test]
        fn a_clean_exit_succeeds_and_keeps_stdout() {
            let run = sh("echo 10.0.1", 5_000).expect("stub ran");
            assert!(run.succeeded());
            assert!(run.failure_reason().is_none());
            assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "10.0.1");
        }

        #[test]
        fn a_hung_tool_is_killed_and_reported_as_timeout() {
            let started = Instant::now();
            let run = sh("sleep 30", 300).expect("stub ran");
            assert!(run.timed_out);
            assert_eq!(run.failure_reason(), Some(UnknownReason::Timeout));
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the budget must be enforced, not waited out: {:?}",
                started.elapsed()
            );
            let detail = run.failure_detail(Duration::from_millis(300));
            assert!(detail.contains("did not finish"), "{detail}");
        }

        #[test]
        fn a_chatty_tool_does_not_deadlock_the_poll_loop() {
            let run = sh(
                "i=0; while [ $i -lt 400 ]; do \
                   printf '%0.sx' $(seq 1 1000) >&2; i=$((i+1)); done; exit 1",
                20_000,
            )
            .expect("stub ran");
            assert!(!run.timed_out, "draining readers must prevent a deadlock");
            assert_eq!(run.failure_reason(), Some(UnknownReason::CommandFailed));
            assert!(
                run.stderr.len() > 64 * 1024,
                "expected more than one pipe buffer of stderr, got {}",
                run.stderr.len()
            );
        }
    }

    #[test]
    fn timeout_falls_back_on_a_zero_or_junk_override() {
        if std::env::var_os("SYNTH_KICAD_CLI_TIMEOUT_SECS").is_none() {
            assert_eq!(timeout(), Duration::from_secs(DEFAULT_TIMEOUT_SECS));
        }
    }
}
