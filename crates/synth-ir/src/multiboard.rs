// SPDX-License-Identifier: Apache-2.0

//! Multi-board co-design engine: multi-board system representation, inter-board pin mapping,
//! signal continuity checking, and cross-board diagnostic verification.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Severity};

use crate::board::{Board, ComponentId, PinId};
use crate::power_domains::{infer_power_domains, PowerDomainKind};

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
    ///
    /// Beyond the original board/component existence check this verifies that
    /// each named pin exists, that the two pins carry the same signal name,
    /// that every pin of a mapped connector appears exactly once, and that the
    /// two pins sit in compatible voltage domains. It is a structural check of
    /// the mapping, not an electrical simulation.
    #[must_use]
    pub fn validate_multiboard_system(&self) -> MultiBoardValidationResult {
        let mut matched_pins = 0;
        let mut diagnostics = Vec::new();
        let domains: Vec<_> = self.boards.iter().map(infer_power_domains).collect();
        // (board name, refdes) -> pin ids used by a mapping, for the
        // connector coverage check.
        let mut used: HashMap<(String, String), Vec<PinId>> = HashMap::new();

        for mapping in &self.interboard_mappings {
            let from_b = self
                .boards
                .iter()
                .position(|b| b.name == mapping.from_board);
            let Some(from_index) = from_b else {
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
            let to_b = self.boards.iter().position(|b| b.name == mapping.to_board);
            let Some(to_index) = to_b else {
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

            let from_board = &self.boards[from_index];
            let to_board = &self.boards[to_index];
            let from_comp = from_board
                .components
                .iter()
                .find(|c| c.refdes == mapping.from_refdes);
            let to_comp = to_board
                .components
                .iter()
                .find(|c| c.refdes == mapping.to_refdes);

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

            let from_comp = from_comp.expect("checked");
            let to_comp = to_comp.expect("checked");
            let mut mapping_ok = true;

            let from_pin = pin_index(from_comp, &mapping.from_pin);
            if from_comp.part.is_some() && from_pin.is_none() {
                diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-MULTIBOARD-002",
                        Severity::Error,
                        format!(
                            "Unknown pin '{}' on '{}.{}' in mapping",
                            mapping.from_pin, mapping.from_board, mapping.from_refdes
                        ),
                    )
                    .build(),
                );
                mapping_ok = false;
            }
            let to_pin = pin_index(to_comp, &mapping.to_pin);
            if to_comp.part.is_some() && to_pin.is_none() {
                diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-MULTIBOARD-002",
                        Severity::Error,
                        format!(
                            "Unknown pin '{}' on '{}.{}' in mapping",
                            mapping.to_pin, mapping.to_board, mapping.to_refdes
                        ),
                    )
                    .build(),
                );
                mapping_ok = false;
            }

            if let (Some(from_pin), Some(to_pin)) = (from_pin, to_pin) {
                if let (Some(from_name), Some(to_name)) =
                    (pin_name(from_comp, from_pin), pin_name(to_comp, to_pin))
                {
                    if from_name.eq_ignore_ascii_case(to_name) {
                        if let Some(reason) = domain_conflict(
                            from_board,
                            &domains[from_index],
                            from_comp.id,
                            from_pin,
                            to_board,
                            &domains[to_index],
                            to_comp.id,
                            to_pin,
                        ) {
                            diagnostics.push(
                                DiagnosticBuilder::new(
                                    "E-SYNTH-MULTIBOARD-005",
                                    Severity::Error,
                                    format!(
                                        "Mapped pins '{}.{}.{}' and '{}.{}.{}' sit in incompatible voltage domains: {reason}",
                                        mapping.from_board,
                                        mapping.from_refdes,
                                        mapping.from_pin,
                                        mapping.to_board,
                                        mapping.to_refdes,
                                        mapping.to_pin
                                    ),
                                )
                                .build(),
                            );
                            mapping_ok = false;
                        }
                    } else {
                        diagnostics.push(
                            DiagnosticBuilder::new(
                                "E-SYNTH-MULTIBOARD-003",
                                Severity::Error,
                                format!(
                                    "Mapped pins carry different signals: '{}.{}.{}' is '{from_name}' but '{}.{}.{}' is '{to_name}'",
                                    mapping.from_board,
                                    mapping.from_refdes,
                                    mapping.from_pin,
                                    mapping.to_board,
                                    mapping.to_refdes,
                                    mapping.to_pin
                                ),
                            )
                            .build(),
                        );
                        mapping_ok = false;
                    }
                }

                if from_comp
                    .part
                    .as_ref()
                    .is_some_and(|p| p.kind == "connector")
                {
                    used.entry((mapping.from_board.clone(), mapping.from_refdes.clone()))
                        .or_default()
                        .push(from_pin);
                }
                if to_comp.part.as_ref().is_some_and(|p| p.kind == "connector") {
                    used.entry((mapping.to_board.clone(), mapping.to_refdes.clone()))
                        .or_default()
                        .push(to_pin);
                }
            }

            if mapping_ok {
                matched_pins += 1;
            }
        }

        diagnostics.extend(self.coverage_diagnostics(&used));

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

    /// Every pin of a connector used by a mapping must be mapped exactly once.
    ///
    /// Only connectors are checked: a mapping may legitimately name one pin of
    /// an IC without claiming to carry the rest of it.
    fn coverage_diagnostics(
        &self,
        used: &HashMap<(String, String), Vec<PinId>>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for ((board_name, refdes), pins) in used {
            let Some(board) = self.boards.iter().find(|b| &b.name == board_name) else {
                continue;
            };
            let Some(component) = board.components.iter().find(|c| &c.refdes == refdes) else {
                continue;
            };
            let Some(part) = component.part.as_ref() else {
                continue;
            };

            let mut seen = HashSet::new();
            for pin in pins {
                if !seen.insert(*pin) {
                    let name = part
                        .pins
                        .get(pin.0 as usize)
                        .map_or_else(|| pin.0.to_string(), |p| p.number.0.clone());
                    diagnostics.push(
                        DiagnosticBuilder::new(
                            "E-SYNTH-MULTIBOARD-004",
                            Severity::Error,
                            format!(
                                "Pin {name} of '{board_name}.{refdes}' is mapped more than once"
                            ),
                        )
                        .build(),
                    );
                }
            }

            if let Some(missing) = part
                .pins
                .iter()
                .enumerate()
                .find(|(index, _)| !seen.contains(&PinId(*index as u32)))
            {
                let (_, pin) = missing;
                diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-MULTIBOARD-004",
                        Severity::Error,
                        format!(
                            "Pin {} ('{}') of '{board_name}.{refdes}' has no inter-board mapping",
                            pin.number.0, pin.name
                        ),
                    )
                    .build(),
                );
            }
        }
        diagnostics
    }
}

fn pin_index(component: &crate::board::Component, pin: &str) -> Option<PinId> {
    let part = component.part.as_ref()?;
    part.pins
        .iter()
        .position(|p| p.number.0 == pin || p.name.eq_ignore_ascii_case(pin))
        .map(|index| PinId(index as u32))
}

fn pin_name<'a>(component: &'a crate::board::Component, pin: PinId) -> Option<&'a str> {
    component
        .part
        .as_ref()?
        .pins
        .get(pin.0 as usize)
        .map(|p| p.name.as_str())
}

/// Compare the inferred power domains of the nets at the two mapped pins.
///
/// Returns a human-readable reason when the domains cannot be the same
/// signal. Unknown domains are left alone: the check refuses to guess.
fn domain_conflict(
    from_board: &Board,
    from_domains: &crate::power_domains::PowerDomainMap,
    from_component: ComponentId,
    from_pin: PinId,
    to_board: &Board,
    to_domains: &crate::power_domains::PowerDomainMap,
    to_component: ComponentId,
    to_pin: PinId,
) -> Option<String> {
    let from = net_domain(from_board, from_domains, from_component, from_pin)?;
    let to = net_domain(to_board, to_domains, to_component, to_pin)?;
    let mismatch = match (from, to) {
        (PowerDomainKind::Unknown, _) | (_, PowerDomainKind::Unknown) => false,
        (PowerDomainKind::Ground, PowerDomainKind::Ground) => false,
        (PowerDomainKind::Ground, _) | (_, PowerDomainKind::Ground) => true,
        (
            PowerDomainKind::Rail { nominal_v: a, .. },
            PowerDomainKind::Rail { nominal_v: b, .. },
        )
        | (
            PowerDomainKind::Rail { nominal_v: a, .. },
            PowerDomainKind::Signal {
                driven_voltage: Some(b),
                ..
            },
        )
        | (
            PowerDomainKind::Signal {
                driven_voltage: Some(a),
                ..
            },
            PowerDomainKind::Rail { nominal_v: b, .. },
        )
        | (
            PowerDomainKind::Signal {
                driven_voltage: Some(a),
                ..
            },
            PowerDomainKind::Signal {
                driven_voltage: Some(b),
                ..
            },
        ) => !voltage_close(*a, *b),
        _ => false,
    };
    mismatch.then(|| format!("{from:?} against {to:?}"))
}

fn net_domain<'a>(
    board: &Board,
    domains: &'a crate::power_domains::PowerDomainMap,
    component: ComponentId,
    pin: PinId,
) -> Option<&'a PowerDomainKind> {
    let (net_id, _) = board.nets_containing(component, pin).next()?;
    domains.get(net_id)
}

fn voltage_close(a: f64, b: f64) -> bool {
    let largest = a.abs().max(b.abs());
    if largest < 0.05 {
        return true;
    }
    (a - b).abs() / largest <= 0.10
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
