// SPDX-License-Identifier: Apache-2.0

//! `E-SYNTH-MODULE-009`: every recorded deviation from a reference module,
//! listed in one place.
//!
//! A reference module is copied 1:1 where it matters; a change to a value
//! the module fixes is an explicit `deviation "reason"` on the `use`. This
//! rule gathers every one of them into a single info diagnostic, so a
//! reviewer reads the whole list of exceptions at once instead of hunting
//! through the source.

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity};
use synth_ir::Board;

use crate::{ErcCategory, ErcRule};

pub(crate) struct ModuleDeviationsRule;

impl ErcRule for ModuleDeviationsRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-MODULE-009"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Board
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut lines = Vec::new();
        let mut first = None;
        for module in &board.modules {
            for d in &module.deviations {
                first.get_or_insert(d.span);
                let what = match (&d.param, &d.value) {
                    (Some(param), Some(value)) => format!("`{param}` = {value}"),
                    (Some(param), None) => format!("`{param}`"),
                    _ => "the instance".to_string(),
                };
                lines.push(format!(
                    "{} ({}): {what}: {}",
                    d.instance, module.name, d.reason
                ));
            }
        }
        let Some(span) = first else {
            return Vec::new();
        };
        vec![
            DiagnosticBuilder::new(
                self.code(),
                Severity::Info,
                format!(
                    "{} recorded deviation(s) from reference modules",
                    lines.len()
                ),
            )
            .location(Location::from_span(file.to_string(), span))
            .message(lines.join("\n"))
            .found(lines.join("; "))
            .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
            .build(),
        ]
    }
}
