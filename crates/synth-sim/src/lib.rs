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

use serde::{Deserialize, Serialize};
use synth_diagnostics::{Diagnostic, DiagnosticBuilder, EntityRef, Location, Severity, Span};
use synth_ir::{Board, Component, ComponentId, Net, NetId, PinId};
use synth_registry::{SpiceDialect, SpiceModel};
use synth_router::process::{Invocation, KillScope, run_cancellable};

mod suggest;
pub use suggest::{TestbenchSuggestion, suggest_testbench};

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
    /// Parsed through `toml::Spanned` so each measure carries its own block.
    #[serde(default, deserialize_with = "spanned_measures")]
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

/// An independent source added by the testbench. It drives either a net
/// (`dc`/`ac`/`pulse`) or an IC pin's output (a `level` step, or a `pulse`
/// whose levels come from the pin's `[pins.drive]` data).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Synth net name the source drives.
    #[serde(default)]
    pub net: Option<String>,
    /// `REF.PIN` whose output drive supplies the net, e.g. `U1.gpio5`.
    #[serde(default)]
    pub pin: Option<String>,
    /// Output level for a pin source. A pulse pin source uses both levels
    /// and rejects this.
    #[serde(default)]
    pub level: Option<SourceLevel>,
    #[serde(default)]
    pub kind: SourceKind,
    /// DC value in volts.
    #[serde(default)]
    pub dc: Option<f64>,
    /// AC magnitude in volts.
    #[serde(default)]
    pub ac: Option<f64>,
    /// PULSE low level in volts. Required for a net pulse source; a pin
    /// pulse source takes it from the pin's `[pins.drive]` `low_v`.
    #[serde(default)]
    pub low: Option<f64>,
    /// PULSE high level in volts. Required for a net pulse source; a pin
    /// pulse source takes it from the pin's `[pins.drive]` `high_v`.
    #[serde(default)]
    pub high: Option<f64>,
    /// PULSE period.
    #[serde(default, deserialize_with = "deserialize_duration_option")]
    pub period: Option<f64>,
    /// High fraction of the period, from 0 (exclusive) to 1 (inclusive).
    #[serde(default)]
    pub duty: Option<f64>,
    /// PULSE rise time.
    #[serde(default, deserialize_with = "deserialize_duration_option")]
    pub rise: Option<f64>,
    /// PULSE fall time.
    #[serde(default, deserialize_with = "deserialize_duration_option")]
    pub fall: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    #[default]
    Dc,
    /// A `PULSE(Vlow Vhigh 0 Trise Tfall Pwidth Pperiod)` source.
    Pulse,
}

impl Source {
    /// The pulse fields a source carries, for the "field present but this
    /// kind does not use it" checks.
    fn pulse_fields(&self) -> [(&'static str, Option<f64>); 6] {
        [
            ("low", self.low),
            ("high", self.high),
            ("period", self.period),
            ("duty", self.duty),
            ("rise", self.rise),
            ("fall", self.fall),
        ]
    }
}

/// The output level a pin source drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLevel {
    High,
    Low,
}

impl SourceLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Low => "low",
        }
    }
}

/// A sidecar duration: a plain number of seconds, or a number with an
/// `ns`/`us`/`ms`/`ps`/`s` suffix. SPICE's `M` is milli, so a bare `m` is
/// rejected rather than read as milliseconds.
pub(crate) fn parse_duration(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    let split = trimmed
        .char_indices()
        .find(|(_, character)| character.is_ascii_alphabetic() || *character == 'µ')
        .map(|(index, _)| index);
    let (number, suffix) = match split {
        Some(index) => (&trimmed[..index], &trimmed[index..]),
        None => (trimmed, ""),
    };
    // The number must parse on its own; the suffix only moves the exponent.
    number.trim().parse::<f64>().ok()?;
    // Attaching the scale as an exponent instead of multiplying snaps the
    // result to the double the decimal notation names: `500e-9` is exactly
    // `5e-7`, while `500 * 1e-9` is not.
    let exponent = match suffix.to_ascii_lowercase().replace('µ', "u").as_str() {
        "" => {
            let seconds: f64 = number.trim().parse().ok()?;
            return seconds.is_finite().then_some(seconds);
        }
        "s" => "",
        "ms" => "e-3",
        "us" => "e-6",
        "ns" => "e-9",
        "ps" => "e-12",
        _ => return None,
    };
    let seconds: f64 = format!("{number}{exponent}").parse().ok()?;
    seconds.is_finite().then_some(seconds)
}

/// The sidecar's duration error text; `M` is milli in SPICE, so `m` alone is
/// a unit trap worth naming.
fn duration_hint() -> &'static str {
    "use a number of seconds or a suffix of ns, us, ms or s (a bare `m` is rejected \
     because SPICE `M` is milli: write `ms`)"
}

/// Deserialize one duration field: a TOML number of seconds or a string like
/// `"5ms"`.
fn deserialize_duration<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct DurationVisitor;
    impl serde::de::Visitor<'_> for DurationVisitor {
        type Value = f64;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a duration: a number of seconds or a string like `5ms`")
        }
        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<f64, E> {
            Ok(value)
        }
        #[allow(clippy::cast_precision_loss)]
        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<f64, E> {
            Ok(value as f64)
        }
        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<f64, E> {
            #[allow(clippy::cast_precision_loss)]
            Ok(value as f64)
        }
        fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<f64, E> {
            parse_duration(text).ok_or_else(|| {
                serde::de::Error::custom(format!("`{text}` is not a duration; {}", duration_hint()))
            })
        }
    }
    deserializer.deserialize_any(DurationVisitor)
}

/// Deserialize an optional duration field: a TOML number of seconds or a
/// string like `"5ms"`.
fn deserialize_duration_option<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_duration(deserializer).map(Some)
}

/// The analysis the deck runs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Analysis {
    #[serde(default)]
    pub op: bool,
    #[serde(default)]
    pub ac: Option<AcAnalysis>,
    #[serde(default)]
    pub tran: Option<TranAnalysis>,
}

/// A transient run: `tran <step> <stop>`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranAnalysis {
    /// Simulation end time, in seconds.
    #[serde(deserialize_with = "deserialize_duration")]
    pub stop: f64,
    /// Output interval, in seconds.
    #[serde(deserialize_with = "deserialize_duration")]
    pub step: f64,
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
    /// `REF.PIN` whose output current is measured by `pin_current`.
    #[serde(default)]
    pub pin: Option<String>,
    /// Relative-dB drop for `relative_db`. Defaults to 3.0.
    #[serde(default)]
    pub drop_db: Option<f64>,
    /// Frequency in Hz for an absolute `vdb` gain on a `node_voltage`
    /// measure; requires the `ac` analysis.
    #[serde(default)]
    pub at_frequency: Option<f64>,
    /// Window start for a transient measure. Defaults to 0.
    #[serde(default, deserialize_with = "deserialize_duration_option")]
    pub window_start: Option<f64>,
    /// Window end for a transient measure. Defaults to the `tran` stop.
    #[serde(default, deserialize_with = "deserialize_duration_option")]
    pub window_end: Option<f64>,
    /// Absolute tolerance band around the window's final value for
    /// `settling_time`, in the waveform's unit.
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// Lower threshold for `rise_time`, in volts.
    #[serde(default)]
    pub low_threshold: Option<f64>,
    /// Upper threshold for `rise_time`, in volts.
    #[serde(default)]
    pub high_threshold: Option<f64>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// Byte range of this `[[measure]]` block in the sidecar; filled after
    /// parsing and never read from TOML.
    #[serde(skip, default = "empty_span")]
    pub span: Span,
}

fn empty_span() -> Span {
    Span::new(0, 0)
}

fn spanned_measures<'de, D>(deserializer: D) -> Result<Vec<Measure>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let measures = Vec::<toml::Spanned<Measure>>::deserialize(deserializer)?;
    Ok(measures
        .into_iter()
        .map(|spanned| {
            let span = spanned.span();
            let mut measure = spanned.into_inner();
            measure.span = Span::new(
                u32::try_from(span.start).unwrap_or(u32::MAX),
                u32::try_from(span.end).unwrap_or(u32::MAX),
            );
            measure
        })
        .collect())
}

/// `toml::Spanned` starts each measure at its header; extend the span to the
/// next measure header or table so a diagnostic underlines the whole block.
fn expand_measure_spans(testbench: &mut Testbench, text: &str) {
    for measure in &mut testbench.measure {
        let start = measure.span.byte_start as usize;
        let end = next_table_start(text, start);
        measure.span = Span::new(
            u32::try_from(start).unwrap_or(u32::MAX),
            u32::try_from(end).unwrap_or(u32::MAX),
        );
    }
}

fn next_table_start(text: &str, after: usize) -> usize {
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset = offset.saturating_add(line.len());
        if start <= after {
            continue;
        }
        if line.trim_start().starts_with('[') {
            return start;
        }
    }
    text.len()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasureKind {
    NodeVoltage,
    Current,
    RelativeDb,
    /// The current out of an IC pin, against its `[pins.drive]` limit.
    #[serde(alias = "pin_current_limit")]
    PinCurrent,
    /// Transient mean of the waveform over the window.
    Average,
    /// Transient root-mean-square of the waveform over the window.
    Rms,
    /// Transient maximum minus minimum over the window.
    PeakToPeak,
    /// Time from the window start until the waveform enters and stays inside
    /// a tolerance band around its final value.
    SettlingTime,
    /// Time between the first upward crossings of the two thresholds.
    RiseTime,
    /// How far the waveform rises above its window's final value, as a
    /// percentage of that final value.
    Overshoot,
}

impl MeasureKind {
    /// The measure kinds that read a transient waveform from `wrdata` data.
    fn is_transient(self) -> bool {
        matches!(
            self,
            Self::Average
                | Self::Rms
                | Self::PeakToPeak
                | Self::SettlingTime
                | Self::RiseTime
                | Self::Overshoot
        )
    }

    /// Transient kinds that may read an element current instead of a node
    /// voltage.
    fn allows_element(self) -> bool {
        matches!(self, Self::Average | Self::Rms | Self::PeakToPeak)
    }
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
        let mut testbench: Self = toml::from_str(text).map_err(|error| {
            let span = error.span().map_or(Span::new(0, 0), |range| {
                Span::new(
                    u32::try_from(range.start).unwrap_or(u32::MAX),
                    u32::try_from(range.end).unwrap_or(u32::MAX),
                )
            });
            testbench_error_at_span(path, span, format!("could not parse: {error}"))
        })?;
        expand_measure_spans(&mut testbench, text);
        testbench.validate_at(path)?;
        Ok(testbench)
    }

    fn validate_at(&self, path: Option<&Path>) -> Result<(), Diagnostic> {
        self.validate_inner().map_err(|mut diagnostic| {
            let span = diagnostic
                .location
                .as_ref()
                .map_or(Span::new(0, 0), |location| location.span);
            diagnostic.location =
                path.map(|path| Location::from_span(path.display().to_string(), span));
            diagnostic
        })
    }

    #[allow(clippy::too_many_lines)]
    fn validate_inner(&self) -> Result<(), Diagnostic> {
        if self.ground.trim().is_empty() {
            return Err(testbench_error("ground net is empty"));
        }
        reject_newline("ground", &self.ground)?;
        if let Some(timeout) = self.options.timeout
            && (!timeout.is_finite() || timeout <= 0.0 || timeout > MAX_TIMEOUT_SECONDS)
        {
            return Err(testbench_error(format!(
                "`[options] timeout` must be greater than 0 and at most {MAX_TIMEOUT_SECONDS} seconds"
            )));
        }
        let selected_analyses = usize::from(self.analysis.op)
            + usize::from(self.analysis.ac.is_some())
            + usize::from(self.analysis.tran.is_some());
        if selected_analyses != 1 {
            return Err(testbench_error(
                "testbench must select exactly one analysis: `op`, `ac` or `tran`",
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
            // ngspice hangs on a sweep with less than one point in it, e.g.
            // `.ac dec 1 10 55`. Reject it before starting the run.
            if f64::from(ac.points_per_decade) * (ac.stop / ac.start).log10() < 1.0 {
                return Err(testbench_error(format!(
                    "`[analysis.ac]` sweep {} to {} with {} points per decade has fewer than \
                     two points; ngspice hangs on it. Widen the sweep or raise \
                     `points_per_decade`.",
                    ac.start, ac.stop, ac.points_per_decade
                )));
            }
        }
        if let Some(tran) = &self.analysis.tran {
            if !tran.stop.is_finite() || tran.stop <= 0.0 {
                return Err(testbench_error(
                    "`[analysis.tran] stop` must be a positive finite time",
                ));
            }
            if !tran.step.is_finite() || tran.step <= 0.0 {
                return Err(testbench_error(
                    "`[analysis.tran] step` must be a positive finite time",
                ));
            }
            if tran.step >= tran.stop {
                return Err(testbench_error(format!(
                    "`[analysis.tran] step` must be less than stop {}; use a smaller step or \
                     a longer run",
                    tran.stop
                )));
            }
            let points = tran.stop / tran.step;
            if points > MAX_TRAN_POINTS {
                return Err(testbench_error(format!(
                    "`[analysis.tran]` needs {points:.0} output points (stop/step); the cap is \
                     {MAX_TRAN_POINTS} to keep runs bounded. Raise the step or shorten the run."
                )));
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
        let mut source_nets = HashSet::new();
        let mut source_pins = HashSet::new();
        for source in &self.source {
            match (source.net.as_deref(), source.pin.as_deref()) {
                (Some(net), None) => {
                    if net.trim().is_empty() {
                        return Err(testbench_error("source net is empty"));
                    }
                    reject_newline("net", net)?;
                    if source.level.is_some() {
                        return Err(testbench_error(format!(
                            "source on net `{net}` has a `level` but no `pin`; `level` only \
                             applies to a pin source"
                        )));
                    }
                    if !source_nets.insert(net) {
                        return Err(testbench_error(format!(
                            "source net `{net}` is declared twice"
                        )));
                    }
                    match source.kind {
                        SourceKind::Dc => {
                            for (field, value) in source.pulse_fields() {
                                if value.is_some() {
                                    return Err(testbench_error(format!(
                                        "source on net `{net}` has `{field}` but `kind` is \
                                         `dc`; the pulse fields need `kind = \"pulse\"`"
                                    )));
                                }
                            }
                            if source.dc.is_none() && source.ac.is_none() {
                                return Err(testbench_error(format!(
                                    "source on net `{net}` has neither a `dc` nor an `ac` value"
                                )));
                            }
                            if let Some(dc) = source.dc
                                && !dc.is_finite()
                            {
                                return Err(testbench_error(format!(
                                    "source on net `{net}` has a non-finite `dc` value"
                                )));
                            }
                            if let Some(ac) = source.ac {
                                if !ac.is_finite() {
                                    return Err(testbench_error(format!(
                                        "source on net `{net}` has a non-finite `ac` value"
                                    )));
                                }
                                if ac < 0.0 {
                                    return Err(testbench_error(format!(
                                        "source on net `{net}` has a negative `ac` value"
                                    )));
                                }
                            }
                        }
                        SourceKind::Pulse => {
                            if source.dc.is_some() || source.ac.is_some() {
                                return Err(testbench_error(format!(
                                    "source on net `{net}` must not have a `dc` or `ac` value; \
                                     a pulse source is fully described by its pulse fields"
                                )));
                            }
                            validate_net_pulse(net, source)?;
                        }
                    }
                }
                (None, Some(pin)) => {
                    if pin.trim().is_empty() {
                        return Err(testbench_error("source pin is empty"));
                    }
                    reject_newline("pin", pin)?;
                    let Some((refdes, pin_name)) = pin.split_once('.') else {
                        return Err(testbench_error(format!(
                            "source pin `{pin}` must be `REF.PIN`, for example `U1.gpio5`"
                        )));
                    };
                    if refdes.is_empty() || pin_name.is_empty() {
                        return Err(testbench_error(format!(
                            "source pin `{pin}` must be `REF.PIN` with both halves non-empty"
                        )));
                    }
                    match source.kind {
                        SourceKind::Dc => {
                            if source.level.is_none() {
                                return Err(testbench_error(format!(
                                    "source pin `{pin}` has no `level`; use `level = \"high\"` \
                                     or `level = \"low\"`"
                                )));
                            }
                            for (field, value) in source.pulse_fields() {
                                if value.is_some() {
                                    return Err(testbench_error(format!(
                                        "source pin `{pin}` has `{field}` but the `dc` kind \
                                         only steps between the pin's levels; the pulse fields \
                                         need `kind = \"pulse\"`"
                                    )));
                                }
                            }
                        }
                        SourceKind::Pulse => {
                            if source.level.is_some() {
                                return Err(testbench_error(format!(
                                    "source pin `{pin}` has a `level`; a pulse pin source \
                                     uses both of the pin's `[pins.drive]` levels, so omit it"
                                )));
                            }
                            if source.low.is_some() || source.high.is_some() {
                                return Err(testbench_error(format!(
                                    "source pin `{pin}` must not have a `low` or `high`; the \
                                     pulse levels come from the pin's `[pins.drive]` data"
                                )));
                            }
                        }
                    }
                    if source.dc.is_some() || source.ac.is_some() {
                        return Err(testbench_error(format!(
                            "source pin `{pin}` must not have a `dc` or `ac` value; the \
                             voltage comes from the pin's `[pins.drive]` data"
                        )));
                    }
                    if source.kind == SourceKind::Pulse {
                        validate_pulse_timing(
                            pulse_timing(source),
                            &format!("source pin `{pin}`"),
                        )?;
                    }
                    if !source_pins.insert(pin) {
                        return Err(testbench_error(format!(
                            "source pin `{pin}` is declared twice"
                        )));
                    }
                }
                (Some(net), Some(pin)) => {
                    return Err(testbench_error(format!(
                        "source has both `net` (`{net}`) and `pin` (`{pin}`); a source drives \
                         exactly one"
                    )));
                }
                (None, None) => {
                    return Err(testbench_error("source must have a `net` or a `pin`"));
                }
            }
        }

        for measure in &self.measure {
            if !valid_sentinel_name(&measure.name) {
                return Err(invalid_measure(
                    measure,
                    format!(
                        "measure name `{}` must be letters, digits, `_`, `-` or `.`",
                        measure.name
                    ),
                ));
            }
            if !names.insert(measure.name.as_str()) {
                return Err(invalid_measure(
                    measure,
                    format!("measure name `{}` is declared twice", measure.name),
                ));
            }
            match measure.kind {
                MeasureKind::NodeVoltage | MeasureKind::RelativeDb => {
                    match measure.node.as_deref() {
                        Some(node) if !node.trim().is_empty() => {
                            if let Err(diagnostic) = reject_newline("node", node) {
                                return Err(at_measure(diagnostic, measure));
                            }
                        }
                        _ => {
                            return Err(invalid_measure(
                                measure,
                                format!("measure `{}` has an empty node", measure.name),
                            ));
                        }
                    }
                    if measure.element.is_some() {
                        return Err(invalid_measure(
                            measure,
                            format!(
                                "measure `{}` reads a node and must not have an `element`",
                                measure.name
                            ),
                        ));
                    }
                    if measure.pin.is_some() {
                        return Err(invalid_measure(
                            measure,
                            format!(
                                "measure `{}` reads a node and must not have a `pin`",
                                measure.name
                            ),
                        ));
                    }
                    if measure.kind == MeasureKind::NodeVoltage && measure.drop_db.is_some() {
                        return Err(invalid_measure(
                            measure,
                            format!(
                                "measure `{}` is a node_voltage measure and must not have `drop_db`",
                                measure.name
                            ),
                        ));
                    }
                    if measure.kind == MeasureKind::RelativeDb && measure.at_frequency.is_some() {
                        return Err(invalid_measure(
                            measure,
                            format!(
                                "measure `{}` is a relative_db measure and must not have `at_frequency`",
                                measure.name
                            ),
                        ));
                    }
                    if let Some(drop_db) = measure.drop_db
                        && (!drop_db.is_finite() || drop_db <= 0.0)
                    {
                        return Err(invalid_measure(
                            measure,
                            format!(
                                "measure `{}` has a `drop_db` that must be finite and greater than 0",
                                measure.name
                            ),
                        ));
                    }
                }
                MeasureKind::Current => {
                    match measure.element.as_deref() {
                        Some(element) if !element.trim().is_empty() => {
                            if let Err(diagnostic) = reject_newline("element", element) {
                                return Err(at_measure(diagnostic, measure));
                            }
                        }
                        _ => {
                            return Err(invalid_measure(
                                measure,
                                format!("measure `{}` has an empty element", measure.name),
                            ));
                        }
                    }
                    if measure.node.is_some() {
                        return Err(invalid_measure(
                            measure,
                            format!(
                                "measure `{}` is a current measure and must not have a `node`",
                                measure.name
                            ),
                        ));
                    }
                    if measure.pin.is_some() {
                        return Err(invalid_measure(
                            measure,
                            format!(
                                "measure `{}` is a current measure and must not have a `pin`",
                                measure.name
                            ),
                        ));
                    }
                    if measure.drop_db.is_some() {
                        return Err(invalid_measure(
                            measure,
                            format!(
                                "measure `{}` is a current measure and must not have `drop_db`",
                                measure.name
                            ),
                        ));
                    }
                    if measure.at_frequency.is_some() {
                        return Err(invalid_measure(
                            measure,
                            format!(
                                "measure `{}` is a current measure and must not have `at_frequency`",
                                measure.name
                            ),
                        ));
                    }
                }
                MeasureKind::PinCurrent => {
                    match measure.pin.as_deref() {
                        Some(pin) if !pin.trim().is_empty() => {
                            if let Err(diagnostic) = reject_newline("pin", pin) {
                                return Err(at_measure(diagnostic, measure));
                            }
                        }
                        _ => {
                            return Err(invalid_measure(
                                measure,
                                format!("measure `{}` has an empty pin", measure.name),
                            ));
                        }
                    }
                    for (field, present) in [
                        ("node", measure.node.is_some()),
                        ("element", measure.element.is_some()),
                        ("drop_db", measure.drop_db.is_some()),
                        ("at_frequency", measure.at_frequency.is_some()),
                        ("min", measure.min.is_some()),
                        ("max", measure.max.is_some()),
                    ] {
                        if present {
                            return Err(invalid_measure(
                                measure,
                                format!(
                                    "measure `{}` is a pin_current measure and must not have \
                                     `{field}`; its limit comes from the pin's \
                                     `[pins.drive]` data",
                                    measure.name
                                ),
                            ));
                        }
                    }
                }
                kind if kind.is_transient() => {
                    let node = measure
                        .node
                        .as_deref()
                        .map(str::trim)
                        .filter(|node| !node.is_empty());
                    let element = measure
                        .element
                        .as_deref()
                        .map(str::trim)
                        .filter(|element| !element.is_empty());
                    match (node, element) {
                        (Some(_), Some(_)) => {
                            return Err(invalid_measure(
                                measure,
                                format!(
                                    "measure `{}` has both a `node` and an `element`; a \
                                     transient measure reads one waveform",
                                    measure.name
                                ),
                            ));
                        }
                        (None, None) => {
                            return Err(invalid_measure(
                                measure,
                                format!(
                                    "measure `{}` has no waveform to read; give it a `node` \
                                     or, for average, rms and peak_to_peak, an `element`",
                                    measure.name
                                ),
                            ));
                        }
                        (None, Some(_)) if !kind.allows_element() => {
                            return Err(invalid_measure(
                                measure,
                                format!(
                                    "measure `{}` is a {} measure and reads a node; an \
                                     `element` current is only for average, rms and \
                                     peak_to_peak",
                                    measure.name,
                                    kind_toml_name(measure.kind)
                                ),
                            ));
                        }
                        (Some(node), None) => {
                            if let Err(diagnostic) = reject_newline("node", node) {
                                return Err(at_measure(diagnostic, measure));
                            }
                        }
                        (None, Some(element)) => {
                            if let Err(diagnostic) = reject_newline("element", element) {
                                return Err(at_measure(diagnostic, measure));
                            }
                        }
                    }
                    for (field, present) in [
                        ("pin", measure.pin.is_some()),
                        ("drop_db", measure.drop_db.is_some()),
                        ("at_frequency", measure.at_frequency.is_some()),
                    ] {
                        if present {
                            return Err(invalid_measure(
                                measure,
                                format!(
                                    "measure `{}` is a {} measure and must not have `{field}`",
                                    measure.name,
                                    kind_toml_name(measure.kind)
                                ),
                            ));
                        }
                    }
                    match measure.kind {
                        MeasureKind::SettlingTime => match measure.tolerance {
                            Some(tolerance) if tolerance.is_finite() && tolerance > 0.0 => {}
                            _ => {
                                return Err(invalid_measure(
                                    measure,
                                    format!(
                                        "measure `{}` is a settling_time measure and \
                                             needs a positive finite `tolerance`; the band is \
                                             the window's final value plus and minus it",
                                        measure.name
                                    ),
                                ));
                            }
                        },
                        MeasureKind::RiseTime => {
                            let thresholds = (measure.low_threshold, measure.high_threshold);
                            match thresholds {
                                (Some(low), Some(high))
                                    if low.is_finite() && high.is_finite() && low < high => {}
                                _ => {
                                    return Err(invalid_measure(
                                        measure,
                                        format!(
                                            "measure `{}` is a rise_time measure and needs a \
                                             `low_threshold` below a finite `high_threshold`",
                                            measure.name
                                        ),
                                    ));
                                }
                            }
                        }
                        _ => {
                            for (field, present) in [
                                ("tolerance", measure.tolerance.is_some()),
                                ("low_threshold", measure.low_threshold.is_some()),
                                ("high_threshold", measure.high_threshold.is_some()),
                            ] {
                                if present {
                                    return Err(invalid_measure(
                                        measure,
                                        format!(
                                            "measure `{}` is a {} measure and must not have \
                                             `{field}`; only settling_time uses `tolerance` \
                                             and rise_time uses the thresholds",
                                            measure.name,
                                            kind_toml_name(measure.kind)
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                }
                _ => unreachable!("handled by the arms above"),
            }
            if let Some(at_frequency) = measure.at_frequency {
                if !at_frequency.is_finite() || at_frequency <= 0.0 {
                    return Err(invalid_measure(
                        measure,
                        format!(
                            "measure `{}` has an `at_frequency` that must be a positive finite frequency",
                            measure.name
                        ),
                    ));
                }
                if let Some(ac) = &self.analysis.ac {
                    // ngspice cannot interpolate at the last sweep point
                    // (`find(AT) : out of interval`), so stop itself is out.
                    if at_frequency < ac.start || at_frequency >= ac.stop {
                        let suffix = if at_frequency >= ac.stop {
                            " (the stop point has no interval)"
                        } else {
                            ""
                        };
                        return Err(invalid_measure(
                            measure,
                            format!(
                                "measure `{}` reads at {at_frequency} Hz, outside the ac sweep \
                                 {}-{} Hz{suffix}",
                                measure.name, ac.start, ac.stop
                            ),
                        ));
                    }
                }
            }
            let requires_ac = measure.kind == MeasureKind::RelativeDb
                || (measure.kind == MeasureKind::NodeVoltage && measure.at_frequency.is_some());
            if requires_ac && self.analysis.ac.is_none() {
                if measure.kind == MeasureKind::RelativeDb {
                    return Err(invalid_measure(
                        measure,
                        format!(
                            "measure `{}` is a relative_db measure and requires the `ac` analysis",
                            measure.name
                        ),
                    ));
                }
                return Err(invalid_measure(
                    measure,
                    format!(
                        "measure `{}` reads a gain at a fixed frequency and requires the `ac` analysis",
                        measure.name
                    ),
                ));
            }
            if requires_ac
                && !self
                    .source
                    .iter()
                    .any(|source| source.ac.is_some_and(|ac| ac > 0.0))
            {
                if measure.kind == MeasureKind::RelativeDb {
                    return Err(invalid_measure(
                        measure,
                        format!(
                            "measure `{}`: relative_db needs a source with a non-zero `ac`",
                            measure.name
                        ),
                    ));
                }
                return Err(invalid_measure(
                    measure,
                    format!(
                        "measure `{}`: at_frequency needs a source with a non-zero `ac`",
                        measure.name
                    ),
                ));
            }
            if measure.kind.is_transient() {
                let Some(tran) = &self.analysis.tran else {
                    return Err(invalid_measure(
                        measure,
                        format!(
                            "measure `{}` is a {} measure and requires the `tran` analysis",
                            measure.name,
                            kind_toml_name(measure.kind)
                        ),
                    ));
                };
                for (field, value) in [
                    ("window_start", measure.window_start),
                    ("window_end", measure.window_end),
                ] {
                    if let Some(value) = value
                        && !value.is_finite()
                    {
                        return Err(invalid_measure(
                            measure,
                            format!("measure `{}` has a non-finite `{field}`", measure.name),
                        ));
                    }
                }
                let start = measure.window_start.unwrap_or(0.0);
                let end = measure.window_end.unwrap_or(tran.stop);
                if start < 0.0 {
                    return Err(invalid_measure(
                        measure,
                        format!("measure `{}` has a negative `window_start`", measure.name),
                    ));
                }
                if end > tran.stop {
                    return Err(invalid_measure(
                        measure,
                        format!(
                            "measure `{}` has a `window_end` ({end}) past the tran stop {}",
                            measure.name, tran.stop
                        ),
                    ));
                }
                if end <= start {
                    return Err(invalid_measure(
                        measure,
                        format!(
                            "measure `{}` has a `window_end` ({end}) not greater than its \
                             `window_start` ({start})",
                            measure.name
                        ),
                    ));
                }
            } else if !requires_ac && !self.analysis.op {
                return Err(invalid_measure(
                    measure,
                    format!("measure `{}` requires the `op` analysis", measure.name),
                ));
            }
            for (label, value) in [("min", measure.min), ("max", measure.max)] {
                if let Some(value) = value
                    && !value.is_finite()
                {
                    return Err(invalid_measure(
                        measure,
                        format!("measure `{}` has a non-finite `{label}`", measure.name),
                    ));
                }
            }
            if let (Some(min), Some(max)) = (measure.min, measure.max)
                && min > max
            {
                return Err(invalid_measure(
                    measure,
                    format!(
                        "measure `{}` has min {min} greater than max {max}",
                        measure.name
                    ),
                ));
            }
        }
        Ok(())
    }
}

fn invalid_measure(measure: &Measure, message: impl Into<String>) -> Diagnostic {
    at_measure(testbench_error(message), measure)
}

/// The four pulse timing values, only when every one is present.
fn pulse_timing(source: &Source) -> Option<(f64, f64, f64, f64)> {
    Some((source.period?, source.duty?, source.rise?, source.fall?))
}

/// The pulse checks every pulse source shares: a complete, usable timing.
fn validate_pulse_timing(
    timing: Option<(f64, f64, f64, f64)>,
    description: &str,
) -> Result<(), Diagnostic> {
    let Some((period, duty, rise, fall)) = timing else {
        return Err(testbench_error(format!(
            "{description} has no complete pulse timing; a pulse source needs `period`, \
             `duty`, `rise` and `fall`"
        )));
    };
    if !period.is_finite() || period <= 0.0 {
        return Err(testbench_error(format!(
            "{description} has a `period` that must be a positive finite time"
        )));
    }
    if !(duty.is_finite() && duty > 0.0 && duty <= 1.0) {
        return Err(testbench_error(format!(
            "{description} has a `duty` that must be greater than 0 and at most 1"
        )));
    }
    for (field, value) in [("rise", rise), ("fall", fall)] {
        if !value.is_finite() || value < 0.0 {
            return Err(testbench_error(format!(
                "{description} has a `{field}` that must be a non-negative finite time"
            )));
        }
    }
    if rise + fall > period {
        return Err(testbench_error(format!(
            "{description} has `rise` + `fall` greater than `period`; the pulse never \
             reaches its levels"
        )));
    }
    Ok(())
}

/// The extra checks a net pulse source needs: its own low and high levels.
fn validate_net_pulse(net: &str, source: &Source) -> Result<(), Diagnostic> {
    let description = format!("source on net `{net}`");
    for (field, value) in [("low", source.low), ("high", source.high)] {
        let Some(value) = value else {
            return Err(testbench_error(format!(
                "{description} has no `{field}`; a net pulse source needs its own pulse levels"
            )));
        };
        if !value.is_finite() {
            return Err(testbench_error(format!(
                "{description} has a non-finite `{field}` value"
            )));
        }
    }
    let low = source.low.unwrap_or_default();
    let high = source.high.unwrap_or_default();
    if low >= high {
        return Err(testbench_error(format!(
            "{description} has `low` ({low}) not less than `high` ({high})"
        )));
    }
    validate_pulse_timing(pulse_timing(source), &description)
}

/// Mark a diagnostic as belonging to one `[[measure]]` block.
fn at_measure(mut diagnostic: Diagnostic, measure: &Measure) -> Diagnostic {
    if !measure.span.is_empty() {
        diagnostic.location = Some(Location::from_span(String::new(), measure.span));
    }
    diagnostic
}

fn at_named_measure(diagnostic: Diagnostic, testbench: &Testbench, name: &str) -> Diagnostic {
    match testbench
        .measure
        .iter()
        .find(|measure| measure.name == name)
    {
        Some(measure) => at_measure(diagnostic, measure),
        None => diagnostic,
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
    /// Absolute wall-clock deadline for this run. The testbench's own
    /// timeout still applies when it is shorter.
    pub deadline: Option<std::time::Instant>,
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
    /// Unit of `value`: `V`, `A`, `dB`, `Hz` or `mA`.
    pub unit: &'static str,
}

impl MeasureResult {
    pub fn passed(&self) -> bool {
        matches!(
            limit_outcome(self.min, self.max, self.value),
            LimitOutcome::Pass
        )
    }
}

/// The unit `simulate` reports for a measure. An `at_frequency` node voltage
/// reads `vdb`, so it is dB rather than V. A transient element current is
/// amps, a node transient is volts.
fn measure_unit(measure: &Measure) -> &'static str {
    match measure.kind {
        MeasureKind::NodeVoltage if measure.at_frequency.is_some() => "dB",
        MeasureKind::NodeVoltage => "V",
        MeasureKind::Current => "A",
        MeasureKind::RelativeDb => "Hz",
        MeasureKind::PinCurrent => "mA",
        MeasureKind::Average | MeasureKind::Rms | MeasureKind::PeakToPeak => {
            if measure.element.is_some() { "A" } else { "V" }
        }
        MeasureKind::SettlingTime | MeasureKind::RiseTime => "s",
        MeasureKind::Overshoot => "%",
    }
}

/// The measure kind's TOML spelling, for diagnostics that name the kind.
fn kind_toml_name(kind: MeasureKind) -> &'static str {
    match kind {
        MeasureKind::NodeVoltage => "node_voltage",
        MeasureKind::Current => "current",
        MeasureKind::RelativeDb => "relative_db",
        MeasureKind::PinCurrent => "pin_current",
        MeasureKind::Average => "average",
        MeasureKind::Rms => "rms",
        MeasureKind::PeakToPeak => "peak_to_peak",
        MeasureKind::SettlingTime => "settling_time",
        MeasureKind::RiseTime => "rise_time",
        MeasureKind::Overshoot => "overshoot",
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
    if let Some(min) = min
        && value < min
        && !at_print_precision(value, min)
    {
        return LimitOutcome::Below(min);
    }
    if let Some(max) = max
        && value > max
        && !at_print_precision(value, max)
    {
        return LimitOutcome::Above(max);
    }
    LimitOutcome::Pass
}

/// The JSON schema version shared by `synth sim --json` and `synth_simulate`.
pub const SIM_REPORT_SCHEMA: &str = "synth.sim.v1";

/// What the simulation actually used and measured.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SimCoverage {
    /// Components represented in the generated deck.
    pub modelled: Vec<String>,
    /// Components named by the sidecar's `ignore` list.
    pub ignored: Vec<String>,
    /// Components with no usable SPICE model that were not ignored.
    pub unmodelled: Vec<String>,
    /// Net, element and pin targets named by measures.
    pub measured: Vec<String>,
}

/// What a simulation produced. Blocking diagnostics mean it did not pass.
#[derive(Debug, Clone)]
pub struct SimReport {
    pub measures: Vec<MeasureResult>,
    pub diagnostics: Vec<Diagnostic>,
    pub coverage: SimCoverage,
}

impl SimReport {
    pub fn passed(&self) -> bool {
        !self.diagnostics.iter().any(|d| d.severity.is_blocking())
    }
}

/// The one JSON shape for a simulation result.
///
/// Callers may add compile diagnostics that predate the simulation, which is
/// why the pieces are passed separately rather than taking a [`SimReport`].
#[must_use]
pub fn report_json(
    passed: bool,
    measures: &[MeasureResult],
    diagnostics: &[Diagnostic],
    coverage: &SimCoverage,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": SIM_REPORT_SCHEMA,
        "passed": passed,
        "measures": measures.iter().map(|measure| serde_json::json!({
            "name": &measure.name,
            "value": measure.value,
            "unit": measure.unit,
            "min": measure.min,
            "max": measure.max,
            // A measure with no limits has nothing to pass or fail; the CLI
            // prints MEASURED, so `null` keeps agents from reading it as a pass.
            "passed": (measure.min.is_some() || measure.max.is_some()).then(|| measure.passed()),
        })).collect::<Vec<_>>(),
        "diagnostics": diagnostics,
        "coverage": coverage,
    })
}

/// A report for an error that happened before a simulation could start.
#[must_use]
pub fn error_report_json(message: &str) -> serde_json::Value {
    let mut report = report_json(false, &[], &[], &SimCoverage::default());
    report["error"] = serde_json::Value::String(message.to_string());
    report
}

/// Coverage known before a deck exists: the explicit ignore list and the
/// measure targets. No component is reported as modelled until the emitter
/// has written it.
fn requested_coverage(testbench: &Testbench) -> SimCoverage {
    SimCoverage {
        modelled: Vec::new(),
        ignored: sorted_unique(testbench.ignore.clone()),
        unmodelled: Vec::new(),
        measured: measured_targets(testbench),
    }
}

fn measured_targets(testbench: &Testbench) -> Vec<String> {
    let measured = testbench
        .measure
        .iter()
        .flat_map(|measure| {
            [&measure.node, &measure.element, &measure.pin]
                .into_iter()
                .flatten()
                .cloned()
        })
        .collect();
    sorted_unique(measured)
}

fn sorted_unique(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
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
    let EmittedDeck {
        deck,
        dialect,
        plot,
        pin_limits,
        tran_files,
        coverage,
    } = match emit_deck(
        design,
        testbench,
        options.design_path.as_deref(),
        options.testbench_path.as_deref(),
        &options.registry_dirs,
    ) {
        Ok(emitted) => emitted,
        Err(failure) => {
            return failed(
                failure.diagnostics,
                options.testbench_path.as_deref(),
                &failure.coverage,
            );
        }
    };

    // `wrdata` fails when the output directory is absent; create it before
    // ngspice starts so the failure is a start-up error, not a lost plot.
    if let Some(plot) = &plot
        && let Some(parent) = plot.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return failed(
            vec![testbench_error_at(
                options.testbench_path.as_deref(),
                format!(
                    "could not create the plot directory `{}`: {error}",
                    parent.display()
                ),
            )],
            options.testbench_path.as_deref(),
            &coverage,
        );
    }

    let timeout = testbench
        .options
        .timeout
        .map_or(DEFAULT_TIMEOUT, Duration::from_secs_f64);
    let timeout = if let Some(deadline) = options.deadline {
        timeout.min(deadline.saturating_duration_since(std::time::Instant::now()))
    } else {
        timeout
    };
    if timeout.is_zero() {
        return failed(
            vec![deadline_error()],
            options.testbench_path.as_deref(),
            &coverage,
        );
    }
    let (run, run_dir) = match run_ngspice(&deck, dialect, options, timeout) {
        Ok(run) => run,
        Err(diagnostic) => {
            return failed(
                vec![diagnostic],
                options.testbench_path.as_deref(),
                &coverage,
            );
        }
    };

    // A capped log can hide the singular-matrix warning that would have
    // failed the run, so a partial capture is never trusted.
    if run.output_truncated {
        return failed(
            vec![ngspice_error(format!(
                "ngspice output exceeded the {MAX_OUTPUT_BYTES}-byte cap; refusing to trust a truncated log"
            ))],
            options.testbench_path.as_deref(),
            &coverage,
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
            &coverage,
        );
    }

    // A singular matrix, including one with a missing SYNTH_DONE, is a
    // broken run and must classify as `-006` before measurement parsing.
    if let Some(diagnostic) = convergence_stderr_error(&run.stderr) {
        return failed(
            vec![diagnostic],
            options.testbench_path.as_deref(),
            &coverage,
        );
    }

    // `Error: measure ...` is a measurement failure even when ngspice exits
    // 0. It runs after the singular check so a failed op that also drops a
    // vector is `-006`, not a measurement failure.
    if let Some(diagnostic) = measurement_failure_from_stderr(&run.stderr, testbench) {
        return failed(
            vec![diagnostic],
            options.testbench_path.as_deref(),
            &coverage,
        );
    }

    // Other stderr errors must classify as `-006` before sentinel parsing
    // turns the same run into a missing measurement.
    if let Some(diagnostic) = stderr_error(&run.stderr) {
        return failed(
            vec![diagnostic],
            options.testbench_path.as_deref(),
            &coverage,
        );
    }

    let values = match parse_sentinels(&run.stdout, &run.stderr, testbench) {
        Ok(values) => values,
        Err(diagnostics) => {
            return failed(diagnostics, options.testbench_path.as_deref(), &coverage);
        }
    };

    // Transient measures read their waveforms out of the `wrdata` files
    // ngspice wrote into the run directory; every value still goes through
    // the same limit pipeline.
    let mut tran_diagnostics: Vec<Diagnostic> = Vec::new();
    let mut tran_values: HashMap<String, f64> = HashMap::new();
    if let Some(tran) = &testbench.analysis.tran {
        let tran_stop = tran.stop;
        for measure in &testbench.measure {
            if !measure.kind.is_transient() {
                continue;
            }
            let Some(file) = tran_files.get(&measure.name) else {
                // Emission already checked every transient measure; keep a
                // report instead of panicking if that changes.
                tran_diagnostics.push(with_location_span(
                    measure_error(format!(
                        "measure `{}` has no transient data in the run",
                        measure.name
                    )),
                    options.testbench_path.as_deref(),
                    measure.span,
                ));
                continue;
            };
            let path = run_dir.path().join(file);
            let waveform = match read_waveform(&path) {
                Ok(waveform) => waveform,
                Err(error) => {
                    tran_diagnostics.push(with_location_span(
                        measure_error(format!(
                            "measure `{}` could not read its transient data `{}`: {error}",
                            measure.name,
                            file.as_str()
                        )),
                        options.testbench_path.as_deref(),
                        measure.span,
                    ));
                    continue;
                }
            };
            match transient_value(
                measure,
                measure.window_start.unwrap_or(0.0),
                measure.window_end.unwrap_or(tran_stop),
                &waveform,
            ) {
                Ok(value) => {
                    tran_values.insert(measure.name.clone(), printed_precision(value));
                }
                Err(error) => tran_diagnostics.push(with_location_span(
                    measure_error(format!("measure `{}` failed: {error}", measure.name)),
                    options.testbench_path.as_deref(),
                    measure.span,
                )),
            }
        }
    }
    if !tran_diagnostics.is_empty() {
        let mut diagnostics = tran_diagnostics;
        locate(&mut diagnostics, options.testbench_path.as_deref());
        return SimReport {
            measures: Vec::new(),
            diagnostics,
            coverage,
        };
    }

    let mut measures = Vec::new();
    let mut diagnostics = Vec::new();
    for measure in &testbench.measure {
        let value = if measure.kind.is_transient() {
            tran_values[measure.name.as_str()]
        } else {
            values[measure.name.as_str()]
        };
        if measure.kind == MeasureKind::PinCurrent {
            let pin = measure.pin.as_deref().unwrap_or_default();
            let Some(limit) = pin_limits.get(pin) else {
                // Emission already checked every pin_current against its
                // source; keep a report instead of panicking if that changes.
                diagnostics.push(with_location_span(
                    testbench_error(format!(
                        "measure `{}` pin `{pin}` has no matching `[[source]]`",
                        measure.name
                    )),
                    options.testbench_path.as_deref(),
                    measure.span,
                ));
                continue;
            };
            measures.push(MeasureResult {
                name: measure.name.clone(),
                value,
                min: Some(0.0),
                max: Some(limit.limit_ma),
                unit: measure_unit(measure),
            });
            let diagnostic = if value < 0.0 {
                pin_current_wrong_way(&measure.name, value, limit)
            } else if matches!(
                limit_outcome(None, Some(limit.limit_ma), value),
                LimitOutcome::Pass
            ) {
                pin_current_info(&measure.name, value, limit)
            } else {
                pin_current_error(&measure.name, value, limit)
            };
            diagnostics.push(with_location_span(
                diagnostic,
                options.testbench_path.as_deref(),
                measure.span,
            ));
            continue;
        }
        let result = MeasureResult {
            name: measure.name.clone(),
            value,
            min: measure.min,
            max: measure.max,
            unit: measure_unit(measure),
        };
        match limit_outcome(measure.min, measure.max, value) {
            LimitOutcome::Pass => {
                if measure.min.is_some() || measure.max.is_some() {
                    diagnostics.push(with_location_span(
                        limit_info(&measure.name, value),
                        options.testbench_path.as_deref(),
                        measure.span,
                    ));
                }
            }
            LimitOutcome::Below(min) => diagnostics.push(with_location_span(
                limit_error(&measure.name, value, &format!("below min {min}")),
                options.testbench_path.as_deref(),
                measure.span,
            )),
            LimitOutcome::Above(max) => diagnostics.push(with_location_span(
                limit_error(&measure.name, value, &format!("above max {max}")),
                options.testbench_path.as_deref(),
                measure.span,
            )),
        }
        measures.push(result);
    }

    locate(&mut diagnostics, options.testbench_path.as_deref());
    SimReport {
        measures,
        diagnostics,
        coverage,
    }
}

fn failed(
    mut diagnostics: Vec<Diagnostic>,
    path: Option<&Path>,
    coverage: &SimCoverage,
) -> SimReport {
    locate(&mut diagnostics, path);
    SimReport {
        measures: Vec::new(),
        diagnostics,
        coverage: coverage.clone(),
    }
}

/// Anchor diagnostics that have no source location of their own to the
/// sidecar. Design-anchored diagnostics already carry their component span.
fn locate(diagnostics: &mut [Diagnostic], path: Option<&Path>) {
    let Some(path) = path else {
        // A measure span with no sidecar path cannot be shown; keep run-level
        // diagnostics only.
        for diagnostic in diagnostics {
            if diagnostic
                .location
                .as_ref()
                .is_some_and(|location| location.file.is_empty())
            {
                diagnostic.location = None;
            }
        }
        return;
    };
    let file = path.display().to_string();
    for diagnostic in diagnostics {
        match &mut diagnostic.location {
            None => {
                diagnostic.location = Some(Location::from_span(file.clone(), Span::new(0, 0)));
            }
            Some(location) if location.file.is_empty() => location.file.clone_from(&file),
            Some(_) => {}
        }
    }
}

/// Default wall-clock budget, overridable with `[options] timeout`.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// Upper bound on `[options] timeout`; `Instant + Duration` must not overflow.
/// Upper bound for a simulation deadline, in seconds.
pub const MAX_TIMEOUT_SECONDS: f64 = 3600.0;
/// Upper bound on `[analysis.tran]` output points (stop/step). A transient
/// is the run most likely to hang or fill the disk, so the cap keeps a
/// sidecar typo from turning into hours of wall clock and gigabytes of
/// `wrdata` output.
const MAX_TRAN_POINTS: f64 = 100_000.0;
/// ngspice logs are small; this only bounds a runaway `.print` or `.plot`.
const MAX_OUTPUT_BYTES: usize = 1 << 20;

/// Run ngspice in a fresh temp directory with its own `.spiceinit`, so a
/// user's `~/.spiceinit` and concurrent runs cannot interfere. The directory
/// is returned so the caller can read `wrdata` output (transient measures)
/// before it is removed.
fn run_ngspice(
    deck: &str,
    dialect: Option<SpiceDialect>,
    options: &SimOptions,
    timeout: Duration,
) -> Result<(Invocation, TempDir), Diagnostic> {
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
    Ok((run, dir))
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

/// Stderr lines that say why an analysis stopped early. A transient
/// convergence failure aborts the control block, so `SYNTH_DONE` is never
/// printed; the reason goes into the `-003` diagnostic with the sentinel.
fn stderr_analysis_failure(stderr: &str) -> Option<String> {
    let offenders = stderr_offenders(stderr, |line| {
        line.contains("timestep too small")
            || line.contains("failed to converge")
            || line.contains("no convergence")
            || line.contains("doanalyses")
    });
    (!offenders.is_empty()).then(|| excerpt(&offenders.join("\n")))
}

fn parse_sentinels(
    stdout: &str,
    stderr: &str,
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
            let diagnostic = measure_error(format!("malformed measurement sentinel `{line}`"));
            diagnostics.push(match name {
                Some(name) => at_named_measure(diagnostic, testbench, name),
                None => diagnostic,
            });
            continue;
        };
        if !expected.contains(name) {
            diagnostics.push(measure_error(format!(
                "unexpected measurement `{name}` in ngspice output"
            )));
            continue;
        }
        if values.contains_key(name) {
            diagnostics.push(at_named_measure(
                measure_error(format!("measurement `{name}` was printed twice")),
                testbench,
                name,
            ));
            continue;
        }
        let Ok(value) = raw.parse::<f64>() else {
            diagnostics.push(at_named_measure(
                measure_error(format!(
                    "measurement `{name}` has unparseable value `{raw}`"
                )),
                testbench,
                name,
            ));
            continue;
        };
        if !value.is_finite() {
            diagnostics.push(at_named_measure(
                measure_error(format!("measurement `{name}` is not finite: `{raw}`")),
                testbench,
                name,
            ));
            continue;
        }
        values.insert(name.to_string(), value);
    }

    if !done {
        let reason = stderr_analysis_failure(stderr).map_or_else(String::new, |reason| {
            format!("; ngspice reported: {reason}")
        });
        diagnostics.push(measure_error(format!(
            "ngspice did not print SYNTH_DONE{reason}"
        )));
    }
    for measure in &testbench.measure {
        // A transient measure is computed from its `wrdata` file, not from a
        // sentinel.
        if measure.kind.is_transient() {
            continue;
        }
        if !values.contains_key(&measure.name) && !reported.contains(&measure.name) {
            diagnostics.push(at_measure(
                measure_error(format!("measurement `{}` was not printed", measure.name)),
                measure,
            ));
        }
    }

    if diagnostics.is_empty() {
        Ok(values)
    } else {
        Err(diagnostics)
    }
}

/// A transient waveform read back from a `wrdata` file: one time and value
/// pair per output point.
struct Waveform {
    times: Vec<f64>,
    values: Vec<f64>,
}

fn read_waveform(path: &Path) -> Result<Waveform, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut times = Vec::new();
    let mut values = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut columns = line.split_whitespace();
        let (Some(time), Some(value), None) = (columns.next(), columns.next(), columns.next())
        else {
            return Err(format!(
                "unexpected row `{line}`; `wrdata` writes time and value columns"
            ));
        };
        let time: f64 = time
            .parse()
            .map_err(|error| format!("row `{line}`: {error}"))?;
        let value: f64 = value
            .parse()
            .map_err(|error| format!("row `{line}`: {error}"))?;
        times.push(time);
        values.push(value);
    }
    if times.is_empty() {
        return Err("the analysis produced no output points".to_string());
    }
    Ok(Waveform { times, values })
}

/// The rows of a waveform inside `[start, end]`.
fn window_slice(wave: &Waveform, start: f64, end: f64) -> (Vec<f64>, Vec<f64>) {
    let times: Vec<f64> = wave
        .times
        .iter()
        .copied()
        .filter(|time| *time >= start && *time <= end)
        .collect();
    let values: Vec<f64> = wave
        .times
        .iter()
        .zip(&wave.values)
        .filter(|(time, _)| **time >= start && **time <= end)
        .map(|(_, value)| *value)
        .collect();
    (times, values)
}

/// The time of the first upward crossing of `threshold` at or after `after`,
/// interpolated between the two samples that straddle it.
fn upward_crossing(
    times: &[f64],
    values: &[f64],
    threshold: f64,
    after: f64,
) -> Result<f64, String> {
    if times.len() < 2 {
        return Err(format!("no rising crossing of {threshold} in the window"));
    }
    for index in 0..times.len() - 1 {
        if times[index] < after {
            continue;
        }
        if values[index] < threshold && values[index + 1] >= threshold {
            let rise = values[index + 1] - values[index];
            let fraction = if rise > 0.0 {
                (threshold - values[index]) / rise
            } else {
                0.0
            };
            return Ok(times[index] + fraction * (times[index + 1] - times[index]));
        }
    }
    Err(format!("no rising crossing of {threshold} in the window"))
}

/// Trapezoidal time-weighted mean of `f(value)` over `times`.
fn time_weighted(times: &[f64], values: &[f64], f: impl Fn(f64) -> f64) -> f64 {
    let count = || f64::from(u32::try_from(values.len()).unwrap_or(u32::MAX));
    if times.len() < 2 {
        return values.iter().map(|v| f(*v)).sum::<f64>() / count().max(1.0);
    }
    let mut area = 0.0;
    for index in 0..times.len() - 1 {
        let dt = times[index + 1] - times[index];
        area += f64::midpoint(f(values[index]), f(values[index + 1])) * dt;
    }
    let span = times[times.len() - 1] - times[0];
    if span > 0.0 {
        area / span
    } else {
        values.iter().map(|v| f(*v)).sum::<f64>() / count()
    }
}

/// The transient value of one measure over its window.
fn transient_value(
    measure: &Measure,
    start: f64,
    end: f64,
    wave: &Waveform,
) -> Result<f64, String> {
    let (times, values) = window_slice(wave, start, end);
    if values.is_empty() {
        return Err(format!("no samples inside the window {start}..{end} s"));
    }
    match measure.kind {
        MeasureKind::Average => {
            // ngspice's transient timepoints are adaptive, so a plain sample
            // mean weighs a dense switching region more than a long flat one.
            // Integrate over time instead.
            Ok(time_weighted(&times, &values, |v| v))
        }
        MeasureKind::Rms => {
            let mean_square = time_weighted(&times, &values, |v| v * v);
            Ok(mean_square.max(0.0).sqrt())
        }
        MeasureKind::PeakToPeak => {
            let min = values.iter().copied().fold(f64::INFINITY, f64::min);
            let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            Ok(max - min)
        }
        MeasureKind::SettlingTime => {
            let tolerance = measure
                .tolerance
                .expect("a validated settling_time measure has a tolerance");
            let final_value = values[values.len() - 1];
            let band_low = final_value - tolerance;
            let band_high = final_value + tolerance;
            let last_outside = values
                .iter()
                .rposition(|value| *value < band_low || *value > band_high);
            match last_outside {
                None => Ok(0.0),
                Some(index) if index + 1 == values.len() => Err(format!(
                    "the waveform never enters the ±{tolerance} band around \
                     {final_value} within the window"
                )),
                Some(index) => Ok(times[index + 1] - start),
            }
        }
        MeasureKind::RiseTime => {
            let low = measure
                .low_threshold
                .expect("a validated rise_time measure has a low threshold");
            let high = measure
                .high_threshold
                .expect("a validated rise_time measure has a high threshold");
            let t_low = upward_crossing(&times, &values, low, f64::NEG_INFINITY)?;
            let t_high = upward_crossing(&times, &values, high, t_low)?;
            Ok(t_high - t_low)
        }
        MeasureKind::Overshoot => {
            let final_value = values[values.len() - 1];
            if final_value == 0.0 {
                return Err(
                    "the waveform ends at 0; overshoot is a percentage of the window's \
                     final value"
                        .to_string(),
                );
            }
            let peak = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            Ok((peak - final_value) / final_value * 100.0)
        }
        _ => unreachable!("transient_value is only called for transient measures"),
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

/// A pin source's derived current limit. The sidecar never declares it; the
/// registry's drive data and the source level do.
struct PinDriveLimit {
    pin: String,
    level: SourceLevel,
    limit_ma: f64,
    field: &'static str,
    datasheet_ref: String,
    /// The generated voltage-source element, e.g. `V1`.
    element: String,
}

fn pin_current_message(name: &str, value: f64, direction: &str, limit: &PinDriveLimit) -> String {
    format!(
        "measure `{name}` = {value} mA {direction} the `{}` {} pin limit {} = {} mA \
         (datasheet: {}); ngspice prints 6 significant digits",
        limit.pin,
        limit.level.as_str(),
        limit.field,
        limit.limit_ma,
        limit.datasheet_ref
    )
}

fn pin_current_info(name: &str, value: f64, limit: &PinDriveLimit) -> Diagnostic {
    sim_builder(E_SYNTH_SIM_004, Severity::Info, "measurement limit")
        .message(pin_current_message(name, value, "is within", limit))
        .build()
}

fn pin_current_error(name: &str, value: f64, limit: &PinDriveLimit) -> Diagnostic {
    sim_builder(E_SYNTH_SIM_004, Severity::Error, "measurement limit")
        .message(pin_current_message(name, value, "is above", limit))
        .build()
}

/// A pin current whose sign says the circuit drives the pin the wrong way:
/// a high pin is sinking current, or a low pin is sourcing it.
fn pin_current_wrong_way(name: &str, value: f64, limit: &PinDriveLimit) -> Diagnostic {
    let direction = match limit.level {
        SourceLevel::High => "sinking",
        SourceLevel::Low => "sourcing",
    };
    sim_builder(E_SYNTH_SIM_004, Severity::Error, "measurement limit")
        .message(format!(
            "measure `{name}` = {value} mA: pin is being driven against its level: \
             {direction} {} mA while {} on `{}` (datasheet: {}); ngspice prints 6 significant digits",
            value.abs(),
            limit.level.as_str(),
            limit.pin,
            limit.datasheet_ref
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

fn deadline_error() -> Diagnostic {
    sim_builder(
        E_SYNTH_SIM_005,
        Severity::Error,
        "simulation deadline expired",
    )
    .message(
        "the simulation deadline expired before ngspice started; \
             raise `--timeout` or simplify the design",
    )
    .build()
}

/// A transient value is computed in Rust at full precision, but every other
/// measure is read back at ngspice's six printed digits. Rounding here keeps
/// the print-precision limit rule and the CLI output consistent.
fn printed_precision(value: f64) -> f64 {
    format!("{value:.5e}").parse().unwrap_or(value)
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
///
/// A failed corner names the generated `synth_corner<N>` vector; map it back
/// to the Synth measure so the message names what the author wrote.
fn measurement_failure_from_stderr(stderr: &str, testbench: &Testbench) -> Option<Diagnostic> {
    let failed = stderr_offenders(stderr, |line| {
        line.contains("error: measure") || line.contains("no such variable")
    });
    if failed.is_empty() {
        return None;
    }
    if let Some(diagnostic) = relative_db_failure(stderr, testbench) {
        return Some(diagnostic);
    }
    let excerpt = excerpt(&failed.join("\n"));
    // `let m<N>` is emitted per measure, so a missing variable identifies it;
    // an `at_frequency` gain names `synth_gain<N>` instead.
    Some(
        match stderr
            .lines()
            .find_map(|line| gain_measure_index(line).or_else(|| let_measure_index(line)))
            .and_then(|index| testbench.measure.get(index))
        {
            Some(measure) => at_measure(
                measure_error(format!(
                    "measure `{}` failed: ngspice reported a failed measurement: {excerpt}",
                    measure.name
                )),
                measure,
            ),
            None => measure_error(format!("ngspice reported a failed measurement: {excerpt}")),
        },
    )
}

/// The index in a generated `m<N>` vector on a line, when it carries one.
fn let_measure_index(line: &str) -> Option<usize> {
    let rest = line.split_once("&m")?.1;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The index in a generated `synth_gain<N>` vector on a line, when it carries
/// one; a failed `at_frequency` measure names that vector, not `m<N>`.
fn gain_measure_index(line: &str) -> Option<usize> {
    let rest = line.split_once("synth_gain")?.1;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The index in `synth_corner<N>` on a line, when it carries one.
fn corner_measure_index(line: &str) -> Option<usize> {
    let rest = line.split_once("synth_corner")?.1;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn relative_db_failure(stderr: &str, testbench: &Testbench) -> Option<Diagnostic> {
    let ac = testbench.analysis.ac.as_ref()?;
    let index = stderr.lines().find_map(corner_measure_index)?;
    let measure = testbench.measure.get(index)?;
    if measure.kind != MeasureKind::RelativeDb {
        return None;
    }
    let node = measure.node.as_deref()?;
    let drop_db = measure.drop_db.unwrap_or(3.0);
    Some(at_measure(
        measure_error(format!(
            "measure `{}` at node `{}` failed: no -{drop_db} dB crossing in {}-{} Hz",
            measure.name, node, ac.start, ac.stop
        )),
        measure,
    ))
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
    emit_deck(design, testbench, None, None, registry_dirs)
        .map(|emitted| emitted.deck)
        .map_err(|failure| failure.diagnostics)
}

/// Why a deck was not emitted, with the coverage known at that point.
struct EmitFailure {
    diagnostics: Vec<Diagnostic>,
    coverage: SimCoverage,
}

/// The emitted deck plus the data `simulate` needs after ngspice runs.
struct EmittedDeck {
    deck: String,
    dialect: Option<SpiceDialect>,
    plot: Option<PathBuf>,
    /// Pin-source current limits, keyed by the source's `REF.PIN` string.
    pin_limits: HashMap<String, PinDriveLimit>,
    /// `wrdata` files for transient measures, keyed by measure name. The
    /// files sit in the ngspice run directory and are read after the run.
    tran_files: HashMap<String, String>,
    /// What the emitter actually put in the deck.
    coverage: SimCoverage,
}

#[allow(clippy::too_many_lines)]
fn emit_deck(
    design: &Board,
    testbench: &Testbench,
    design_path: Option<&Path>,
    testbench_path: Option<&Path>,
    registry_dirs: &[PathBuf],
) -> Result<EmittedDeck, EmitFailure> {
    let requested = requested_coverage(testbench);
    if let Err(diagnostic) = testbench.validate_at(testbench_path) {
        return Err(EmitFailure {
            diagnostics: vec![diagnostic],
            coverage: requested,
        });
    }
    let plot_path = match &testbench.options.plot {
        Some(plot) => match resolve_plot_path(plot, testbench_path) {
            Ok(path) => Some(path),
            Err(diagnostic) => {
                return Err(EmitFailure {
                    diagnostics: vec![diagnostic],
                    coverage: requested,
                });
            }
        },
        None => None,
    };
    let mut diagnostics = Vec::new();
    let tb_error = |message: String| testbench_error_at(testbench_path, message);

    let Some(ground) = design.nets.iter().find(|net| net.name == testbench.ground) else {
        return Err(EmitFailure {
            diagnostics: vec![tb_error(format!(
                "ground net `{}` not found in design",
                testbench.ground
            ))],
            coverage: requested,
        });
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
            if let Some(previous) = seen_nodes.insert(key, net.name.clone())
                && previous != net.name
            {
                diagnostics.push(tb_error(format!(
                    "nets `{previous}` and `{}` both emit as `{node}`",
                    net.name
                )));
            }
        }
        node_by_net.insert(net.id, node);
    }

    let mut pin_net: HashMap<(ComponentId, PinId), NetId> = HashMap::new();
    for net in &design.nets {
        for endpoint in &net.endpoints {
            pin_net.insert((endpoint.component, endpoint.pin), net.id);
        }
    }
    let mut seen_elements: HashMap<String, String> = HashMap::new();
    let mut source_lines = Vec::new();
    let mut sourced_nets: HashSet<NetId> = HashSet::new();
    let mut first_source_on_net: HashMap<NetId, String> = HashMap::new();
    let mut pin_limits: HashMap<String, PinDriveLimit> = HashMap::new();
    for (index, source) in testbench.source.iter().enumerate() {
        let element = format!("V{}", index + 1);
        // Build the source's deck lines first; a source that loses the
        // one-source-per-net check must leave no lines behind.
        let mut pending_lines: Vec<String> = Vec::new();
        let mut pending_limit: Option<(String, PinDriveLimit)> = None;
        let mut pending_resistor: Option<(String, String)> = None;
        let (net_id, description) = match (source.net.as_deref(), source.pin.as_deref()) {
            (Some(net_name), None) => {
                let Some(net) = net_by_name.get(net_name) else {
                    diagnostics.push(tb_error(format!(
                        "source net `{net_name}` not found in design"
                    )));
                    continue;
                };
                if net.id == ground.id {
                    diagnostics.push(tb_error(format!(
                        "source net `{net_name}` is the ground net"
                    )));
                    continue;
                }
                if source.kind == SourceKind::Dc && source.dc.is_none() && source.ac.is_none() {
                    diagnostics.push(tb_error(format!(
                        "source on net `{net_name}` has neither a `dc` nor an `ac` value"
                    )));
                    continue;
                }
                let waveform = match source.kind {
                    SourceKind::Dc => {
                        let mut line = format!("{element} {} 0", node_by_net[&net.id]);
                        if let Some(dc) = source.dc {
                            let _ = write!(line, " DC {}", exponent(dc));
                        }
                        if let Some(ac) = source.ac {
                            let _ = write!(line, " AC {}", exponent(ac));
                        }
                        line
                    }
                    SourceKind::Pulse => {
                        let low = source
                            .low
                            .expect("a validated net pulse source has a low level");
                        let high = source
                            .high
                            .expect("a validated net pulse source has a high level");
                        format!(
                            "{element} {} 0 {}",
                            node_by_net[&net.id],
                            pulse_spec(
                                low,
                                high,
                                source.rise.unwrap_or_default(),
                                source.fall.unwrap_or_default(),
                                source.duty.unwrap_or_default(),
                                source.period.unwrap_or_default(),
                            )
                        )
                    }
                };
                pending_lines.push(waveform);
                (net.id, format!("net `{net_name}`"))
            }
            (None, Some(pin_ref)) => {
                let (refdes, pin_name) = pin_ref
                    .split_once('.')
                    .expect("a validated pin source is REF.PIN");
                let Some(component) = design
                    .components
                    .iter()
                    .find(|component| component.refdes == refdes)
                else {
                    diagnostics.push(tb_error(format!(
                        "pin source `{pin_ref}` names reference `{refdes}`, which is not a \
                         placed part"
                    )));
                    continue;
                };
                let Some((pin_id, pin)) = component.find_pin(pin_name) else {
                    diagnostics.push(tb_error(format!(
                        "pin source `{pin_ref}`: part `{refdes}` has no pin `{pin_name}`"
                    )));
                    continue;
                };
                let Some(&net_id) = pin_net.get(&(component.id, pin_id)) else {
                    diagnostics.push(tb_error(format!(
                        "pin source `{pin_ref}`: pin is not connected to a net"
                    )));
                    continue;
                };
                if net_id == ground.id {
                    diagnostics.push(tb_error(format!(
                        "pin source `{pin_ref}`: pin is on the ground net"
                    )));
                    continue;
                }
                let Some(drive) = pin.drive.as_ref() else {
                    diagnostics.push(pin_drive_error(
                        component,
                        pin_ref,
                        "pin has no drive data in the registry; add a `[pins.drive]` block \
                         with the datasheet `high_v`, `low_v`, `max_source_ma`, `max_sink_ma` \
                         and `datasheet_ref`",
                        design_path,
                    ));
                    continue;
                };
                let Some(datasheet_ref) = drive
                    .datasheet_ref
                    .as_deref()
                    .map(str::trim)
                    .filter(|reference| !reference.is_empty())
                else {
                    diagnostics.push(pin_drive_error(
                        component,
                        pin_ref,
                        "pin has no `datasheet_ref` in its `[pins.drive]` block; record the \
                         datasheet page or table before simulating it",
                        design_path,
                    ));
                    continue;
                };
                let level = source.level;
                // A pulse pin source steps between both drive levels and
                // needs no `pin_current` limit at its level; that measure
                // requires the `op` analysis, which a pulse source cannot
                // share.
                let (waveform, limit) = if source.kind == SourceKind::Pulse {
                    let waveform = pulse_spec(
                        drive.low_v,
                        drive.high_v,
                        source.rise.unwrap_or_default(),
                        source.fall.unwrap_or_default(),
                        source.duty.unwrap_or_default(),
                        source.period.unwrap_or_default(),
                    );
                    (waveform, None)
                } else {
                    let level = level.expect("a validated dc pin source has a level");
                    let voltage = match level {
                        SourceLevel::High => drive.high_v,
                        SourceLevel::Low => drive.low_v,
                    };
                    let (limit_ma, field) = match level {
                        SourceLevel::High => (drive.max_source_ma, "max_source_ma"),
                        SourceLevel::Low => (drive.max_sink_ma, "max_sink_ma"),
                    };
                    (format!("DC {}", exponent(voltage)), Some((limit_ma, field)))
                };
                let net_node = &node_by_net[&net_id];
                if let Some(resistance) = drive.output_resistance_ohm {
                    let internal_node = format!("vpin_{}", index + 1);
                    let resistor = format!("Rpin{}", index + 1);
                    let key = resistor.to_ascii_lowercase();
                    if let Some(previous) = seen_elements.get(&key) {
                        diagnostics.push(tb_error(format!(
                            "pin source `{pin_ref}` generates element `{resistor}`, which \
                             collides with `{previous}`"
                        )));
                        continue;
                    }
                    pending_resistor = Some((key, format!("pin source {pin_ref}")));
                    pending_lines.push(format!("{element} {internal_node} 0 {waveform}"));
                    pending_lines.push(format!(
                        "{resistor} {net_node} {internal_node} {}",
                        exponent(resistance)
                    ));
                } else {
                    pending_lines.push(format!("{element} {net_node} 0 {waveform}"));
                }
                pending_limit = limit.map(|(limit_ma, field)| {
                    (
                        pin_ref.to_string(),
                        PinDriveLimit {
                            pin: pin_ref.to_string(),
                            level: level.expect("a limited pin source has a level"),
                            limit_ma,
                            field,
                            datasheet_ref: datasheet_ref.to_string(),
                            element,
                        },
                    )
                });
                (net_id, format!("pin `{pin_ref}`"))
            }
            (Some(net), Some(pin)) => {
                diagnostics.push(tb_error(format!(
                    "source has both `net` (`{net}`) and `pin` (`{pin}`); a source drives \
                     exactly one"
                )));
                continue;
            }
            (None, None) => {
                diagnostics.push(tb_error("source must have a `net` or a `pin`".to_string()));
                continue;
            }
        };
        if let Some(previous) = first_source_on_net.insert(net_id, description.clone()) {
            let net_name = design.net(net_id).map_or("", |net| net.name.as_str());
            diagnostics.push(tb_error(format!(
                "sources {previous} and {description} both resolve to net `{net_name}`; \
                 only one source may drive a net"
            )));
            continue;
        }
        source_lines.extend(pending_lines);
        if let Some((key, owner)) = pending_resistor {
            seen_elements.insert(key, owner);
        }
        if let Some((pin, limit)) = pending_limit {
            pin_limits.insert(pin, limit);
        }
        sourced_nets.insert(net_id);
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

    check_topology(
        design,
        ground.id,
        &ignored_ids,
        &pin_net,
        &sourced_nets,
        design_path,
        &mut diagnostics,
    );

    let roots: Vec<PathBuf> = registry_dirs
        .iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .collect();
    let mut model_text: Vec<String> = Vec::new();
    let mut included_files: HashSet<PathBuf> = HashSet::new();
    let mut dialects: Vec<(Option<SpiceDialect>, String, String)> = Vec::new();
    let mut element_lines = Vec::new();
    let mut model_lines = Vec::new();
    let mut modelled = Vec::new();
    let mut unmodelled = Vec::new();
    for component in &design.components {
        if ignored_ids.contains(&component.id) {
            continue;
        }
        let model = match model_for(component) {
            Ok(model) => model,
            Err(message) => {
                diagnostics.push(no_model_error(component, &message, design_path));
                unmodelled.push(component.refdes.clone());
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
                    diagnostics.push(model_error(component, &message, true, design_path));
                    unmodelled.push(component.refdes.clone());
                    continue;
                }
            };
            let (file, text) = match resolve_spice_model(spice, &roots) {
                Ok(resolved) => resolved,
                Err(message) => {
                    diagnostics.push(model_error(component, &message, true, design_path));
                    unmodelled.push(component.refdes.clone());
                    continue;
                }
            };
            // The checked text goes into the deck itself, so ngspice never
            // re-opens the file after the check.
            if included_files.insert(file) {
                model_text.push(text);
            }
            let dialect = spice.dialect;
            if let Some((known, known_refdes, known_part)) = dialects.first()
                && *known != dialect
            {
                diagnostics.push(model_error(
                    component,
                    &format!(
                        "SPICE dialect conflict: `{}` uses {}, `{known_refdes}` \
                             (`{known_part}`) uses {}; all parts in one run must agree",
                        component.refdes,
                        dialect_name(dialect),
                        dialect_name(*known)
                    ),
                    true,
                    design_path,
                ));
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
                diagnostics.push(model_error(
                    component,
                    &format!("a two-terminal model needs 2 pins, part has {pin_count}"),
                    true,
                    design_path,
                ));
                unmodelled.push(component.refdes.clone());
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
        if let Some(previous) = seen_elements.insert(key, component.refdes.clone())
            && previous != component.refdes
        {
            diagnostics.push(tb_error(format!(
                "references `{previous}` and `{}` both emit as `{name}`",
                component.refdes
            )));
            continue;
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
        modelled.push(component.refdes.clone());
    }

    let mut measure_lines = Vec::new();
    let mut tran_data: Vec<(String, String)> = Vec::new();
    let mut tran_files: HashMap<String, String> = HashMap::new();
    let mut current_saves = BTreeSet::new();
    for (index, measure) in testbench.measure.iter().enumerate() {
        match measure.kind {
            MeasureKind::NodeVoltage | MeasureKind::RelativeDb => {
                let node = match measure_net(
                    design,
                    &net_by_name,
                    ground.id,
                    &ignored_ids,
                    &sourced_nets,
                    measure,
                    testbench_path,
                    design_path,
                ) {
                    Ok(node) => node,
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        continue;
                    }
                };
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
                } else if let Some(at_frequency) = measure.at_frequency {
                    measure_lines.push(format!(
                        "meas ac synth_gain{index} find vdb({node}) at={}\n\
                         echo SYNTH_MEAS {} $&synth_gain{index}",
                        exponent(at_frequency),
                        measure.name
                    ));
                } else {
                    measure_lines.push(format!(
                        "let m{index} = v({node})\necho SYNTH_MEAS {} $&m{index}",
                        measure.name
                    ));
                }
            }
            MeasureKind::Current => {
                match element_current_vector(design, &ignored_ids, measure, testbench_path) {
                    Ok(vector) => {
                        current_saves.insert(vector.clone());
                        measure_lines.push(format!(
                            "let m{index} = {vector}\necho SYNTH_MEAS {} $&m{index}",
                            measure.name
                        ));
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
            }
            MeasureKind::PinCurrent => {
                let pin_ref = measure
                    .pin
                    .as_deref()
                    .expect("a validated pin_current measure has a pin");
                let Some(limit) = pin_limits.get(pin_ref) else {
                    diagnostics.push(tb_error_measure(
                        testbench_path,
                        measure,
                        format!(
                            "measure `{}` pin `{pin_ref}` has no `[[source]]` with that pin",
                            measure.name
                        ),
                    ));
                    continue;
                };
                let vector = format!("@{}[i]", limit.element);
                current_saves.insert(vector.clone());
                // High sources read negative when sourcing; low sources read
                // positive when sinking. Flip high so positive always means
                // the pin is working in its declared direction.
                let signed = match limit.level {
                    SourceLevel::High => format!("-{vector}"),
                    SourceLevel::Low => vector,
                };
                measure_lines.push(format!(
                    "let m{index} = {signed} * 1000\necho SYNTH_MEAS {} $&m{index}",
                    measure.name
                ));
            }
            kind if kind.is_transient() => {
                // The waveform goes out through `wrdata` and is measured in
                // Rust, so settling and edge behaviour do not depend on what
                // ngspice's `.meas` happens to support.
                let file = format!("meas_{index}.dat");
                if measure.node.is_some() {
                    match measure_net(
                        design,
                        &net_by_name,
                        ground.id,
                        &ignored_ids,
                        &sourced_nets,
                        measure,
                        testbench_path,
                        design_path,
                    ) {
                        Ok(node) => tran_data.push((file.clone(), format!("v({node})"))),
                        Err(diagnostic) => {
                            diagnostics.push(diagnostic);
                            continue;
                        }
                    }
                } else {
                    match element_current_vector(design, &ignored_ids, measure, testbench_path) {
                        Ok(vector) => {
                            current_saves.insert(vector.clone());
                            // `@R1[i]` is not a `wrdata` operand on its own;
                            // a control variable holding it is.
                            let name = format!("icur{index}");
                            measure_lines.push(format!("let {name} = {vector}"));
                            tran_data.push((file.clone(), name));
                        }
                        Err(diagnostic) => {
                            diagnostics.push(diagnostic);
                            continue;
                        }
                    }
                }
                tran_files.insert(measure.name.clone(), file);
            }
            _ => unreachable!("handled by the arms above"),
        }
    }

    let coverage = SimCoverage {
        modelled: sorted_unique(modelled),
        ignored: sorted_unique(testbench.ignore.clone()),
        unmodelled: sorted_unique(unmodelled),
        measured: measured_targets(testbench),
    };

    if !diagnostics.is_empty() {
        return Err(EmitFailure {
            diagnostics,
            coverage,
        });
    }

    let mut deck = String::new();
    let title = design.name.replace(['\n', '\r'], " ");
    let _ = writeln!(deck, "* Synth simulation: {title}");
    for line in source_lines {
        let _ = writeln!(deck, "{line}");
    }
    for text in model_text {
        let _ = writeln!(deck, "{}", text.trim_end());
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
    } else if let Some(tran) = &testbench.analysis.tran {
        let _ = writeln!(deck, "tran {} {}", exponent(tran.step), exponent(tran.stop));
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
    for (file, vector) in &tran_data {
        // ngspice 42 rejects double quotes here; single quotes work.
        let _ = writeln!(deck, "wrdata '{file}' {vector}");
    }
    if let Some(plot) = &plot_path {
        // ngspice 42 rejects double quotes here; single quotes work.
        let _ = writeln!(deck, "wrdata '{}' allv", plot.display());
    }
    deck.push_str("echo SYNTH_DONE\n.endc\n.end\n");
    let dialect = dialects.first().and_then(|(dialect, _, _)| *dialect);
    Ok(EmittedDeck {
        deck,
        dialect,
        plot: plot_path,
        pin_limits,
        tran_files,
        coverage,
    })
}

/// Resolve `[options] plot` inside the sidecar's directory, or the working
/// directory when no sidecar path is known.
///
/// The value must be a relative path of plain components: an absolute path or
/// a `..` would let a sidecar write or create directories anywhere. A symlink
/// already on the path is followed before the check, so it cannot lead out of
/// the base either.
fn resolve_plot_path(plot: &str, testbench_path: Option<&Path>) -> Result<PathBuf, Diagnostic> {
    let relative = Path::new(plot);
    if relative.is_absolute()
        || relative.components().any(|part| {
            !matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        return Err(testbench_error(format!(
            "`[options] plot` path `{plot}` must be relative and stay inside the sidecar's \
             directory (no absolute path, no `..`)"
        )));
    }
    let base = testbench_path
        .and_then(Path::parent)
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let unresolved = |error: std::io::Error| {
        testbench_error(format!(
            "could not resolve `[options] plot` path `{plot}`: {error}"
        ))
    };
    let base = base.canonicalize().map_err(unresolved)?;
    let target = base.join(relative);
    // The nearest part of the path that already exists decides where it
    // really lands; everything below it is created later, by plain names.
    if let Some(existing) = target
        .ancestors()
        .find(|ancestor| std::fs::symlink_metadata(ancestor).is_ok())
    {
        let real = existing.canonicalize().map_err(unresolved)?;
        if !real.starts_with(&base) {
            return Err(testbench_error(format!(
                "`[options] plot` path `{plot}` resolves outside the sidecar's directory"
            )));
        }
    }
    Ok(target)
}

/// The ngspice `PULSE(V1 V2 TD TR TF PW PER)` parameters: low, high, zero
/// delay, rise, fall, pulse width and period. Every number goes out through
/// [`exponent`], so SPICE's milli-`M` trap cannot bite.
fn pulse_spec(low: f64, high: f64, rise: f64, fall: f64, duty: f64, period: f64) -> String {
    let pulse_width = duty * period;
    format!(
        "PULSE({} {} 0 {} {} {} {})",
        exponent(low),
        exponent(high),
        exponent(rise),
        exponent(fall),
        exponent(pulse_width),
        exponent(period)
    )
}

/// Resolve the net a voltage measure reads, with the not-found, ground and
/// no-simulated-parts checks shared by `node_voltage` and the transient
/// node measures.
#[allow(clippy::too_many_arguments)]
fn measure_net(
    design: &Board,
    net_by_name: &HashMap<&str, &Net>,
    ground: NetId,
    ignored_ids: &HashSet<ComponentId>,
    sourced_nets: &HashSet<NetId>,
    measure: &Measure,
    testbench_path: Option<&Path>,
    design_path: Option<&Path>,
) -> Result<String, Diagnostic> {
    let node_name = measure
        .node
        .as_deref()
        .expect("a validated node measure has a node");
    let Some(net) = net_by_name.get(node_name) else {
        return Err(tb_error_measure(
            testbench_path,
            measure,
            format!(
                "measure `{}` node `{node_name}` not found in design",
                measure.name
            ),
        ));
    };
    if net.id == ground {
        return Err(tb_error_measure(
            testbench_path,
            measure,
            format!(
                "measure `{}` reads ground net `{}`; ground is 0 V by definition and \
                 ngspice has no vector for it",
                measure.name, net.name
            ),
        ));
    }
    let simulated = net
        .endpoints
        .iter()
        .any(|endpoint| !ignored_ids.contains(&endpoint.component));
    if !simulated && !sourced_nets.contains(&net.id) {
        return Err(with_location_span(
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
    }
    Ok(format!("n_{}", sanitize(&net.name)))
}

/// Resolve the ngspice device-current vector for a `current` measure or a
/// transient element measure. An ignored, unknown or unmodelled part has no
/// device current to read.
fn element_current_vector(
    design: &Board,
    ignored_ids: &HashSet<ComponentId>,
    measure: &Measure,
    testbench_path: Option<&Path>,
) -> Result<String, Diagnostic> {
    let refdes = measure
        .element
        .as_deref()
        .expect("a validated element measure has an element");
    let Some(component) = design
        .components
        .iter()
        .find(|component| component.refdes == refdes)
    else {
        return Err(tb_error_measure(
            testbench_path,
            measure,
            format!(
                "measure `{}` element `{refdes}` is not a placed part",
                measure.name
            ),
        ));
    };
    if ignored_ids.contains(&component.id) {
        return Err(tb_error_measure(
            testbench_path,
            measure,
            format!(
                "measure `{}` element `{refdes}` is ignored by the testbench and cannot \
                 be measured",
                measure.name
            ),
        ));
    }
    let model = model_for(component).map_err(|_| {
        tb_error_measure(
            testbench_path,
            measure,
            format!(
                "measure `{}` element `{refdes}` has no built-in model; a current measure \
                 is not supported there",
                measure.name
            ),
        )
    })?;
    let (letter, parameter) = match &model {
        Model::Value { letter, .. } => (*letter, "i"),
        Model::Diode { .. } => ("D", "id"),
        Model::Subckt => {
            return Err(tb_error_measure(
                testbench_path,
                measure,
                format!(
                    "measure `{}` element `{refdes}` is a registry subcircuit; a current \
                     measure is not supported there",
                    measure.name
                ),
            ));
        }
    };
    Ok(format!("@{}{}[{parameter}]", letter, sanitize(refdes)))
}

/// The dialect's TOML spelling; a part without one is its own (native) mode.
fn dialect_name(dialect: Option<SpiceDialect>) -> String {
    dialect.map_or_else(
        || "no `dialect`".to_string(),
        |dialect| format!("`{}`", dialect.name()),
    )
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
/// Returns the checked text, which is what the deck must carry: ngspice never
/// re-opens the path, so the file cannot change after the check.
fn resolve_spice_model(spice: &SpiceModel, roots: &[PathBuf]) -> Result<(PathBuf, String), String> {
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
        let text = check_model_file(&canonical, spice)?;
        return Ok((canonical, text));
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
            let text = check_model_file(&canonical, spice)?;
            return Ok((canonical, text));
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
fn check_model_file(path: &Path, spice: &SpiceModel) -> Result<String, String> {
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
    // Allowlist, not denylist: anything ngspice could act on that is not
    // named here (`.control`, `.include`, `.lib`, `.end`, `.csparam`, ...) is
    // rejected, so a new directive is refused by default.
    for line in text.lines() {
        let trimmed = strip_inline_comment(line).trim_start();
        // Blank, comment, and `+` continuation lines extend a line already
        // checked.
        if trimmed.is_empty() || trimmed.starts_with(['*', '+']) {
            continue;
        }
        let first = trimmed.split_whitespace().next().unwrap_or_default();
        let lower = first.to_ascii_lowercase();
        let allowed = if lower.starts_with('.') {
            matches!(lower.as_str(), ".subckt" | ".ends" | ".model" | ".param")
        } else {
            // A device/element line: its first letter names the element type.
            lower.starts_with(|c: char| c.is_ascii_alphabetic())
        };
        if !allowed {
            return Err(format!(
                "model file `{}` contains `{first}`, which is not allowed in a model \
                 (only `.subckt`, `.ends`, `.model`, `.param`, comments and element lines)",
                path.display()
            ));
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
    Ok(text)
}

/// Strip a SPICE inline comment: from `;` anywhere, or from `$` that follows
/// whitespace or starts the line.
fn strip_inline_comment(line: &str) -> &str {
    let mut after_whitespace = true;
    for (index, character) in line.char_indices() {
        if character == ';' || (character == '$' && after_whitespace) {
            return &line[..index];
        }
        after_whitespace = character.is_whitespace();
    }
    line
}

/// Count the terminal names on a `.subckt` declaration. Terminals end at an
/// explicit `params:` or the first `name=value` parameter, and a `+` line
/// continues the declaration. `None` means the subcircuit is not defined.
fn subckt_terminal_count(text: &str, subckt: &str) -> Option<usize> {
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = strip_inline_comment(line).trim_start();
        if trimmed.is_empty() || trimmed.starts_with('*') {
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
            let continuation = strip_inline_comment(continuation).trim_start();
            if continuation.is_empty() || continuation.starts_with('*') {
                continue;
            }
            let Some(rest) = continuation.strip_prefix('+') else {
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
    ground: NetId,
    ignored_ids: &HashSet<ComponentId>,
    pin_net: &HashMap<(ComponentId, PinId), NetId>,
    sourced_nets: &HashSet<NetId>,
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

    // A resolved source is a DC path from its net to ground, whether it was
    // written as a net source or a pin drive.
    for &net in sourced_nets {
        *connections.entry(net).or_default() += 1;
        *connections.entry(ground).or_default() += 1;
        if net != ground {
            adjacency.entry(net).or_default().push(ground);
            adjacency.entry(ground).or_default().push(net);
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

/// The forward current the built-in diode model draws when `supply` volts drive
/// `resistance` into a diode specified by its registry forward point.
///
/// Solves `I*R + Vf0 + N*Vt*ln(I/If0) = Vs` by bisection. The fixed-point form
/// of this equation diverges when the resistor is tiny; sharing the lookup
/// keeps `suggest` and the emitted model on the same constants.
pub(crate) fn diode_forward_current(
    supply: f64,
    resistance: f64,
    forward_voltage: f64,
    forward_current_ma: f64,
) -> Option<f64> {
    let forward_current = forward_current_ma / 1000.0;
    if !(supply.is_finite()
        && supply > 0.0
        && resistance.is_finite()
        && resistance > 0.0
        && forward_voltage.is_finite()
        && forward_voltage > 0.0
        && forward_current.is_finite()
        && forward_current > 0.0)
    {
        return None;
    }
    let thermal = DIODE_EMISSION_COEFFICIENT * THERMAL_VOLTAGE_27C;
    // The diode voltage rises with the log of the current, so the equation is
    // monotone: the root lies in 0 .. Vs/R, where Vs/R is the short-circuit
    // current and the diode drop is positive.
    let mut low = 0.0;
    let mut high = supply / resistance;
    for _ in 0..100 {
        let mid = f64::midpoint(low, high);
        if mid * resistance + forward_voltage + thermal * (mid / forward_current).ln() > supply {
            high = mid;
        } else {
            low = mid;
        }
    }
    let current = f64::midpoint(low, high);
    (current.is_finite() && current > 0.0).then_some(current)
}

/// Why a part has no built-in SPICE model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelError {
    /// The kind, pins or registry data cannot make a model. Ignoring the part
    /// is a valid testbench fix.
    NoModelForKind(String),
    /// The part has a value that cannot be used. Ignoring it would hide the
    /// typo, so this stays a hard error.
    BadValue(String),
}

impl ModelError {
    pub(crate) fn message(&self) -> &str {
        match self {
            Self::NoModelForKind(message) | Self::BadValue(message) => message,
        }
    }

    pub(crate) fn is_missing_model(&self) -> bool {
        matches!(self, Self::NoModelForKind(_))
    }
}

fn model_for(component: &Component) -> Result<Model, ModelError> {
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
            return Err(ModelError::NoModelForKind(format!(
                "part kind `{other}` has no built-in SPICE model"
            )));
        }
    };
    let raw = component
        .value
        .as_deref()
        .ok_or_else(|| ModelError::BadValue("part has no value".to_string()))?;
    let value = parse(raw).ok_or_else(|| {
        let hint = if raw.to_ascii_lowercase().contains("meg") {
            "; Synth uses `M` for mega (`1M`), not `meg`"
        } else {
            ""
        };
        ModelError::BadValue(format!("value `{raw}` is not a valid {letter} value{hint}"))
    })?;
    if !value.is_finite() || value <= 0.0 {
        return Err(ModelError::BadValue(format!(
            "value `{raw}` is not a positive finite number"
        )));
    }
    Ok(Model::Value { letter, value })
}

fn diode_model(component: &Component) -> Result<Model, ModelError> {
    let part = component.part.as_ref().ok_or_else(|| {
        ModelError::NoModelForKind("diode part is not in the registry".to_string())
    })?;
    let pins = match (
        part.pins
            .iter()
            .position(|pin| pin.name.eq_ignore_ascii_case("anode")),
        part.pins
            .iter()
            .position(|pin| pin.name.eq_ignore_ascii_case("cathode")),
    ) {
        (Some(anode), Some(cathode)) => [anode as u32, cathode as u32],
        _ => {
            return Err(ModelError::NoModelForKind(
                "diode model needs pins named `anode` and `cathode`".to_string(),
            ));
        }
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
            ModelError::NoModelForKind(
                "diode part needs `[operating_conditions] forward_voltage_v` and \
                 `forward_current_ma`, or a registry `[spice]` model"
                    .to_string(),
            )
        })?;
    if !forward_voltage.is_finite()
        || forward_voltage <= 0.0
        || !forward_current.is_finite()
        || forward_current <= 0.0
    {
        return Err(ModelError::BadValue(format!(
            "forward voltage {forward_voltage} V and current {forward_current} mA \
             must be positive and finite"
        )));
    }
    let saturation_current = forward_current
        / 1000.0
        / ((forward_voltage / (DIODE_EMISSION_COEFFICIENT * THERMAL_VOLTAGE_27C)).exp() - 1.0);
    if !saturation_current.is_finite() || saturation_current <= 0.0 {
        return Err(ModelError::BadValue(format!(
            "forward voltage {forward_voltage} V at {forward_current} mA \
             does not give a usable diode model"
        )));
    }
    Ok(Model::Diode {
        saturation_current,
        pins,
    })
}

fn no_model_error(
    component: &Component,
    error: &ModelError,
    design_path: Option<&Path>,
) -> Diagnostic {
    model_error(
        component,
        error.message(),
        error.is_missing_model(),
        design_path,
    )
}

/// A pin source cannot be driven because the registry has no usable
/// `[pins.drive]` data. It is `-002` because the fix is registry data, not a
/// malformed testbench, and a missing value is never defaulted.
fn pin_drive_error(
    component: &Component,
    pin_ref: &str,
    message: &str,
    design_path: Option<&Path>,
) -> Diagnostic {
    with_location_span(
        sim_builder(E_SYNTH_SIM_002, Severity::Error, "pin has no drive data")
            .message(format!(
                "{} (`{}`): pin source `{pin_ref}`: {message}",
                component.refdes,
                component
                    .part
                    .as_ref()
                    .map_or(component.kind.as_str(), |part| part.id.as_str())
            ))
            .primary_entity(EntityRef::Component {
                id: component.refdes.clone(),
            })
            .build(),
        design_path,
        component.source_span,
    )
}

fn model_error(
    component: &Component,
    message: &str,
    ignorable: bool,
    design_path: Option<&Path>,
) -> Diagnostic {
    let part = component
        .part
        .as_ref()
        .map_or(component.kind.as_str(), |part| part.id.as_str());
    let mut message = format!("{} (`{part}`): {message}", component.refdes);
    if ignorable {
        message.push_str("; add it to `ignore` in the testbench or give it a `[spice]` model");
    }
    with_location_span(
        sim_builder(E_SYNTH_SIM_002, Severity::Error, "part has no SPICE model")
            .message(message)
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

fn tb_error_measure(
    path: Option<&Path>,
    measure: &Measure,
    message: impl Into<String>,
) -> Diagnostic {
    with_location_span(testbench_error(message), path, measure.span)
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

#[cfg(test)]
mod time_weighted_tests {
    use super::time_weighted;

    /// A 0.8 s low region sampled densely, then a 0.2 s high region sampled
    /// twice. ngspice's adaptive timepoints can look like this around a
    /// switching edge; a sample mean would call the signal almost zero.
    fn densely_sampled_low_region() -> (Vec<f64>, Vec<f64>) {
        let mut times: Vec<f64> = (0..=80).map(|step| f64::from(step) * 0.01).collect();
        let mut values = vec![0.0; times.len()];
        times.extend([0.800_000_001, 1.0]);
        values.extend([1.0, 1.0]);
        (times, values)
    }

    #[test]
    fn an_adaptive_timepoint_average_weights_by_time_not_by_sample() {
        let (times, values) = densely_sampled_low_region();
        let sample_mean =
            values.iter().sum::<f64>() / f64::from(u32::try_from(values.len()).unwrap_or(u32::MAX));
        let weighted = time_weighted(&times, &values, |v| v);
        assert!((weighted - 0.2).abs() < 1e-6, "{weighted}");
        assert!(
            sample_mean < 0.05,
            "the dense low region dominates a plain sample mean: {sample_mean}"
        );
    }

    #[test]
    fn an_adaptive_timepoint_rms_integrates_the_square_over_time() {
        let (times, values) = densely_sampled_low_region();
        let rms = time_weighted(&times, &values, |v| v * v).max(0.0).sqrt();
        assert!((rms - 0.2_f64.sqrt()).abs() < 1e-6, "{rms}");
    }
}
