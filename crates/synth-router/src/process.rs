// SPDX-License-Identifier: Apache-2.0

//! Bounded, cancellable execution of an external process.
//!
//! Every external router runs through here. The bounds are not
//! defensive padding — they are the difference between a router that
//! occasionally fails and a release pipeline that hangs:
//!
//! - **Wall-clock budget.** A JVM that wedges is killed, not waited on.
//! - **Output cap.** A chatty engine cannot exhaust memory, and the
//!   overflow is reported as a finding rather than silently truncated.
//! - **Cancellation.** A cooperative token lets a caller abandon a run
//!   without waiting for the child, so a retried job does not leave the
//!   previous attempt still writing to the same candidate.
//! - **Duplicate-run guard.** Two concurrent runs against one candidate
//!   path are refused, because they would race on the install.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Outcome of one bounded invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The command as executed, for the run record.
    pub command: Vec<String>,
    /// Captured stdout, capped at the requested size.
    pub stdout: String,
    /// Captured stderr, capped at the requested size.
    pub stderr: String,
    pub exit_code: Option<i32>,
    /// The budget elapsed and the child was terminated.
    pub timed_out: bool,
    /// Cancellation was observed and the child was terminated.
    pub cancelled: bool,
    /// Bytes discarded because the cap was reached.
    pub output_truncated: bool,
    /// Working directory the child ran in.
    pub cwd: std::path::PathBuf,
}

impl Invocation {
    /// Whether the process exited `0` within every bound.
    ///
    /// A timeout or a cancellation is never a success, even if the child
    /// managed to exit zero on its way out.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        !self.timed_out && !self.cancelled && self.exit_code == Some(0)
    }

    /// Everything the process wrote, for diagnostics.
    #[must_use]
    pub fn combined_output(&self) -> String {
        let mut text = self.stderr.clone();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&self.stdout);
        text
    }
}

/// Why a process could not be started at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnError {
    pub command: Vec<String>,
    pub detail: String,
    /// `true` when the executable does not exist, which is a
    /// configuration problem rather than a crash.
    pub not_found: bool,
}

/// How often the wait loop re-checks cancellation and the deadline.
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Grace period allowed for pipe readers to drain after termination.
///
/// Without it a child that is killed while holding a full pipe buffer
/// leaves the reader thread blocked forever.
const DRAIN_GRACE: Duration = Duration::from_secs(5);

/// How much of the spawned process tree to stop on timeout or cancellation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KillScope {
    /// Stop only the direct child. The default for router engines.
    #[default]
    Child,
    /// Stop the child and everything in its process group.
    ///
    /// For wrappers that start the engine without `exec`, the engine is not
    /// the direct child; killing only the wrapper leaves it running.
    ProcessTree,
}

/// Run `binary` with `args`, bounded by `budget` and `max_output_bytes`.
///
/// The child's stdin is closed: a router that stops to read stdin would
/// otherwise block forever, since nothing is going to type into it.
///
/// Stops early, as a cancelled run, when the ambient token (see
/// [`set_ambient_cancel`]) is cancelled.
pub fn run(
    binary: &Path,
    args: &[String],
    budget: Duration,
    max_output_bytes: usize,
) -> Result<Invocation, SpawnError> {
    let ambient = ambient_cancel();
    run_cancellable(
        binary,
        args,
        budget,
        max_output_bytes,
        ambient.as_ref(),
        None,
        KillScope::Child,
    )
}

thread_local! {
    static AMBIENT: std::cell::RefCell<Option<CancelToken>> = const { std::cell::RefCell::new(None) };
}

/// The token every [`run`] on this thread watches, if one is set.
fn ambient_cancel() -> Option<CancelToken> {
    AMBIENT.with(|slot| slot.borrow().clone())
}

/// Sets the token that every [`run`] on this thread watches, or clears it.
///
/// A tool call reaches the routers and KiCad through many layers that take no
/// token; the server sets the call's token on the thread that runs the call, so
/// cancelling it ends the external router or `kicad-cli` that call is waiting
/// on, and every later one on that thread starts already cancelled. Threads the
/// call spawns do not inherit it.
pub fn set_ambient_cancel(token: Option<CancelToken>) {
    AMBIENT.with(|slot| *slot.borrow_mut() = token);
}

/// Run with cancellation, a working directory and an explicit kill scope.
pub fn run_cancellable(
    binary: &Path,
    args: &[String],
    budget: Duration,
    max_output_bytes: usize,
    cancel: Option<&CancelToken>,
    cwd: Option<&Path>,
    kill: KillScope,
) -> Result<Invocation, SpawnError> {
    let mut command_repr = Vec::with_capacity(args.len() + 1);
    command_repr.push(binary.display().to_string());
    command_repr.extend(args.iter().cloned());
    let command_repr_for_error = command_repr.clone();

    let mut command = Command::new(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    if kill == KillScope::ProcessTree {
        // The child leads its own group, so stopping the group reaches a
        // grandchild started by a non-exec wrapper.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt as _;
            command.process_group(0);
        }
    }

    let mut child = command.spawn().map_err(|e| SpawnError {
        detail: format!("could not start `{}`: {e}", binary.display()),
        not_found: e.kind() == std::io::ErrorKind::NotFound,
        command: command_repr_for_error.clone(),
    })?;

    let stdout_reader = drain(child.stdout.take(), max_output_bytes);
    let stderr_reader = drain(child.stderr.take(), max_output_bytes);

    let deadline = Instant::now() + budget;
    let mut timed_out = false;
    let mut cancelled = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(e) => {
                stop(&mut child, kill);
                return Err(SpawnError {
                    detail: format!("could not wait on `{}`: {e}", binary.display()),
                    not_found: false,
                    command: command_repr_for_error.clone(),
                });
            }
        }
        if cancel.is_some_and(CancelToken::is_cancelled) {
            cancelled = true;
            stop(&mut child, kill);
            break None;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            stop(&mut child, kill);
            break None;
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    let drain_deadline = Instant::now() + DRAIN_GRACE;
    let (stdout, stdout_truncated) = collect(&stdout_reader, drain_deadline, max_output_bytes);
    let (stderr, stderr_truncated) = collect(&stderr_reader, drain_deadline, max_output_bytes);

    Ok(Invocation {
        command: command_repr,
        stdout,
        stderr,
        exit_code: if timed_out || cancelled {
            None
        } else {
            status.and_then(|s| s.code())
        },
        timed_out,
        cancelled,
        output_truncated: stdout_truncated || stderr_truncated,
        cwd: cwd.map_or_else(|| PathBuf::from("."), Path::to_path_buf),
    })
}

fn stop(child: &mut std::process::Child, kill: KillScope) {
    if kill == KillScope::ProcessTree {
        kill_process_group(child.id());
    }
    // The group kill is best-effort; the direct kill and wait always reap
    // the process the caller owns.
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    use nix::sys::signal::{killpg, Signal};
    use nix::unistd::Pid;

    if let Ok(pid) = i32::try_from(pid) {
        let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
    }
}

#[cfg(windows)]
fn kill_process_group(pid: u32) {
    // Windows has no process group equivalent here; `/T` is the best-effort
    // tree kill. The direct kill below still stops the owned child.
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(any(unix, windows)))]
fn kill_process_group(_pid: u32) {
    // No process-group primitive on this target; the direct child is all we
    // can stop.
}

use std::path::PathBuf;

/// Read a pipe on a background thread, keeping at most `cap` bytes.
///
/// Reading on a separate thread is what prevents the classic deadlock: a
/// child that fills a pipe buffer blocks in `write` until someone reads,
/// and a parent that polls `try_wait` without reading never does.
fn drain(pipe: Option<impl Read + Send + 'static>, cap: usize) -> Receiver<Drain> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut truncated = false;
        if let Some(mut pipe) = pipe {
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if buf.len() + n <= cap {
                            buf.extend_from_slice(&chunk[..n]);
                        } else {
                            let room = cap.saturating_sub(buf.len());
                            buf.extend_from_slice(&chunk[..room]);
                            truncated = true;
                            // Keep draining so the child never blocks on a
                            // full pipe, but stop accumulating.
                        }
                    }
                }
            }
        }
        let _ = tx.send(Drain {
            text: String::from_utf8_lossy(&buf).into_owned(),
            truncated,
        });
    });
    rx
}

struct Drain {
    text: String,
    truncated: bool,
}

fn collect(rx: &Receiver<Drain>, deadline: Instant, cap: usize) -> (String, bool) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    match rx.recv_timeout(remaining) {
        Ok(drain) => (truncate(&drain.text, cap), drain.truncated),
        // A reader that did not report back means the child died with the
        // pipe still open. Partial output is still worth keeping: it is
        // usually the error message that explains the failure.
        Err(_) => (String::new(), false),
    }
}

/// Hard-limit a string to `cap` bytes on a char boundary.
fn truncate(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_string();
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Cooperative cancellation for an external run.
///
/// Checked by [`run_cancellable`] while it waits. Separate from the
/// process handle on purpose: the caller decides *when* to cancel (a
/// retry, a shutdown, a superseded job), and the runner decides *how* to
/// stop cleanly.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: std::sync::Arc<AtomicBool>,
}

impl CancelToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation. Idempotent.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// Refuse concurrent runs that would write the same candidate.
///
/// Two routers writing one candidate is a silent-corruption failure: both
/// succeed, both install, and the winner is whichever `rename` landed
/// last. A named lock turns that race into an explicit error, and is
/// released when the guard drops so a crashed run does not wedge the next
/// one.
#[derive(Debug)]
pub struct RunGuard {
    key: String,
    _held: (),
}

impl RunGuard {
    /// Acquire the lock for `key`, or report that it is already held.
    pub fn acquire(key: impl Into<String>) -> Result<Self, String> {
        let key = key.into();
        let mut active = active_runs()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.contains(&key) {
            return Err(format!(
                "a routing run for {key} is already in progress; wait for it to finish \
                 or cancel it before starting another"
            ));
        }
        active.insert(key.clone());
        drop(active);
        Ok(Self { key, _held: () })
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        active_runs()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.key);
    }
}

fn active_runs() -> &'static Mutex<HashSet<String>> {
    static ACTIVE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Monotonic sequence used to make scratch artifact names unique.
#[must_use]
pub fn next_sequence() -> u64 {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Vec<String> {
        vec!["-c".to_string(), script.to_string()]
    }

    fn shell() -> PathBuf {
        PathBuf::from("/bin/sh")
    }

    #[test]
    fn a_clean_exit_reports_success_and_keeps_output() {
        let run = run(
            &shell(),
            &sh("echo out; echo err >&2"),
            Duration::from_secs(10),
            4096,
        )
        .expect("stub ran");
        assert!(run.succeeded());
        assert_eq!(run.exit_code, Some(0));
        assert_eq!(run.stdout.trim(), "out");
        assert_eq!(run.stderr.trim(), "err");
        assert!(run.combined_output().contains("out"));
        assert!(run.combined_output().contains("err"));
    }

    #[test]
    fn a_nonzero_exit_keeps_its_code_and_output() {
        let run = run(
            &shell(),
            &sh("echo boom >&2; exit 3"),
            Duration::from_secs(10),
            4096,
        )
        .expect("stub ran");
        assert!(!run.succeeded());
        assert_eq!(run.exit_code, Some(3));
        assert!(run.stderr.contains("boom"));
    }

    #[test]
    fn a_hung_process_is_killed_at_the_budget() {
        let started = Instant::now();
        let run =
            run(&shell(), &sh("sleep 60"), Duration::from_millis(300), 4096).expect("stub ran");
        assert!(run.timed_out);
        assert!(!run.succeeded());
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "budget must be enforced, not waited out: {:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn kill_process_tree_stops_a_wrapper_grandchild() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = std::env::temp_dir().join(format!("synth_proc_tree_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        let pid_file = dir.join("child.pid");
        let script = format!(
            "#!/bin/sh\nsleep 60 &\necho $! > '{}'\nwait\n",
            pid_file.display()
        );
        let wrapper = dir.join("wrapper.sh");
        std::fs::write(&wrapper, script).expect("write wrapper");
        let mut permissions = std::fs::metadata(&wrapper)
            .expect("stat wrapper")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&wrapper, permissions).expect("chmod wrapper");

        let started = Instant::now();
        let run = run_cancellable(
            &wrapper,
            &[],
            Duration::from_millis(300),
            4096,
            None,
            None,
            KillScope::ProcessTree,
        )
        .expect("wrapper ran");
        assert!(run.timed_out);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "group kill must close the pipes: {:?}",
            started.elapsed()
        );

        let pid: u32 = std::fs::read_to_string(&pid_file)
            .expect("child pid")
            .trim()
            .parse()
            .expect("numeric pid");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            assert!(
                Instant::now() < deadline,
                "wrapper grandchild {pid} survived"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_run_reports_cancellation_not_timeout() {
        // These are different failures with different remedies, so they
        // must not collapse into one flag.
        let token = CancelToken::new();
        std::thread::spawn({
            let token = token.clone();
            move || {
                std::thread::sleep(Duration::from_millis(150));
                token.cancel();
            }
        });
        let run = run_cancellable(
            &shell(),
            &sh("sleep 60"),
            Duration::from_secs(30),
            4096,
            Some(&token),
            None,
            KillScope::Child,
        )
        .expect("stub ran");
        assert!(run.cancelled);
        assert!(!run.timed_out);
        assert!(!run.succeeded());
        assert_eq!(run.exit_code, None);
    }

    #[test]
    fn the_ambient_token_stops_a_plain_run_and_the_next_one() {
        let token = CancelToken::new();
        set_ambient_cancel(Some(token.clone()));
        std::thread::spawn({
            let token = token.clone();
            move || {
                std::thread::sleep(Duration::from_millis(150));
                token.cancel();
            }
        });
        let started = Instant::now();
        let first = run(&shell(), &sh("sleep 60"), Duration::from_secs(30), 4096).expect("ran");
        let second = run(&shell(), &sh("sleep 60"), Duration::from_secs(30), 4096).expect("ran");
        set_ambient_cancel(None);
        assert!(first.cancelled && second.cancelled);
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "cancelling must stop the work, not wait it out: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn an_already_cancelled_token_stops_before_the_budget() {
        let token = CancelToken::new();
        token.cancel();
        let started = Instant::now();
        let run = run_cancellable(
            &shell(),
            &sh("sleep 60"),
            Duration::from_secs(30),
            4096,
            Some(&token),
            None,
            KillScope::Child,
        )
        .expect("stub ran");
        assert!(run.cancelled);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn output_beyond_the_cap_is_dropped_and_reported() {
        // A run that printed megabytes is a signal in itself; reporting it
        // beats silently keeping the first N bytes and calling it complete.
        let run = run(
            &shell(),
            &sh("i=0; while [ $i -lt 200 ]; do printf '%0.sx' $(seq 1 100); i=$((i+1)); done"),
            Duration::from_secs(20),
            1024,
        )
        .expect("stub ran");
        assert!(run.output_truncated);
        assert_eq!(run.stdout.len(), 1024);
    }

    #[test]
    fn a_chatty_child_does_not_deadlock_the_wait_loop() {
        let run = run(
            &shell(),
            &sh("i=0; while [ $i -lt 500 ]; do printf '%0.sx' $(seq 1 1000) >&2; i=$((i+1)); done"),
            Duration::from_secs(30),
            64 * 1024,
        )
        .expect("stub ran");
        assert!(!run.timed_out, "draining readers must prevent a deadlock");
        assert!(run.stderr.len() > 4096);
    }

    #[test]
    fn a_missing_executable_is_reported_as_not_found() {
        let err = run(
            Path::new("/definitely/not/an/executable"),
            &[],
            Duration::from_secs(5),
            4096,
        )
        .expect_err("cannot run");
        assert!(err.not_found, "{}", err.detail);
    }

    #[test]
    fn the_working_directory_is_honoured() {
        let dir = std::env::temp_dir().join(format!("synth_proc_cwd_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        let run = run_cancellable(
            &shell(),
            &sh("pwd"),
            Duration::from_secs(10),
            4096,
            None,
            Some(&dir),
            KillScope::Child,
        )
        .expect("stub ran");
        assert!(run.succeeded());
        assert_eq!(PathBuf::from(run.stdout.trim()), dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_run_guard_refuses_a_concurrent_duplicate_and_frees_on_drop() {
        let guard = RunGuard::acquire("board.kicad_pcb").expect("first acquire");
        let second = RunGuard::acquire("board.kicad_pcb").expect_err("second must be refused");
        assert!(second.contains("already in progress"), "{second}");
        // A different board is a different lock.
        let other = RunGuard::acquire("other.kicad_pcb").expect("independent lock");
        drop(other);
        drop(guard);
        RunGuard::acquire("board.kicad_pcb").expect("lock released on drop");
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        let text = "héllo wörld";
        let cut = truncate(text, 3);
        assert_eq!(cut, "hé");
        assert_eq!(truncate(text, 1000), text);
        assert_eq!(truncate("abc", 3), "abc");
    }

    #[test]
    fn cancellation_is_idempotent_and_observable() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        token.cancel();
        assert!(token.is_cancelled());
    }
}
