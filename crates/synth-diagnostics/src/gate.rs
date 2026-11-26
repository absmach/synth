// SPDX-License-Identifier: Apache-2.0

//! One gate and verdict model shared by every report.
//!
//! Each tool used to answer "is this board ready?" in its own vocabulary:
//! `"ok"`, `"not_run"`, `"drift"`, `blocking_count`. A caller that read only
//! one of those could take "DRC never ran" for "DRC found nothing". This
//! module is the shared answer, and it rests on one rule: **a check that did
//! not run can never read as a pass.**
//!
//! A report lists its [`Gate`]s and folds them into a [`Verdict`]. The fold
//! uses the precedence `Empty > Blocked > Fail > Warn > Pass`, so a single
//! gate that could not be evaluated outranks any number of passes, and even
//! outranks a failure: "we could not evaluate it" is a weaker readiness
//! position than "we evaluated it and found N problems".

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The outcome of one gate.
///
/// The derived order is the fold precedence: a later variant outranks every
/// earlier one in [`combined_status`].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    /// The check ran over everything it covers and found nothing.
    Pass,
    /// The check ran and found only advisory findings.
    Warn,
    /// The check ran and found blocking problems.
    Fail,
    /// The check was skipped, a prerequisite (router, `kicad-cli`, stackup,
    /// ngspice) is missing, or its evidence is incomplete.
    Blocked,
    /// There is nothing to check: the design has no components or nets.
    Empty,
}

impl GateStatus {
    /// Every status, in fold precedence order (weakest first).
    pub const ALL: [Self; 5] = [
        Self::Pass,
        Self::Warn,
        Self::Fail,
        Self::Blocked,
        Self::Empty,
    ];

    /// The wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
            Self::Blocked => "blocked",
            Self::Empty => "empty",
        }
    }
}

impl std::fmt::Display for GateStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Fold statuses into one, by precedence `Empty > Blocked > Fail > Warn > Pass`.
///
/// An empty iterator folds to [`GateStatus::Empty`]: no gates is not a pass.
pub fn combined_status<I>(statuses: I) -> GateStatus
where
    I: IntoIterator<Item = GateStatus>,
{
    statuses.into_iter().max().unwrap_or(GateStatus::Empty)
}

/// Grade a measure where smaller is better against two ceilings.
///
/// `value <= pass_ceiling` passes, `value <= warn_ceiling` warns, anything
/// above fails. A value that is not a number (NaN) was not measured, so it is
/// [`GateStatus::Blocked`].
#[must_use]
pub fn threshold_status(value: f64, pass_ceiling: f64, warn_ceiling: f64) -> GateStatus {
    if value.is_nan() {
        GateStatus::Blocked
    } else if value <= pass_ceiling {
        GateStatus::Pass
    } else if value <= warn_ceiling {
        GateStatus::Warn
    } else {
        GateStatus::Fail
    }
}

/// How much of the design a gate actually examined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Coverage {
    pub checked: usize,
    pub total: usize,
}

impl Coverage {
    /// Whether every item was examined.
    #[must_use]
    pub fn is_complete(self) -> bool {
        self.checked >= self.total
    }
}

/// One named check and its result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Gate {
    pub name: String,
    pub status: GateStatus,
    /// Why the gate has this status. Always set for a non-pass gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Where the evidence lives: a payload field (`"validation.kicad_drc"`)
    /// or a file path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<Coverage>,
    /// The upstream gate whose failure stopped this one from running.
    ///
    /// Such a gate is still `blocked`, but while that upstream gate fails it
    /// does not outrank the failure in the verdict: the design is NOT_READY
    /// for a reason already found, not INCOMPLETE for want of a tool. It can
    /// never make a report READY, since a failure is present by definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<String>,
}

impl Gate {
    #[must_use]
    pub fn new(name: impl Into<String>, status: GateStatus) -> Self {
        Self {
            name: name.into(),
            status,
            reason: None,
            evidence_ref: None,
            coverage: None,
            blocked_by: None,
        }
    }

    /// A gate that did not run because `upstream` failed.
    #[must_use]
    pub fn after_failure(name: impl Into<String>, upstream: &str) -> Self {
        let mut gate = Self::blocked(name, format!("not run: {upstream} failed"));
        gate.blocked_by = Some(upstream.to_string());
        gate
    }

    #[must_use]
    pub fn pass(name: impl Into<String>) -> Self {
        Self::new(name, GateStatus::Pass)
    }

    #[must_use]
    pub fn warn(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(name, GateStatus::Warn).with_reason(reason)
    }

    #[must_use]
    pub fn fail(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(name, GateStatus::Fail).with_reason(reason)
    }

    #[must_use]
    pub fn blocked(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(name, GateStatus::Blocked).with_reason(reason)
    }

    #[must_use]
    pub fn empty(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(name, GateStatus::Empty).with_reason(reason)
    }

    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    #[must_use]
    pub fn with_evidence(mut self, evidence_ref: impl Into<String>) -> Self {
        self.evidence_ref = Some(evidence_ref.into());
        self
    }

    /// Record how much the gate examined.
    ///
    /// Partial coverage cannot pass: a gate that would otherwise pass or
    /// warn becomes [`GateStatus::Blocked`], because the unexamined part is
    /// exactly a check that did not run.
    #[must_use]
    pub fn with_coverage(mut self, checked: usize, total: usize) -> Self {
        let coverage = Coverage { checked, total };
        self.coverage = Some(coverage);
        if !coverage.is_complete() && self.status <= GateStatus::Warn {
            self.status = GateStatus::Blocked;
            let partial = format!("partial coverage: {checked} of {total}");
            self.reason = Some(match self.reason.take() {
                Some(r) => format!("{r}; {partial}"),
                None => partial,
            });
        }
        self
    }
}

/// The readiness answer for a set of gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    /// Every gate ran and passed (warnings allowed).
    Ready,
    /// At least one gate ran and failed; none was blocked.
    NotReady,
    /// At least one gate could not be evaluated.
    Incomplete,
    /// There is nothing to evaluate.
    Empty,
}

impl Verdict {
    #[must_use]
    pub fn from_status(status: GateStatus) -> Self {
        match status {
            GateStatus::Pass | GateStatus::Warn => Self::Ready,
            GateStatus::Fail => Self::NotReady,
            GateStatus::Blocked => Self::Incomplete,
            GateStatus::Empty => Self::Empty,
        }
    }

    /// The wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "READY",
            Self::NotReady => "NOT_READY",
            Self::Incomplete => "INCOMPLETE",
            Self::Empty => "EMPTY",
        }
    }

    /// Parse [`Self::as_str`].
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "READY" => Some(Self::Ready),
            "NOT_READY" => Some(Self::NotReady),
            "INCOMPLETE" => Some(Self::Incomplete),
            "EMPTY" => Some(Self::Empty),
            _ => None,
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A report's gates and the verdict they fold to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GateReport {
    pub verdict: Verdict,
    /// Gates that ran and failed.
    pub failing_gates: Vec<String>,
    /// Gates that could not be evaluated.
    pub blocked_gates: Vec<String>,
    /// One line naming why the verdict is what it is.
    pub summary: String,
    pub gates: Vec<Gate>,
}

impl GateReport {
    #[must_use]
    pub fn new(gates: Vec<Gate>) -> Self {
        let failed: std::collections::BTreeSet<&str> = gates
            .iter()
            .filter(|g| g.status == GateStatus::Fail)
            .map(|g| g.name.as_str())
            .collect();
        // A gate stopped by a failure that is itself in this report folds in
        // behind that failure, not above it.
        let counted = gates.iter().filter(|g| {
            !g.blocked_by
                .as_deref()
                .is_some_and(|upstream| failed.contains(upstream))
        });
        let verdict = Verdict::from_status(combined_status(counted.map(|g| g.status)));
        let names = |status: GateStatus| -> Vec<String> {
            gates
                .iter()
                .filter(|g| g.status == status)
                .map(|g| g.name.clone())
                .collect()
        };
        let failing_gates = names(GateStatus::Fail);
        let blocked_gates = names(GateStatus::Blocked);
        let explain = |status: GateStatus| -> String {
            gates
                .iter()
                .filter(|g| g.status == status)
                .map(|g| match &g.reason {
                    Some(r) => format!("{}: {r}", g.name),
                    None => g.name.clone(),
                })
                .collect::<Vec<_>>()
                .join("; ")
        };
        let summary = match verdict {
            Verdict::Ready => "every gate ran and passed".to_string(),
            Verdict::NotReady => format!(
                "{} gate(s) failed — {}",
                failing_gates.len(),
                explain(GateStatus::Fail)
            ),
            Verdict::Incomplete => format!(
                "{} gate(s) could not be evaluated — {}",
                blocked_gates.len(),
                explain(GateStatus::Blocked)
            ),
            Verdict::Empty if gates.is_empty() => "no gates were evaluated".to_string(),
            Verdict::Empty => format!("nothing to evaluate — {}", explain(GateStatus::Empty)),
        };
        Self {
            verdict,
            failing_gates,
            blocked_gates,
            summary,
            gates,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn status() -> impl Strategy<Value = GateStatus> {
        prop::sample::select(GateStatus::ALL.to_vec())
    }

    proptest! {
        /// The central rule: nothing but passes folds to a pass.
        #[test]
        fn a_non_pass_member_never_folds_to_pass(statuses in prop::collection::vec(status(), 0..12)) {
            let folded = combined_status(statuses.iter().copied());
            if statuses.iter().any(|s| *s != GateStatus::Pass) || statuses.is_empty() {
                prop_assert_ne!(folded, GateStatus::Pass);
            } else {
                prop_assert_eq!(folded, GateStatus::Pass);
            }
        }

        /// The fold is the precedence maximum, whatever the order.
        #[test]
        fn fold_is_the_highest_precedence_member(mut statuses in prop::collection::vec(status(), 1..12)) {
            let expected = *statuses.iter().max().unwrap();
            prop_assert_eq!(combined_status(statuses.iter().copied()), expected);
            statuses.reverse();
            prop_assert_eq!(combined_status(statuses), expected);
        }

        /// A report with any blocked gate is never READY or NOT_READY.
        #[test]
        fn a_blocked_gate_makes_the_verdict_incomplete(statuses in prop::collection::vec(status(), 0..8)) {
            let mut gates: Vec<Gate> = statuses.iter().enumerate()
                .map(|(i, s)| Gate::new(format!("g{i}"), *s)).collect();
            gates.push(Gate::blocked("router", "not installed"));
            let report = GateReport::new(gates);
            prop_assert!(matches!(report.verdict, Verdict::Incomplete | Verdict::Empty));
        }

        /// Gates stopped by a failure can never make a report READY.
        #[test]
        fn gates_after_a_failure_never_read_ready(statuses in prop::collection::vec(status(), 0..8)) {
            let mut gates: Vec<Gate> = statuses.iter().enumerate()
                .map(|(i, s)| Gate::new(format!("g{i}"), *s)).collect();
            gates.push(Gate::after_failure("drc", "g0"));
            let report = GateReport::new(gates);
            prop_assert_ne!(report.verdict, Verdict::Ready);
        }

        /// Partial coverage never passes.
        #[test]
        fn partial_coverage_never_passes(checked in 0usize..50, extra in 1usize..50) {
            let gate = Gate::pass("routing").with_coverage(checked, checked + extra);
            prop_assert_eq!(gate.status, GateStatus::Blocked);
        }
    }

    #[test]
    fn empty_iterator_folds_to_empty() {
        assert_eq!(combined_status([]), GateStatus::Empty);
        assert_eq!(GateReport::new(Vec::new()).verdict, Verdict::Empty);
    }

    #[test]
    fn blocked_outranks_fail() {
        let report = GateReport::new(vec![
            Gate::fail("erc", "2 errors"),
            Gate::blocked("kicad_drc", "kicad-cli not installed"),
        ]);
        assert_eq!(report.verdict, Verdict::Incomplete);
        assert_eq!(report.blocked_gates, vec!["kicad_drc"]);
        assert_eq!(report.failing_gates, vec!["erc"]);
        assert!(report.summary.contains("kicad-cli not installed"));
    }

    #[test]
    fn a_gate_stopped_by_a_failure_does_not_outrank_it() {
        let report = GateReport::new(vec![
            Gate::fail("source", "2 errors"),
            Gate::after_failure("drc", "source"),
        ]);
        assert_eq!(report.verdict, Verdict::NotReady);
        assert_eq!(report.blocked_gates, vec!["drc"]);
        // A genuinely missing tool still outranks the failure.
        let report = GateReport::new(vec![
            Gate::fail("source", "2 errors"),
            Gate::after_failure("drc", "source"),
            Gate::blocked("kicad_drc", "kicad-cli not installed"),
        ]);
        assert_eq!(report.verdict, Verdict::Incomplete);
        // Naming an upstream gate that did not fail changes nothing.
        let report = GateReport::new(vec![
            Gate::pass("source"),
            Gate::after_failure("drc", "source"),
        ]);
        assert_eq!(report.verdict, Verdict::Incomplete);
    }

    #[test]
    fn warnings_are_ready() {
        let report = GateReport::new(vec![Gate::pass("erc"), Gate::warn("readability", "1")]);
        assert_eq!(report.verdict, Verdict::Ready);
    }

    #[test]
    fn full_coverage_keeps_the_status() {
        assert_eq!(
            Gate::pass("routing").with_coverage(4, 4).status,
            GateStatus::Pass
        );
        // A failure stays a failure: partial coverage only lifts weaker results.
        assert_eq!(
            Gate::fail("routing", "open").with_coverage(1, 4).status,
            GateStatus::Fail
        );
    }

    #[test]
    fn threshold_grades_three_levels() {
        assert_eq!(threshold_status(1.0, 2.0, 5.0), GateStatus::Pass);
        assert_eq!(threshold_status(3.0, 2.0, 5.0), GateStatus::Warn);
        assert_eq!(threshold_status(9.0, 2.0, 5.0), GateStatus::Fail);
        assert_eq!(threshold_status(f64::NAN, 2.0, 5.0), GateStatus::Blocked);
    }

    #[test]
    fn wire_spelling() {
        let report = GateReport::new(vec![Gate::pass("erc")]);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["verdict"], "READY");
        assert_eq!(json["gates"][0]["status"], "pass");
        assert_eq!(
            serde_json::to_value(Verdict::NotReady).unwrap(),
            serde_json::json!("NOT_READY")
        );
        for v in [
            Verdict::Ready,
            Verdict::NotReady,
            Verdict::Incomplete,
            Verdict::Empty,
        ] {
            assert_eq!(Verdict::parse(v.as_str()), Some(v));
        }
    }
}
