// SPDX-License-Identifier: Apache-2.0

//! Electromagnetic-compatibility placement rules.
//!
//! Geometry `kicad-cli` does not judge: how far a noise-sensitive part is from
//! what it talks to, and whether a switching converter's input loop is closed.
//! Advisory warnings, evaluated on the placement the export chose, so the
//! candidate selection prefers a placement that satisfies them.

use synth_geometry::{nm_to_mm, Point};
use synth_ir::{Board, Component, ComponentId};
use synth_place::Placement;

use crate::Violation;

/// A crystal or oscillator beyond this from the part it drives is far enough
/// that its loop is long and its edges radiate. Millimetres.
const CRYSTAL_MAX_MM: f64 = 20.0;

/// A switching converter's input capacitor should sit within this of it, so the
/// hot loop (input cap -> switch -> ground) is small. Millimetres.
const SWITCHING_INPUT_CAP_MAX_MM: f64 = 12.0;

/// Whether a component is a crystal or oscillator.
fn is_crystal(component: &Component) -> bool {
    let id = component
        .part
        .as_ref()
        .map(|part| part.id.0.to_ascii_lowercase())
        .unwrap_or_default();
    component.kind.as_str() == "crystal"
        || component.kind.as_str() == "oscillator"
        || id.contains("xtal")
        || id.contains("crystal")
        || id.contains("osc")
}

/// Whether a component is a switching converter (buck/boost/DC-DC).
fn is_switching_converter(component: &Component) -> bool {
    let id = component
        .part
        .as_ref()
        .map(|part| part.id.0.to_ascii_lowercase())
        .unwrap_or_default();
    id.contains("buck") || id.contains("boost") || id.contains("dcdc") || id.contains("dc_dc")
}

fn is_capacitor(component: &Component) -> bool {
    component.kind.as_str() == "capacitor"
}

fn position(placement: &Placement, id: ComponentId) -> Option<Point> {
    placement
        .components
        .iter()
        .find(|placed| placed.id == id)
        .map(|placed| placed.center)
}

/// Chebyshev distance in nanometres — the larger axis gap, so a part is far
/// only when it is far on both axes in the easy sense.
fn distance_nm(a: Point, b: Point) -> i64 {
    (a.x_nm - b.x_nm).abs().max((a.y_nm - b.y_nm).abs())
}

fn refdes(board: &Board, id: ComponentId) -> String {
    board.component(id).map_or_else(
        || format!("U{}", id.0),
        |component| component.refdes.clone(),
    )
}

/// EMC placement findings. Code: `W-SYNTH-DRC-021`.
///
/// Two checks: a crystal/oscillator further than [`CRYSTAL_MAX_MM`] from the
/// part it connects to, and a switching converter with no input capacitor
/// within [`SWITCHING_INPUT_CAP_MAX_MM`] on any of its nets. Advisory, and
/// silent on a board that has neither part.
#[must_use]
pub fn check_emc_placement(board: &Board, placement: &Placement) -> Vec<Violation> {
    let mut out = Vec::new();

    for component in &board.components {
        let Some(here) = position(placement, component.id) else {
            continue;
        };

        if is_crystal(component) {
            let nearest = nearest_partner(board, placement, component.id);
            if let Some((distance_mm, other)) = nearest {
                if distance_mm > CRYSTAL_MAX_MM {
                    out.push(Violation {
                        code: "W-SYNTH-DRC-021".to_string(),
                        message: format!(
                            "crystal/oscillator {} is {distance_mm:.1} mm from {}, beyond the \
                             {CRYSTAL_MAX_MM:.0} mm at which its loop stops being compact",
                            component.refdes, other
                        ),
                        witness: vec![here],
                        nets: Vec::new(),
                        components: vec![component.refdes.clone(), other],
                        pos_mm: Some((nm_to_mm(here.x_nm), nm_to_mm(here.y_nm))),
                        suggested_override: None,
                    });
                }
            }
        }

        if is_switching_converter(component) {
            let has_near_input_cap = nearest_input_cap(board, placement, component.id)
                .is_some_and(|distance_mm| distance_mm <= SWITCHING_INPUT_CAP_MAX_MM);
            if !has_near_input_cap {
                out.push(Violation {
                    code: "W-SYNTH-DRC-021".to_string(),
                    message: format!(
                        "switching converter {} has no input capacitor within \
                         {SWITCHING_INPUT_CAP_MAX_MM:.0} mm, so its hot loop is large and \
                         radiates",
                        component.refdes
                    ),
                    witness: vec![here],
                    nets: Vec::new(),
                    components: vec![component.refdes.clone()],
                    pos_mm: Some((nm_to_mm(here.x_nm), nm_to_mm(here.y_nm))),
                    suggested_override: None,
                });
            }
        }
    }
    out
}

/// The nearest component sharing a net with `id`, as `(distance_mm, refdes)`.
fn nearest_partner(board: &Board, placement: &Placement, id: ComponentId) -> Option<(f64, String)> {
    let here = position(placement, id)?;
    let mut nearest: Option<(f64, String)> = None;
    for net in &board.nets {
        if !net
            .endpoints
            .iter()
            .any(|endpoint| endpoint.component == id)
        {
            continue;
        }
        for endpoint in &net.endpoints {
            if endpoint.component == id {
                continue;
            }
            let Some(there) = position(placement, endpoint.component) else {
                continue;
            };
            let distance_mm = nm_to_mm(distance_nm(here, there));
            if nearest.as_ref().is_none_or(|(best, _)| distance_mm < *best) {
                nearest = Some((distance_mm, refdes(board, endpoint.component)));
            }
        }
    }
    nearest
}

/// The nearest capacitor sharing a net with `id`, as a distance in mm.
fn nearest_input_cap(board: &Board, placement: &Placement, id: ComponentId) -> Option<f64> {
    let here = position(placement, id)?;
    let mut nearest: Option<f64> = None;
    for net in &board.nets {
        if !net
            .endpoints
            .iter()
            .any(|endpoint| endpoint.component == id)
        {
            continue;
        }
        for endpoint in &net.endpoints {
            if endpoint.component == id {
                continue;
            }
            let Some(other) = board.component(endpoint.component) else {
                continue;
            };
            if !is_capacitor(other) {
                continue;
            }
            let Some(there) = position(placement, endpoint.component) else {
                continue;
            };
            let distance_mm = nm_to_mm(distance_nm(here, there));
            if nearest.is_none_or(|best| distance_mm < best) {
                nearest = Some(distance_mm);
            }
        }
    }
    nearest
}
