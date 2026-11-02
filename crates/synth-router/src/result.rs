// SPDX-License-Identifier: Apache-2.0

//! What a routing run produced, in the shape a consumer can act on.
//!
//! The central type is [`RouteReport`]: one JSON document per run that
//! names the engine, pins its version, records the input and output
//! hashes, lists the retained artifacts, and ends in exactly one
//! [`RouteState`]. Every layer — CLI, MCP, renderer, SaaS — reports from
//! this same type, so they cannot disagree about what happened.
//!
//! The four terminal states are deliberately not collapsed:
//!
//! - [`RouteState::Routed`] — copper present, independently validated,
//!   fabrication-ready. The only state that permits manufacturing output.
//! - [`RouteState::ReviewRequired`] — copper exists, nothing blocking was
//!   found, but a required check could not be performed. An unavailable
//!   DRC is not a clean DRC.
//! - [`RouteState::RouterUnavailable`] — the selected engine could not run.
//! - [`RouteState::ValidationFailed`] — the engine produced a board that
//!   independent checks rejected.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::capability::RouterCapability;
use crate::contract::{RouteRequest, RouterEngine};
use crate::failure::{RouterFailure, RouterFailureReason, RouterStage};
use crate::validate::FabricationVerdict;

/// Terminal state of a routing run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteState {
    /// Independently validated, fabrication-ready copper. The only state
    /// that unlocks manufacturing artifacts.
    Routed,
    /// Copper exists but a required check could not be performed, so the
    /// board is retained for human review rather than rejected outright.
    ReviewRequired,
    /// The selected engine is not installed or not runnable.
    RouterUnavailable,
    /// The engine produced a board that independent validation rejected.
    ValidationFailed,
}

impl RouteState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Routed => "routed",
            Self::ReviewRequired => "review_required",
            Self::RouterUnavailable => "router_unavailable",
            Self::ValidationFailed => "validation_failed",
        }
    }

    /// Whether this state may produce manufacturing artifacts.
    ///
    /// Fail-closed by construction: only [`RouteState::Routed`] returns
    /// `true`, and every other state — including "we could not check" —
    /// returns `false`.
    #[must_use]
    pub fn is_fabrication_ready(self) -> bool {
        matches!(self, Self::Routed)
    }

    /// Parse the identifier produced by [`RouteState::as_str`].
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "routed" => Some(Self::Routed),
            "review_required" => Some(Self::ReviewRequired),
            "router_unavailable" => Some(Self::RouterUnavailable),
            "validation_failed" => Some(Self::ValidationFailed),
            _ => None,
        }
    }
}

impl std::fmt::Display for RouteState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Copper geometry the router reported, normalized to one shape.
///
/// Deliberately normalized rather than engine-specific: FreeRouting
/// reports in Specctra units and KiCadRoutingTools in millimetres, and a
/// consumer comparing two runs should not have to know which engine
/// produced which file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteStatistics {
    /// Nets with every pad connected to a common copper network.
    pub connected_nets: usize,
    /// Nets still open. Any non-zero value blocks fabrication.
    pub open_nets: usize,
    /// Names of the open nets, for diagnosis.
    pub open_net_names: Vec<String>,
    /// Track segments present on the candidate board.
    pub segments: usize,
    /// Vias present on the candidate board.
    pub vias: usize,
    /// Total copper length in millimetres.
    pub wire_length_mm: f64,
    /// Pads the engine could not escape from.
    #[serde(default)]
    pub pad_escape_rejections: usize,
    /// Engine-specific findings preserved verbatim.
    #[serde(default)]
    pub engine_findings: BTreeMap<String, String>,
}

impl RouteStatistics {
    /// Statistics for a run that produced no board.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Which router ran, which version, with what settings, on what input.
///
/// Everything a reader needs to decide whether two runs are comparable.
/// In particular the version pin: FreeRouting's output changes between
/// releases, so a route without a version is not reproducible.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RouterProvenance {
    pub engine: String,
    /// Engine version, from the JAR name or the checkout commit.
    pub engine_version: Option<String>,
    /// Runtime that executed the engine (Java / Python version line).
    pub runtime_version: Option<String>,
    /// Manufacturing profile the board was exported against.
    pub profile: String,
    /// Exact command line, for reproducing or diagnosing the run.
    #[serde(default)]
    pub command: Vec<String>,
    /// Settings actually applied, after clamping to the run's limits.
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
    /// SHA-256 of the un-routed board handed to the router.
    pub input_hash: String,
    /// SHA-256 of the routed candidate.
    #[serde(default)]
    pub output_hash: String,
    /// Source revision the board was compiled from, when known.
    #[serde(default)]
    pub source_revision: Option<String>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
}

/// Whether a retained artifact is present on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactPresence {
    /// Written and readable.
    #[default]
    Present,
    /// Not produced by this run.
    Missing,
}

/// Paths of everything a run retained.
///
/// Deliberately exhaustive even on failure: the baseline, the router's
/// own session log, and the DRC/connectivity reports are what a human
/// needs to find out what went wrong, and a run record that only lists
/// artifacts when everything succeeded throws that away.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteArtifacts {
    /// The un-routed Synth export. Never overwritten by a failed run.
    pub baseline: PathBuf,
    /// The router's output, whether or not it was installed.
    #[serde(default)]
    pub candidate: Option<PathBuf>,
    /// The board as delivered: the installed candidate when validated,
    /// otherwise the untouched baseline.
    #[serde(default)]
    pub delivered: Option<PathBuf>,
    /// The router's raw session log.
    #[serde(default)]
    pub session_log: Option<PathBuf>,
    /// FreeRouting's Specctra session result, when that engine produced one.
    #[serde(default)]
    pub ses: Option<PathBuf>,
    /// The engine's own JSON report (KiCadRoutingTools route summary).
    #[serde(default)]
    pub router_report: Option<PathBuf>,
    /// Independently derived connectivity findings.
    #[serde(default)]
    pub connectivity_report: Option<PathBuf>,
    /// `kicad-cli pcb drc` JSON report.
    #[serde(default)]
    pub drc_report: Option<PathBuf>,
}

impl RouteArtifacts {
    /// Record which declared paths actually exist.
    ///
    /// A report that claims an artifact exists when it does not is worse
    /// than one that omits it, so presence is read from the filesystem
    /// rather than assumed from what the run intended to write.
    #[must_use]
    pub fn resolved(mut self) -> Self {
        self.baseline = resolve(&self.baseline);
        self.candidate = self
            .candidate
            .map(|p| resolve(&p))
            .filter(|p| !p.as_os_str().is_empty());
        self.delivered = self
            .delivered
            .map(|p| resolve(&p))
            .filter(|p| !p.as_os_str().is_empty());
        self.ses = self.ses.as_ref().filter(|p| p.is_file()).cloned();
        self.session_log = self
            .session_log
            .map(|p| resolve(&p))
            .filter(|p| !p.as_os_str().is_empty());
        self.router_report = self
            .router_report
            .map(|p| resolve(&p))
            .filter(|p| !p.as_os_str().is_empty());
        self.connectivity_report = self
            .connectivity_report
            .map(|p| resolve(&p))
            .filter(|p| !p.as_os_str().is_empty());
        self.drc_report = self
            .drc_report
            .map(|p| resolve(&p))
            .filter(|p| !p.as_os_str().is_empty());
        self
    }

    /// Paths that were declared but are not on disk.
    ///
    /// Exposed so a consumer can tell "the router wrote nothing" from "we
    /// never asked for anything".
    #[must_use]
    pub fn missing(&self) -> Vec<PathBuf> {
        let mut missing = Vec::new();
        if !self.baseline.is_file() {
            missing.push(self.baseline.clone());
        }
        for path in [
            &self.candidate,
            &self.delivered,
            &self.session_log,
            &self.router_report,
            &self.connectivity_report,
            &self.drc_report,
        ]
        .into_iter()
        .flatten()
        {
            if !path.is_file() {
                missing.push(path.clone());
            }
        }
        missing
    }
}

/// Keep a path only when it names a readable file.
fn resolve(path: &Path) -> PathBuf {
    if path.is_file() {
        path.to_path_buf()
    } else {
        PathBuf::new()
    }
}

/// Summary of the independent checks applied to a candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationSummary {
    pub fabrication_ready: bool,
    /// Topology comparison against the baseline the router was given.
    pub topology: crate::validate::TopologyReport,
    /// Connectivity re-derived from copper.
    pub connectivity: crate::validate::ConnectivityReport,
    /// `kicad-cli pcb drc` counts. `None` means DRC could not be run,
    /// which is *not* a pass.
    #[serde(default)]
    pub kicad_drc: Option<DrcCounts>,
    /// Reasons the candidate is not fabrication-ready.
    #[serde(default)]
    pub blocking_reasons: Vec<String>,
    /// Required checks that could not be performed.
    #[serde(default)]
    pub unavailable_checks: Vec<String>,
    /// Findings listed for the review that do not hold the board back: a layer
    /// change on a slow net with no room for a ground return via.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advisories: Vec<String>,
}

impl ValidationSummary {
    /// Whether anything blocking was found.
    #[must_use]
    pub fn blocking_count(&self) -> usize {
        self.blocking_reasons.len()
    }
}

/// KiCad DRC counts, in a shape this crate can serialise.
///
/// Mirrors `synth_drc::DrcCounts` rather than embedding it, so the run
/// record stays readable when this crate is consumed without the DRC
/// engine present.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrcCounts {
    pub errors: usize,
    /// Pads KiCad reported as not connected to the rest of their net.
    pub unconnected: usize,
    pub warnings: usize,
}

impl DrcCounts {
    /// Errors plus unconnected items — the findings that block fabrication.
    #[must_use]
    pub fn blocking_count(self) -> usize {
        self.errors + self.unconnected
    }
}

impl From<synth_drc::DrcCounts> for DrcCounts {
    fn from(value: synth_drc::DrcCounts) -> Self {
        Self {
            errors: value.errors,
            unconnected: value.unconnected,
            warnings: value.warnings,
        }
    }
}

impl From<DrcCounts> for synth_drc::DrcCounts {
    fn from(value: DrcCounts) -> Self {
        Self {
            errors: value.errors,
            unconnected: value.unconnected,
            warnings: value.warnings,
        }
    }
}

/// One routing run, as recorded to `<stem>.routing.json`.
///
/// The single source of truth for what happened: the CLI prints from it,
/// the MCP tool returns it, and the release gate reads it. A layer that
/// reported a different router or a different terminal state than this
/// document would be a bug, and the acceptance gate is that none exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteReport {
    pub schema_version: String,
    pub state: RouteState,
    pub engine: RouterEngine,
    #[serde(default)]
    pub capability: Option<RouterCapability>,
    #[serde(default)]
    pub provenance: RouterProvenance,
    #[serde(default)]
    pub statistics: RouteStatistics,
    #[serde(default)]
    pub artifacts: RouteArtifacts,
    #[serde(default)]
    pub validation: Option<ValidationSummary>,
    #[serde(default)]
    pub failure: Option<RouterFailure>,
    /// Dense packages whose pads cannot be escaped at the fabrication floor.
    ///
    /// Computed from the un-routed baseline before the engine runs, so it is a
    /// property of the design and process rather than of the attempt. Empty
    /// when every package can be fanned out. See [`crate::escape`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub escape: Vec<crate::escape::PackageEscape>,
    /// Net names to try first on a retry, hardest first.
    ///
    /// A router that failed on this board is telling us something: these are
    /// the nets it could not complete. Re-running with them first is the
    /// cheapest way to use that, and costs nothing when the run succeeded
    /// (the list is empty). Advisory, exactly like `synth_route`'s
    /// `routing_order`: it changes the search order, never the verdict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommended_routing_order: Vec<String>,
}

impl RouteReport {
    /// A run that never started because the engine is not installed.
    #[must_use]
    pub fn unavailable(
        request: &RouteRequest,
        capability: &Option<RouterCapability>,
        failure: &RouterFailure,
    ) -> Self {
        Self {
            schema_version: crate::ROUTE_REPORT_SCHEMA.to_string(),
            state: RouteState::RouterUnavailable,
            engine: request.engine,
            capability: capability.clone(),
            provenance: RouterProvenance {
                engine: request.engine.as_str().to_string(),
                profile: request.profile_name.clone(),
                input_hash: request.input_hash.clone(),
                source_revision: request.source_revision.clone(),
                ..RouterProvenance::default()
            },
            statistics: RouteStatistics::empty(),
            artifacts: RouteArtifacts {
                baseline: request.baseline_path(),
                ..RouteArtifacts::default()
            }
            .resolved(),
            validation: None,
            escape: Vec::new(),
            recommended_routing_order: Vec::new(),
            failure: Some(failure.clone()),
        }
    }

    /// A run that started and failed.
    #[must_use]
    pub fn failed(
        request: &RouteRequest,
        capability: &Option<RouterCapability>,
        failure: RouterFailure,
    ) -> Self {
        let state = if failure.reason.is_unavailable() {
            RouteState::RouterUnavailable
        } else {
            RouteState::ValidationFailed
        };
        Self {
            schema_version: crate::ROUTE_REPORT_SCHEMA.to_string(),
            state,
            engine: request.engine,
            capability: capability.clone(),
            provenance: RouterProvenance {
                engine: request.engine.as_str().to_string(),
                profile: request.profile_name.clone(),
                input_hash: request.input_hash.clone(),
                source_revision: request.source_revision.clone(),
                ..RouterProvenance::default()
            },
            statistics: RouteStatistics::empty(),
            artifacts: RouteArtifacts {
                baseline: request.baseline_path(),
                ..RouteArtifacts::default()
            }
            .resolved(),
            validation: None,
            escape: Vec::new(),
            recommended_routing_order: Vec::new(),
            failure: Some(failure),
        }
    }

    /// Whether this run may produce manufacturing artifacts.
    #[must_use]
    pub fn is_fabrication_ready(&self) -> bool {
        self.state.is_fabrication_ready()
    }

    /// Ordering key for choosing between engines' attempts on one board.
    ///
    /// Larger is better. The state leads because a validated route beats a
    /// merely-present one however short its copper; the tiebreakers favour
    /// fewer blocking findings, fewer vias, and shorter copper, then the
    /// engine name so the winner does not depend on iteration order.
    #[must_use]
    pub fn portfolio_rank(&self) -> PortfolioRank {
        PortfolioRank {
            state: match self.state {
                RouteState::Routed => 3,
                RouteState::ReviewRequired => 2,
                RouteState::ValidationFailed => 1,
                RouteState::RouterUnavailable => 0,
            },
            produced_a_board: u8::from(
                self.artifacts.delivered.is_some() || self.artifacts.candidate.is_some(),
            ),
            blocking: std::cmp::Reverse(
                self.validation
                    .as_ref()
                    .map_or(usize::MAX, ValidationSummary::blocking_count),
            ),
            vias: std::cmp::Reverse(self.statistics.vias),
            shorter_copper: -(self.statistics.wire_length_mm * 1000.0) as i64,
            engine: self.engine.as_str().to_string(),
        }
    }

    /// Ordering key for choosing among candidate **placements** of one design.
    ///
    /// Lower is better, so a best-of picks the minimum. The state leads so an
    /// unrouted candidate never wins on copper count; then open nets; then
    /// blocking validation findings; then total copper (segments + vias) as a
    /// congestion proxy; finally wire length. All terms are integers except the
    /// length, which is quantised to micrometres so the key is a total order.
    #[must_use]
    #[allow(clippy::cast_sign_loss)] // `max(0.0)` guarantees a non-negative value
    pub fn layout_cost(&self) -> (u8, usize, usize, usize, u64) {
        let state = match self.state {
            RouteState::Routed => 0,
            RouteState::ReviewRequired => 1,
            RouteState::ValidationFailed => 2,
            RouteState::RouterUnavailable => 3,
        };
        let blocking = self
            .validation
            .as_ref()
            .map_or(usize::MAX, ValidationSummary::blocking_count);
        let copper = self.statistics.segments + self.statistics.vias;
        let wire_um = (self.statistics.wire_length_mm.max(0.0) * 1000.0) as u64;
        (state, self.statistics.open_nets, blocking, copper, wire_um)
    }

    /// One-line summary for stderr.
    #[must_use]
    pub fn summary(&self) -> String {
        match &self.failure {
            Some(failure) => format!("{}: {}", self.state, failure.summary()),
            None => format!(
                "{}: {} ({} connected net(s), {} open, {} segment(s), {} via(s))",
                self.state,
                self.engine,
                self.statistics.connected_nets,
                self.statistics.open_nets,
                self.statistics.segments,
                self.statistics.vias
            ),
        }
    }

    /// Promote a verdict into the state this run ended in.
    ///
    /// Split out so the adapter and [`crate::route`] cannot disagree about
    /// which verdict maps to which state. An unperformed check is *not* a
    /// blocking finding: it means nobody knows, which is review, not a
    /// verdict of failure.
    #[must_use]
    pub fn state_for_verdict(verdict: &FabricationVerdict) -> RouteState {
        if verdict.fabrication_ready {
            RouteState::Routed
        } else if verdict.has_blocking_findings() {
            RouteState::ValidationFailed
        } else {
            RouteState::ReviewRequired
        }
    }

    /// Write this report to `path` as pretty JSON.
    ///
    /// # Errors
    /// Propagates any filesystem or serialization error.
    pub fn write_to(&self, path: &Path) -> std::io::Result<()> {
        crate::write_json(path, self)
    }

    /// Read a report written by [`RouteReport::write_to`].
    ///
    /// # Errors
    /// Returns the underlying I/O or JSON error, so a truncated report is
    /// reported as malformed rather than silently read as empty.
    pub fn read_from(path: &Path) -> Result<Self, std::io::Error> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("malformed route report {}: {e}", path.display()),
            )
        })
    }

    /// The failure, if any, expressed as a stable reason.
    #[must_use]
    pub fn failure_reason(&self) -> Option<RouterFailureReason> {
        self.failure.as_ref().map(|f| f.reason)
    }

    /// The stage a failed run stopped at.
    #[must_use]
    pub fn failed_stage(&self) -> Option<RouterStage> {
        self.failure.as_ref().map(|f| f.stage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::RouterEngine;

    fn verdict(ready: bool, blocking: usize) -> FabricationVerdict {
        FabricationVerdict {
            fabrication_ready: ready,
            blocking_reasons: (0..blocking).map(|i| format!("blocking {i}")).collect(),
            ..FabricationVerdict::synthetic()
        }
    }

    #[test]
    fn only_routed_is_fabrication_ready() {
        assert!(RouteState::Routed.is_fabrication_ready());
        // Fail-closed: "we could not check" is not "it passed".
        assert!(!RouteState::ReviewRequired.is_fabrication_ready());
        assert!(!RouteState::RouterUnavailable.is_fabrication_ready());
        assert!(!RouteState::ValidationFailed.is_fabrication_ready());
    }

    #[test]
    fn the_four_states_stay_distinct_in_the_wire_format() {
        let states = [
            RouteState::Routed,
            RouteState::ReviewRequired,
            RouteState::RouterUnavailable,
            RouteState::ValidationFailed,
        ];
        let mut seen = std::collections::BTreeSet::new();
        for state in states {
            assert!(seen.insert(state.as_str()), "duplicate: {state}");
            assert_eq!(RouteState::parse(state.as_str()), Some(state));
        }
        assert_eq!(RouteState::parse("almost_routed"), None);
    }

    #[test]
    fn a_verdict_with_findings_but_no_blockers_is_review_not_failure() {
        assert_eq!(
            RouteReport::state_for_verdict(&verdict(true, 0)),
            RouteState::Routed
        );
        assert_eq!(
            RouteReport::state_for_verdict(&verdict(false, 0)),
            RouteState::ReviewRequired
        );
        assert_eq!(
            RouteReport::state_for_verdict(&verdict(false, 1)),
            RouteState::ValidationFailed
        );
    }

    #[test]
    fn missing_artifacts_are_reported_as_missing_not_claimed() {
        let artifacts = RouteArtifacts {
            baseline: PathBuf::from("/nonexistent/baseline.kicad_pcb"),
            candidate: Some(PathBuf::from("/nonexistent/candidate.kicad_pcb")),
            ..RouteArtifacts::default()
        }
        .resolved();

        assert!(
            artifacts.candidate.is_none(),
            "absent candidate must not be listed"
        );
        assert!(!artifacts.missing().is_empty());
    }

    #[test]
    fn a_present_baseline_is_retained() {
        let dir =
            std::env::temp_dir().join(format!("synth_router_artifacts_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        let baseline = dir.join("demo.synth.kicad_pcb");
        std::fs::write(&baseline, b"(kicad_pcb)").expect("write baseline");

        let artifacts = RouteArtifacts {
            baseline: baseline.clone(),
            ..RouteArtifacts::default()
        }
        .resolved();
        assert_eq!(artifacts.baseline, baseline);
        assert!(artifacts.missing().is_empty(), "{:?}", artifacts.missing());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_report_round_trips_through_json() {
        let report = RouteReport {
            schema_version: crate::ROUTE_REPORT_SCHEMA.to_string(),
            state: RouteState::ReviewRequired,
            engine: RouterEngine::KiCadRoutingTools,
            capability: None,
            provenance: RouterProvenance {
                engine: "kicad-routing-tools".to_string(),
                engine_version: Some("abc1234".to_string()),
                profile: "jlc-standard".to_string(),
                duration_ms: 1234,
                ..RouterProvenance::default()
            },
            statistics: RouteStatistics {
                connected_nets: 12,
                open_nets: 1,
                open_net_names: vec!["net_7".to_string()],
                segments: 88,
                vias: 14,
                wire_length_mm: 91.5,
                ..RouteStatistics::default()
            },
            artifacts: RouteArtifacts::default(),
            validation: None,
            escape: Vec::new(),
            recommended_routing_order: Vec::new(),
            failure: Some(
                RouterFailure::for_stage(
                    RouterEngine::KiCadRoutingTools,
                    RouterStage::Connectivity,
                    RouterFailureReason::ValidationFailed,
                    "net_7 is open",
                )
                .with_exit_code(Some(3)),
            ),
        };

        let text = serde_json::to_string(&report).expect("serialise");
        assert_eq!(
            serde_json::from_str::<RouteReport>(&text).expect("deserialise"),
            report
        );
    }

    #[test]
    fn a_report_discards_paths_that_do_not_exist() {
        // A report claiming an artifact exists when it does not is worse
        // than omitting it, so presence is read from disk.
        let artifacts = RouteArtifacts {
            baseline: PathBuf::from("/definitely/not/here.kicad_pcb"),
            session_log: Some(PathBuf::from("/definitely/not/here.log")),
            ..RouteArtifacts::default()
        }
        .resolved();
        assert!(artifacts.baseline.as_os_str().is_empty());
        assert!(artifacts.session_log.is_none());
    }
}

/// Total order over run reports for portfolio selection; larger is better.
///
/// Named rather than a tuple so the intent of each term survives the next
/// edit; see [`RouteReport::portfolio_rank`] for what each one means.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortfolioRank {
    state: u8,
    produced_a_board: u8,
    blocking: std::cmp::Reverse<usize>,
    vias: std::cmp::Reverse<usize>,
    shorter_copper: i64,
    engine: String,
}

#[cfg(test)]
mod portfolio_tests {
    use super::*;
    use crate::contract::{RouteRequest, RouterEngine};
    use crate::failure::{RouterFailure, RouterFailureReason, RouterStage};

    fn request() -> RouteRequest {
        RouteRequest::new(
            std::path::Path::new("/tmp/x.kicad_pcb"),
            std::path::Path::new("/tmp"),
            RouterEngine::Freerouting,
        )
    }

    #[test]
    fn an_attempt_that_ran_outranks_one_that_could_not_start() {
        // The portfolio must never let a missing engine win over one that
        // produced a board it could at least judge.
        let unavailable = RouteReport::unavailable(
            &request(),
            &None,
            &RouterFailure::for_stage(
                RouterEngine::Freerouting,
                RouterStage::Capability,
                RouterFailureReason::JarMissing,
                "no jar",
            ),
        );
        let crashed = RouteReport::failed(
            &request(),
            &None,
            RouterFailure::for_stage(
                RouterEngine::Freerouting,
                RouterStage::Route,
                RouterFailureReason::Crashed,
                "engine exited 139",
            ),
        );
        assert!(
            crashed.portfolio_rank() > unavailable.portfolio_rank(),
            "a run that started must outrank one that could not"
        );
    }

    fn candidate_report(state: RouteState, open_nets: usize, segments: usize) -> RouteReport {
        RouteReport {
            schema_version: String::new(),
            state,
            engine: RouterEngine::KiCadRoutingTools,
            capability: None,
            provenance: RouterProvenance::default(),
            statistics: RouteStatistics {
                open_nets,
                segments,
                vias: 4,
                wire_length_mm: 100.0,
                ..RouteStatistics::default()
            },
            artifacts: RouteArtifacts::default(),
            validation: None,
            failure: None,
            escape: Vec::new(),
            recommended_routing_order: Vec::new(),
        }
    }

    #[test]
    fn layout_cost_leads_with_state_then_open_nets_then_copper() {
        let routed = candidate_report(RouteState::Routed, 0, 100);
        let review = candidate_report(RouteState::ReviewRequired, 0, 50);
        // A routed board with open nets still ranks ahead of a review-only
        // board: the state leads, so a candidate that finished wins.
        let routed_open = candidate_report(RouteState::Routed, 2, 100);
        assert!(routed.layout_cost() < routed_open.layout_cost());
        assert!(routed_open.layout_cost() < review.layout_cost());
        // Fewer segments is better, all else equal.
        let leaner = candidate_report(RouteState::Routed, 0, 90);
        assert!(leaner.layout_cost() < routed.layout_cost());
    }
}
