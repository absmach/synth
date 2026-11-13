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
use synth_ir::{Board, ComponentId, Net, PinId};

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
            let Some(theta) = operating.thermal_resistance_c_per_w else {
                continue;
            };
            let power_mw = if supplies_a_rail(component) {
                // A regulator's `max_power_mw` is the package rating, not what
                // this board makes it dissipate. Estimate the real figure from
                // the drop across the device and the current its rail draws.
                // Without a declared input/output rail and load, skip rather
                // than reporting the rating as if it were dissipation.
                let Some(power_mw) = regulator_dissipation_mw(board, component) else {
                    // The package rating is not this board's dissipation, so
                    // reporting it would be wrong. But silently skipping the
                    // part hides that the check never ran: say so, as an
                    // unverified claim rather than a temperature.
                    out.push(
                        DiagnosticBuilder::new(
                            "W-SYNTH-THERMAL-002",
                            Severity::Warning,
                            "junction temperature was not estimated",
                        )
                        .location(Location::from_span(file.to_string(), component.source_span))
                        .expected(
                            "a declared input rail, output rail and load current, so the \
                             regulator's own dissipation can be estimated",
                        )
                        .found(format!(
                            "{} supplies a rail, but its input/output voltage or the current on \
                             its output rail is not declared",
                            component.refdes
                        ))
                        .message(format!(
                            "{} is a rail supply, so its `max_power_mw` is the package rating, \
                             not the board's dissipation. Synth did not estimate a junction \
                             temperature for it. Declare the rail voltages and load current, or \
                             review the part's thermal behaviour by hand.",
                            component.refdes
                        ))
                        .explanation_url("synth.docs/diagnostics/W-SYNTH-THERMAL-002")
                        .build(),
                    );
                    continue;
                };
                power_mw
            } else {
                let Some(power_mw) = operating.max_power_mw else {
                    continue;
                };
                power_mw
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

/// Whether `max_current_ma` is a budget this part spends on a rail rather
/// than a draw it consumes. Mirrors `PowerBudgetRule`'s classification.
fn supplies_a_rail(component: &synth_ir::Component) -> bool {
    let Some(part) = component.part.as_ref() else {
        return false;
    };
    let Some(operating) = part.operating_conditions.as_ref() else {
        return false;
    };
    if operating.max_current_ma.is_none() {
        return false;
    }
    if let Some(budget) = operating.power_budget {
        if !budget {
            return false;
        }
    }
    part.pins
        .iter()
        .any(|p| p.electrical_type == synth_registry::ElectricalType::PowerOutput)
}

/// The net carrying a pin of `component` with the given electrical role.
fn pin_net(
    board: &Board,
    component: ComponentId,
    role: synth_registry::ElectricalType,
) -> Option<&Net> {
    let part = board.component(component)?.part.as_ref()?;
    let pin = part.pins.iter().position(|p| p.electrical_type == role)?;
    let pin = PinId(pin as u32);
    board.nets.iter().find(|net| {
        net.endpoints
            .iter()
            .any(|e| e.component == component && e.pin == pin)
    })
}

/// `(Vin - Vout) * Iload` for a regulator, using the declared rail voltages and
/// the summed current of the distinct loads on its output rail.
fn regulator_dissipation_mw(board: &Board, regulator: &synth_ir::Component) -> Option<f64> {
    let part = regulator.part.as_ref()?;
    // The first `power_input` pin is often ground; take the highest declared
    // supply voltage instead.
    let vin = part
        .pins
        .iter()
        .enumerate()
        .filter(|(_, pin)| pin.electrical_type == synth_registry::ElectricalType::PowerInput)
        .filter(|(_, pin)| {
            !matches!(
                pin.name.to_lowercase().as_str(),
                "gnd" | "vss" | "vssa" | "vee" | "agnd" | "dgnd" | "vneg"
            )
        })
        .filter_map(|(index, _)| {
            board
                .nets_containing(regulator.id, PinId(index as u32))
                .next()
                .and_then(|(_, net)| net.voltage)
                .map(synth_ir::Voltage::to_v)
        })
        .reduce(f64::max)?;
    let output = pin_net(
        board,
        regulator.id,
        synth_registry::ElectricalType::PowerOutput,
    )?;
    let vout = output.voltage?.to_v();
    let mut loads = std::collections::BTreeSet::new();
    let mut load_ma = 0.0;
    for endpoint in &output.endpoints {
        if endpoint.component == regulator.id || !loads.insert(endpoint.component) {
            continue;
        }
        let Some(load) = board.component(endpoint.component) else {
            continue;
        };
        let Some(part) = load.part.as_ref() else {
            continue;
        };
        if matches!(part.kind.as_str(), "capacitor" | "connector" | "jack") {
            continue;
        }
        if let Some(draw) = part
            .operating_conditions
            .as_ref()
            .and_then(|c| c.max_current_ma)
        {
            load_ma += draw;
        }
    }
    if load_ma <= 0.0 {
        return None;
    }
    Some((vin - vout).max(0.0) * load_ma)
}
