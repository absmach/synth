// SPDX-License-Identifier: Apache-2.0

//! SPICE simulation checks for Synth designs.
//!
//! The seam is [`simulate`]: a compiled [`Board`] plus a [`Testbench`] sidecar
//! produce a [`SimReport`]. [`emit_netlist`] is the text seam used by golden
//! tests. ngspice is an external process; Synth owns the result protocol, so a
//! missing measurement can never read as a pass.

#![forbid(unsafe_code)]
// `Diagnostic` is the workspace's shared, intentionally wide error record;
// boxing it in every private helper would only move the allocation.
#![allow(clippy::result_large_err)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use synth_diagnostics::{Diagnostic, DiagnosticBuilder, EntityRef, Location, Severity, Span};
use synth_ir::{Board, Component, ComponentId, Net, NetId, PinId};
use synth_registry::{SpiceDialect, SpiceModel};
use synth_router::process::{run_cancellable, Invocation, KillScope};

/// ngspice could not be found or started.
pub const E_SYNTH_SIM_001: &str = "E-SYNTH-SIM-001";
/// A part has no SPICE model.
pub const E_SYNTH_SIM_002: &str = "E-SYNTH-SIM-002";
/// A measurement is missing, failed, non-finite or unparseable.
pub const E_SYNTH_SIM_003: &str = "E-SYNTH-SIM-003";
/// A measurement's declared limit check (info when inside, error when outside).
pub const E_SYNTH_SIM_004: &str = "E-SYNTH-SIM-004";
/// ngspice exceeded the time budget and was killed.
pub const E_SYNTH_SIM_005: &str = "E-SYNTH-SIM-005";
/// ngspice reported an error unrelated to a measurement.
pub const E_SYNTH_SIM_006: &str = "E-SYNTH-SIM-006";
/// The testbench is malformed or a name collides after sanitizing.
pub const E_SYNTH_SIM_007: &str = "E-SYNTH-SIM-007";
/// A node is floating or has no DC path.
pub const E_SYNTH_SIM_008: &str = "E-SYNTH-SIM-008";

/// A parsed `<design>.sim.toml` sidecar.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Testbench {
    /// Synth net name that maps to SPICE node `0`.
    pub ground: String,
    #[serde(default)]
    pub source: Vec<Source>,
    pub analysis: Analysis,
    #[serde(default)]
    pub measure: Vec<Measure>,
    #[serde(default)]
    pub options: TestbenchOptions,
    /// Placed parts to leave out of the deck and its topology check.
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// Run-wide knobs; a missing `[options]` table keeps the defaults.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestbenchOptions {
    /// Wall-clock budget in seconds. Defaults to 10.
    #[serde(default)]
    pub timeout: Option<f64>,
    /// `wrdata` output path, resolved beside the sidecar. Requires `ac`.
    #[serde(default)]
    pub plot: Option<String>,
}

/// An independent source added by the testbench.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Synth net name the source drives.
    pub net: String,
    pub kind: SourceKind,
    /// DC value in volts.
    #[serde(default)]
    pub dc: Option<f64>,
    /// AC magnitude in volts.
    #[serde(default)]
    pub ac: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Dc,
}

/// The analysis the deck runs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Analysis {
    #[serde(default)]
    pub op: bool,
    #[serde(default)]
    pub ac: Option<AcAnalysis>,
}

/// A decade sweep for `ac dec <points> <start> <stop>`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcAnalysis {
    #[serde(default = "default_ac_start")]
    pub start: f64,
    #[serde(default = "default_ac_stop")]
    pub stop: f64,
    #[serde(default = "default_ac_points_per_decade")]
    pub points_per_decade: u32,
}

fn default_ac_start() -> f64 {
    1.0
}

fn default_ac_stop() -> f64 {
    1e6
}

fn default_ac_points_per_decade() -> u32 {
    100
}

/// A named measurement with optional pass/fail limits.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measure {
    pub name: String,
    pub kind: MeasureKind,
    /// Synth net name whose voltage is measured.
    #[serde(default)]
    pub node: Option<String>,
    /// Synth reference of the part whose current is measured.
    #[serde(default)]
    pub element: Option<String>,
    /// Relative-dB drop for `relative_db`. Defaults to 3.0.
    #[serde(default)]
    pub drop_db: Option<f64>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasureKind {
    NodeVoltage,
    Current,
    RelativeDb,
}

impl Testbench {
    /// Parse and validate a sidecar from text. Every failure is `E-SYNTH-SIM-007`.
    pub fn from_toml(text: &str) -> Result<Self, Diagnostic> {
        Self::parse(text, None)
    }

    /// Read, parse and validate a `<design>.sim.toml` sidecar. A missing or
    /// unreadable file is `E-SYNTH-SIM-007` too.
    pub fn from_path(path: &Path) -> Result<Self, Diagnostic> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            testbench_error_at(
                Some(path),
                format!("could not read {}: {error}", path.display()),
            )
        })?;
        Self::parse(&text, Some(path))
    }

    fn parse(text: &str, path: Option<&Path>) -> Result<Self, Diagnostic> {
        let testbench: Self = toml::from_str(text).map_err(|error| {
            let span = error.span().map_or(Span::new(0, 0), |range| {
                Span::new(
                    u32::try_from(range.start).unwrap_or(u32::MAX),
                    u32::try_from(range.end).unwrap_or(u32::MAX),
                )
            });
            testbench_error_at_span(path, span, format!("could not parse: {error}"))
        })?;
        testbench.validate_at(path)?;
        Ok(testbench)
    }

    fn validate_at(&self, path: Option<&Path>) -> Result<(), Diagnostic> {
        self.validate_inner()
            .map_err(|diagnostic| with_location(diagnostic, path))
    }

    #[allow(clippy::too_many_lines)]
    fn validate_inner(&self) -> Result<(), Diagnostic> {
        if self.ground.trim().is_empty() {
            return Err(testbench_error("ground net is empty"));
        }
        reject_newline("ground", &self.ground)?;
        if let Some(timeout) = self.options.timeout {
            if !timeout.is_finite() || timeout <= 0.0 || timeout > MAX_TIMEOUT_SECONDS {
                return Err(testbench_error(format!(
                    "`[options] timeout` must be greater than 0 and at most {MAX_TIMEOUT_SECONDS} seconds"
                )));
            }
        }
        if self.analysis.op == self.analysis.ac.is_some() {
            return Err(testbench_error(
                "testbench must select exactly one analysis: `op` or `ac`",
            ));
        }
        if let Some(ac) = &self.analysis.ac {
            if !ac.start.is_finite() || ac.start <= 0.0 {
                return Err(testbench_error(
                    "`[analysis.ac] start` must be a positive finite frequency",
                ));
            }
            if !ac.stop.is_finite() || ac.stop <= ac.start {
                return Err(testbench_error(format!(
                    "`[analysis.ac] stop` must be finite and greater than start {}",
                    ac.start
                )));
            }
            if !(1..=1000).contains(&ac.points_per_decade) {
                return Err(testbench_error(
                    "`[analysis.ac] points_per_decade` must be between 1 and 1000",
                ));
            }
        }
        if let Some(plot) = &self.options.plot {
            if plot.trim().is_empty() {
                return Err(testbench_error("`[options] plot` path is empty"));
            }
            reject_newline("plot", plot)?;
            if plot.contains('\'') {
                return Err(testbench_error(
                    "`[options] plot` path must not contain a single quote; it is emitted single-quoted",
                ));
            }
            if self.analysis.ac.is_none() {
                return Err(testbench_error(
                    "`[options] plot` requires the `ac` analysis",
                ));
            }
        }
        if self.source.is_empty() {
            return Err(testbench_error("testbench has no [[source]]"));
        }
        if self.measure.is_empty() {
            return Err(testbench_error("testbench has no [[measure]]"));
        }

        let mut ignored = HashSet::new();
        for refdes in &self.ignore {
            if refdes.trim().is_empty() {
                return Err(testbench_error("ignore has an empty reference"));
            }
            reject_newline("ignore", refdes)?;
            if !ignored.insert(refdes.as_str()) {
                return Err(testbench_error(format!("ignore lists `{refdes}` twice")));
            }
        }

        let mut names = HashSet::new();
        for source in &self.source {
            if source.net.trim().is_empty() {
                return Err(testbench_error("source net is empty"));
            }
            reject_newline("net", &source.net)?;
            if source.dc.is_none() && source.ac.is_none() {
                return Err(testbench_error(format!(
                    "source on net `{}` has neither a `dc` nor an `ac` value",
                    source.net
                )));
            }
            if let Some(dc) = source.dc {
                if !dc.is_finite() {
                    return Err(testbench_error(format!(
                        "source on net `{}` has a non-finite `dc` value",
                        source.net
                    )));
                }
            }
            if let Some(ac) = source.ac {
                if !ac.is_finite() {
                    return Err(testbench_error(format!(
                        "source on net `{}` has a non-finite `ac` value",
                        source.net
                    )));
                }
                if ac < 0.0 {
                    return Err(testbench_error(format!(
                        "source on net `{}` has a negative `ac` value",
                        source.net
                    )));
                }
            }
        }

        for measure in &self.measure {
            if !valid_sentinel_name(&measure.name) {
                return Err(testbench_error(format!(
                    "measure name `{}` must be letters, digits, `_`, `-` or `.`",
                    measure.name
                )));
            }
            if !names.insert(measure.name.as_str()) {
                return Err(testbench_error(format!(
                    "measure name `{}` is declared twice",
                    measure.name
                )));
            }
            match measure.kind {
                MeasureKind::NodeVoltage | MeasureKind::RelativeDb => {
                    match measure.node.as_deref() {
                        Some(node) if !node.trim().is_empty() => {
                            reject_newline("node", node)?;
                        }
                        _ => {
                            return Err(testbench_error(format!(
                                "measure `{}` has an empty node",
                                measure.name
                            )));
                        }
                    }
                    if measure.element.is_some() {
                        return Err(testbench_error(format!(
                            "measure `{}` reads a node and must not have an `element`",
                            measure.name
                        )));
                    }
                    if measure.kind == MeasureKind::NodeVoltage && measure.drop_db.is_some() {
                        return Err(testbench_error(format!(
                            "measure `{}` is a node_voltage measure and must not have `drop_db`",
                            measure.name
                        )));
                    }
                    if let Some(drop_db) = measure.drop_db {
                        if !drop_db.is_finite() || drop_db <= 0.0 {
                            return Err(testbench_error(format!(
                                "measure `{}` has a `drop_db` that must be finite and greater than 0",
                                measure.name
                            )));
                        }
                    }
                }
                MeasureKind::Current => {
                    match measure.element.as_deref() {
                        Some(element) if !element.trim().is_empty() => {
                            reject_newline("element", element)?;
                        }
                        _ => {
                            return Err(testbench_error(format!(
                                "measure `{}` has an empty element",
                                measure.name
                            )));
                        }
                    }
                    if measure.node.is_some() {
                        return Err(testbench_error(format!(
                            "measure `{}` is a current measure and must not have a `node`",
                            measure.name
                        )));
                    }
                    if measure.drop_db.is_some() {
                        return Err(testbench_error(format!(
                            "measure `{}` is a current measure and must not have `drop_db`",
                            measure.name
                        )));
                    }
                }
            }
            let requires_ac = measure.kind == MeasureKind::RelativeDb;
            if requires_ac && self.analysis.ac.is_none() {
                return Err(testbench_error(format!(
                    "measure `{}` is a relative_db measure and requires the `ac` analysis",
                    measure.name
                )));
            }
            if requires_ac && !self.source.iter().any(|source| source.ac.is_some()) {
                return Err(testbench_error(format!(
                    "measure `{}`: relative_db needs a source with `ac`",
                    measure.name
                )));
            }
            if !requires_ac && !self.analysis.op {
                return Err(testbench_error(format!(
                    "measure `{}` requires the `op` analysis",
                    measure.name
                )));
            }
            for (label, value) in [("min", measure.min), ("max", measure.max)] {
                if let Some(value) = value {
                    if !value.is_finite() {
                        return Err(testbench_error(format!(
                            "measure `{}` has a non-finite `{label}`",
                            measure.name
                        )));
                    }
                }
            }
            if let (Some(min), Some(max)) = (measure.min, measure.max) {
                if min > max {
                    return Err(testbench_error(format!(
                        "measure `{}` has min {min} greater than max {max}",
                        measure.name
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Options for one simulation run.
#[derive(Debug, Clone, Default)]
pub struct SimOptions {
    /// Path to the ngspice binary. `None` uses `NGSPICE`, then `PATH`.
    pub ngspice: Option<PathBuf>,
    /// Design source file, used to anchor `-002` and `-008`.
    pub design_path: Option<PathBuf>,
    /// `<design>.sim.toml`, used to anchor run-level diagnostics.
    pub testbench_path: Option<PathBuf>,
    /// Registry directories a `[spice]` model file may be resolved from.
    pub registry_dirs: Vec<PathBuf>,
}

impl SimOptions {
    fn binary(&self) -> PathBuf {
        self.ngspice
            .clone()
            .or_else(|| std::env::var_os("NGSPICE").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("ngspice"))
    }
}

/// One measured value and its declared limits.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasureResult {
    pub name: String,
    pub value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl MeasureResult {
    pub fn passed(&self) -> bool {
        matches!(
            limit_outcome(self.min, self.max, self.value),
            LimitOutcome::Pass
        )
    }
}

/// ngspice prints measured values with six significant digits. Half a unit
/// in the last printed digit is the most the true value can differ from the
/// printed one, so a limit inside that band is treated as equal.
fn print_half_ulp(value: f64) -> f64 {
    if value == 0.0 {
        return 0.0;
    }
    let exponent = value.abs().log10().floor();
    0.5 * 10f64.powf(exponent - 5.0)
}

/// One comparison used by both the public pass predicate and the diagnostic.
enum LimitOutcome {
    Pass,
    Below(f64),
    Above(f64),
}

fn at_print_precision(value: f64, limit: f64) -> bool {
    (value - limit).abs() <= print_half_ulp(value)
}

fn limit_outcome(min: Option<f64>, max: Option<f64>, value: f64) -> LimitOutcome {
    if let Some(min) = min {
        if value < min && !at_print_precision(value, min) {
            return LimitOutcome::Below(min);
        }
    }
    if let Some(max) = max {
        if value > max && !at_print_precision(value, max) {
            return LimitOutcome::Above(max);
        }
    }
    LimitOutcome::Pass
}

/// What a simulation produced. Blocking diagnostics mean it did not pass.
#[derive(Debug, Clone)]
pub struct SimReport {
    pub measures: Vec<MeasureResult>,
    pub diagnostics: Vec<Diagnostic>,
}

impl SimReport {
    pub fn passed(&self) -> bool {
        !self.diagnostics.iter().any(|d| d.severity.is_blocking())
    }
}

/// `<design>.sim.toml` beside the design.
pub fn sidecar_path(design: &Path) -> PathBuf {
    design.with_extension("sim.toml")
}

/// Simulate a compiled design against its testbench sidecar.
///
/// A missing measurement, a failed run or an out-of-limit value is an error
/// diagnostic, never a silent pass.
#[allow(clippy::too_many_lines)]
pub fn simulate(design: &Board, testbench: &Testbench, options: &SimOptions) -> SimReport {
    let (deck, dialect, plot) = match emit_deck(
        design,
        testbench,
        options.design_path.as_deref(),
        options.testbench_path.as_deref(),
        &options.registry_dirs,
    ) {
        Ok(emitted) => emitted,
        Err(diagnostics) => return failed(diagnostics, options.testbench_path.as_deref()),
    };

    // `wrdata` fails when the output directory is absent; create it before
    // ngspice starts so the failure is a start-up error, not a lost plot.
    if let Some(plot) = &plot {
        if let Some(parent) = plot.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                return failed(
                    vec![testbench_error_at(
                        options.testbench_path.as_deref(),
                        format!(
                            "could not create the plot directory `{}`: {error}",
                            parent.display()
                        ),
                    )],
                    options.testbench_path.as_deref(),
                );
            }
        }
    }

    let timeout = testbench
        .options
        .timeout
        .map_or(DEFAULT_TIMEOUT, Duration::from_secs_f64);
    let run = match run_ngspice(&deck, dialect, options, timeout) {
        Ok(run) => run,
        Err(diagnostic) => return failed(vec![diagnostic], options.testbench_path.as_deref()),
    };

    // A capped log can hide the singular-matrix warning that would have
    // failed the run, so a partial capture is never trusted.
    if run.output_truncated {
        return failed(
            vec![ngspice_error(format!(
                "ngspice output exceeded the {MAX_OUTPUT_BYTES}-byte cap; refusing to trust a truncated log"
            ))],
            options.testbench_path.as_deref(),
        );
    }

    if run.exit_code != Some(0) {
        let status = run
            .exit_code
            .map_or_else(|| "signal".to_string(), |code| code.to_string());
        let detail = if run.stderr.trim().is_empty() {
            excerpt(&run.stdout)
        } else {
            excerpt(&run.stderr)
        };
        return failed(
            vec![ngspice_error(format!(
                "ngspice exited with status {status}: {detail}"
            ))],
            options.testbench_path.as_deref(),
        );
    }

    // A singular matrix, including one with a missing SYNTH_DONE, is a
    // broken run and must classify as `-006` before measurement parsing.
    if let Some(diagnostic) = convergence_stderr_error(&run.stderr) {
        return failed(vec![diagnostic], options.testbench_path.as_deref());
    }

    // `Error: measure ...` is a measurement failure even when ngspice exits
    // 0. It runs after the singular check so a failed op that also drops a
    // vector is `-006`, not a measurement failure.
    if let Some(diagnostic) = measurement_failure_from_stderr(&run.stderr) {
        return failed(vec![diagnostic], options.testbench_path.as_deref());
    }

    // Other stderr errors must classify as `-006` before sentinel parsing
    // turns the same run into a missing measurement.
    if let Some(diagnostic) = stderr_error(&run.stderr) {
        return failed(vec![diagnostic], options.testbench_path.as_deref());
    }

    let values = match parse_sentinels(&run.stdout, testbench) {
        Ok(values) => values,
        Err(diagnostics) => return failed(diagnostics, options.testbench_path.as_deref()),
    };

    let mut measures = Vec::new();
    let mut diagnostics = Vec::new();
    for measure in &testbench.measure {
        let value = values[measure.name.as_str()];
        let result = MeasureResult {
            name: measure.name.clone(),
            value,
            min: measure.min,
            max: measure.max,
        };
        match limit_outcome(measure.min, measure.max, value) {
            LimitOutcome::Pass => {
                if measure.min.is_some() || measure.max.is_some() {
                    diagnostics.push(limit_info(&measure.name, value));
                }
            }
            LimitOutcome::Below(min) => diagnostics.push(limit_error(
                &measure.name,
                value,
                &format!("below min {min}"),
            )),
            LimitOutcome::Above(max) => diagnostics.push(limit_error(
                &measure.name,
                value,
                &format!("above max {max}"),
            )),
        }
        measures.push(result);
    }

    locate(&mut diagnostics, options.testbench_path.as_deref());
    SimReport {
        measures,
        diagnostics,
    }
}

fn failed(mut diagnostics: Vec<Diagnostic>, path: Option<&Path>) -> SimReport {
    locate(&mut diagnostics, path);
    SimReport {
        measures: Vec::new(),
        diagnostics,
    }
}

/// Anchor diagnostics that have no source location of their own to the
/// sidecar. Design-anchored diagnostics already carry their component span.
fn locate(diagnostics: &mut [Diagnostic], path: Option<&Path>) {
    let Some(path) = path else {
        return;
    };
    let file = path.display().to_string();
    for diagnostic in diagnostics {
        if diagnostic.location.is_none() {
            diagnostic.location = Some(Location::from_span(file.clone(), Span::new(0, 0)));
        }
    }
}

/// Default wall-clock budget, overridable with `[options] timeout`.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// Upper bound on `[options] timeout`; `Instant + Duration` must not overflow.
const MAX_TIMEOUT_SECONDS: f64 = 3600.0;
/// ngspice logs are small; this only bounds a runaway `.print` or `.plot`.
const MAX_OUTPUT_BYTES: usize = 1 << 20;

/// Run ngspice in a fresh temp directory with its own `.spiceinit`, so a
/// user's `~/.spiceinit` and concurrent runs cannot interfere.
fn run_ngspice(
    deck: &str,
    dialect: Option<SpiceDialect>,
    options: &SimOptions,
    timeout: Duration,
) -> Result<Invocation, Diagnostic> {
    let dir = TempDir::create()
        .map_err(|e| ngspice_error(format!("could not create a temporary directory: {e}")))?;
    let mut spiceinit = String::from("set strict_errorhandling\n");
    if let Some(dialect) = dialect {
        let _ = writeln!(spiceinit, "set ngbehavior={}", dialect.name());
    }
    std::fs::write(dir.path().join(".spiceinit"), spiceinit)
        .map_err(|e| ngspice_error(format!("could not write .spiceinit: {e}")))?;
    std::fs::write(dir.path().join("deck.cir"), deck)
        .map_err(|e| ngspice_error(format!("could not write deck.cir: {e}")))?;

    let binary = options.binary();
    let args = ["-b".to_string(), "deck.cir".to_string()];
    let run = run_cancellable(
        &binary,
        &args,
        timeout,
        MAX_OUTPUT_BYTES,
        None,
        Some(dir.path()),
        KillScope::ProcessTree,
    )
    .map_err(|error| {
        if error.not_found {
            sim_builder(E_SYNTH_SIM_001, Severity::Error, "ngspice not found")
                .message(format!(
                    "could not start `{}`; install ngspice ({}), or point `NGSPICE` at the binary",
                    binary.display(),
                    install_hint()
                ))
                .build()
        } else {
            ngspice_error(error.detail)
        }
    })?;

    if run.timed_out {
        return Err(timeout_error(timeout.as_secs_f64()));
    }
    Ok(run)
}

fn install_hint() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "Debian/Ubuntu: sudo apt install ngspice"
    }
    #[cfg(target_os = "macos")]
    {
        "macOS: brew install ngspice"
    }
    #[cfg(windows)]
    {
        "Windows: choco install ngspice"
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        "https://ngspice.sourceforge.io/download.html"
    }
}

fn parse_sentinels(
    stdout: &str,
    testbench: &Testbench,
) -> Result<HashMap<String, f64>, Vec<Diagnostic>> {
    let expected: HashSet<&str> = testbench
        .measure
        .iter()
        .map(|measure| measure.name.as_str())
        .collect();
    let mut values = HashMap::new();
    let mut reported = HashSet::new();
    let mut diagnostics = Vec::new();
    let mut done = false;

    for line in stdout.lines() {
        let line = line.trim_end_matches('\r');
        if line == "SYNTH_DONE" {
            done = true;
            continue;
        }
        let Some(rest) = line.strip_prefix("SYNTH_MEAS ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let name = parts.next();
        if let Some(name) = name {
            reported.insert(name.to_string());
        }
        let (Some(name), Some(raw), None) = (name, parts.next(), parts.next()) else {
            diagnostics.push(measure_error(format!(
                "malformed measurement sentinel `{line}`"
            )));
            continue;
        };
        if !expected.contains(name) {
            diagnostics.push(measure_error(format!(
                "unexpected measurement `{name}` in ngspice output"
            )));
            continue;
        }
        if values.contains_key(name) {
            diagnostics.push(measure_error(format!(
                "measurement `{name}` was printed twice"
            )));
            continue;
        }
        let Ok(value) = raw.parse::<f64>() else {
            diagnostics.push(measure_error(format!(
                "measurement `{name}` has unparseable value `{raw}`"
            )));
            continue;
        };
        if !value.is_finite() {
            diagnostics.push(measure_error(format!(
                "measurement `{name}` is not finite: `{raw}`"
            )));
            continue;
        }
        values.insert(name.to_string(), value);
    }

    if !done {
        diagnostics.push(measure_error("ngspice did not print SYNTH_DONE"));
    }
    for measure in &testbench.measure {
        if !values.contains_key(&measure.name) && !reported.contains(&measure.name) {
            diagnostics.push(measure_error(format!(
                "measurement `{}` was not printed",
                measure.name
            )));
        }
    }

    if diagnostics.is_empty() {
        Ok(values)
    } else {
        Err(diagnostics)
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn create() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("synth-sim-{}-{nanos}-{nonce}", std::process::id()));
            // `create_dir` fails if anything already owns the path, so a
            // pre-created symlink cannot redirect the run.
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not create a unique temporary directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sim_builder(code: &str, severity: Severity, title: &str) -> DiagnosticBuilder {
    DiagnosticBuilder::new(code, severity, title)
        .explanation_url(format!("synth.docs/diagnostics/{code}"))
}

fn measure_error(message: impl Into<String>) -> Diagnostic {
    sim_builder(E_SYNTH_SIM_003, Severity::Error, "measurement failed")
        .message(message)
        .build()
}

fn limit_error(name: &str, value: f64, direction: &str) -> Diagnostic {
    sim_builder(E_SYNTH_SIM_004, Severity::Error, "measurement limit")
        .message(format!(
            "measure `{name}` = {value} is {direction} (ngspice prints 6 significant digits)"
        ))
        .build()
}

fn limit_info(name: &str, value: f64) -> Diagnostic {
    sim_builder(E_SYNTH_SIM_004, Severity::Info, "measurement limit")
        .message(format!(
            "measure `{name}` = {value} is within its declared limits (ngspice prints 6 significant digits)"
        ))
        .build()
}

fn timeout_error(seconds: f64) -> Diagnostic {
    sim_builder(E_SYNTH_SIM_005, Severity::Error, "ngspice timed out")
        .message(format!(
            "ngspice was killed after {seconds}s. \
             Raise `[options] timeout` or shorten the analysis."
        ))
        .build()
}

/// Trimmed stderr lines whose lowercased form matches `predicate`.
fn stderr_offenders(stderr: &str, predicate: impl Fn(&str) -> bool) -> Vec<&str> {
    stderr
        .lines()
        .map(str::trim)
        .filter(|line| predicate(&line.to_ascii_lowercase()))
        .collect()
}

/// A failed measurement prints either `Error: measure ...` or, for the
/// generated `let mN` vector, `Error: &mN: no such variable`.
fn measurement_failure_from_stderr(stderr: &str) -> Option<Diagnostic> {
    let failed = stderr_offenders(stderr, |line| {
        line.contains("error: measure") || line.contains("no such variable")
    });
    if failed.is_empty() {
        return None;
    }
    Some(measure_error(format!(
        "ngspice reported a failed measurement: {}",
        excerpt(&failed.join("\n"))
    )))
}

/// A singular matrix, or the convergence failure it causes, makes the
/// operating point wrong even when ngspice recovers and exits 0.
fn convergence_stderr_error(stderr: &str) -> Option<Diagnostic> {
    let offenders = stderr_offenders(stderr, |line| {
        line.contains("singular matrix")
            || line.contains("gmin stepping failed")
            || line.contains("source stepping failed")
    });
    if offenders.is_empty() {
        return None;
    }
    Some(ngspice_error(format!(
        "ngspice reported a singular matrix or failed convergence even though it exited 0: {}",
        excerpt(&offenders.join("\n"))
    )))
}

/// ngspice can exit 0 with a wrong result and say so only on stderr.
fn stderr_error(stderr: &str) -> Option<Diagnostic> {
    let offenders = stderr_offenders(stderr, |line| {
        line.starts_with("error:") || line.contains("no such subckt")
    });
    if offenders.is_empty() {
        return None;
    }
    Some(ngspice_error(format!(
        "ngspice reported an error even though it exited 0: {}",
        excerpt(&offenders.join("\n"))
    )))
}

fn topology_error(net: &str, message: impl Into<String>) -> Diagnostic {
    sim_builder(
        E_SYNTH_SIM_008,
        Severity::Error,
        "floating node or no DC path",
    )
    .message(message)
    .primary_entity(EntityRef::Net {
        name: net.to_string(),
    })
    .build()
}

fn ngspice_error(message: impl Into<String>) -> Diagnostic {
    sim_builder(E_SYNTH_SIM_006, Severity::Error, "ngspice error")
        .message(message)
        .build()
}

fn excerpt(text: &str) -> String {
    const LIMIT: usize = 400;
    let trimmed = text.trim();
    let excerpt: String = trimmed.chars().take(LIMIT).collect();
    if excerpt.len() < trimmed.len() {
        format!("{excerpt}…")
    } else {
        excerpt
    }
}

/// Emit the SPICE deck for a design and testbench. Stable order, no
/// timestamps; every failure is returned as a diagnostic.
pub fn emit_netlist(design: &Board, testbench: &Testbench) -> Result<String, Vec<Diagnostic>> {
    emit_netlist_with_registry_dirs(design, testbench, &[])
}

/// Like [`emit_netlist`], but resolves `[spice]` model files under
/// `registry_dirs`.
pub fn emit_netlist_with_registry_dirs(
    design: &Board,
    testbench: &Testbench,
    registry_dirs: &[PathBuf],
) -> Result<String, Vec<Diagnostic>> {
    emit_deck(design, testbench, None, None, registry_dirs).map(|(deck, _, _)| deck)
}

#[allow(clippy::too_many_lines)]
fn emit_deck(
    design: &Board,
    testbench: &Testbench,
    design_path: Option<&Path>,
    testbench_path: Option<&Path>,
    registry_dirs: &[PathBuf],
) -> Result<(String, Option<SpiceDialect>, Option<PathBuf>), Vec<Diagnostic>> {
    if let Err(diagnostic) = testbench.validate_at(testbench_path) {
        return Err(vec![diagnostic]);
    }
    let plot_path = match &testbench.options.plot {
        Some(plot) => match resolve_plot_path(plot, testbench_path) {
            Ok(path) => Some(path),
            Err(diagnostic) => return Err(vec![diagnostic]),
        },
        None => None,
    };
    let mut diagnostics = Vec::new();
    let tb_error = |message: String| testbench_error_at(testbench_path, message);

    let Some(ground) = design.nets.iter().find(|net| net.name == testbench.ground) else {
        return Err(vec![tb_error(format!(
            "ground net `{}` not found in design",
            testbench.ground
        ))]);
    };
    let net_by_name: HashMap<&str, &Net> = design
        .nets
        .iter()
        .map(|net| (net.name.as_str(), net))
        .collect();

    // SPICE is case-insensitive; catch names that collapse to one node.
    let mut node_by_net: HashMap<NetId, String> = HashMap::new();
    let mut seen_nodes: HashMap<String, String> = HashMap::new();
    for net in &design.nets {
        let node = if net.id == ground.id {
            "0".to_string()
        } else {
            format!("n_{}", sanitize(&net.name))
        };
        if node != "0" {
            let key = node.to_ascii_lowercase();
            if let Some(previous) = seen_nodes.insert(key, net.name.clone()) {
                if previous != net.name {
                    diagnostics.push(tb_error(format!(
                        "nets `{previous}` and `{}` both emit as `{node}`",
                        net.name
                    )));
                }
            }
        }
        node_by_net.insert(net.id, node);
    }

    let mut source_lines = Vec::new();
    for (index, source) in testbench.source.iter().enumerate() {
        let Some(net) = net_by_name.get(source.net.as_str()) else {
            diagnostics.push(tb_error(format!(
                "source net `{}` not found in design",
                source.net
            )));
            continue;
        };
        if net.id == ground.id {
            diagnostics.push(tb_error(format!(
                "source net `{}` is the ground net",
                source.net
            )));
            continue;
        }
        if source.dc.is_none() && source.ac.is_none() {
            diagnostics.push(tb_error(format!(
                "source on net `{}` has neither a `dc` nor an `ac` value",
                source.net
            )));
            continue;
        }
        let mut line = format!("V{} {} 0", index + 1, node_by_net[&net.id]);
        if let Some(dc) = source.dc {
            let _ = write!(line, " DC {}", exponent(dc));
        }
        if let Some(ac) = source.ac {
            let _ = write!(line, " AC {}", exponent(ac));
        }
        source_lines.push(line);
    }

    let mut ignored_ids: HashSet<ComponentId> = HashSet::new();
    for refdes in &testbench.ignore {
        match design
            .components
            .iter()
            .find(|component| &component.refdes == refdes)
        {
            Some(component) => {
                ignored_ids.insert(component.id);
            }
            None => diagnostics.push(tb_error(format!(
                "ignore lists `{refdes}`, which is not a placed part"
            ))),
        }
    }

    let mut pin_net: HashMap<(ComponentId, PinId), NetId> = HashMap::new();
    for net in &design.nets {
        for endpoint in &net.endpoints {
            pin_net.insert((endpoint.component, endpoint.pin), net.id);
        }
    }

    check_topology(
        design,
        testbench,
        ground.id,
        &ignored_ids,
        &pin_net,
        design_path,
        &mut diagnostics,
    );

    let roots: Vec<PathBuf> = registry_dirs
        .iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .collect();
    let mut include_lines = Vec::new();
    let mut included_files: HashSet<PathBuf> = HashSet::new();
    let mut dialects: Vec<(Option<SpiceDialect>, String, String)> = Vec::new();
    let mut element_lines = Vec::new();
    let mut model_lines = Vec::new();
    let mut seen_elements: HashMap<String, String> = HashMap::new();
    for component in &design.components {
        if ignored_ids.contains(&component.id) {
            continue;
        }
        let model = match model_for(component) {
            Ok(model) => model,
            Err(message) => {
                diagnostics.push(no_model_error(component, &message, design_path));
                continue;
            }
        };
        let nodes = if let Model::Subckt = &model {
            let part = component
                .part
                .as_ref()
                .expect("a subcircuit model comes from a registry part");
            let spice = part
                .spice
                .as_ref()
                .expect("a subcircuit model comes from `[spice]`");
            let indexes = match subckt_pin_indexes(part, spice) {
                Ok(indexes) => indexes,
                Err(message) => {
                    diagnostics.push(no_model_error(component, &message, design_path));
                    continue;
                }
            };
            let file = match resolve_spice_model(spice, &roots) {
                Ok(file) => file,
                Err(message) => {
                    diagnostics.push(no_model_error(component, &message, design_path));
                    continue;
                }
            };
            if included_files.insert(file.clone()) {
                let Some(path) = file.to_str() else {
                    diagnostics.push(no_model_error(
                        component,
                        "model file path is not valid UTF-8",
                        design_path,
                    ));
                    continue;
                };
                if path.contains('"') || path.contains(['\n', '\r']) {
                    diagnostics.push(no_model_error(
                        component,
                        &format!("model file path `{path}` cannot be quoted in an `.include`"),
                        design_path,
                    ));
                    continue;
                }
                include_lines.push(format!(".include \"{path}\""));
            }
            let dialect = spice.dialect;
            if let Some((known, known_refdes, known_part)) = dialects.first() {
                if *known != dialect {
                    diagnostics.push(no_model_error(
                        component,
                        &format!(
                            "SPICE dialect `{}` conflicts with `{known_refdes}` \
                             (`{known_part}`) using `{}`; all parts in one run must agree",
                            dialect_name(dialect),
                            dialect_name(*known)
                        ),
                        design_path,
                    ));
                }
            }
            if !dialects.iter().any(|(known, _, _)| *known == dialect) {
                dialects.push((
                    dialect,
                    component.refdes.clone(),
                    part.id.as_str().to_string(),
                ));
            }
            match connected_nodes(
                component,
                &indexes,
                &pin_net,
                &node_by_net,
                design_path,
                &mut diagnostics,
            ) {
                Some(nodes) => nodes,
                None => continue,
            }
        } else {
            let pin_count = component.part.as_ref().map_or(0, |part| part.pins.len());
            if pin_count != 2 {
                diagnostics.push(no_model_error(
                    component,
                    &format!("a two-terminal model needs 2 pins, part has {pin_count}"),
                    design_path,
                ));
                continue;
            }
            match connected_nodes(
                component,
                &model.pins(),
                &pin_net,
                &node_by_net,
                design_path,
                &mut diagnostics,
            ) {
                Some(nodes) => nodes,
                None => continue,
            }
        };

        let letter = model.letter();
        let name = format!("{letter}{}", sanitize(&component.refdes));
        if name.len() == letter.len() {
            diagnostics.push(tb_error(format!(
                "component `{}` has an empty reference",
                component.refdes
            )));
            continue;
        }
        let key = name.to_ascii_lowercase();
        if let Some(previous) = seen_elements.insert(key, component.refdes.clone()) {
            if previous != component.refdes {
                diagnostics.push(tb_error(format!(
                    "references `{previous}` and `{}` both emit as `{name}`",
                    component.refdes
                )));
                continue;
            }
        }
        match model {
            Model::Value { value, .. } => element_lines.push(format!(
                "{name} {} {} {}",
                nodes[0],
                nodes[1],
                exponent(value)
            )),
            Model::Diode {
                saturation_current, ..
            } => {
                element_lines.push(format!("{name} {} {} {name}", nodes[0], nodes[1]));
                model_lines.push(format!(
                    ".model {name} D(IS={} N={})",
                    model_parameter(saturation_current),
                    DIODE_EMISSION_COEFFICIENT
                ));
            }
            Model::Subckt => {
                let spice = component
                    .part
                    .as_ref()
                    .and_then(|part| part.spice.as_ref())
                    .expect("a subcircuit model comes from `[spice]`");
                let mut params = String::new();
                for (name, value) in &spice.params {
                    let _ = write!(params, " {name}={value}");
                }
                element_lines.push(format!(
                    "{name} {} {}{params}",
                    nodes.join(" "),
                    spice.subckt
                ));
            }
        }
    }

    let sourced: HashSet<&str> = testbench
        .source
        .iter()
        .map(|source| source.net.as_str())
        .collect();
    let mut measure_lines = Vec::new();
    let mut current_saves = BTreeSet::new();
    for (index, measure) in testbench.measure.iter().enumerate() {
        match measure.kind {
            MeasureKind::NodeVoltage | MeasureKind::RelativeDb => {
                let node_name = measure
                    .node
                    .as_deref()
                    .expect("a validated node measure has a node");
                let Some(net) = net_by_name.get(node_name) else {
                    diagnostics.push(tb_error(format!(
                        "measure `{}` node `{node_name}` not found in design",
                        measure.name
                    )));
                    continue;
                };
                if net.id == ground.id {
                    diagnostics.push(tb_error(format!(
                        "measure `{}` reads ground net `{}`; ground is 0 V by definition \
                         and ngspice has no vector for it",
                        measure.name, net.name
                    )));
                    continue;
                }
                let simulated = net
                    .endpoints
                    .iter()
                    .any(|endpoint| !ignored_ids.contains(&endpoint.component));
                if !simulated && !sourced.contains(net.name.as_str()) {
                    diagnostics.push(with_location_span(
                        topology_error(
                            &net.name,
                            format!(
                                "measure `{}` reads net `{}`, which has no simulated parts",
                                measure.name, net.name
                            ),
                        ),
                        design_path,
                        net_span(design, net),
                    ));
                    continue;
                }
                let node = &node_by_net[&net.id];
                if measure.kind == MeasureKind::RelativeDb {
                    let drop_db = measure.drop_db.unwrap_or(3.0);
                    let mut lines = String::new();
                    let _ = write!(
                        lines,
                        "meas ac synth_max{index} max vdb({node})\n\
                         let synth_target{index} = synth_max{index} - {}\n\
                         meas ac synth_corner{index} when vdb({node}) = synth_target{index}\n\
                         echo SYNTH_MEAS {} $&synth_corner{index}",
                        exponent(drop_db),
                        measure.name
                    );
                    measure_lines.push(lines);
                } else {
                    measure_lines.push(format!(
                        "let m{index} = v({node})\necho SYNTH_MEAS {} $&m{index}",
                        measure.name
                    ));
                }
            }
            MeasureKind::Current => {
                let refdes = measure
                    .element
                    .as_deref()
                    .expect("a validated current measure has an element");
                let Some(component) = design
                    .components
                    .iter()
                    .find(|component| component.refdes == refdes)
                else {
                    diagnostics.push(tb_error(format!(
                        "measure `{}` element `{refdes}` is not a placed part",
                        measure.name
                    )));
                    continue;
                };
                if ignored_ids.contains(&component.id) {
                    diagnostics.push(tb_error(format!(
                        "measure `{}` element `{refdes}` is ignored by the testbench \
                         and cannot be measured",
                        measure.name
                    )));
                    continue;
                }
                let Ok(model) = model_for(component) else {
                    diagnostics.push(tb_error(format!(
                        "measure `{}` element `{refdes}` has no built-in model; \
                         a current measure is not supported there",
                        measure.name
                    )));
                    continue;
                };
                let (letter, parameter) = match &model {
                    Model::Value { letter, .. } => (*letter, "i"),
                    Model::Diode { .. } => ("D", "id"),
                    Model::Subckt => {
                        diagnostics.push(tb_error(format!(
                            "measure `{}` element `{refdes}` is a registry subcircuit; \
                             a current measure is not supported there",
                            measure.name
                        )));
                        continue;
                    }
                };
                let vector = format!("@{}{}[{parameter}]", letter, sanitize(refdes));
                current_saves.insert(vector.clone());
                measure_lines.push(format!(
                    "let m{index} = {vector}\necho SYNTH_MEAS {} $&m{index}",
                    measure.name
                ));
            }
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let mut deck = String::new();
    let title = design.name.replace(['\n', '\r'], " ");
    let _ = writeln!(deck, "* Synth simulation: {title}");
    for line in source_lines {
        let _ = writeln!(deck, "{line}");
    }
    for line in include_lines {
        let _ = writeln!(deck, "{line}");
    }
    for line in element_lines {
        let _ = writeln!(deck, "{line}");
    }
    for line in model_lines {
        let _ = writeln!(deck, "{line}");
    }
    deck.push_str(".control\n");
    if !current_saves.is_empty() {
        let _ = writeln!(
            deck,
            "save all {}",
            current_saves.into_iter().collect::<Vec<_>>().join(" ")
        );
    }
    if testbench.analysis.op {
        deck.push_str("op\n");
    } else {
        let ac = testbench
            .analysis
            .ac
            .as_ref()
            .expect("a validated testbench selects one analysis");
        let _ = writeln!(
            deck,
            "ac dec {} {} {}",
            ac.points_per_decade,
            exponent(ac.start),
            exponent(ac.stop)
        );
    }
    for line in measure_lines {
        let _ = writeln!(deck, "{line}");
    }
    if let Some(plot) = &plot_path {
        // ngspice 42 rejects double quotes here; single quotes work.
        let _ = writeln!(deck, "wrdata '{}' allv", plot.display());
    }
    deck.push_str("echo SYNTH_DONE\n.endc\n.end\n");
    let dialect = dialects.first().and_then(|(dialect, _, _)| *dialect);
    Ok((deck, dialect, plot_path))
}

/// Resolve `[options] plot` relative to the sidecar's directory, or the
/// working directory when no sidecar path is known.
fn resolve_plot_path(plot: &str, testbench_path: Option<&Path>) -> Result<PathBuf, Diagnostic> {
    let base = testbench_path
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new(""));
    std::path::absolute(base.join(plot)).map_err(|error| {
        testbench_error(format!(
            "could not resolve `[options] plot` path `{plot}`: {error}"
        ))
    })
}

/// The dialect's TOML spelling; a part without one is its own (native) mode.
fn dialect_name(dialect: Option<SpiceDialect>) -> &'static str {
    dialect.map_or("none", SpiceDialect::name)
}

/// Map a part's `[spice] pin_order` to pin indexes, in subcircuit terminal
/// order. Every pin of the part must appear exactly once.
fn subckt_pin_indexes(part: &synth_registry::Part, spice: &SpiceModel) -> Result<Vec<u32>, String> {
    let mut seen = HashSet::new();
    let mut indexes = Vec::with_capacity(spice.pin_order.len());
    for name in &spice.pin_order {
        if !seen.insert(name.as_str()) {
            return Err(format!("`[spice] pin_order` lists pin `{name}` twice"));
        }
        let Some(index) = part.pins.iter().position(|pin| &pin.name == name) else {
            return Err(format!(
                "`[spice] pin_order` names pin `{name}`, which the part does not have"
            ));
        };
        indexes.push(index as u32);
    }
    for pin in &part.pins {
        if !seen.contains(pin.name.as_str()) {
            return Err(format!(
                "`[spice] pin_order` omits pin `{}`; list every pin in subcircuit terminal order",
                pin.name
            ));
        }
    }
    Ok(indexes)
}

/// Resolve and check a model file under the canonical registry directories.
fn resolve_spice_model(spice: &SpiceModel, roots: &[PathBuf]) -> Result<PathBuf, String> {
    if roots.is_empty() {
        return Err(format!(
            "model file `{}` cannot be resolved because no registry directory is available",
            spice.file
        ));
    }
    let configured = Path::new(&spice.file);
    if configured.is_absolute() {
        let canonical = configured
            .canonicalize()
            .map_err(|error| format!("model file `{}` is not readable: {error}", spice.file))?;
        if !roots.iter().any(|root| canonical.starts_with(root)) {
            return Err(format!(
                "model file `{}` is outside the registry directories",
                spice.file
            ));
        }
        check_model_file(&canonical, spice)?;
        return Ok(canonical);
    }
    let mut found_outside = false;
    // Last registry directory wins, matching the loader's user-over-seed
    // overlay precedence, so a shadowing part cannot silently use the seed
    // copy of the same relative model path.
    for root in roots.iter().rev() {
        let candidate = root.join(configured);
        if !candidate.exists() {
            continue;
        }
        let canonical = candidate
            .canonicalize()
            .map_err(|error| format!("model file `{}` is not readable: {error}", spice.file))?;
        if roots.iter().any(|root| canonical.starts_with(root)) {
            check_model_file(&canonical, spice)?;
            return Ok(canonical);
        }
        found_outside = true;
    }
    if found_outside {
        Err(format!(
            "model file `{}` resolves outside the registry directories",
            spice.file
        ))
    } else {
        Err(format!(
            "model file `{}` was not found under the registry directories",
            spice.file
        ))
    }
}

/// Reject model files ngspice cannot safely run, and require the named
/// subcircuit to be present with exactly the terminals `pin_order` binds.
fn check_model_file(path: &Path, spice: &SpiceModel) -> Result<(), String> {
    let subckt = spice.subckt.as_str();
    let verilog_a = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "va" | "vams" | "vhd" | "vhdl"
            )
        });
    if verilog_a {
        return Err(format!(
            "model file `{}` is a Verilog-A/HDL model, which ngspice cannot run",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("model file `{}` is not readable: {error}", path.display()))?;
    let lower = text.to_ascii_lowercase();
    if lower.contains("analog begin") {
        return Err(format!(
            "model file `{}` is a Verilog-A model, which ngspice cannot run",
            path.display()
        ));
    }
    for marker in ["$cdnencstart", "$encrypted_lib", "ltspice encrypted file"] {
        if lower.contains(marker) {
            return Err(format!(
                "model file `{}` is encrypted, which ngspice cannot run",
                path.display()
            ));
        }
    }
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('*') {
            continue;
        }
        let directive = trimmed.to_ascii_lowercase();
        // v1 has no vendor models: a nested include could smuggle in
        // `.control`/`shell`, which the checks below would never see.
        for forbidden in [
            ".control", ".endc", "shell", "system", ".include", ".inc", ".lib",
        ] {
            if directive.starts_with(forbidden) {
                return Err(format!(
                    "model file `{}` contains `{forbidden}`, which is not allowed in a model",
                    path.display()
                ));
            }
        }
    }
    let Some(terminal_count) = subckt_terminal_count(&text, subckt) else {
        return Err(format!(
            "model file `{}` does not define `.subckt {subckt}`",
            path.display()
        ));
    };
    if terminal_count != spice.pin_order.len() {
        return Err(format!(
            "model file `{}` defines `.subckt {subckt}` with {terminal_count} terminals, \
             but `[spice] pin_order` lists {} pins",
            path.display(),
            spice.pin_order.len()
        ));
    }
    Ok(())
}

/// Count the terminal names on a `.subckt` declaration. Terminals end at an
/// explicit `params:` or the first `name=value` parameter, and a `+` line
/// continues the declaration. `None` means the subcircuit is not defined.
fn subckt_terminal_count(text: &str, subckt: &str) -> Option<usize> {
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('*') {
            continue;
        }
        let mut tokens = trimmed.split_whitespace();
        if !tokens
            .next()
            .is_some_and(|token| token.eq_ignore_ascii_case(".subckt"))
        {
            continue;
        }
        if !tokens
            .next()
            .is_some_and(|token| token.eq_ignore_ascii_case(subckt))
        {
            continue;
        }
        let mut count = 0;
        let mut terminal = |token: &str| {
            if token.eq_ignore_ascii_case("params:") || token.contains('=') {
                return false;
            }
            count += 1;
            true
        };
        let mut finished = false;
        for token in tokens {
            if !terminal(token) {
                finished = true;
                break;
            }
        }
        while !finished {
            let Some(continuation) = lines.next() else {
                break;
            };
            let Some(rest) = continuation.trim_start().strip_prefix('+') else {
                break;
            };
            for token in rest.split_whitespace() {
                if !terminal(token) {
                    finished = true;
                    break;
                }
            }
        }
        return Some(count);
    }
    None
}

fn connected_nodes(
    component: &Component,
    pin_indexes: &[u32],
    pin_net: &HashMap<(ComponentId, PinId), NetId>,
    node_by_net: &HashMap<NetId, String>,
    design_path: Option<&Path>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<String>> {
    let mut nodes = Vec::with_capacity(pin_indexes.len());
    for &index in pin_indexes {
        let Some(net_id) = pin_net.get(&(component.id, PinId(index))) else {
            diagnostics.push(with_location_span(
                sim_builder(
                    E_SYNTH_SIM_008,
                    Severity::Error,
                    "floating node or no DC path",
                )
                .message(format!(
                    "component `{}` pin {} is not connected to a net",
                    component.refdes,
                    index + 1
                ))
                .primary_entity(EntityRef::Component {
                    id: component.refdes.clone(),
                })
                .build(),
                design_path,
                component.source_span,
            ));
            return None;
        };
        nodes.push(node_by_net[net_id].clone());
    }
    Some(nodes)
}

fn net_span(design: &Board, net: &Net) -> Span {
    net.endpoints
        .first()
        .and_then(|endpoint| {
            design
                .components
                .iter()
                .find(|component| component.id == endpoint.component)
        })
        .map_or(design.source_span, |component| component.source_span)
}

/// Every net needs at least two connections and a DC path to ground.
///
/// This is the check that keeps a series capacitor from producing a
/// plausible-looking operating point (ngspice recovers from the singular
/// matrix with a transient op) and calling it a pass. Capacitors are the only
/// modelled part that breaks a DC path; every other modelled part conducts
/// across all of its connected pins. An unmodelled part is checked across pins
/// 0 and 1 only: it already fails `-002`, so its internals should not decide
/// whether some unrelated net looks floating.
fn check_topology(
    design: &Board,
    testbench: &Testbench,
    ground: NetId,
    ignored_ids: &HashSet<ComponentId>,
    pin_net: &HashMap<(ComponentId, PinId), NetId>,
    design_path: Option<&Path>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut connections: HashMap<NetId, usize> = design
        .nets
        .iter()
        .map(|net| {
            let count = net
                .endpoints
                .iter()
                .filter(|endpoint| !ignored_ids.contains(&endpoint.component))
                .count();
            (net.id, count)
        })
        .collect();
    let mut adjacency: HashMap<NetId, Vec<NetId>> = HashMap::new();

    for source in &testbench.source {
        let Some(net) = design.nets.iter().find(|net| net.name == source.net) else {
            continue;
        };
        *connections.entry(net.id).or_default() += 1;
        *connections.entry(ground).or_default() += 1;
        if net.id != ground {
            adjacency.entry(net.id).or_default().push(ground);
            adjacency.entry(ground).or_default().push(net.id);
        }
    }

    for component in &design.components {
        if component.kind == "capacitor" || ignored_ids.contains(&component.id) {
            continue;
        }
        // Every connected pin of a modelled part belongs to the same DC
        // island; capacitors are the only open circuit at DC. An
        // unmodelled multi-pin part keeps the pins 0/1 heuristic rather
        // than pretending its internals are a short — it already fails
        // `-002`, so the topology check should not invent paths through it.
        let pin_count = if model_for(component).is_ok() {
            component.part.as_ref().map_or(0, |part| part.pins.len())
        } else {
            2
        };
        let nets: Vec<NetId> = (0..pin_count as u32)
            .filter_map(|index| pin_net.get(&(component.id, PinId(index))).copied())
            .collect();
        if let Some((&first, rest)) = nets.split_first() {
            for net in rest {
                if *net != first {
                    adjacency.entry(first).or_default().push(*net);
                    adjacency.entry(*net).or_default().push(first);
                }
            }
        }
    }

    let mut reachable = HashSet::new();
    let mut pending = vec![ground];
    while let Some(net) = pending.pop() {
        if !reachable.insert(net) {
            continue;
        }
        if let Some(neighbours) = adjacency.get(&net) {
            pending.extend(neighbours.iter().copied());
        }
    }

    for net in &design.nets {
        let count = connections.get(&net.id).copied().unwrap_or(0);
        // Every endpoint is ignored, so the net never becomes a deck node.
        if count == 0 {
            continue;
        }
        if count < 2 {
            diagnostics.push(with_location_span(
                topology_error(
                    &net.name,
                    format!(
                        "net `{}` has {count} connection(s); a simulated net needs at least two",
                        net.name
                    ),
                ),
                design_path,
                net_span(design, net),
            ));
        } else if !reachable.contains(&net.id) {
            diagnostics.push(with_location_span(
                topology_error(
                    &net.name,
                    format!(
                        "net `{}` has no DC path to ground; check for a series capacitor or an unconnected part",
                        net.name
                    ),
                ),
                design_path,
                net_span(design, net),
            ));
        }
    }
}

/// The built-in SPICE model for a placed part.
enum Model {
    /// A two-terminal primitive whose value is the third element field.
    Value { letter: &'static str, value: f64 },
    /// A diode with a Shockley model derived from the registry's forward
    /// voltage and current, plus the anode/cathode pin order.
    Diode {
        saturation_current: f64,
        pins: [u32; 2],
    },
    /// A registry `[spice]` subcircuit; its terminals come from `pin_order`.
    Subckt,
}

impl Model {
    fn letter(&self) -> &'static str {
        match self {
            Self::Value { letter, .. } => letter,
            Self::Diode { .. } => "D",
            Self::Subckt => "X",
        }
    }

    fn pins(&self) -> [u32; 2] {
        match self {
            Self::Value { .. } => [0, 1],
            Self::Diode { pins, .. } => *pins,
            Self::Subckt => unreachable!("subcircuit pins come from pin_order"),
        }
    }
}

/// Shockley emission coefficient. LEDs are not ideal diodes; a fixed
/// value of 2 keeps the derived IS above ngspice's lower clamp.
const DIODE_EMISSION_COEFFICIENT: f64 = 2.0;
/// kT/q at ngspice's default 27 °C, in volts.
const THERMAL_VOLTAGE_27C: f64 = 0.025_864_925_786_328_753;

fn model_for(component: &Component) -> Result<Model, String> {
    // A registry `[spice]` table wins over every built-in model, so any
    // kind (a diode, a regulator) can carry a real subcircuit.
    if component
        .part
        .as_ref()
        .is_some_and(|part| part.spice.is_some())
    {
        return Ok(Model::Subckt);
    }
    let (letter, parse) = match component.kind.as_str() {
        "diode" | "led" => return diode_model(component),
        "resistor" => (
            "R",
            synth_validate::parse_resistance as fn(&str) -> Option<f64>,
        ),
        "capacitor" => (
            "C",
            synth_validate::parse_capacitance as fn(&str) -> Option<f64>,
        ),
        "inductor" => (
            "L",
            synth_validate::parse_inductance as fn(&str) -> Option<f64>,
        ),
        other => {
            return Err(format!("part kind `{other}` has no built-in SPICE model"));
        }
    };
    let raw = component
        .value
        .as_deref()
        .ok_or_else(|| "part has no value".to_string())?;
    let value = parse(raw).ok_or_else(|| {
        let hint = if raw.to_ascii_lowercase().contains("meg") {
            "; Synth uses `M` for mega (`1M`), not `meg`"
        } else {
            ""
        };
        format!("value `{raw}` is not a valid {letter} value{hint}")
    })?;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("value `{raw}` is not a positive finite number"));
    }
    Ok(Model::Value { letter, value })
}

fn diode_model(component: &Component) -> Result<Model, String> {
    let part = component
        .part
        .as_ref()
        .ok_or_else(|| "diode part is not in the registry".to_string())?;
    let pins = match (
        part.pins
            .iter()
            .position(|pin| pin.name.eq_ignore_ascii_case("anode")),
        part.pins
            .iter()
            .position(|pin| pin.name.eq_ignore_ascii_case("cathode")),
    ) {
        (Some(anode), Some(cathode)) => [anode as u32, cathode as u32],
        _ => return Err("diode model needs pins named `anode` and `cathode`".to_string()),
    };
    let (forward_voltage, forward_current) = part
        .operating_conditions
        .as_ref()
        .and_then(|conditions| {
            Some((
                conditions.forward_voltage_v?,
                conditions.forward_current_ma?,
            ))
        })
        .ok_or_else(|| {
            "diode part needs `[operating_conditions] forward_voltage_v` and \
             `forward_current_ma`, or a registry `[spice]` model"
                .to_string()
        })?;
    if !forward_voltage.is_finite()
        || forward_voltage <= 0.0
        || !forward_current.is_finite()
        || forward_current <= 0.0
    {
        return Err(format!(
            "forward voltage {forward_voltage} V and current {forward_current} mA \
             must be positive and finite"
        ));
    }
    let saturation_current = forward_current
        / 1000.0
        / ((forward_voltage / (DIODE_EMISSION_COEFFICIENT * THERMAL_VOLTAGE_27C)).exp() - 1.0);
    if !saturation_current.is_finite() || saturation_current <= 0.0 {
        return Err(format!(
            "forward voltage {forward_voltage} V at {forward_current} mA \
             does not give a usable diode model"
        ));
    }
    Ok(Model::Diode {
        saturation_current,
        pins,
    })
}

fn no_model_error(component: &Component, message: &str, design_path: Option<&Path>) -> Diagnostic {
    let part = component
        .part
        .as_ref()
        .map_or(component.kind.as_str(), |part| part.id.as_str());
    with_location_span(
        sim_builder(E_SYNTH_SIM_002, Severity::Error, "part has no SPICE model")
            .message(format!("{} (`{part}`): {message}", component.refdes))
            .primary_entity(EntityRef::Component {
                id: component.refdes.clone(),
            })
            .build(),
        design_path,
        component.source_span,
    )
}

/// Newlines end a SPICE line, so a testbench value carrying one could inject
/// deck commands. The sidecar is a trust boundary.
fn reject_newline(field: &str, value: &str) -> Result<(), Diagnostic> {
    if value.contains(['\n', '\r']) {
        return Err(testbench_error(format!(
            "testbench expression `{field}` must not contain a newline; it is emitted into the SPICE deck"
        )));
    }
    Ok(())
}

fn testbench_error(message: impl Into<String>) -> Diagnostic {
    sim_builder(
        E_SYNTH_SIM_007,
        Severity::Error,
        "malformed testbench or name collision",
    )
    .message(message)
    .build()
}

fn testbench_error_at(path: Option<&Path>, message: impl Into<String>) -> Diagnostic {
    with_location(testbench_error(message), path)
}

fn testbench_error_at_span(
    path: Option<&Path>,
    span: Span,
    message: impl Into<String>,
) -> Diagnostic {
    with_location_span(testbench_error(message), path, span)
}

fn with_location(diagnostic: Diagnostic, path: Option<&Path>) -> Diagnostic {
    with_location_span(diagnostic, path, Span::new(0, 0))
}

fn with_location_span(mut diagnostic: Diagnostic, path: Option<&Path>, span: Span) -> Diagnostic {
    if let Some(path) = path {
        diagnostic.location = Some(Location::from_span(path.display().to_string(), span));
    }
    diagnostic
}

fn valid_sentinel_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// SPICE node and element names accept a narrow, case-insensitive set.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Synth values are decimal; SPICE's `M` is milli, so emit exponents.
/// Thirteen significant digits hide binary noise (`10uH` is
/// `9.999999999999999e-6`) without losing component tolerances.
fn exponent(value: f64) -> String {
    let formatted = format!("{value:.12e}");
    let (mantissa, exponent) = formatted
        .split_once('e')
        .expect("`{:e}` always has an exponent");
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    format!("{mantissa}e{exponent}")
}

/// Model parameters need stability in golden decks; nine significant
/// digits carry more than ngspice's six-digit print precision.
fn model_parameter(value: f64) -> String {
    format!("{value:.8e}")
}
