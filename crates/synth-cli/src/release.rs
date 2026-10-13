// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "synth.release.v1";

pub const MANIFEST_FILENAME: &str = "release.json";

pub const UNTRUSTED_BANNER: &str = "UNTRUSTED / NOT FOR FABRICATION";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideKind {
    ErcErrors,
    BoundingBoxFootprints,
    NativeDrcErrors,
    UnverifiedParts,
}

impl OverrideKind {
    pub fn flag(self) -> &'static str {
        match self {
            Self::ErcErrors | Self::BoundingBoxFootprints | Self::NativeDrcErrors => "--force",
            Self::UnverifiedParts => "--allow-unverified-parts",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::ErcErrors => "E-SYNTH-ERC-OVERRIDE",
            Self::BoundingBoxFootprints => "E-SYNTH-EXPORT-001",
            Self::NativeDrcErrors => "E-SYNTH-DRC-OVERRIDE",
            Self::UnverifiedParts => "W-SYNTH-PART-UNVERIFIED",
        }
    }

    pub fn consequence(self) -> &'static str {
        match self {
            Self::ErcErrors => {
                "electrical-rule errors were suppressed, so the netlist is known to be wrong"
            }
            Self::BoundingBoxFootprints => {
                "pad geometry is a synthesized guess, not the manufacturer's land pattern"
            }
            Self::NativeDrcErrors => "KiCad reported design-rule violations that were suppressed",
            Self::UnverifiedParts => {
                "no reviewer has checked these part definitions against their datasheets"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverrideRecord {
    pub kind: OverrideKind,
    pub flag: String,
    pub code: String,
    pub consequence: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_parts: Vec<String>,
    pub detail: String,
}

impl OverrideRecord {
    pub fn new(kind: OverrideKind, affected_parts: Vec<String>, detail: impl Into<String>) -> Self {
        Self {
            kind,
            flag: kind.flag().to_string(),
            code: kind.code().to_string(),
            consequence: kind.consequence().to_string(),
            affected_parts,
            detail: detail.into(),
        }
    }

    pub fn summary_line(&self) -> String {
        let parts = if self.affected_parts.is_empty() {
            String::new()
        } else {
            format!(" [{}]", self.affected_parts.join(", "))
        };
        format!(
            "{} via {}{}: {}",
            self.code, self.flag, parts, self.consequence
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewerState {
    pub part_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_at: Option<String>,
    pub reviewed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exception {
    pub authorized_by: String,
    pub reason: String,
}

impl Exception {
    pub fn new(authorized_by: &str, reason: &str) -> Result<Self, String> {
        let authorized_by = authorized_by.trim();
        let reason = reason.trim();
        if authorized_by.is_empty() {
            return Err("an override exception needs an authorizing identity".to_string());
        }
        if reason.is_empty() {
            return Err("an override exception needs a stated reason".to_string());
        }
        Ok(Self {
            authorized_by: authorized_by.to_string(),
            reason: reason.to_string(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionStatus {
    Production,
    Untrusted,
}

/// What the release gate knows about the board's routing.
///
/// Recorded rather than inferred, because "which router produced this
/// copper, at what version, and did anyone check it" is the first question
/// a fabricator or an auditor asks, and it cannot be recovered from a
/// `.kicad_pcb` afterwards.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingState {
    /// Which external engine generated the copper.
    pub engine: String,
    /// That engine's version, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    /// Terminal state: `routed`, `review_required`, `router_unavailable`,
    /// or `validation_failed`.
    pub state: String,
    /// SHA-256 of the board handed to the router.
    pub input_hash: String,
    /// Path of the run record, so the details can be read.
    pub run_record: String,
    /// Why the gate is not satisfied, when it is not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    /// What the fabricator must be told about this board's copper.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fabrication_notes: Vec<String>,
}

/// One order note per via found inside a pad of a routed board.
///
/// The validator lists every via-in-pad site whatever the policy, and an
/// unapproved one blocks the route. So a `Routed` state with sites in the
/// list means they were approved; any other state gets no order note.
fn fabrication_notes(
    state: synth_router::RouteState,
    validation: Option<&synth_router::ValidationSummary>,
) -> Vec<String> {
    let Some(validation) = validation.filter(|_| state == synth_router::RouteState::Routed) else {
        return Vec::new();
    };
    validation
        .connectivity
        .via_in_pad
        .iter()
        .map(|site| format!("{site}: via-in-pad, order with filled and capped vias"))
        .collect()
}

impl RoutingState {
    /// Build the release-gate view of a routing run record.
    pub fn from_report(report: &synth_router::RouteReport, run_record: &Path) -> Self {
        let mut reasons: Vec<String> = report
            .validation
            .as_ref()
            .map(|v| {
                v.blocking_reasons
                    .iter()
                    .chain(v.unavailable_checks.iter())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if let Some(failure) = &report.failure {
            reasons.push(failure.remediation.clone());
        }
        Self {
            engine: report.engine.as_str().to_string(),
            engine_version: report.provenance.engine_version.clone(),
            state: report.state.as_str().to_string(),
            input_hash: report.provenance.input_hash.clone(),
            run_record: run_record.display().to_string(),
            reasons,
            fabrication_notes: fabrication_notes(report.state, report.validation.as_ref()),
        }
    }

    /// Whether this board may go to fabrication.
    ///
    /// Fail-closed: only a validated route qualifies. Neither a router's
    /// exit code nor the absence of complaints can turn an un-routed or
    /// unchecked board into a deliverable one.
    pub fn is_fabrication_ready(&self) -> bool {
        self.state == "routed" && self.reasons.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub schema_version: String,
    pub tool_version: String,
    pub input: String,
    pub fab_requested: bool,
    pub production_status: ProductionStatus,
    pub release_ready: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<OverrideRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exception: Option<Exception>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviewer_state: Vec<ReviewerState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<RoutingState>,
}

impl ReleaseManifest {
    pub fn new(
        tool_version: &str,
        input: &Path,
        fab_requested: bool,
        mut overrides: Vec<OverrideRecord>,
        reviewer_state: Vec<ReviewerState>,
    ) -> Self {
        overrides.sort_by_key(|o| o.kind);
        let clean = overrides.is_empty();
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            tool_version: tool_version.to_string(),
            input: input.display().to_string(),
            fab_requested,
            production_status: if clean {
                ProductionStatus::Production
            } else {
                ProductionStatus::Untrusted
            },
            release_ready: clean,
            overrides,
            exception: None,
            reviewer_state,
            routing: None,
        }
    }

    /// Record the routing run the package was built from.
    ///
    /// An unvalidated route downgrades the manifest the same way an
    /// override does: the package is not fabricable, and the reason is in
    /// the manifest rather than only in a stderr line nobody archived.
    pub fn with_routing(mut self, routing: Option<RoutingState>) -> Self {
        if let Some(state) = &routing {
            if !state.is_fabrication_ready() {
                self.production_status = ProductionStatus::Untrusted;
                self.release_ready = false;
            }
        }
        self.routing = routing;
        self
    }

    pub fn with_release_blocked(mut self, blocked: bool) -> Self {
        if blocked {
            self.production_status = ProductionStatus::Untrusted;
            self.release_ready = false;
        }
        self
    }

    pub fn with_exception(mut self, exception: Option<Exception>) -> Self {
        self.exception = exception;
        self
    }

    pub fn has_overrides(&self) -> bool {
        !self.overrides.is_empty()
    }

    pub fn banner(&self) -> Option<String> {
        let routing_block = self
            .routing
            .as_ref()
            .is_some_and(|r| !r.is_fabrication_ready());
        if !self.has_overrides() && !routing_block {
            return None;
        }
        if routing_block {
            let state = self.routing.as_ref();
            let mut out = format!(
                "{UNTRUSTED_BANNER} — this package is not fabricable:\n  \
                 routing state: {}\n",
                state.map_or("unknown", |s| s.state.as_str())
            );
            let reasons: &[String] = state.map_or(&[], |s| s.reasons.as_slice());
            for reason in reasons {
                out.push_str("    ");
                out.push_str(reason);
                out.push('\n');
            }
            if let Some(state) = state {
                use std::fmt::Write as _;
                let _ = writeln!(out, "    run record: {}", state.run_record);
            }
            if self.has_overrides() {
                out.push_str("  the package also carries export overrides:\n");
            }
            for record in &self.overrides {
                out.push_str("  ");
                out.push_str(&record.summary_line());
                out.push('\n');
            }
            out.push_str(
                "\nDo not submit this package for fabrication. Copper is generated by an \
                 external router and must pass independent validation; see \
                 docs/kicad-workflows.md.\n",
            );
            return Some(out);
        }
        let mut out = format!("{UNTRUSTED_BANNER} — this package carries export overrides:\n");
        for record in &self.overrides {
            out.push_str("  ");
            out.push_str(&record.summary_line());
            out.push('\n');
        }
        match &self.exception {
            Some(exception) => {
                use std::fmt::Write as _;
                let _ = write!(
                    out,
                    "A release exception is recorded: {} — {}",
                    exception.authorized_by, exception.reason
                );
            }
            None => out.push_str(
                "Do not submit this package for fabrication. Fix the findings, or record an \
                 authorized exception on the release gate.",
            ),
        }
        Some(out)
    }

    pub fn gate_verdict(&self) -> GateVerdict {
        if !self.has_overrides() {
            return GateVerdict::Accepted;
        }
        match &self.exception {
            Some(exception) => GateVerdict::AcceptedUnderException {
                authorized_by: exception.authorized_by.clone(),
            },
            None => GateVerdict::Rejected,
        }
    }

    pub fn write_to(&self, dir: &Path) -> std::io::Result<std::path::PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(MANIFEST_FILENAME);
        let mut json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        json.push('\n');
        std::fs::write(&path, json)?;
        Ok(path)
    }

    pub fn read_from(dir: &Path) -> std::io::Result<Self> {
        let path = dir.join(MANIFEST_FILENAME);
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateVerdict {
    Accepted,
    AcceptedUnderException { authorized_by: String },
    Rejected,
}

pub fn reviewer_state(board: &synth_ir::Board) -> Vec<ReviewerState> {
    let mut by_id: BTreeMap<String, ReviewerState> = BTreeMap::new();
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let provenance = part.provenance.as_ref();
        let reviewed_by = provenance
            .and_then(|p| p.reviewed_by.clone())
            .filter(|r| !r.trim().is_empty());
        let reviewed_at = provenance
            .and_then(|p| p.reviewed_at.clone())
            .filter(|d| !d.trim().is_empty());
        by_id.insert(
            part.id.0.clone(),
            ReviewerState {
                part_id: part.id.0.clone(),
                reviewed: reviewed_by.is_some(),
                reviewed_by,
                reviewed_at,
            },
        );
    }
    by_id.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(overrides: Vec<OverrideRecord>) -> ReleaseManifest {
        ReleaseManifest::new(
            "synth-cli test",
            Path::new("board.synth"),
            true,
            overrides,
            Vec::new(),
        )
    }

    fn unverified() -> OverrideRecord {
        OverrideRecord::new(
            OverrideKind::UnverifiedParts,
            vec!["ams1117_3v3".into()],
            "1 part has no reviewer",
        )
    }

    fn validation_with_via_in_pad() -> synth_router::ValidationSummary {
        synth_router::ValidationSummary {
            fabrication_ready: true,
            topology: synth_router::validate::TopologyReport::default(),
            connectivity: synth_router::validate::ConnectivityReport {
                via_in_pad: vec!["via at (11.000, 10.000) mm U1.EP".to_string()],
                ..Default::default()
            },
            kicad_drc: None,
            blocking_reasons: Vec::new(),
            unavailable_checks: Vec::new(),
        }
    }

    #[test]
    fn a_routed_via_in_pad_becomes_a_fabrication_note_naming_the_site() {
        let validation = validation_with_via_in_pad();
        let notes = fabrication_notes(synth_router::RouteState::Routed, Some(&validation));
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("U1.EP"), "{notes:?}");
        assert!(notes[0].contains("filled and capped"), "{notes:?}");
    }

    #[test]
    fn a_board_that_is_not_routed_gets_no_order_note() {
        let validation = validation_with_via_in_pad();
        for state in [
            synth_router::RouteState::ValidationFailed,
            synth_router::RouteState::ReviewRequired,
            synth_router::RouteState::RouterUnavailable,
        ] {
            assert!(fabrication_notes(state, Some(&validation)).is_empty());
        }
        assert!(fabrication_notes(synth_router::RouteState::Routed, None).is_empty());
    }

    #[test]
    fn fabrication_notes_are_omitted_from_the_record_when_there_are_none() {
        let none = serde_json::to_value(RoutingState::default()).unwrap();
        assert!(none.get("fabrication_notes").is_none(), "{none}");
        let some = serde_json::to_value(RoutingState {
            fabrication_notes: vec!["n".into()],
            ..RoutingState::default()
        })
        .unwrap();
        assert_eq!(some["fabrication_notes"][0], "n");
    }

    #[test]
    fn a_clean_export_is_production_and_release_ready() {
        let m = manifest(Vec::new());
        assert_eq!(m.production_status, ProductionStatus::Production);
        assert!(m.release_ready);
        assert!(m.banner().is_none());
        assert_eq!(m.gate_verdict(), GateVerdict::Accepted);
    }

    #[test]
    fn an_override_can_never_be_release_ready() {
        for kind in [
            OverrideKind::ErcErrors,
            OverrideKind::BoundingBoxFootprints,
            OverrideKind::NativeDrcErrors,
            OverrideKind::UnverifiedParts,
        ] {
            let m = manifest(vec![OverrideRecord::new(kind, Vec::new(), "detail")]);
            assert!(
                !m.release_ready,
                "{kind:?} must not produce release_ready=true"
            );
            assert_eq!(m.production_status, ProductionStatus::Untrusted);
        }
    }

    #[test]
    fn an_exception_does_not_make_a_package_release_ready() {
        let m = manifest(vec![unverified()]).with_exception(Some(
            Exception::new("a release manager", "prototype run").unwrap(),
        ));
        assert!(
            !m.release_ready,
            "an exception authorizes the run, it does not clean the package"
        );
        assert_eq!(m.production_status, ProductionStatus::Untrusted);
        assert_eq!(
            m.gate_verdict(),
            GateVerdict::AcceptedUnderException {
                authorized_by: "a release manager".into()
            }
        );
    }

    #[test]
    fn the_gate_rejects_overrides_without_an_exception() {
        assert_eq!(
            manifest(vec![unverified()]).gate_verdict(),
            GateVerdict::Rejected
        );
    }

    #[test]
    fn the_banner_names_the_flag_the_parts_and_the_consequence() {
        let banner = manifest(vec![unverified()]).banner().expect("banner");
        assert!(banner.starts_with(UNTRUSTED_BANNER), "{banner}");
        assert!(banner.contains("--allow-unverified-parts"), "{banner}");
        assert!(banner.contains("ams1117_3v3"), "{banner}");
        assert!(banner.contains("no reviewer"), "{banner}");
        assert!(banner.contains("Do not submit"), "{banner}");
    }

    #[test]
    fn the_banner_records_an_exception_instead_of_the_refusal() {
        let banner = manifest(vec![unverified()])
            .with_exception(Some(
                Exception::new("a release manager", "prototype run").unwrap(),
            ))
            .banner()
            .expect("banner");
        assert!(banner.contains("a release manager"), "{banner}");
        assert!(banner.contains("prototype run"), "{banner}");
        assert!(!banner.contains("Do not submit"), "{banner}");
    }

    #[test]
    fn an_exception_needs_both_an_identity_and_a_reason() {
        assert!(Exception::new("", "a reason").is_err());
        assert!(Exception::new("someone", "   ").is_err());
        assert!(Exception::new(" someone ", " a reason ").is_ok());
    }

    #[test]
    fn overrides_are_recorded_in_a_stable_order() {
        let m = manifest(vec![
            unverified(),
            OverrideRecord::new(OverrideKind::ErcErrors, Vec::new(), "d"),
        ]);
        let kinds: Vec<OverrideKind> = m.overrides.iter().map(|o| o.kind).collect();
        assert_eq!(
            kinds,
            vec![OverrideKind::ErcErrors, OverrideKind::UnverifiedParts]
        );
    }

    #[test]
    fn the_manifest_round_trips_through_the_output_directory() {
        let dir =
            std::env::temp_dir().join(format!("synth_release_manifest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let original = manifest(vec![unverified()]);
        let path = original.write_to(&dir).expect("write");
        assert_eq!(path.file_name().unwrap(), MANIFEST_FILENAME);
        let back = ReleaseManifest::read_from(&dir).expect("read");
        assert_eq!(original, back);
        assert!(!back.release_ready);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_manifest_claiming_release_ready_with_overrides_is_not_constructible() {
        let json = serde_json::to_value(manifest(vec![unverified()])).unwrap();
        assert_eq!(json["release_ready"], false);
        assert_eq!(json["production_status"], "untrusted");
        assert_eq!(json["overrides"][0]["flag"], "--allow-unverified-parts");
        assert_eq!(json["overrides"][0]["affected_parts"][0], "ams1117_3v3");
    }
}
