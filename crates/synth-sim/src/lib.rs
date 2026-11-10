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

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use synth_diagnostics::{Diagnostic, DiagnosticBuilder, EntityRef, Severity};
use synth_ir::{Board, Component, ComponentId, Net, NetId, PinId};

/// ngspice could not be found or started.
pub const E_SYNTH_SIM_001: &str = "E-SYNTH-SIM-001";
/// A part has no SPICE model.
pub const E_SYNTH_SIM_002: &str = "E-SYNTH-SIM-002";
/// A measurement is missing, failed, non-finite or unparseable.
pub const E_SYNTH_SIM_003: &str = "E-SYNTH-SIM-003";
/// A measurement's declared limit check (info when inside, error when outside).
pub const E_SYNTH_SIM_004: &str = "E-SYNTH-SIM-004";
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
}

/// A named measurement with optional pass/fail limits.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measure {
    pub name: String,
    pub kind: MeasureKind,
    /// Synth net name whose voltage is measured.
    pub node: String,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasureKind {
    NodeVoltage,
}

impl Testbench {
    /// Parse and validate a sidecar. Every failure is `E-SYNTH-SIM-007`.
    pub fn from_toml(text: &str) -> Result<Self, Diagnostic> {
        let testbench: Self =
            toml::from_str(text).map_err(|e| testbench_error(format!("could not parse: {e}")))?;
        testbench.validate()?;
        Ok(testbench)
    }

    fn validate(&self) -> Result<(), Diagnostic> {
        if self.ground.trim().is_empty() {
            return Err(testbench_error("ground net is empty"));
        }
        if !self.analysis.op {
            return Err(testbench_error(
                "testbench must select exactly one analysis; only `op` is supported",
            ));
        }
        if self.source.is_empty() {
            return Err(testbench_error("testbench has no [[source]]"));
        }
        if self.measure.is_empty() {
            return Err(testbench_error("testbench has no [[measure]]"));
        }

        let mut names = HashSet::new();
        for source in &self.source {
            if source.net.trim().is_empty() {
                return Err(testbench_error("source net is empty"));
            }
            let Some(dc) = source.dc else {
                return Err(testbench_error(format!(
                    "source on net `{}` has no `dc` value",
                    source.net
                )));
            };
            if !dc.is_finite() {
                return Err(testbench_error(format!(
                    "source on net `{}` has a non-finite `dc` value",
                    source.net
                )));
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
            if measure.node.trim().is_empty() {
                return Err(testbench_error(format!(
                    "measure `{}` has an empty node",
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

/// One comparison used by both the public pass predicate and the diagnostic.
enum LimitOutcome {
    Pass,
    Below(f64),
    Above(f64),
}

fn limit_outcome(min: Option<f64>, max: Option<f64>, value: f64) -> LimitOutcome {
    if let Some(min) = min {
        if value < min {
            return LimitOutcome::Below(min);
        }
    }
    if let Some(max) = max {
        if value > max {
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
pub fn simulate(design: &Board, testbench: &Testbench, options: &SimOptions) -> SimReport {
    let deck = match emit_netlist(design, testbench) {
        Ok(deck) => deck,
        Err(diagnostics) => {
            return SimReport {
                measures: Vec::new(),
                diagnostics,
            }
        }
    };

    let stdout = match run_ngspice(&deck, options) {
        Ok(stdout) => stdout,
        Err(diagnostic) => {
            return SimReport {
                measures: Vec::new(),
                diagnostics: vec![diagnostic],
            }
        }
    };

    let values = match parse_sentinels(&stdout, testbench) {
        Ok(values) => values,
        Err(diagnostics) => {
            return SimReport {
                measures: Vec::new(),
                diagnostics,
            }
        }
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

    SimReport {
        measures,
        diagnostics,
    }
}

/// Run ngspice in a fresh temp directory with its own `.spiceinit`, so a
/// user's `~/.spiceinit` and concurrent runs cannot interfere.
fn run_ngspice(deck: &str, options: &SimOptions) -> Result<String, Diagnostic> {
    let dir = TempDir::create()
        .map_err(|e| ngspice_error(format!("could not create a temporary directory: {e}")))?;
    std::fs::write(dir.path().join(".spiceinit"), "set strict_errorhandling\n")
        .map_err(|e| ngspice_error(format!("could not write .spiceinit: {e}")))?;
    std::fs::write(dir.path().join("deck.cir"), deck)
        .map_err(|e| ngspice_error(format!("could not write deck.cir: {e}")))?;

    let binary = options.binary();
    let output = Command::new(&binary)
        .arg("-b")
        .arg("deck.cir")
        .current_dir(dir.path())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(DiagnosticBuilder::new(
                E_SYNTH_SIM_001,
                Severity::Error,
                "ngspice not found",
            )
            .message(format!(
                "could not start `{}`; install ngspice or point `NGSPICE` at the binary",
                binary.display()
            ))
            .build())
        }
        Err(error) => {
            return Err(ngspice_error(format!(
                "could not run `{}`: {error}",
                binary.display()
            )))
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let status = output
            .status
            .code()
            .map_or_else(|| "signal".to_string(), |code| code.to_string());
        return Err(ngspice_error(format!(
            "ngspice exited with status {status}: {}",
            excerpt(&stderr)
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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

fn measure_error(message: impl Into<String>) -> Diagnostic {
    DiagnosticBuilder::new(E_SYNTH_SIM_003, Severity::Error, "measurement failed")
        .message(message)
        .build()
}

fn limit_error(name: &str, value: f64, direction: &str) -> Diagnostic {
    DiagnosticBuilder::new(E_SYNTH_SIM_004, Severity::Error, "measurement limit")
        .message(format!("measure `{name}` = {value} is {direction}"))
        .build()
}

fn limit_info(name: &str, value: f64) -> Diagnostic {
    DiagnosticBuilder::new(E_SYNTH_SIM_004, Severity::Info, "measurement limit")
        .message(format!(
            "measure `{name}` = {value} is within its declared limits"
        ))
        .build()
}

fn topology_error(message: impl Into<String>) -> Diagnostic {
    DiagnosticBuilder::new(
        E_SYNTH_SIM_008,
        Severity::Error,
        "floating node or no DC path",
    )
    .message(message)
    .build()
}

fn ngspice_error(message: impl Into<String>) -> Diagnostic {
    DiagnosticBuilder::new(E_SYNTH_SIM_006, Severity::Error, "ngspice error")
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
#[allow(clippy::too_many_lines)]
pub fn emit_netlist(design: &Board, testbench: &Testbench) -> Result<String, Vec<Diagnostic>> {
    if let Err(diagnostic) = testbench.validate() {
        return Err(vec![diagnostic]);
    }
    let mut diagnostics = Vec::new();

    let Some(ground) = design.nets.iter().find(|net| net.name == testbench.ground) else {
        return Err(vec![testbench_error(format!(
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
                    diagnostics.push(testbench_error(format!(
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
            diagnostics.push(testbench_error(format!(
                "source net `{}` not found in design",
                source.net
            )));
            continue;
        };
        if net.id == ground.id {
            diagnostics.push(testbench_error(format!(
                "source net `{}` is the ground net",
                source.net
            )));
            continue;
        }
        let Some(dc) = source.dc else {
            diagnostics.push(testbench_error(format!(
                "source on net `{}` has no `dc` value",
                source.net
            )));
            continue;
        };
        source_lines.push(format!(
            "V{} {} 0 DC {}",
            index + 1,
            node_by_net[&net.id],
            exponent(dc)
        ));
    }

    let mut pin_net: HashMap<(ComponentId, PinId), NetId> = HashMap::new();
    for net in &design.nets {
        for endpoint in &net.endpoints {
            pin_net.insert((endpoint.component, endpoint.pin), net.id);
        }
    }

    let mut element_lines = Vec::new();
    let mut seen_elements: HashMap<String, String> = HashMap::new();
    for component in &design.components {
        let (letter, value) = match model_for(component) {
            Ok(model) => model,
            Err(message) => {
                diagnostics.push(no_model_error(component, &message));
                continue;
            }
        };
        let pin_count = component.part.as_ref().map_or(0, |part| part.pins.len());
        if pin_count != 2 {
            diagnostics.push(no_model_error(
                component,
                &format!("a two-terminal model needs 2 pins, part has {pin_count}"),
            ));
            continue;
        }
        let mut nodes = Vec::with_capacity(2);
        let mut connected = true;
        for index in 0..2u32 {
            if let Some(net_id) = pin_net.get(&(component.id, PinId(index))) {
                nodes.push(node_by_net[net_id].clone());
            } else {
                diagnostics.push(topology_error(format!(
                    "component `{}` pin {} is not connected to a net",
                    component.refdes,
                    index + 1
                )));
                connected = false;
                break;
            }
        }
        if !connected {
            continue;
        }

        let name = format!("{letter}{}", sanitize(&component.refdes));
        if name.len() == letter.len() {
            diagnostics.push(testbench_error(format!(
                "component `{}` has an empty reference",
                component.refdes
            )));
            continue;
        }
        let key = name.to_ascii_lowercase();
        if let Some(previous) = seen_elements.insert(key, component.refdes.clone()) {
            if previous != component.refdes {
                diagnostics.push(testbench_error(format!(
                    "references `{previous}` and `{}` both emit as `{name}`",
                    component.refdes
                )));
                continue;
            }
        }
        element_lines.push(format!(
            "{name} {} {} {}",
            nodes[0],
            nodes[1],
            exponent(value)
        ));
    }

    let mut measure_lines = Vec::new();
    for (index, measure) in testbench.measure.iter().enumerate() {
        let Some(net) = net_by_name.get(measure.node.as_str()) else {
            diagnostics.push(testbench_error(format!(
                "measure `{}` node `{}` not found in design",
                measure.name, measure.node
            )));
            continue;
        };
        let node = &node_by_net[&net.id];
        measure_lines.push(format!(
            "let m{index} = v({node})\necho SYNTH_MEAS {} $&m{index}",
            measure.name
        ));
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
    for line in element_lines {
        let _ = writeln!(deck, "{line}");
    }
    deck.push_str(".control\nop\n");
    for line in measure_lines {
        let _ = writeln!(deck, "{line}");
    }
    deck.push_str("echo SYNTH_DONE\n.endc\n.end\n");
    Ok(deck)
}

fn model_for(component: &Component) -> Result<(&'static str, f64), String> {
    let raw = component
        .value
        .as_deref()
        .ok_or_else(|| "part has no value".to_string())?;
    let (letter, value) = match component.kind.as_str() {
        "resistor" => ("R", synth_validate::parse_resistance(raw)),
        "capacitor" => ("C", synth_validate::parse_capacitance(raw)),
        other => {
            return Err(format!("part kind `{other}` has no built-in SPICE model"));
        }
    };
    let value = value.ok_or_else(|| format!("value `{raw}` is not a valid {letter} value"))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("value `{raw}` is not a positive finite number"));
    }
    Ok((letter, value))
}

fn no_model_error(component: &Component, message: &str) -> Diagnostic {
    let part = component
        .part
        .as_ref()
        .map_or(component.kind.as_str(), |part| part.id.as_str());
    DiagnosticBuilder::new(E_SYNTH_SIM_002, Severity::Error, "part has no SPICE model")
        .message(format!("{} (`{part}`): {message}", component.refdes))
        .primary_entity(EntityRef::Component {
            id: component.refdes.clone(),
        })
        .build()
}

fn testbench_error(message: impl Into<String>) -> Diagnostic {
    DiagnosticBuilder::new(
        E_SYNTH_SIM_007,
        Severity::Error,
        "malformed testbench or name collision",
    )
    .message(message)
    .build()
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
fn exponent(value: f64) -> String {
    format!("{value:e}")
}
