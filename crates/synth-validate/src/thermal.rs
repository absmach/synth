// SPDX-License-Identifier: Apache-2.0

//! Component thermal check.
//!
//! A part that declares both its dissipation (`max_power_mw`) and its
//! junction-to-ambient resistance (`thermal_resistance_c_per_w`) gets a
//! junction-temperature estimate: `Tj = ambient + P · Rθja`. This is the same
//! physics the `synth_evaluate_thermal_si` oracle computes, moved into the
//! compiler so it runs in `synth validate` / `synth check` rather than only
//! when an agent asks for it. A part that declares neither is not checked, so
//! adding the fields is what opts a part into the gate.

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity};
use synth_ir::Board;

use crate::{ErcCategory, ErcRule};

/// Assumed ambient temperature, in degrees Celsius.
const AMBIENT_C: f64 = 25.0;
/// Junction temperature, in degrees Celsius, above which a warning fires.
const WARN_C: f64 = 85.0;

pub(crate) struct ComponentThermalRule;

impl ErcRule for ComponentThermalRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-THERMAL-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Board
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let Some(operating) = component
                .part
                .as_ref()
                .and_then(|part| part.operating_conditions.as_ref())
            else {
                continue;
            };
            let (Some(power_mw), Some(theta)) =
                (operating.max_power_mw, operating.thermal_resistance_c_per_w)
            else {
                continue;
            };
            let junction_c = AMBIENT_C + (power_mw / 1000.0) * theta;
            if junction_c <= WARN_C {
                continue;
            }
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Warning,
                    "component junction temperature is high",
                )
                .location(Location::from_span(file.to_string(), component.source_span))
                .expected(format!(
                    "a junction temperature at or below {WARN_C:.0} C in {AMBIENT_C:.0} C ambient"
                ))
                .found(format!(
                    "{} reaches about {junction_c:.0} C ({power_mw:.0} mW over {theta:.0} C/W)",
                    component.refdes
                ))
                .message(format!(
                    "{} dissipates {power_mw:.0} mW through {theta:.0} C/W, so its junction sits \
                     near {junction_c:.0} C. An estimate from declared figures, not a simulation.",
                    component.refdes
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        out
    }
}
