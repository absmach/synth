// SPDX-License-Identifier: Apache-2.0

//! Reference agent test harness.
//!
//! Simulates closed-loop agent repair iterations:
//! 1. Evaluates validation diagnostics on broken source files.
//! 2. Selects top patch based on selected strategy (`ConsequenceModel` vs `ConfidenceOnly`).
//! 3. Applies patch in reverse byte order.
//! 4. Repeats until clean or `max_iterations` reached.

use crate::{Diagnostic, Patch, PatchKind};

/// Strategy used by the agent harness to select patches for repair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HarnessStrategy {
    /// Selects patch using Patch-Consequence model confidence and predicted diagnostic reduction.
    #[default]
    ConsequenceModel,
    /// Selects patch strictly by raw patch confidence score.
    ConfidenceOnly,
}

/// Result of a single closed-loop agent repair simulation run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessRunResult {
    /// Whether the design converged to a clean (0 blocking errors) state.
    pub converged: bool,
    /// Number of repair iterations executed.
    pub iterations: usize,
    /// Number of diagnostic errors present initially.
    pub initial_diagnostics: usize,
    /// Number of diagnostic errors remaining at termination.
    pub final_diagnostics: usize,
}

/// Reference agent test harness.
#[derive(Debug, Clone)]
pub struct AgentHarness {
    pub max_iterations: usize,
    pub strategy: HarnessStrategy,
}

impl Default for AgentHarness {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            strategy: HarnessStrategy::ConsequenceModel,
        }
    }
}

impl AgentHarness {
    pub fn new(max_iterations: usize, strategy: HarnessStrategy) -> Self {
        Self {
            max_iterations,
            strategy,
        }
    }

    /// Execute a simulated repair loop on `source` using `validator` function to emit diagnostics.
    pub fn run<F>(&self, source: &str, validator: F) -> HarnessRunResult
    where
        F: Fn(&str) -> Vec<Diagnostic>,
    {
        let mut current = source.to_string();
        let initial_diags = validator(&current);
        let initial_errors = initial_diags
            .iter()
            .filter(|d| d.severity.is_blocking())
            .count();

        if initial_errors == 0 {
            return HarnessRunResult {
                converged: true,
                iterations: 0,
                initial_diagnostics: 0,
                final_diagnostics: 0,
            };
        }

        let mut iterations = 0;

        while iterations < self.max_iterations {
            let diags = validator(&current);
            let blocking_diags: Vec<_> =
                diags.iter().filter(|d| d.severity.is_blocking()).collect();

            if blocking_diags.is_empty() {
                return HarnessRunResult {
                    converged: true,
                    iterations,
                    initial_diagnostics: initial_errors,
                    final_diagnostics: 0,
                };
            }

            // Collect available patches
            let mut patches: Vec<Patch> = blocking_diags
                .iter()
                .filter_map(|d| d.suggested_fixes.first().cloned())
                .collect();

            if patches.is_empty() {
                // No patches available to progress
                return HarnessRunResult {
                    converged: false,
                    iterations,
                    initial_diagnostics: initial_errors,
                    final_diagnostics: blocking_diags.len(),
                };
            }

            // Rank patches by strategy
            match self.strategy {
                HarnessStrategy::ConsequenceModel => {
                    patches.sort_by(|a, b| {
                        let score_a = a
                            .patch_consequence_preview
                            .as_ref()
                            .map_or(a.confidence, |p| p.model_confidence);
                        let score_b = b
                            .patch_consequence_preview
                            .as_ref()
                            .map_or(b.confidence, |p| p.model_confidence);
                        score_b
                            .partial_cmp(&score_a)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
                HarnessStrategy::ConfidenceOnly => {
                    patches.sort_by(|a, b| {
                        b.confidence
                            .partial_cmp(&a.confidence)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
            }

            // Reverse byte order for multi-patch application
            patches.sort_by_key(|p| std::cmp::Reverse(patch_anchor_offset(p)));

            let mut applied_any = false;
            for patch in &patches {
                if let Ok(next) = patch.apply(&current) {
                    if next != current {
                        current = next;
                        applied_any = true;
                    }
                }
            }

            iterations += 1;

            if !applied_any {
                break;
            }
        }

        let final_diags = validator(&current);
        let final_errors = final_diags
            .iter()
            .filter(|d| d.severity.is_blocking())
            .count();

        HarnessRunResult {
            converged: final_errors == 0,
            iterations,
            initial_diagnostics: initial_errors,
            final_diagnostics: final_errors,
        }
    }
}

fn patch_anchor_offset(p: &Patch) -> u32 {
    match &p.kind {
        PatchKind::ReplaceRange { range, .. } | PatchKind::DeleteRange { range } => {
            range.byte_start
        }
        PatchKind::InsertAt { at, .. } => *at,
        PatchKind::AddStatement { .. }
        | PatchKind::RemoveStatement { .. }
        | PatchKind::SolveSmt { .. } => u32::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Severity;

    #[test]
    fn harness_runs_to_convergence() {
        let harness = AgentHarness::default();
        let initial_src = "board \"foo\"";

        let result = harness.run(initial_src, |src| {
            if src.contains('{') && src.contains('}') {
                vec![]
            } else {
                vec![crate::DiagnosticBuilder::new(
                    "E-SYNTH-PARSE-003",
                    Severity::Error,
                    "expected {",
                )
                .suggested_fix(Patch {
                    confidence: 0.9,
                    rationale: None,
                    patch_consequence_preview: None,
                    kind: PatchKind::InsertAt {
                        at: src.len() as u32,
                        text: " {}\n".into(),
                    },
                })
                .build()]
            }
        });

        assert!(result.converged);
        assert_eq!(result.iterations, 1);
        assert_eq!(result.initial_diagnostics, 1);
        assert_eq!(result.final_diagnostics, 0);
    }
}
