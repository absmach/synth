// SPDX-License-Identifier: Apache-2.0

//! The KiCadRoutingTools adapter.
//!
//! KiCadRoutingTools is optional and must be selected explicitly, because
//! it is an external checkout with its own Python environment rather than
//! something the project installs. An absent checkout produces a
//! capability error naming what to point at — never a silent switch to
//! another engine.
//!
//! Its policy knobs (escalation, fab tier, via-in-pad clearance) live in
//! [`KiCadRoutingToolsOptions`] and are forwarded only here. Keeping them
//! out of the shared contract is deliberate: a flag this engine honours and
//! FreeRouting ignores would be worse than no flag at all, because the
//! run record would imply a constraint that was never applied.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::capability::KRT_ENTRYPOINT;
use crate::contract::{KiCadRoutingToolsOptions, RouteRequest, RouterEngine};
use crate::failure::{RouterFailure, RouterFailureReason, RouterStage};
use crate::freerouting::Candidate;
use crate::result::{RouteArtifacts, RouteStatistics, RouterProvenance};

/// The helper script that drives the external `py_router/route.py`.
pub const HELPER: &str = "kicad_routing_tools_route.py";

/// KiCadRoutingTools' own JSON route summary, as written by the engine.
///
/// Fields the engine may omit are optional; the fields Synth relies on
/// for policy (the open-net lists and the via-in-pad count) are also
/// optional, and their absence is treated as "unknown" rather than "none",
/// so a report that does not mention via-in-pad cannot be read as proof
/// that there is none.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub struct KrtReport {
    #[serde(default)]
    pub successful: Option<usize>,
    #[serde(default)]
    pub failed: Option<usize>,
    #[serde(default)]
    pub total_vias: Option<usize>,
    #[serde(default)]
    pub total_time: Option<f64>,
    #[serde(default)]
    pub failed_single: Vec<String>,
    #[serde(default)]
    pub open_single: Vec<String>,
    #[serde(default)]
    pub failed_multipoint: Vec<String>,
    #[serde(default)]
    pub design_rules: DesignRules,
    #[serde(default)]
    pub via_in_pad: ViaInPad,
}

impl KrtReport {
    /// Nets the engine could not connect.
    #[must_use]
    pub fn open_nets(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        names.extend(self.failed_single.iter().cloned());
        names.extend(self.open_single.iter().cloned());
        names.extend(self.failed_multipoint.iter().cloned());
        names.sort();
        names.dedup();
        names
    }

    /// Whether the engine's own policy floor was satisfied.
    ///
    /// Recorded, never trusted: a key the engine reported but did not
    /// deliver is exactly the case a router's self-assessment misses.
    #[must_use]
    pub fn below_floor(&self) -> Vec<String> {
        let mut below = Vec::new();
        for (key, floor) in &self.design_rules.board_floors {
            if let Some(delivered) = self.design_rules.min_delivered.get(key) {
                if *delivered + 1e-9 < *floor {
                    below.push(format!(
                        "{key}: delivered {delivered:.4} mm is below the board floor {floor:.4} mm"
                    ));
                }
            }
        }
        below
    }
}

/// The rule minima KiCadRoutingTools reports on.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub struct DesignRules {
    /// Minimums the board itself declares.
    #[serde(default)]
    pub board_floors: BTreeMap<String, f64>,
    /// Minimums the engine actually delivered.
    #[serde(default)]
    pub min_delivered: BTreeMap<String, f64>,
}

/// The engine's via-in-pad accounting.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub struct ViaInPad {
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub locations: Vec<String>,
}

/// Build the command line for a KiCadRoutingTools run.
///
/// Pure, so the argv — including the fact that no FreeRouting flag ever
/// appears here — is assertable without a checkout.
#[must_use]
pub fn build_command(
    request: &RouteRequest,
    python: &Path,
    helper: &Path,
    repo: &Path,
) -> Vec<String> {
    let options: &KiCadRoutingToolsOptions = &request.kicad_routing_tools;
    let mut args = vec![
        helper.display().to_string(),
        request.router_input_path().display().to_string(),
        request.candidate_path().display().to_string(),
        "--repo".to_string(),
        repo.display().to_string(),
        "--python".to_string(),
        python.display().to_string(),
        "--stats".to_string(),
        request.krt_stats_path().display().to_string(),
        "--escalation".to_string(),
        options.escalation.as_str().to_string(),
        "--fab-tier".to_string(),
        // `auto` lets the engine choose a fabrication tier, and an advanced
        // tier permits a finer drill than the board's declared process. On
        // the Class C board that produced 11 filled-and-capped vias at a
        // 0.2 mm drill against a declared 0.3 mm floor: manufacturable
        // somewhere, just not where this board says it is being built. An
        // explicit operator choice still wins; `auto` now resolves against
        // the floor the board declared instead of against the engine's
        // judgement.
        if options.fab_tier == "auto" {
            if request.policy.min_drill_diameter_nm <= 200_000 {
                "advanced".to_string()
            } else {
                "standard".to_string()
            }
        } else {
            options.fab_tier.clone()
        },
        // Via-in-pad is expressed as a clearance sentinel by the engine,
        // not as its own flag, so the adapter maps the policy decision onto
        // that encoding here rather than leaking a second meaning.
        "--same-net-pad-clearance".to_string(),
        if request.policy.allow_via_in_pad {
            "-1".to_string()
        } else {
            format!("{:.4}", options.same_net_pad_clearance_mm)
        },
    ];
    if let Some(overrides) = &options.fab_overrides {
        args.extend([
            "--fab-overrides".to_string(),
            overrides.display().to_string(),
        ]);
    }
    if options.strict_sizes {
        args.push("--strict-sizes".to_string());
    }
    // Hand every dense grid array to the escape engine before the general
    // router sees the board. Left to the maze router, a 0.8 mm ball grid comes
    // back with a stub on every lane rather than an escape under every pad.
    //
    // Measured on the Class C board: driving this from `escape::analyse`
    // instead — the packages that *cannot* be escaped at the floor — fanned
    // out the two 0.5 mm connectors and skipped the 121-ball BGA, because
    // 0.8 mm clears the 0.687 mm two vias need between centres. Needing an
    // escape and failing one are different questions.
    let candidates = crate::escape::grid_arrays_in_baseline(request);
    if !candidates.is_empty() {
        // Escape with the smallest via the declared process permits, not with
        // the engine's default. Measured without this, the escape placed 11
        // vias under the fabrication minimum drill — the independent gate
        // caught them, but a board that cannot be drilled is not an escape.
        //
        // The floor rather than a netclass value because a ball grid's nets
        // span several classes, and the one thing every escape via on the
        // package must satisfy is the process.
        let drill_nm = request.policy.min_drill_diameter_nm;
        let diameter_nm = drill_nm + 2 * request.policy.min_annular_ring_nm;
        args.extend([
            "--escape-via-drill".to_string(),
            format!("{:.4}", synth_ir::Length(drill_nm).to_mm()),
            "--escape-via-size".to_string(),
            format!("{:.4}", synth_ir::Length(diameter_nm).to_mm()),
        ]);
        for refdes in candidates {
            args.extend(["--fanout-component".to_string(), refdes]);
        }
    }
    args
}

/// Run KiCadRoutingTools over the request's baseline.
#[allow(clippy::too_many_lines)]
pub fn route(request: &RouteRequest) -> Result<Candidate, RouterFailure> {
    let repo = resolve_repo().ok_or_else(|| {
        RouterFailure::for_stage(
            RouterEngine::KiCadRoutingTools,
            RouterStage::Capability,
            RouterFailureReason::CheckoutMissing,
            "no KiCadRoutingTools checkout configured; pass --kicad-routing-tools-repo or \
             set KICAD_ROUTING_TOOLS_REPO",
        )
    })?;
    if !repo.join(KRT_ENTRYPOINT).is_file() {
        return Err(RouterFailure::for_stage(
            RouterEngine::KiCadRoutingTools,
            RouterStage::Capability,
            RouterFailureReason::CheckoutMissing,
            format!(
                "{} is not a KiCadRoutingTools checkout: {KRT_ENTRYPOINT} is missing",
                repo.display()
            ),
        ));
    }
    let python = resolve_python().ok_or_else(|| {
        RouterFailure::for_stage(
            RouterEngine::KiCadRoutingTools,
            RouterStage::Capability,
            RouterFailureReason::PythonMissing,
            "no Python interpreter found; pass --kicad-routing-tools-python",
        )
    })?;
    let helper = crate::repo_tool(HELPER).ok_or_else(|| {
        RouterFailure::for_stage(
            RouterEngine::KiCadRoutingTools,
            RouterStage::Capability,
            RouterFailureReason::NotInstalled,
            format!("{HELPER} was not found in the repository's tools/ directory"),
        )
    })?;

    // The duplicate-run guard is held by `crate::route`, the single
    // supported entry point. Taking it again here would refuse every run.

    let _ = std::fs::remove_file(request.candidate_path());
    if let Some(parent) = request.candidate_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let argv = build_command(request, &python, &helper, &repo);
    let start = std::time::Instant::now();
    let invocation = crate::process::run(
        &python,
        &argv,
        request.limits.wall_clock,
        request.limits.max_output_bytes,
    )
    .map_err(|error| {
        RouterFailure::for_stage(
            RouterEngine::KiCadRoutingTools,
            RouterStage::Route,
            if error.not_found {
                RouterFailureReason::NotInstalled
            } else {
                RouterFailureReason::SpawnFailed
            },
            error.detail,
        )
    })?;
    let duration_ms = start.elapsed().as_millis() as u64;

    let log_path = request.session_log_path();
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&log_path, invocation.combined_output());

    let report = read_report(&request.krt_stats_path());
    let mut settings = BTreeMap::new();
    settings.insert(
        "escalation".to_string(),
        request.kicad_routing_tools.escalation.as_str().to_string(),
    );
    settings.insert(
        "fab_tier".to_string(),
        request.kicad_routing_tools.fab_tier.clone(),
    );
    settings.insert(
        "strict_sizes".to_string(),
        request.kicad_routing_tools.strict_sizes.to_string(),
    );
    settings.insert(
        "allow_via_in_pad".to_string(),
        request.policy.allow_via_in_pad.to_string(),
    );
    if let Some(overrides) = &request.kicad_routing_tools.fab_overrides {
        settings.insert("fab_overrides".to_string(), overrides.display().to_string());
    }
    settings.insert(
        "wall_clock_secs".to_string(),
        request.limits.wall_clock.as_secs().to_string(),
    );

    let provenance = RouterProvenance {
        engine: RouterEngine::KiCadRoutingTools.as_str().to_string(),
        engine_version: Some(crate::capability::checkout_commit(&repo)),
        runtime_version: crate::capability::probe_runtime_version(&python),
        profile: request.profile_name.clone(),
        command: invocation.command.clone(),
        settings,
        input_hash: request.input_hash.clone(),
        output_hash: String::new(),
        source_revision: request.source_revision.clone(),
        duration_ms,
    };

    // A crashed or timed-out run is a hard failure with nothing to keep.
    // A non-zero exit *with* a board is different: the engine ran and
    // declined to call the result policy-compliant, so the copper exists
    // and is kept for review rather than discarded.
    let has_board = request.candidate_path().is_file();
    if !has_board {
        crate::freerouting::check_process_outcome(
            RouterEngine::KiCadRoutingTools,
            HELPER,
            &invocation,
            request,
        )?;
    } else if invocation.timed_out || invocation.cancelled {
        return Err(crate::freerouting::check_process_outcome(
            RouterEngine::KiCadRoutingTools,
            HELPER,
            &invocation,
            request,
        )
        .expect_err("a timeout or cancellation is always a failure"));
    }
    if !has_board {
        return Err(RouterFailure::for_stage(
            RouterEngine::KiCadRoutingTools,
            RouterStage::Import,
            RouterFailureReason::NoOutput,
            format!(
                "KiCadRoutingTools produced no board at {}; the baseline export is \
                 preserved unchanged",
                request.candidate_path().display()
            ),
        )
        .with_exit_code(invocation.exit_code));
    }

    let board = std::fs::read_to_string(request.candidate_path())
        .ok()
        .and_then(|text| crate::pcb_read::PcbBoard::parse(&text).ok());

    let mut candidate = Candidate {
        candidate_path: request.candidate_path(),
        artifacts: RouteArtifacts {
            baseline: request.baseline_path(),
            candidate: Some(request.candidate_path()),
            delivered: Some(request.board_path.clone()),
            session_log: Some(log_path),
            router_report: Some(request.krt_stats_path()),
            ..RouteArtifacts::default()
        }
        .resolved(),
        provenance,
        statistics: statistics_from(board.as_ref(), report.as_ref()),
    };

    // The engine's own policy verdict is recorded and then re-checked
    // independently. When the two disagree, the board is handed to
    // validation anyway and the disagreement is surfaced in the report:
    // the router is not the authority on whether it met the policy.
    if invocation.exit_code != Some(0) {
        candidate
            .statistics
            .engine_findings
            .insert("engine_policy_verdict".to_string(), "rejected".to_string());
    }

    Ok(candidate)
}

/// Both resolvers are shared with capability discovery so the path an engine
/// is *reported* available at is the path it actually runs from.
fn resolve_repo() -> Option<PathBuf> {
    crate::capability::resolve_krt_repo()
}

fn resolve_python() -> Option<PathBuf> {
    crate::capability::resolve_python()
}

fn read_report(path: &Path) -> Option<KrtReport> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Statistics for the candidate.
///
/// Copper counts come from the board that was written. The engine's own
/// counts and open-net list are recorded under `engine_findings`: they are
/// provenance and, when they disagree with the re-derived connectivity,
/// evidence that the board needs a human.
#[must_use]
pub fn statistics_from(
    board: Option<&crate::pcb_read::PcbBoard>,
    report: Option<&KrtReport>,
) -> RouteStatistics {
    let mut statistics = RouteStatistics::default();
    let mut findings = BTreeMap::new();
    if let Some(report) = report {
        let open = report.open_nets();
        statistics.open_net_names.clone_from(&open);
        statistics.open_nets = open.len();
        findings.insert(
            "claimed_successful".to_string(),
            report
                .successful
                .map_or_else(|| "unknown".to_string(), |n| n.to_string()),
        );
        findings.insert(
            "claimed_total_vias".to_string(),
            report
                .total_vias
                .map_or_else(|| "unknown".to_string(), |n| n.to_string()),
        );
        findings.insert(
            "claimed_via_in_pad".to_string(),
            report.via_in_pad.count.to_string(),
        );
        let below = report.below_floor();
        findings.insert(
            "rules_below_board_floor".to_string(),
            if below.is_empty() {
                "none reported".to_string()
            } else {
                below.join("; ")
            },
        );
    }
    if let Some(board) = board {
        statistics.segments = board.segments.len();
        statistics.vias = board.vias.len();
        statistics.wire_length_mm = board.wire_length_mm();
    }
    statistics.engine_findings = findings;
    statistics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{FabricationPolicy, KrtEscalation, RouterEngine, RouterLimits};

    fn request() -> RouteRequest {
        let mut request = RouteRequest::new(
            Path::new("/tmp/out/demo.kicad_pcb"),
            Path::new("/tmp/out"),
            RouterEngine::KiCadRoutingTools,
        );
        request.stem = "demo".to_string();
        request.limits = RouterLimits::default();
        request
    }

    fn argv(request: &RouteRequest) -> String {
        build_command(
            request,
            Path::new("/usr/bin/python3"),
            Path::new("/repo/tools/kicad_routing_tools_route.py"),
            Path::new("/opt/krt"),
        )
        .join(" ")
    }

    #[test]
    fn the_command_carries_the_policy_the_run_record_claims() {
        let request = RouteRequest {
            kicad_routing_tools: KiCadRoutingToolsOptions {
                escalation: KrtEscalation::Fab,
                fab_tier: "advanced".to_string(),
                fab_overrides: Some(PathBuf::from("/tmp/fab-floor.txt")),
                same_net_pad_clearance_mm: 0.25,
                strict_sizes: true,
            },
            ..request()
        };
        let joined = argv(&request);
        assert!(joined.contains("--escalation fab"), "{joined}");
        assert!(joined.contains("--fab-tier advanced"), "{joined}");
        assert!(
            joined.contains("--same-net-pad-clearance 0.2500"),
            "{joined}"
        );
        assert!(joined.contains("--strict-sizes"), "{joined}");
    }

    /// A baseline on disk, because the escape engine reads the board the
    /// engine will be given rather than the request.
    fn with_baseline(pads: &str) -> (RouteRequest, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "synth-krt-escape-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let mut request = RouteRequest::new(
            &dir.join("demo.kicad_pcb"),
            &dir,
            RouterEngine::KiCadRoutingTools,
        );
        request.stem = "demo".to_string();
        request.limits = RouterLimits::default();
        let board = format!(
            "(kicad_pcb\n  (version 20260206)\n  (generator \"synth-eda\")\n  \
             (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal) (44 \"Edge.Cuts\" user))\n  \
             (net 0 \"\")\n  (footprint \"Test:Part\"\n    (layer \"F.Cu\")\n    \
             (at 10 20)\n    (property \"Reference\" \"U1\" (at 0 -1) (layer \"F.SilkS\"))\n\
             {pads}  )\n  (gr_line (start 0 0) (end 40 0) (layer \"Edge.Cuts\") (width 0.1))\n  \
             (gr_line (start 0 0) (end 0 30) (layer \"Edge.Cuts\") (width 0.1))\n  \
             (gr_line (start 40 0) (end 40 30) (layer \"Edge.Cuts\") (width 0.1))\n  \
             (gr_line (start 0 30) (end 40 30) (layer \"Edge.Cuts\") (width 0.1))\n)"
        );
        std::fs::write(request.baseline_path(), board).expect("write baseline");
        (request, dir)
    }

    /// Stepped rather than multiplied by the index, so the helper needs no
    /// `usize as f64` cast and no pedantic allow for one.
    fn grid_pads(rows: usize, cols: usize, pitch: f64) -> String {
        use std::fmt::Write as _;
        let mut pads = String::new();
        let mut n = 1;
        let mut y = 0.0_f64;
        for _ in 0..rows {
            let mut x = 0.0_f64;
            for _ in 0..cols {
                let _ = write!(
                    pads,
                    "    (pad \"{n}\" smd circle (at {x:.4} {y:.4}) (size 0.400 0.400)\n      \
                     (layers \"F.Cu\" \"F.Paste\" \"F.Mask\") (net {n} \"N{n}\"))\n"
                );
                x += pitch;
                n += 1;
            }
            y += pitch;
        }
        pads
    }

    /// The escape engine is driven from the baseline, so the flags only
    /// appear when the board actually carries a grid array. Without this the
    /// maze router returns a 0.8 mm ball grid with a stub on every lane.
    #[test]
    fn a_grid_array_in_the_baseline_is_handed_to_the_escape_engine() {
        let (request, dir) = with_baseline(&grid_pads(11, 11, 0.8));
        let joined = argv(&request);
        std::fs::remove_dir_all(&dir).ok();
        assert!(joined.contains("--fanout-component U1"), "{joined}");
        // The declared floor, not the engine's default: 0.3 mm drill plus
        // two 0.13 mm rings. Escaping with the engine's own via put 11 of
        // them under the fabrication minimum.
        assert!(joined.contains("--escape-via-drill 0.3000"), "{joined}");
        assert!(joined.contains("--escape-via-size 0.5600"), "{joined}");
    }

    /// A board with nothing dense on it must not ask for an escape at all,
    /// or every two-pad design pays for a stage it does not need.
    #[test]
    fn a_board_with_no_grid_array_asks_for_no_escape() {
        let (request, dir) = with_baseline(&grid_pads(1, 2, 2.54));
        let joined = argv(&request);
        std::fs::remove_dir_all(&dir).ok();
        for flag in [
            "--fanout-component",
            "--escape-via-drill",
            "--escape-via-size",
        ] {
            assert!(!joined.contains(flag), "{flag} leaked: {joined}");
        }
    }

    /// `auto` must resolve against the floor the board declared. Letting the
    /// engine choose produced filled-and-capped vias at a 0.2 mm drill on a
    /// board whose declared minimum was 0.3 mm.
    #[test]
    fn auto_resolves_the_fab_tier_against_the_declared_floor() {
        let (mut request, dir) = with_baseline(&grid_pads(1, 2, 2.54));
        request.kicad_routing_tools.fab_tier = "auto".to_string();
        assert!(argv(&request).contains("--fab-tier standard"));

        request.policy.min_drill_diameter_nm = 200_000;
        assert!(argv(&request).contains("--fab-tier advanced"));

        // An explicit operator choice still wins.
        request.kicad_routing_tools.fab_tier = "standard".to_string();
        assert!(argv(&request).contains("--fab-tier standard"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_freerouting_option_leaks_into_this_contract() {
        // A flag one engine honours and another ignores would make the run
        // record claim a constraint that was never applied.
        let joined = argv(&request());
        for leaked in [
            "--passes",
            "--threads",
            "--jar",
            "--java",
            "--clean-netlist",
        ] {
            assert!(!joined.contains(leaked), "{leaked} leaked: {joined}");
        }
    }

    #[test]
    fn no_kicad_routing_tools_option_leaks_into_the_freerouting_contract() {
        // `--repo` is deliberately absent from this list: it is a prefix
        // of `--report`, so a substring check would flag the FreeRouting
        // contract for a flag it does not carry.
        let freerouting = crate::freerouting::build_command(
            &RouteRequest::new(
                Path::new("/tmp/out/demo.kicad_pcb"),
                Path::new("/tmp/out"),
                RouterEngine::Freerouting,
            ),
            Path::new("helper.py"),
            Path::new("jar"),
            Path::new("java"),
        )
        .join(" ");
        for leaked in ["--escalation", "--fab-tier", "--repo", "--strict-sizes"] {
            assert!(
                !freerouting.split_whitespace().any(|token| token == leaked),
                "{leaked} leaked: {freerouting}"
            );
        }
    }

    #[test]
    fn via_in_pad_is_encoded_as_the_engines_clearance_sentinel() {
        let permissive = RouteRequest {
            policy: FabricationPolicy {
                allow_via_in_pad: true,
                ..FabricationPolicy::default()
            },
            ..request()
        };
        assert!(argv(&permissive).contains("--same-net-pad-clearance -1"));

        let strict = request();
        assert!(argv(&strict).contains("--same-net-pad-clearance 0.1000"));
    }

    #[test]
    fn the_engine_is_handed_the_baseline_and_writes_a_tagged_candidate() {
        let args = build_command(
            &request(),
            Path::new("python3"),
            Path::new("helper.py"),
            Path::new("/opt/krt"),
        );
        assert!(args[1].ends_with("demo.synth.kicad_pcb"), "{args:?}");
        assert!(
            args[2].ends_with("demo.kicadroutingtools.kicad_pcb"),
            "{args:?}"
        );
    }

    #[test]
    fn the_engines_open_net_list_is_collected_and_deduplicated() {
        let report = KrtReport {
            failed_single: vec!["net_3".to_string()],
            open_single: vec!["net_1".to_string(), "net_3".to_string()],
            failed_multipoint: vec!["net_9".to_string()],
            ..KrtReport::default()
        };
        assert_eq!(
            report.open_nets(),
            vec![
                "net_1".to_string(),
                "net_3".to_string(),
                "net_9".to_string()
            ]
        );
    }

    #[test]
    fn a_rule_below_the_board_floor_is_named() {
        let report = KrtReport {
            design_rules: DesignRules {
                board_floors: BTreeMap::from([
                    ("trace_width".to_string(), 0.127),
                    ("clearance".to_string(), 0.127),
                ]),
                min_delivered: BTreeMap::from([
                    ("trace_width".to_string(), 0.100),
                    ("clearance".to_string(), 0.200),
                ]),
            },
            ..KrtReport::default()
        };
        let below = report.below_floor();
        assert_eq!(below.len(), 1, "{below:?}");
        assert!(below[0].contains("trace_width"), "{below:?}");
    }

    #[test]
    fn a_rule_the_engine_did_not_report_is_not_counted_as_a_violation() {
        // Absent evidence is not evidence of a violation, and it is also
        // not evidence of compliance — which is why connectivity is
        // re-derived rather than taken from this report.
        let report = KrtReport {
            design_rules: DesignRules {
                board_floors: BTreeMap::from([("trace_width".to_string(), 0.127)]),
                min_delivered: BTreeMap::new(),
            },
            ..KrtReport::default()
        };
        assert!(report.below_floor().is_empty());
    }

    #[test]
    fn statistics_record_the_engines_claim_and_count_the_board() {
        let board =
            crate::pcb_read::PcbBoard::parse(crate::pcb_read::SAMPLE_BOARD).expect("parses");
        let report = KrtReport {
            successful: Some(2),
            total_vias: Some(1),
            open_single: vec!["net_7".to_string()],
            via_in_pad: ViaInPad {
                count: 2,
                locations: vec!["U1.3".to_string()],
            },
            ..KrtReport::default()
        };
        let statistics = statistics_from(Some(&board), Some(&report));
        assert_eq!(statistics.segments, 3);
        assert_eq!(statistics.vias, 1);
        assert_eq!(statistics.open_nets, 1);
        assert_eq!(statistics.open_net_names, vec!["net_7".to_string()]);
        assert_eq!(
            statistics
                .engine_findings
                .get("claimed_via_in_pad")
                .map(String::as_str),
            Some("2")
        );
    }

    #[test]
    fn an_absent_summary_leaves_the_claim_fields_unknown() {
        let statistics = statistics_from(None, None);
        assert!(statistics.engine_findings.is_empty());
        assert_eq!(statistics.open_nets, 0);
        assert_eq!(statistics.open_net_names.len(), 0);
    }
}
