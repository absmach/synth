// SPDX-License-Identifier: Apache-2.0

//! Why an external routing run did not produce a fabricable board.
//!
//! Every failure carries a stable machine-readable
//! [`RouterFailureReason`], the [`RouterStage`] it happened in, and a
//! remediation string. The reason exists because the three broad outcomes
//! a user has to tell apart — *the tool is not installed*, *the tool ran
//! but produced nothing*, and *the tool produced something the board's own
//! rules reject* — call for three completely different responses, and a
//! single "router error" string forces the user to guess which one they
//! are looking at.

use serde::{Deserialize, Serialize};

use crate::contract::RouterEngine;

/// Where in the pipeline a run stopped.
///
/// Stages are ordered, and a failure records the last one reached, so a
/// report says *where* the run died rather than only that it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterStage {
    /// Deciding whether the engine can run at all.
    Capability,
    /// Copying the un-routed export to the baseline.
    Baseline,
    /// Driving the engine itself.
    Route,
    /// Bringing the engine's output back into a KiCad board.
    Import,
    /// Reading the candidate back and comparing it to the baseline.
    Topology,
    /// Re-deriving connectivity from copper.
    Connectivity,
    /// Zone refill and `kicad-cli pcb drc`.
    Drc,
    /// Promoting a validated candidate over the baseline.
    Install,
}

impl RouterStage {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capability => "capability",
            Self::Baseline => "baseline",
            Self::Route => "route",
            Self::Import => "import",
            Self::Topology => "topology",
            Self::Connectivity => "connectivity",
            Self::Drc => "drc",
            Self::Install => "install",
        }
    }
}

/// Stable classification of why a run failed.
///
/// These strings are part of the run-record contract: automation keys
/// off them, so they change only deliberately, never as a side effect of
/// re-wording a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterFailureReason {
    /// The engine is not installed where the configuration says it is.
    NotInstalled,
    /// FreeRouting JAR missing or unreadable.
    JarMissing,
    /// The Java runtime for FreeRouting is missing or unlaunchable.
    JavaMissing,
    /// The KiCadRoutingTools checkout is missing or incomplete.
    CheckoutMissing,
    /// The Python interpreter KiCadRoutingTools needs is missing.
    PythonMissing,
    /// A Python interpreter exists, but cannot import KiCad's `pcbnew`
    /// bindings, which the FreeRouting helper requires.
    ///
    /// Distinct from [`Self::PythonMissing`]: the interpreter is present and
    /// runs, and the fix is to use the one KiCad ships rather than to install
    /// an interpreter.
    PythonBindingsMissing,
    /// The process could not be started at all.
    SpawnFailed,
    /// The engine exceeded its wall-clock budget and was terminated.
    Timeout,
    /// The run was cancelled before it finished.
    Cancelled,
    /// The engine exited non-zero.
    Crashed,
    /// The engine ran and refused the board.
    Rejected,
    /// The engine's output could not be parsed.
    MalformedOutput,
    /// The engine exited successfully but wrote no output board.
    NoOutput,
    /// The candidate failed independent validation.
    ValidationFailed,
    /// The engine cannot honour a constraint the request carries.
    ///
    /// Distinct from a rejection: the engine did not try and fail, it was
    /// never asked, because honouring the request would need support it
    /// does not have. Reporting it as a plain failure would invite a retry
    /// that cannot succeed; this says the request and the engine disagree.
    Unsupported,
    /// A file could not be read or written.
    Io,
}

impl RouterFailureReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::JarMissing => "jar_missing",
            Self::JavaMissing => "java_missing",
            Self::CheckoutMissing => "checkout_missing",
            Self::PythonMissing => "python_missing",
            Self::PythonBindingsMissing => "python_bindings_missing",
            Self::SpawnFailed => "spawn_failed",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Crashed => "crashed",
            Self::Rejected => "rejected",
            Self::MalformedOutput => "malformed_output",
            Self::NoOutput => "no_output",
            Self::ValidationFailed => "validation_failed",
            Self::Unsupported => "unsupported",
            Self::Io => "io",
        }
    }

    /// Whether this reason means the engine never ran.
    ///
    /// Distinct from "the run failed": a capability failure is a
    /// configuration problem, and the remedy is to install something, not
    /// to redesign the board or loosen a rule.
    #[must_use]
    pub fn is_unavailable(self) -> bool {
        matches!(
            self,
            Self::NotInstalled
                | Self::JarMissing
                | Self::JavaMissing
                | Self::CheckoutMissing
                | Self::PythonMissing
                | Self::PythonBindingsMissing
        )
    }
}

impl std::fmt::Display for RouterFailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A structured routing failure.
///
/// `remediation` is part of the type rather than left to the caller's
/// prose, because the whole point of a structured failure is that the
/// agent, CI job, or UI reading the report can tell the user what to do
/// without re-deriving it from context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterFailure {
    pub engine: RouterEngine,
    pub reason: RouterFailureReason,
    pub stage: RouterStage,
    /// Process exit code, when the engine actually ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Human-readable detail: the error text, capped in length.
    pub detail: String,
    /// What the user should do about it.
    pub remediation: String,
}

impl RouterFailure {
    /// Build a failure with a reason-derived default remediation.
    #[must_use]
    pub fn for_stage(
        engine: RouterEngine,
        stage: RouterStage,
        reason: RouterFailureReason,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            engine,
            reason,
            stage,
            exit_code: None,
            detail: detail.into(),
            remediation: default_remediation(engine, reason),
        }
    }

    /// A filesystem failure.
    #[must_use]
    pub fn io(engine: RouterEngine, detail: impl Into<String>) -> Self {
        Self::for_stage(
            engine,
            RouterStage::Baseline,
            RouterFailureReason::Io,
            detail,
        )
    }

    /// Attach the exit code of a process that did run.
    #[must_use]
    pub fn with_exit_code(mut self, code: Option<i32>) -> Self {
        self.exit_code = code;
        self
    }

    /// Override the default remediation.
    #[must_use]
    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = remediation.into();
        self
    }

    /// One-line summary suitable for stderr.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} routing failed at {} stage ({}): {}",
            self.engine,
            self.stage.as_str(),
            self.reason,
            self.detail
        )
    }
}

impl std::fmt::Display for RouterFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary())
    }
}

impl std::error::Error for RouterFailure {}

/// Per-reason guidance, with an engine-specific install hint where one
/// exists.
///
/// The install hints name the thing that is actually missing, because
/// "install the router" is useless to someone whose FreeRouting install is
/// complete but whose JAR path points at a deleted file.
fn default_remediation(engine: RouterEngine, reason: RouterFailureReason) -> String {
    match reason {
        RouterFailureReason::NotInstalled => match engine {
            RouterEngine::Freerouting => {
                "install FreeRouting and pass --freerouting-jar, or set SYNTH_FREEROUTING_JAR"
                    .to_string()
            }
            RouterEngine::KiCadRoutingTools => {
                "pass --kicad-routing-tools-repo pointing at a KiCadRoutingTools checkout, or \
                 set KICAD_ROUTING_TOOLS_REPO"
                    .to_string()
            }
        },
        RouterFailureReason::JarMissing => {
            "point --freerouting-jar at a readable freerouting JAR (or SYNTH_FREEROUTING_JAR)"
                .to_string()
        }
        RouterFailureReason::JavaMissing => {
            "install a Java runtime (17+) or pass --freerouting-java".to_string()
        }
        RouterFailureReason::Unsupported => {
            "route this board with KiCadRoutingTools, which honours netclass layer pins, or              remove the `layers` constraint from the class"
                .to_string()
        }
        RouterFailureReason::CheckoutMissing => {
            "pass --kicad-routing-tools-repo pointing at a checkout that contains \
             py_router/route.py"
                .to_string()
        }
        RouterFailureReason::PythonMissing => {
            "pass --kicad-routing-tools-python pointing at an interpreter with KiCadRoutingTools' \
             dependencies installed"
                .to_string()
        }
        RouterFailureReason::PythonBindingsMissing => {
            "tools/freeroute_autoroute.py needs KiCad's pcbnew module, which ships with KiCad's \
             own Python; point SYNTH_FREEROUTING_PYTHON at that interpreter"
                .to_string()
        }
        RouterFailureReason::SpawnFailed => {
            "check that the configured executable exists and is executable on this machine"
                .to_string()
        }
        RouterFailureReason::Timeout => {
            "raise the router budget (--router-timeout) or reduce the design; the baseline \
             export is preserved and no partial board was installed"
                .to_string()
        }
        RouterFailureReason::Cancelled => {
            "the run was cancelled; re-run it to retry the same revision deterministically"
                .to_string()
        }
        RouterFailureReason::Crashed => {
            "inspect the retained session log for the engine's own error; the baseline export \
             is unchanged"
                .to_string()
        }
        RouterFailureReason::Rejected => {
            "the engine refused this board; simplify the placement or relax the routing \
             constraints, then retry"
                .to_string()
        }
        RouterFailureReason::MalformedOutput => {
            "the engine's output could not be parsed; the retained log has the raw text and \
             the engine version may be incompatible"
                .to_string()
        }
        RouterFailureReason::NoOutput => {
            "the engine exited successfully but wrote no board; the baseline export is \
             preserved unchanged"
                .to_string()
        }
        RouterFailureReason::ValidationFailed => {
            "the routed board failed independent validation; read the connectivity and DRC \
             reports — a router exit code of zero is not proof of fabricability"
                .to_string()
        }
        RouterFailureReason::Io => {
            "check filesystem permissions and free space in the output directory".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reason_strings_are_stable_and_unique() {
        let all = [
            RouterFailureReason::NotInstalled,
            RouterFailureReason::JarMissing,
            RouterFailureReason::JavaMissing,
            RouterFailureReason::CheckoutMissing,
            RouterFailureReason::PythonMissing,
            RouterFailureReason::SpawnFailed,
            RouterFailureReason::Timeout,
            RouterFailureReason::Cancelled,
            RouterFailureReason::Crashed,
            RouterFailureReason::Rejected,
            RouterFailureReason::MalformedOutput,
            RouterFailureReason::NoOutput,
            RouterFailureReason::ValidationFailed,
            RouterFailureReason::Unsupported,
            RouterFailureReason::Io,
        ];
        let mut seen = std::collections::BTreeSet::new();
        for reason in all {
            assert!(seen.insert(reason.as_str()), "duplicate: {reason}");
        }
    }

    #[test]
    fn availability_is_distinct_from_failure() {
        // The distinction the whole taxonomy exists to preserve: "not
        // installed" is a configuration problem, "validation failed" is a
        // board problem, and neither may be reported as the other.
        for reason in [
            RouterFailureReason::NotInstalled,
            RouterFailureReason::JarMissing,
            RouterFailureReason::JavaMissing,
            RouterFailureReason::CheckoutMissing,
            RouterFailureReason::PythonMissing,
        ] {
            assert!(reason.is_unavailable(), "{reason} should be unavailable");
        }
        for reason in [
            RouterFailureReason::Timeout,
            RouterFailureReason::Crashed,
            RouterFailureReason::NoOutput,
            RouterFailureReason::ValidationFailed,
            // The engine is installed and could run; it just cannot honour
            // this request. That is not an install problem.
            RouterFailureReason::Unsupported,
        ] {
            assert!(
                !reason.is_unavailable(),
                "{reason} is a failure, not absence"
            );
        }
    }

    #[test]
    fn every_reason_carries_an_actionable_remediation() {
        for engine in RouterEngine::all() {
            for reason in [
                RouterFailureReason::NotInstalled,
                RouterFailureReason::JarMissing,
                RouterFailureReason::JavaMissing,
                RouterFailureReason::CheckoutMissing,
                RouterFailureReason::PythonMissing,
                RouterFailureReason::SpawnFailed,
                RouterFailureReason::Timeout,
                RouterFailureReason::Cancelled,
                RouterFailureReason::Crashed,
                RouterFailureReason::Rejected,
                RouterFailureReason::MalformedOutput,
                RouterFailureReason::NoOutput,
                RouterFailureReason::ValidationFailed,
                RouterFailureReason::Unsupported,
                RouterFailureReason::Io,
            ] {
                let failure =
                    RouterFailure::for_stage(engine, RouterStage::Route, reason, "detail");
                assert!(
                    failure.remediation.len() > 20,
                    "{engine}/{reason} remediation is not actionable: {}",
                    failure.remediation
                );
                assert!(!failure.remediation.contains("detail"));
            }
        }
    }

    #[test]
    fn the_remediation_names_the_missing_thing_for_the_selected_engine() {
        // "install the router" is useless to someone whose FreeRouting is
        // installed but whose KRT checkout is absent, so each engine's
        // guidance names its own artifact.
        let freerouting = RouterFailure::for_stage(
            RouterEngine::Freerouting,
            RouterStage::Capability,
            RouterFailureReason::NotInstalled,
            "x",
        );
        assert!(
            freerouting.remediation.contains("freerouting-jar"),
            "{freerouting}"
        );

        let krt = RouterFailure::for_stage(
            RouterEngine::KiCadRoutingTools,
            RouterStage::Capability,
            RouterFailureReason::NotInstalled,
            "x",
        );
        assert!(
            krt.remediation.contains("kicad-routing-tools-repo"),
            "{krt}"
        );
    }

    #[test]
    fn summary_names_engine_stage_and_reason() {
        let failure = RouterFailure::for_stage(
            RouterEngine::Freerouting,
            RouterStage::Drc,
            RouterFailureReason::ValidationFailed,
            "unconnected pads remain",
        )
        .with_exit_code(Some(1));
        let summary = failure.summary();
        assert!(summary.contains("freerouting"), "{summary}");
        assert!(summary.contains("drc"), "{summary}");
        assert!(summary.contains("validation_failed"), "{summary}");
        assert!(summary.contains("unconnected pads remain"), "{summary}");
    }

    #[test]
    fn stages_order_alongside_the_pipeline() {
        let stages = [
            RouterStage::Capability,
            RouterStage::Baseline,
            RouterStage::Route,
            RouterStage::Import,
            RouterStage::Topology,
            RouterStage::Connectivity,
            RouterStage::Drc,
            RouterStage::Install,
        ];
        for pair in stages.windows(2) {
            assert!(pair[0] < pair[1], "{:?} !< {:?}", pair[0], pair[1]);
        }
    }
}
