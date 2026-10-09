// SPDX-License-Identifier: Apache-2.0

//! What an external router is asked to do, and under which limits.
//!
//! The request is deliberately explicit about every limit rather than
//! inheriting ambient defaults from the environment. An external router is
//! an unbounded, non-deterministic third party: without a stated wall-clock
//! budget and output cap, one hung JVM becomes a hung release pipeline.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::failure::RouterFailure;

/// Which external engine generates the copper.
///
/// FreeRouting is the default because it is the bundled, managed path.
/// KiCadRoutingTools must be selected explicitly because it is an external
/// checkout with its own Python environment. There is no third variant and
/// no built-in fallback: if the selected engine cannot run, the run fails
/// (see [`crate::RouterFailure`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RouterEngine {
    /// FreeRouting, driven through the managed JAR + Java runtime.
    #[default]
    Freerouting,
    /// KiCadRoutingTools, driven from an external checkout.
    #[serde(rename = "kicad-routing-tools", alias = "kicadroutingtools")]
    KiCadRoutingTools,
}

impl RouterEngine {
    /// Stable kebab-case identifier used in report fields and CLI values.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Freerouting => "freerouting",
            Self::KiCadRoutingTools => "kicad-routing-tools",
        }
    }

    /// Parse the identifier produced by [`RouterEngine::as_str`].
    ///
    /// Returns `None` for anything else rather than defaulting, so a typo
    /// in configuration is an error the caller must handle instead of
    /// quietly routing with a different engine than the one requested.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "freerouting" | "free-routing" => Some(Self::Freerouting),
            "kicad-routing-tools" | "kicadroutingtools" | "krt" => Some(Self::KiCadRoutingTools),
            _ => None,
        }
    }

    /// File-name fragment identifying artifacts this engine produced.
    ///
    /// Keeping the router name in the artifact name is what makes "which
    /// engine made this copper" answerable from the filesystem alone,
    /// without reading a report.
    #[must_use]
    pub fn artifact_tag(self) -> &'static str {
        match self {
            Self::Freerouting => "freerouting",
            Self::KiCadRoutingTools => "kicadroutingtools",
        }
    }

    /// All engines, in the order capability discovery should report them.
    #[must_use]
    pub fn all() -> [Self; 2] {
        [Self::Freerouting, Self::KiCadRoutingTools]
    }
}

impl std::fmt::Display for RouterEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Hard bounds applied to every external run.
///
/// The defaults are chosen so an unattended release pipeline terminates
/// rather than hanging: 20 minutes of router work, then a deterministic
/// failure with the baseline intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterLimits {
    /// Wall-clock budget for the whole engine invocation, including
    /// SES import and zone refill.
    pub wall_clock: Duration,
    /// Maximum bytes captured from each of stdout and stderr.
    ///
    /// Output beyond the cap is dropped, not truncated into the report:
    /// a run that printed megabytes of progress is a signal, and keeping
    /// the first `n` bytes would misrepresent it.
    pub max_output_bytes: usize,
    /// Maximum optimization passes handed to the engine.
    pub max_passes: u32,
    /// Maximum engine worker threads.
    pub max_threads: u32,
    /// Maximum bytes read when parsing a router report.
    pub max_report_bytes: u64,
}

impl Default for RouterLimits {
    fn default() -> Self {
        Self {
            wall_clock: Duration::from_secs(20 * 60),
            max_output_bytes: 1024 * 1024,
            max_passes: 40,
            max_threads: 4,
            max_report_bytes: 64 * 1024 * 1024,
        }
    }
}

impl RouterLimits {
    /// Apply a wall-clock override, ignoring non-positive values.
    ///
    /// A zero or negative budget would kill every run instantly, so it is
    /// treated as "no override given" rather than as a request to fail.
    #[must_use]
    pub fn with_wall_clock(mut self, secs: u64) -> Self {
        if secs > 0 {
            self.wall_clock = Duration::from_secs(secs);
        }
        self
    }

    /// Apply a pass-count override, ignoring zero.
    #[must_use]
    pub fn with_passes(mut self, passes: u32) -> Self {
        if passes > 0 {
            self.max_passes = passes;
        }
        self
    }

    /// Apply a thread-count override, ignoring zero.
    #[must_use]
    pub fn with_threads(mut self, threads: u32) -> Self {
        if threads > 0 {
            self.max_threads = threads;
        }
        self
    }
}

/// FreeRouting-specific knobs.
///
/// These are the only options that reach the FreeRouting adapter. Keeping
/// them in a per-engine struct is what stops a KiCadRoutingTools flag from
/// leaking into the FreeRouting contract, where it would be silently
/// ignored — or worse, forwarded to the JAR and change its behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeroutingOptions {
    /// Route from a freshly written netlist rather than from whatever
    /// copper the input board already carried.
    ///
    /// This is the difference between a real route and a plausible-looking
    /// no-op: without it, stale Synth copper survives the round trip and
    /// is indistinguishable from copper the engine produced.
    pub clean_netlist: bool,
    /// Retain the `.ses` file and engine log next to the routed board.
    pub retain_session: bool,
}

impl Default for FreeroutingOptions {
    fn default() -> Self {
        Self {
            clean_netlist: true,
            retain_session: true,
        }
    }
}

/// How far KiCadRoutingTools may relax rules to reach the fab floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KrtEscalation {
    /// Never relax; fail instead.
    Off,
    /// Preserve the board's own declared minimums.
    #[default]
    Board,
    /// May fall back to the fabrication capability floor.
    Fab,
}

impl KrtEscalation {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Board => "board",
            Self::Fab => "fab",
        }
    }
}

/// KiCadRoutingTools-specific knobs.
///
/// Recorded as provenance rather than inferred: which floor and which
/// escalation a board was routed against changes what the resulting copper
/// is allowed to claim, so it has to be reproducible from the run record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KiCadRoutingToolsOptions {
    pub escalation: KrtEscalation,
    /// Fabrication capability tier: `standard`, `advanced`, or `auto`.
    pub fab_tier: String,
    /// Optional operator-supplied fab-floor override file, applied when
    /// `escalation` permits falling back to the fabrication floor.
    #[serde(default)]
    pub fab_overrides: Option<PathBuf>,
    /// Minimum same-net pad clearance for a KRT via, in millimetres.
    pub same_net_pad_clearance_mm: f64,
    /// Enforce the board's declared minimum track/via sizes strictly.
    pub strict_sizes: bool,
}

impl Default for KiCadRoutingToolsOptions {
    fn default() -> Self {
        Self {
            escalation: KrtEscalation::Board,
            fab_tier: "auto".to_string(),
            fab_overrides: None,
            same_net_pad_clearance_mm: 0.1,
            strict_sizes: true,
        }
    }
}

/// Fabrication policy decisions that are not derivable from the board.
///
/// Via-in-pad is the one that matters: it is manufacturable on some
/// processes and a solder-bridging defect on others, so it is an explicit
/// approval from the fabrication profile rather than something a router may
/// decide for itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FabricationPolicy {
    /// Permit vias inside SMD pads. Defaults to false: a board that has not
    /// been explicitly approved for via-in-pad is not one.
    pub allow_via_in_pad: bool,
    /// Minimum acceptable copper width, in nanometres.
    pub min_track_width_nm: i64,
    /// Minimum acceptable copper clearance between distinct nets, in nm.
    pub min_clearance_nm: i64,
    /// Minimum acceptable drill diameter, in nanometres.
    pub min_drill_diameter_nm: i64,
    /// Minimum annular ring — copper radius minus drill radius — in nm.
    ///
    /// Carried so the escape check can size a via from the process rather
    /// than from an assumption; it is a profile rule, not a board one.
    pub min_annular_ring_nm: i64,
}

impl Default for FabricationPolicy {
    fn default() -> Self {
        // JLCPCB's standard hobbyist tier, matching `ManufacturerProfile`.
        Self {
            allow_via_in_pad: false,
            min_track_width_nm: 127_000,
            min_clearance_nm: 127_000,
            min_drill_diameter_nm: 300_000,
            min_annular_ring_nm: 130_000,
        }
    }
}

impl FabricationPolicy {
    /// The routing floor a named manufacturer profile imposes.
    ///
    /// The board's declared manufacturer is a *process* choice, and the
    /// minimums that follow from it are what the independent gate and the
    /// escape analysis must enforce. Using a built-in default for every board
    /// meant a design that legitimately declares 0.2 mm vias was judged
    /// against a floor it never agreed to.
    #[must_use]
    pub fn from_profile(profile: &synth_drc::ManufacturerProfile) -> Self {
        Self {
            allow_via_in_pad: false,
            min_track_width_nm: profile.min_trace_width_nm,
            min_clearance_nm: profile.min_copper_clearance_nm,
            min_drill_diameter_nm: profile.min_drill_diameter_nm,
            min_annular_ring_nm: profile.min_annular_ring_nm,
        }
    }

    /// [`Self::from_profile`] for a profile name (`jlcpcb`, `pcbway`, ...).
    ///
    /// An unrecognised name resolves to the JLC standard tier, which is what
    /// `ManufacturerProfile::from_name` does, so an unspecified manufacturer
    /// and an unknown one behave the same and neither silently widens the
    /// floor.
    #[must_use]
    pub fn from_profile_name(name: &str) -> Self {
        Self::from_profile(&synth_drc::ManufacturerProfile::from_name(name))
    }
}

impl RouteRequest {
    /// Adopt the fabrication floor a manufacturer profile imposes.
    ///
    /// Keeps whatever via-in-pad decision was already made: that is a process
    /// approval, not a profile rule, so it is not reset here.
    pub fn use_profile_floor(&mut self, manufacturer: &str) {
        let allow_via_in_pad = self.policy.allow_via_in_pad;
        self.policy = FabricationPolicy::from_profile_name(manufacturer);
        self.policy.allow_via_in_pad = allow_via_in_pad;
        self.profile_name = manufacturer.to_string();
    }
}

/// One routing request: a board on disk, an engine, and the bounds to run
/// it under.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteRequest {
    /// The un-routed KiCad PCB produced by Synth's export boundary.
    pub board_path: PathBuf,
    /// Directory the routed candidate, reports, and logs are written to.
    pub out_dir: PathBuf,
    /// File-name stem, normally the design name.
    pub stem: String,
    pub engine: RouterEngine,
    pub limits: RouterLimits,
    pub freerouting: FreeroutingOptions,
    pub kicad_routing_tools: KiCadRoutingToolsOptions,
    /// Fabrication policy applied during independent validation.
    pub policy: FabricationPolicy,
    /// Name of the manufacturing profile the board was exported against,
    /// recorded in provenance.
    pub profile_name: String,
    /// SHA-256 of the exported board, recorded as the router input hash.
    pub input_hash: String,
    /// Source revision the board was compiled from, when known.
    pub source_revision: Option<String>,
}

impl RouteRequest {
    /// The Synth baseline: the un-routed export, preserved verbatim.
    ///
    /// Written before the engine starts and never overwritten afterwards.
    /// On every failure path the baseline is still the delivered board,
    /// which is the property that makes a failed run recoverable rather
    /// than destructive.
    #[must_use]
    pub fn baseline_path(&self) -> PathBuf {
        self.out_dir.join(format!("{}.synth.kicad_pcb", self.stem))
    }

    /// The routed candidate the engine writes, before validation.
    #[must_use]
    pub fn candidate_path(&self) -> PathBuf {
        self.out_dir.join(format!(
            "{}.{}.kicad_pcb",
            self.stem,
            self.engine.artifact_tag()
        ))
    }

    /// The router's own run record, when it produces one.
    #[must_use]
    pub fn router_report_path(&self) -> PathBuf {
        self.out_dir
            .join(format!("{}.{}.json", self.stem, self.engine.artifact_tag()))
    }

    /// FreeRouting session log.
    #[must_use]
    pub fn session_log_path(&self) -> PathBuf {
        self.out_dir
            .join(format!("{}.{}.log", self.stem, self.engine.artifact_tag()))
    }

    /// FreeRouting's Specctra session result.
    ///
    /// Kept separate from [`Self::session_log_path`] deliberately: that path
    /// holds the engine's console output, and writing the SES there too meant
    /// one overwrote the other and the actual route was lost.
    #[must_use]
    pub fn ses_path(&self) -> PathBuf {
        self.out_dir
            .join(format!("{}.{}.ses", self.stem, self.engine.artifact_tag()))
    }

    /// KiCadRoutingTools JSON summary.
    #[must_use]
    pub fn krt_stats_path(&self) -> PathBuf {
        self.out_dir.join(format!("{}.krt-stats.json", self.stem))
    }

    /// Synth's own normalized run record — the machine-readable answer to
    /// "which router, which version, which settings, what did it produce".
    #[must_use]
    pub fn report_path(&self) -> PathBuf {
        self.out_dir.join(format!("{}.routing.json", self.stem))
    }

    /// Independent connectivity findings.
    #[must_use]
    pub fn connectivity_report_path(&self) -> PathBuf {
        self.out_dir
            .join(format!("{}.connectivity.json", self.stem))
    }

    /// `kicad-cli pcb drc` JSON report.
    #[must_use]
    pub fn drc_report_path(&self) -> PathBuf {
        self.out_dir.join(format!("{}.drc.json", self.stem))
    }

    /// Scratch directory for intermediate engine artifacts.
    #[must_use]
    pub fn work_dir(&self) -> PathBuf {
        self.out_dir
            .join(format!("{}.{}.work", self.stem, self.engine.artifact_tag()))
    }

    /// The project file that belongs to the preserved baseline.
    ///
    /// A `.kicad_pcb` does not carry its own design rules or net classes;
    /// they live in the sibling `.kicad_pro`, which tools find by replacing
    /// the board's extension. The baseline is named `<stem>.synth.kicad_pcb`
    /// while the exported project is `<stem>.kicad_pro`, so without a copy
    /// under this name the router looks for `<stem>.synth.kicad_pro`, finds
    /// nothing, and routes against its own defaults.
    #[must_use]
    pub fn baseline_project_path(&self) -> PathBuf {
        self.out_dir.join(format!("{}.synth.kicad_pro", self.stem))
    }

    /// Copy the un-routed export to the baseline path.
    ///
    /// Fails loudly rather than continuing: routing a board whose baseline
    /// was not preserved would leave a failed run with nothing to fall
    /// back to, which is the one outcome the baseline exists to prevent.
    ///
    /// The sibling project is copied too, and that one is best-effort: a
    /// board exported without a project still routes, just against the
    /// engine's defaults, whereas a missing baseline is unrecoverable.
    pub fn preserve_baseline(&self) -> Result<(), RouterFailure> {
        if self.board_path == self.baseline_path() {
            return Ok(());
        }
        // A portfolio runs each engine in its own subdirectory, so the
        // baseline's parent may not exist yet. Copying into a missing
        // directory fails, and the failure would be reported as the engine
        // crashing rather than as the setup it is.
        if let Some(parent) = self.baseline_path().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::copy(&self.board_path, self.baseline_path())
            .map(|_| ())
            .map_err(|e| {
                RouterFailure::io(
                    self.engine,
                    format!(
                        "could not preserve the un-routed export at {}: {e}",
                        self.baseline_path().display()
                    ),
                )
            })?;
        let exported_project = self.board_path.with_extension("kicad_pro");
        if exported_project.is_file() {
            let _ = std::fs::copy(exported_project, self.baseline_project_path());
        }
        Ok(())
    }

    /// The board a router should read: the preserved baseline, not the
    /// delivery path.
    ///
    /// Routing in place over the delivery path is what makes a failed run
    /// destructive; every adapter is handed this instead.
    #[must_use]
    pub fn router_input_path(&self) -> PathBuf {
        self.baseline_path()
    }
}

/// Convenience for tests and callers that only have a path in hand.
impl RouteRequest {
    /// Build a request with default limits and options.
    #[must_use]
    pub fn new(board_path: &Path, out_dir: &Path, engine: RouterEngine) -> Self {
        Self {
            board_path: board_path.to_path_buf(),
            out_dir: out_dir.to_path_buf(),
            stem: board_path
                .file_stem()
                .map_or_else(|| "board".to_string(), |s| s.to_string_lossy().into_owned()),
            engine,
            limits: RouterLimits::default(),
            freerouting: FreeroutingOptions::default(),
            kicad_routing_tools: KiCadRoutingToolsOptions::default(),
            policy: FabricationPolicy::default(),
            profile_name: "unspecified".to_string(),
            input_hash: String::new(),
            source_revision: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_round_trips_through_its_identifier() {
        for engine in RouterEngine::all() {
            assert_eq!(RouterEngine::parse(engine.as_str()), Some(engine));
        }
    }

    #[test]
    fn engine_parses_its_documented_aliases() {
        assert_eq!(
            RouterEngine::parse("KRT"),
            Some(RouterEngine::KiCadRoutingTools)
        );
        assert_eq!(
            RouterEngine::parse(" freerouting "),
            Some(RouterEngine::Freerouting)
        );
    }

    #[test]
    fn an_unknown_engine_name_is_rejected_rather_than_defaulted() {
        // Defaulting here would route with an engine the caller did not
        // ask for, which is exactly the silent substitution the contract
        // exists to prevent.
        assert_eq!(RouterEngine::parse("ngspice"), None);
        assert_eq!(RouterEngine::parse(""), None);
    }

    #[test]
    fn freerouting_is_the_default_engine() {
        assert_eq!(RouterEngine::default(), RouterEngine::Freerouting);
    }

    #[test]
    fn artifact_names_are_stem_prefixed_and_router_tagged() {
        let request = RouteRequest::new(
            Path::new("/tmp/demo.kicad_pcb"),
            Path::new("/tmp/out"),
            RouterEngine::KiCadRoutingTools,
        );
        assert_eq!(
            request.baseline_path(),
            Path::new("/tmp/out/demo.synth.kicad_pcb")
        );
        assert_eq!(
            request.candidate_path(),
            Path::new("/tmp/out/demo.kicadroutingtools.kicad_pcb")
        );
        assert_eq!(
            request.report_path(),
            Path::new("/tmp/out/demo.routing.json")
        );
    }

    #[test]
    fn the_router_never_reads_the_delivery_path() {
        // Routing in place is what makes a failure destructive. Every
        // adapter reads the baseline; this pins that the two paths are
        // genuinely distinct so the invariant is checkable.
        let request = RouteRequest::new(
            Path::new("/tmp/out/demo.kicad_pcb"),
            Path::new("/tmp/out"),
            RouterEngine::Freerouting,
        );
        assert_ne!(request.router_input_path(), request.board_path);
        assert_eq!(request.router_input_path(), request.baseline_path());
    }

    #[test]
    fn a_non_positive_override_is_ignored_rather_than_instantly_failing() {
        let limits = RouterLimits::default()
            .with_wall_clock(0)
            .with_passes(0)
            .with_threads(0);
        assert_eq!(limits, RouterLimits::default());

        let limits = RouterLimits::default()
            .with_wall_clock(30)
            .with_passes(2)
            .with_threads(8);
        assert_eq!(limits.wall_clock, Duration::from_secs(30));
        assert_eq!(limits.max_passes, 2);
        assert_eq!(limits.max_threads, 8);
    }

    #[test]
    fn engine_serialises_to_the_same_identifier_it_parses() {
        for engine in RouterEngine::all() {
            let json = serde_json::to_string(&engine).expect("serialise");
            assert_eq!(json, format!("\"{}\"", engine.as_str()));
            assert_eq!(
                serde_json::from_str::<RouterEngine>(&json).expect("deserialise"),
                engine
            );
        }
    }

    #[test]
    fn via_in_pad_requires_explicit_approval() {
        assert!(!FabricationPolicy::default().allow_via_in_pad);
    }
}
