// SPDX-License-Identifier: Apache-2.0

//! A compact, engine-independent statement of how good a routed board is.
//!
//! The run record ([`crate::RouteReport`]) is the audit trail: it carries the
//! command, the provenance, the raw findings and every retained artifact.
//! This module is the answer to the shorter question a caller asks after a
//! route — "is this board good?" — in the terms the operator asked for:
//! fully connected and DRC-clean, with the counts that support the verdict.
//!
//! Everything here is re-derived from the independent validation, never
//! from the engine's own summary. A router that reports success but leaves a
//! net open cannot make `fully_connected` true, because the number comes
//! from copper geometry and `kicad-cli pcb drc`, not from the engine.

use serde::{Deserialize, Serialize};

use crate::result::{DrcCounts, RouteReport, ValidationSummary};

/// What "a good routed PCB" means for one run.
///
/// The four booleans are separate named verdicts, not a state machine: a
/// board can be connected but not DRC-clean, or DRC-clean but missing a
/// footprint. Collapsing them into an enum would lose exactly the detail a
/// reader needs to decide what to fix.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteQuality {
    pub schema_version: String,
    /// The bar itself: fully connected, DRC-clean, topologically faithful,
    /// and with every required check actually performed.
    pub passed: bool,
    /// Every pad of every net shares one copper island, and KiCad reports no
    /// unconnected items.
    pub fully_connected: bool,
    /// `kicad-cli pcb drc` ran and reported zero errors and zero unconnected
    /// items. A DRC that could not run is not a clean DRC.
    pub drc_clean: bool,
    /// The candidate has the footprints, nets, pads, layers and outline the
    /// router was given.
    pub topology_clean: bool,
    /// Nets split across more than one copper island, re-derived from copper.
    pub open_nets: usize,
    pub open_net_names: Vec<String>,
    pub drc: Option<DrcCounts>,
    /// Vias landing inside a pad of their own net.
    pub via_in_pad: usize,
    /// Tracks thinner than the fabrication floor.
    pub undersized_tracks: usize,
    /// Vias whose drill is thinner than the fabrication floor.
    pub undersized_vias: usize,
    /// Vias whose annular ring is thinner than the fabrication floor.
    pub undersized_annular: usize,
    pub segments: usize,
    pub vias: usize,
    pub wire_length_mm: f64,
    /// Reasons the board is not fabrication-ready, from independent checks.
    pub blocking_reasons: Vec<String>,
    /// Checks that could not be performed. A missing check blocks the bar
    /// even when nothing was found.
    pub unavailable_checks: Vec<String>,
    /// Which engine produced the copper, and which revision of it.
    pub engine: String,
    pub engine_version: Option<String>,
}

impl RouteQuality {
    /// Derive the quality of a run from its independently validated report.
    #[must_use]
    pub fn from_report(report: &RouteReport) -> Self {
        let validation: Option<&ValidationSummary> = report.validation.as_ref();
        let drc = validation.and_then(|v| v.kicad_drc);
        let connectivity = validation.map(|v| &v.connectivity);

        let topology_clean = validation.is_some_and(|v| v.topology.is_clean());
        let open_nets =
            validation.map_or(report.statistics.open_nets, |v| v.connectivity.open_nets);
        let open_net_names: Vec<String> = connectivity.map_or_else(
            || report.statistics.open_net_names.clone(),
            |c| {
                let mut names: Vec<String> = c.open.iter().map(|item| item.net.clone()).collect();
                names.sort();
                names.dedup();
                names
            },
        );
        let fully_connected = validation.is_some_and(|v| {
            v.connectivity.open_nets == 0
                && v.connectivity.open.is_empty()
                && v.connectivity.unconnected_pads.is_empty()
        }) && drc.is_some_and(|counts| counts.unconnected == 0)
            && open_nets == 0;
        let drc_clean = drc.is_some_and(|counts| counts.errors == 0 && counts.unconnected == 0);

        let blocking_reasons = validation.map_or_else(Vec::new, |v| v.blocking_reasons.clone());
        let unavailable_checks = validation.map_or_else(Vec::new, |v| v.unavailable_checks.clone());

        Self {
            schema_version: crate::ROUTE_REPORT_SCHEMA.to_string(),
            passed: report.is_fabrication_ready()
                && fully_connected
                && drc_clean
                && topology_clean
                && unavailable_checks.is_empty(),
            fully_connected,
            drc_clean,
            topology_clean,
            open_nets,
            open_net_names,
            drc,
            via_in_pad: connectivity.map_or(0, |c| c.via_in_pad.len()),
            undersized_tracks: connectivity.map_or(0, |c| c.undersized_tracks.len()),
            undersized_vias: connectivity.map_or(0, |c| c.undersized_vias.len()),
            undersized_annular: connectivity.map_or(0, |c| c.undersized_annular.len()),
            segments: report.statistics.segments,
            vias: report.statistics.vias,
            wire_length_mm: report.statistics.wire_length_mm,
            blocking_reasons,
            unavailable_checks,
            engine: report.engine.as_str().to_string(),
            engine_version: report
                .provenance
                .engine_version
                .clone()
                .filter(|version| version != "unknown"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::{RouteArtifacts, RouteStatistics};
    use crate::validate::{ConnectivityReport, TopologyReport};

    fn report(validation: Option<ValidationSummary>) -> RouteReport {
        RouteReport {
            schema_version: crate::ROUTE_REPORT_SCHEMA.to_string(),
            state: crate::result::RouteState::Routed,
            engine: crate::RouterEngine::KiCadRoutingTools,
            capability: None,
            provenance: crate::RouterProvenance {
                engine_version: Some("0.24.1".to_string()),
                ..crate::RouterProvenance::default()
            },
            statistics: RouteStatistics {
                segments: 12,
                vias: 3,
                wire_length_mm: 123.5,
                open_nets: 0,
                ..RouteStatistics::default()
            },
            artifacts: RouteArtifacts::default(),
            validation,
            failure: None,
            escape: Vec::new(),
            recommended_routing_order: Vec::new(),
            length_tuning: None,
        }
    }

    fn valid(drc: Option<DrcCounts>) -> ValidationSummary {
        ValidationSummary {
            fabrication_ready: true,
            topology: TopologyReport {
                outline_present: true,
                ..TopologyReport::default()
            },
            connectivity: ConnectivityReport::default(),
            kicad_drc: drc,
            blocking_reasons: Vec::new(),
            unavailable_checks: Vec::new(),
            advisories: Vec::new(),
        }
    }

    #[test]
    fn a_clean_board_passes_the_bar() {
        let quality = RouteQuality::from_report(&report(Some(valid(Some(DrcCounts {
            errors: 0,
            unconnected: 0,
            warnings: 2,
        })))));
        assert!(quality.passed, "{quality:?}");
        assert!(quality.fully_connected);
        assert!(quality.drc_clean);
        assert!(quality.topology_clean);
        assert_eq!(quality.vias, 3);
        assert_eq!(quality.engine_version.as_deref(), Some("0.24.1"));
    }

    #[test]
    fn an_open_net_fails_the_bar_even_when_drc_is_clean() {
        let mut validation = valid(Some(DrcCounts::default()));
        validation.connectivity.open_nets = 1;
        let quality = RouteQuality::from_report(&report(Some(validation)));
        assert!(!quality.fully_connected);
        assert!(!quality.passed);
        assert_eq!(quality.open_nets, 1);
    }

    #[test]
    fn a_drc_that_could_not_run_is_not_a_clean_drc() {
        let quality = RouteQuality::from_report(&report(Some(valid(None))));
        assert!(!quality.drc_clean, "an unavailable DRC is not a pass");
        assert!(!quality.passed);
    }

    #[test]
    fn a_drc_error_fails_the_bar() {
        let quality = RouteQuality::from_report(&report(Some(valid(Some(DrcCounts {
            errors: 1,
            unconnected: 0,
            warnings: 0,
        })))));
        assert!(!quality.drc_clean);
        assert!(!quality.passed);
    }

    #[test]
    fn a_missing_topology_fails_the_bar() {
        let mut validation = valid(Some(DrcCounts::default()));
        validation.topology = TopologyReport {
            outline_present: false,
            ..TopologyReport::default()
        };
        let quality = RouteQuality::from_report(&report(Some(validation)));
        assert!(!quality.topology_clean);
        assert!(!quality.passed);
    }

    #[test]
    fn an_unperformed_required_check_blocks_the_bar() {
        let mut validation = valid(Some(DrcCounts::default()));
        validation.unavailable_checks = vec!["zone refill".to_string()];
        let quality = RouteQuality::from_report(&report(Some(validation)));
        assert!(!quality.passed);
        assert_eq!(quality.unavailable_checks, vec!["zone refill".to_string()]);
    }

    #[test]
    fn a_run_with_no_validation_cannot_pass() {
        let quality = RouteQuality::from_report(&report(None));
        assert!(!quality.passed);
        assert!(!quality.fully_connected);
        assert!(!quality.drc_clean);
    }
}
