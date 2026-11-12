// SPDX-License-Identifier: Apache-2.0

//! Process-wide logging for the `synth` binary.
//!
//! Everything the compiler says *about its own work* — progress, retries,
//! degraded checks, configuration choices — is a `tracing` event on stderr.
//! Stdout stays reserved for the command's actual output (JSON documents,
//! reports, listings) so a pipeline can parse it without stripping prefixes.
//!
//! The subscriber is installed once, in `main`, before any stage runs.
//! Verbosity comes from `-v`/`-q` on the command line; `SYNTH_LOG` overrides
//! both when an operator needs one crate raised without touching the flags,
//! for example `SYNTH_LOG=synth_router=debug,synth=info`.

use std::io::IsTerminal;

use tracing::level_filters::LevelFilter;
use tracing_subscriber::{fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter};

/// Environment variable holding a `tracing` filter directive.
pub const FILTER_ENV: &str = "SYNTH_LOG";

/// Install the global logger for a command-line invocation.
///
/// `verbose` is the number of `-v` flags: none is `info` (the default the
/// CLI has always printed), one is `debug`, two or more is `trace`. `quiet`
/// asks for errors only. A `SYNTH_LOG` directive always wins, so a crate can
/// be raised or lowered without rebuilding the flags.
///
/// A second call is a no-op: the first subscriber wins. A CLI that is
/// re-entered (an embedded host, a test harness calling `main`) must not
/// abort, and losing duplicate log lines is better than losing the run.
pub fn init(verbose: u8, quiet: bool) {
    let fallback = if quiet {
        LevelFilter::ERROR
    } else {
        match verbose {
            0 => LevelFilter::INFO,
            1 => LevelFilter::DEBUG,
            _ => LevelFilter::TRACE,
        }
    };

    let filter = EnvFilter::builder()
        .with_default_directive(fallback.into())
        .with_env_var(FILTER_ENV)
        .from_env_lossy();

    // Colour only on a terminal, and never when `NO_COLOR` is set: logs are
    // often captured to a file or piped into another tool, where ANSI escapes
    // are noise.
    let ansi = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false)
        .without_time()
        .with_ansi(ansi);

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init();
}
