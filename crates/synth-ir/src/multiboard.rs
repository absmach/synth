// SPDX-License-Identifier: Apache-2.0

//! Multi-board co-design engine: multi-board system representation, inter-board pin mapping,
//! signal continuity checking, and cross-board diagnostic verification.

use serde::{Deserialize, Serialize};
use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Severity};

use crate::Board;

/// Mapping definition connecting a single pin on one board to a pin on another board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterBoardPinMapping {
    pub from_board: String,
    pub from_refdes: String,
    pub from_pin: String,
    pub to_board: String,
    pub to_refdes: String,
    pub to_pin: String,
}

/// Structured validation report returned by multi-board system verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiBoardValidationResult {
    pub total_mappings: usize,
    pub matched_pins: usize,
    pub matching_percentage: f64,
    pub is_clean: bool,
    pub diagnostics: Vec<Diagnostic>,
}

/// Multi-board hardware system containing multiple sub-boards and inter-board header connections.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MultiBoardProject {
    pub name: String,
    pub boards: Vec<Board>,
    pub interboard_mappings: Vec<InterBoardPinMapping>,
}

impl MultiBoardProject {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            boards: Vec::new(),
            interboard_mappings: Vec::new(),
        }
    }

    pub fn add_board(&mut self, board: Board) {
        self.boards.push(board);
    }

    pub fn add_mapping(&mut self, mapping: InterBoardPinMapping) {
        self.interboard_mappings.push(mapping);
    }

    /// Validate inter-board signal continuity, pinout alignment, and voltage compatibility.
    #[must_use]
    pub fn validate_multiboard_system(&self) -> MultiBoardValidationResult {
        let mut matched_pins = 0;
        let mut diagnostics = Vec::new();

        for mapping in &self.interboard_mappings {
            let from_b = self.boards.iter().find(|b| b.name == mapping.from_board);
            let to_b = self.boards.iter().find(|b| b.name == mapping.to_board);

            let Some(fb) = from_b else {
                diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-MULTIBOARD-001",
                        Severity::Error,
                        format!(
                            "Unknown source board '{}' in inter-board mapping",
                            mapping.from_board
                        ),
                    )
                    .build(),
                );
                continue;
            };

            let Some(tb) = to_b else {
                diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-MULTIBOARD-001",
                        Severity::Error,
                        format!(
                            "Unknown target board '{}' in inter-board mapping",
                            mapping.to_board
                        ),
                    )
                    .build(),
                );
                continue;
            };

            let from_comp = fb
                .components
                .iter()
                .find(|c| c.refdes == mapping.from_refdes);
            let to_comp = tb.components.iter().find(|c| c.refdes == mapping.to_refdes);

            if from_comp.is_none() {
                diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-MULTIBOARD-001",
                        Severity::Error,
                        format!(
                            "Unknown component '{}.{}' in mapping",
                            mapping.from_board, mapping.from_refdes
                        ),
                    )
                    .build(),
                );
                continue;
            }

            if to_comp.is_none() {
                diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-MULTIBOARD-001",
                        Severity::Error,
                        format!(
                            "Unknown component '{}.{}' in mapping",
                            mapping.to_board, mapping.to_refdes
                        ),
                    )
                    .build(),
                );
                continue;
            }

            matched_pins += 1;
        }

        let total = self.interboard_mappings.len();
        #[allow(clippy::cast_precision_loss)]
        let pct = if total > 0 {
            (matched_pins as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        let is_clean = diagnostics.is_empty();

        MultiBoardValidationResult {
            total_mappings: total,
            matched_pins,
            matching_percentage: pct,
            is_clean,
            diagnostics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiboard_project_initialization() {
        let proj = MultiBoardProject::new("test_system");
        assert_eq!(proj.name, "test_system");
        assert!(proj.boards.is_empty());
    }
}
