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
    Freerouting,
    /// KiCadRoutingTools, driven from an external checkout. The default
    /// engine: it is the one `scripts/setup-routing-engines.sh` installs.
    #[default]
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
    ///
    /// The default engine leads, so a caller reading the first entry gets the
    /// engine an unconfigured run would use.
    #[must_use]
    pub fn all() -> [Self; 2] {
        [Self::KiCadRoutingTools, Self::Freerouting]
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

/// One package to escape before the general router sees the board, and how.
///
/// Carried by reference designator, which is what the engine takes and what
/// the design wrote. A request here is not a claim that the package needs an
/// escape — a design may ask for one on a part the geometry would not have
/// selected, and that is the point of being able to say so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscapeRequest {
    /// Reference designator of the package to escape.
    pub refdes: String,
    /// Engine the design asked for, as the engine spells it. `None` leaves
    /// the choice to the adapter's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Copper layers the escape may route onto. Empty leaves it to the
    /// engine, which distributes across whatever the board has.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<String>,
}

/// Fabrication policy decisions that are not derivable from the board.
///
/// Via-in-pad is the one that matters, and it is two facts rather than one:
/// whether the process can fill and cap a via inside an SMD pad, and whether
/// this board is permitted to use one. Conflating them is how a board ends up
/// approved for a process that would bridge it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FabricationPolicy {
    /// Permit vias inside SMD pads. Defaults to false: a board that has not
    /// been explicitly approved for via-in-pad is not one.
    ///
    /// The resolved permission, not the capability. It is granted either by
    /// [`Self::grant_via_in_pad`], when the board declares it needs via-in-pad
    /// and the process offers it, or by an explicit operator override, which
    /// outranks the profile because a human has taken responsibility for the
    /// panel.
    pub allow_via_in_pad: bool,
    /// Whether the declared process fills and caps a via inside an SMD pad.
    ///
    /// Read from the manufacturer profile, so it describes the fab rather than
    /// the design. Absent from policies serialised before the capability
    /// existed, and absence reads as "not offered".
    #[serde(default)]
    pub process_fills_via_in_pad: bool,
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
            process_fills_via_in_pad: false,
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
            process_fills_via_in_pad: profile.via_in_pad,
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

    /// Grant via-in-pad because the board declared it needs one.
    ///
    /// Returns whether the *process* offers the capability, not whether the
    /// permission ended up set. A board cannot approve its own fabrication:
    /// asking for via-in-pad on a tier that does not fill and cap it grants
    /// nothing, and the caller turns that `false` into a diagnostic naming
    /// the escape arithmetic.
    ///
    /// Adds to the permission rather than replacing it. An operator who
    /// passed `--krt-allow-via-in-pad` has already set it, and
    /// [`RouteRequest::use_profile_floor`] deliberately carries that across a change
    /// of profile; assigning here would revoke it precisely when the tier
    /// cannot fill, which is the case the override exists for. The returned
    /// capability stays a statement about the process either way, so an
    /// overridden run is still reported as unmet.
    pub fn grant_via_in_pad(&mut self) -> bool {
        let offered = self.process_fills_via_in_pad;
        self.allow_via_in_pad |= offered;
        offered
    }
}

impl RouteRequest {
    /// Grant the fabrication processes the board declares it needs.
    ///
    /// Returns the requirements the declared process does not offer, so the
    /// caller can say so. Lives here rather than in a caller because there
    /// are three — the CLI and both MCP routing entry points — and a
    /// requirement honoured in only one of them is a permission the other two
    /// silently withhold.
    ///
    /// Call after [`Self::use_profile_floor`]: the capability comes from the
    /// profile, so granting against a stale floor would answer about the
    /// wrong process.
    pub fn use_board_process_requirements(
        &mut self,
        board: &synth_ir::Board,
    ) -> Vec<synth_ir::ProcessRequirement> {
        let mut unmet = Vec::new();
        for requirement in &board.required_processes {
            let granted = match requirement {
                synth_ir::ProcessRequirement::ViaInPad => self.policy.grant_via_in_pad(),
            };
            if !granted {
                unmet.push(*requirement);
            }
        }
        unmet
    }

    /// Take the per-package escape intent the board declared.
    ///
    /// Lives here for the reason [`Self::use_board_process_requirements`]
    /// does: there are four routing entry points, and an intent honoured by
    /// one of them is a declaration the other three ignore.
    pub fn use_board_escapes(&mut self, board: &synth_ir::Board) {
        self.escapes.clear();
        self.escapes_off.clear();
        for component in &board.components {
            let Some(intent) = &component.escape else {
                continue;
            };
            if intent.off {
                self.escapes_off.push(component.refdes.clone());
                continue;
            }
            self.escapes.push(EscapeRequest {
                refdes: component.refdes.clone(),
                method: intent.method.map(|m| m.as_source().to_string()),
                layers: intent.layers.clone(),
            });
        }
    }

    /// Adopt the fabrication floor a manufacturer profile imposes.
    ///
    /// Keeps whatever via-in-pad permission was already granted: an operator
    /// override is a promise about the panel, not a profile rule, so it is not
    /// reset here. The *capability* is rebuilt from the new profile, because
    /// that one does describe the process being named.
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
    /// Packages the design asked to have escaped, and how. Empty means the
    /// design said nothing, and the dense grid arrays are escaped anyway.
    #[serde(default)]
    pub escapes: Vec<EscapeRequest>,
    /// Packages the design asked to leave unescaped (`escape { off }`),
    /// which overrides geometric selection.
    #[serde(default)]
    pub escapes_off: Vec<String>,
    /// Name of the manufacturing profile the board was exported against,
    /// recorded in provenance.
    pub profile_name: String,
    /// SHA-256 of the exported board, recorded as the router input hash.
    pub input_hash: String,
    /// Source revision the board was compiled from, when known.
    pub source_revision: Option<String>,
    /// Nets to reserve routes for first, hardest first.
    ///
    /// Retry advice from a prior run on this board: the run that failed on
    /// these nets recorded them, and this run starts from that order. Carried
    /// on the request rather than only printed, so a retry actually starts
    /// from it. Empty when there is no advice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routing_order: Vec<String>,
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
    /// The sibling project is copied too, and a failure there is reported
    /// rather than swallowed. Whether a project *exists* is the optional
    /// part: a board exported without one still routes, against the engine's
    /// defaults. A project that exists and cannot be copied is the opposite
    /// case — the router then reads no project and silently falls back to
    /// those same defaults, which is the behaviour this copy exists to
    /// prevent, and it would do so while reporting success.
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
            std::fs::copy(&exported_project, self.baseline_project_path())
                .map(|_| ())
                .map_err(|e| {
                    RouterFailure::io(
                        self.engine,
                        format!(
                            "could not preserve the exported project at {}: {e}. \
                             The router reads the project beside the board it is \
                             given, so without this copy it would route against \
                             the engine's own defaults instead of the design \
                             rules the export wrote.",
                            self.baseline_project_path().display()
                        ),
                    )
                })?;
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
            escapes: Vec::new(),
            escapes_off: Vec::new(),
            policy: FabricationPolicy::default(),
            profile_name: "unspecified".to_string(),
            input_hash: String::new(),
            source_revision: None,
            routing_order: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory holding an exported board and, optionally, its
    /// sibling project.
    fn exported(label: &str, with_project: bool) -> (RouteRequest, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "synth-preserve-{}-{label}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let board = dir.join("demo.kicad_pcb");
        std::fs::write(&board, "(kicad_pcb)").expect("write board");
        if with_project {
            std::fs::write(dir.join("demo.kicad_pro"), "{}").expect("write project");
        }
        let mut request = RouteRequest::new(&board, &dir, RouterEngine::KiCadRoutingTools);
        request.stem = "demo".to_string();
        (request, dir)
    }

    /// The ordinary path: both files land beside each other under the
    /// baseline name the router looks for.
    #[test]
    fn the_exported_project_is_preserved_beside_the_baseline() {
        let (request, dir) = exported("ok", true);
        request.preserve_baseline().expect("preserve");
        assert!(request.baseline_path().is_file(), "baseline board");
        assert!(
            request.baseline_project_path().is_file(),
            "the project the router reads has to be beside the board it reads"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A board exported without a project still routes. Absence is the
    /// optional part; it is a *failed copy* that is not.
    #[test]
    fn a_board_with_no_project_is_preserved_without_one() {
        let (request, dir) = exported("none", false);
        request.preserve_baseline().expect("preserve");
        assert!(request.baseline_path().is_file());
        assert!(!request.baseline_project_path().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The case this guards. A project that exists and cannot be copied used
    /// to be discarded with `let _ =`, so the run continued, the router found
    /// no project beside the board, and it fell back to the engine's own
    /// defaults — the exact behaviour the copy exists to prevent, reported as
    /// success.
    ///
    /// The destination is made a directory, which `fs::copy` cannot write
    /// over on any platform.
    #[test]
    fn a_project_that_cannot_be_copied_fails_the_run() {
        let (request, dir) = exported("blocked", true);
        std::fs::create_dir_all(request.baseline_project_path()).expect("occupy the destination");
        let failure = request
            .preserve_baseline()
            .expect_err("a project that cannot be copied must not report success");
        let text = failure.to_string();
        assert!(
            text.contains("could not preserve the exported project"),
            "the failure must say which copy failed: {text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A board with nothing on it, for the policy questions that turn only
    /// on what it declares.
    fn empty_board() -> synth_ir::Board {
        synth_ir::Board {
            name: "t".to_string(),
            layers: 4,
            manufacturer: None,
            revision: None,
            company: None,
            schematic_paper: None,
            schematic_overflow: None,
            components: Vec::new(),
            nets: Vec::new(),
            diff_pairs: Vec::new(),
            notes: Vec::new(),
            legends: false,
            keepouts: Vec::new(),
            stackup: None,
            netclasses: Vec::new(),
            buses: Vec::new(),
            modules: Vec::new(),
            groups: Vec::new(),
            variants: Vec::new(),
            required_processes: std::collections::BTreeSet::new(),
            source_span: synth_diagnostics::Span::new(0, 0),
        }
    }

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
    fn kicad_routing_tools_is_the_default_engine() {
        assert_eq!(RouterEngine::default(), RouterEngine::KiCadRoutingTools);
        // Discovery leads with the default, so the first entry an operator
        // sees is the engine an unconfigured run would use.
        assert_eq!(RouterEngine::all()[0], RouterEngine::default());
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

    /// A tier that sells the fill carries the capability, and still grants no
    /// permission: the design has not asked for one. Reading the capability as
    /// the permission would hand via-in-pad to every board exported against an
    /// advanced tier, including the ones that never needed it.
    #[test]
    fn a_capable_process_grants_no_permission_by_itself() {
        let policy = FabricationPolicy::from_profile_name("jlc-filled-via");

        assert!(policy.process_fills_via_in_pad);
        assert!(!policy.allow_via_in_pad);
    }

    #[test]
    fn a_standard_process_carries_no_capability() {
        let policy = FabricationPolicy::from_profile_name("jlcpcb");

        assert!(!policy.process_fills_via_in_pad);
        assert!(!policy.allow_via_in_pad);
    }

    /// The gate: a board's declaration grants a permission only where the
    /// process backs it.
    #[test]
    fn granting_via_in_pad_needs_the_process_to_offer_it() {
        let mut capable = FabricationPolicy::from_profile_name("jlc-filled-via");
        assert!(capable.grant_via_in_pad());
        assert!(capable.allow_via_in_pad);

        let mut incapable = FabricationPolicy::from_profile_name("jlcpcb");
        assert!(!incapable.grant_via_in_pad());
        assert!(
            !incapable.allow_via_in_pad,
            "a board must not approve its own fabrication"
        );
    }

    /// Switching profiles re-reads the capability from the tier being named,
    /// so a board cannot keep a process property it no longer declares.
    #[test]
    fn the_profile_floor_rebuilds_the_capability() {
        let mut request = RouteRequest::new(
            std::path::Path::new("board.kicad_pcb"),
            std::path::Path::new("out"),
            RouterEngine::KiCadRoutingTools,
        );

        request.use_profile_floor("jlc-filled-via");
        assert!(request.policy.process_fills_via_in_pad);

        request.use_profile_floor("jlcpcb");
        assert!(!request.policy.process_fills_via_in_pad);
    }

    /// The board's declaration reaches the router, which is what stops the
    /// gate from reporting via-in-pad as blocking on a board that asked for
    /// it and a process that sells it.
    #[test]
    fn a_declared_requirement_grants_the_permission_on_a_capable_tier() {
        let mut request = RouteRequest::new(
            std::path::Path::new("board.kicad_pcb"),
            std::path::Path::new("out"),
            RouterEngine::KiCadRoutingTools,
        );
        let mut board = empty_board();
        board
            .required_processes
            .insert(synth_ir::ProcessRequirement::ViaInPad);

        request.use_profile_floor("jlc-filled-via");
        let unmet = request.use_board_process_requirements(&board);

        assert!(unmet.is_empty());
        assert!(request.policy.allow_via_in_pad);
    }

    #[test]
    fn a_declared_requirement_a_tier_refuses_is_reported_not_granted() {
        let mut request = RouteRequest::new(
            std::path::Path::new("board.kicad_pcb"),
            std::path::Path::new("out"),
            RouterEngine::KiCadRoutingTools,
        );
        let mut board = empty_board();
        board
            .required_processes
            .insert(synth_ir::ProcessRequirement::ViaInPad);

        request.use_profile_floor("jlcpcb");
        let unmet = request.use_board_process_requirements(&board);

        assert_eq!(unmet, vec![synth_ir::ProcessRequirement::ViaInPad]);
        assert!(!request.policy.allow_via_in_pad);
    }

    /// The override is a promise about the panel, so a board declaration must
    /// not take it away. Assigning the capability here revoked it exactly when
    /// the tier cannot fill, which is the case `--krt-allow-via-in-pad` exists
    /// for: the run would then be refused for the one reason the operator had
    /// already answered.
    #[test]
    fn a_declaration_does_not_revoke_an_operator_override() {
        let mut request = RouteRequest::new(
            std::path::Path::new("board.kicad_pcb"),
            std::path::Path::new("out"),
            RouterEngine::KiCadRoutingTools,
        );
        let mut board = empty_board();
        board
            .required_processes
            .insert(synth_ir::ProcessRequirement::ViaInPad);

        // The operator's flag, applied before the floor as the CLI applies it.
        request.policy.allow_via_in_pad = true;
        request.use_profile_floor("jlcpcb");
        let unmet = request.use_board_process_requirements(&board);

        assert!(
            request.policy.allow_via_in_pad,
            "the override must survive a board declaring the same process"
        );
        assert_eq!(
            unmet,
            vec![synth_ir::ProcessRequirement::ViaInPad],
            "and the tier still does not offer it, which is what gets reported"
        );
    }

    /// An operator override is a promise about the panel rather than a claim
    /// about the tier, so it survives a profile swap. The capability does not,
    /// which is what keeps the two facts distinguishable afterwards.
    #[test]
    fn an_operator_override_survives_the_profile_floor() {
        let mut request = RouteRequest::new(
            std::path::Path::new("board.kicad_pcb"),
            std::path::Path::new("out"),
            RouterEngine::KiCadRoutingTools,
        );

        request.policy.allow_via_in_pad = true;
        request.use_profile_floor("jlcpcb");

        assert!(request.policy.allow_via_in_pad);
        assert!(!request.policy.process_fills_via_in_pad);
    }
}
