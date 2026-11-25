// SPDX-License-Identifier: Apache-2.0

//! Worst-case tolerance analysis for regulator feedback dividers
//! (`E-SYNTH-POWER-013`).
//!
//! An adjustable regulator sets `Vout = Vref * (1 + Rtop / Rbottom)`. The
//! nominal value can sit inside a load's supply window while resistor
//! tolerance and reference accuracy push the real output outside it. This
//! rule computes the output at the corners of those tolerances and compares
//! it with the window the loads on the output net declare.
//!
//! Every input comes from the design or the registry. The resistor
//! tolerance is the component's `Tolerance` property (`tolerance "1%"`).
//! The reference range is the regulator's `feedback_vref_min_v` and
//! `feedback_vref_max_v`. The window is each load pin's `voltage_min_v` and
//! `voltage_max_v`, or its part's `operating_conditions` limits. When any
//! input is missing the rule says nothing: it never assumes a tolerance.

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity};
use synth_ir::{Board, Component, ComponentId, Net, NetId};
use synth_registry::{ElectricalType, PinCapability};

use crate::{ErcCategory, ErcRule, deep_erc, endpoint_has_any_capability};

/// The tolerance class used in the suggested fix.
const TIGHT_TOLERANCE: f64 = 0.001;

/// A feedback divider and its reference, with tolerances as fractions
/// (`0.01` is 1 %).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Divider {
    pub vref_min_v: f64,
    pub vref_max_v: f64,
    pub r_top_ohm: f64,
    pub r_bottom_ohm: f64,
    pub tol_top: f64,
    pub tol_bottom: f64,
}

/// The output voltage range `(min, max)` over every corner of the divider.
///
/// The output is highest when the reference is high, the top resistor is
/// high and the bottom resistor is low. It is lowest in the opposite case.
#[must_use]
pub fn worst_case_vout(d: &Divider) -> (f64, f64) {
    let low = d.vref_min_v
        * (1.0 + d.r_top_ohm * (1.0 - d.tol_top) / (d.r_bottom_ohm * (1.0 + d.tol_bottom)));
    let high = d.vref_max_v
        * (1.0 + d.r_top_ohm * (1.0 + d.tol_top) / (d.r_bottom_ohm * (1.0 - d.tol_bottom)));
    (low, high)
}

/// Parse a percent tolerance such as `1%`, `0.1 %` or `±5%` into a fraction.
/// Anything without a percent sign, or not in (0, 100), is `None`.
#[must_use]
pub fn parse_tolerance(input: &str) -> Option<f64> {
    let s = input.trim().trim_start_matches(['±', '+']).trim();
    let number = s.strip_suffix('%')?.trim();
    let pct: f64 = number.parse().ok()?;
    (pct > 0.0 && pct < 100.0).then_some(pct / 100.0)
}

pub(crate) struct FeedbackDividerToleranceRule;

/// One resistor of the divider.
struct Leg {
    ohms: f64,
    tol: f64,
    far_net: NetId,
}

fn leg(board: &Board, component: &Component, from: synth_ir::PinId) -> Option<Leg> {
    let far_net = deep_erc::other_pin_net(board, component, from)?;
    let ohms = component
        .value
        .as_deref()
        .and_then(crate::value::parse_resistance)?;
    let tol = component
        .properties
        .get("Tolerance")
        .and_then(|t| parse_tolerance(t))?;
    (ohms > 0.0).then_some(Leg { ohms, tol, far_net })
}

/// Exactly one resistor to ground and one to the output, `(top, bottom)`.
/// Any other resistor on the net is not a two-value divider.
fn divider_legs(board: &Board, fb_net: &Net) -> Option<(Leg, Leg)> {
    let mut top: Option<Leg> = None;
    let mut bottom: Option<Leg> = None;
    for ep in &fb_net.endpoints {
        let Some(r) = board.component(ep.component) else {
            continue;
        };
        if !r
            .part
            .as_ref()
            .is_some_and(|p| p.kind == "resistor" && p.pins.len() == 2)
        {
            continue;
        }
        let l = leg(board, r, ep.pin)?;
        let to_ground = board
            .net(l.far_net)
            .is_some_and(|n| crate::is_ground_net(board, n));
        let slot = if to_ground { &mut bottom } else { &mut top };
        if slot.replace(l).is_some() {
            return None;
        }
    }
    Some((top?, bottom?))
}

/// The window `(vmin, vmax)` the loads on `net` allow, tightest bound on
/// each side. `skip` is the regulator itself.
fn load_window(board: &Board, net: &Net, skip: ComponentId) -> (Option<f64>, Option<f64>) {
    let mut vmin: Option<f64> = None;
    let mut vmax: Option<f64> = None;
    for ep in &net.endpoints {
        if ep.component == skip {
            continue;
        }
        let (Some(component), Some(pin)) = (
            board.component(ep.component),
            board.pin(ep.component, ep.pin),
        ) else {
            continue;
        };
        if pin.electrical_type != ElectricalType::PowerInput {
            continue;
        }
        let oc = component
            .part
            .as_ref()
            .and_then(|p| p.operating_conditions.as_ref());
        if let Some(lo) = pin
            .voltage_min_v
            .or_else(|| oc.and_then(|o| o.min_voltage_v))
        {
            vmin = Some(vmin.map_or(lo, |v| v.max(lo)));
        }
        if let Some(hi) = pin
            .voltage_max_v
            .or_else(|| oc.and_then(|o| o.max_voltage_v))
        {
            vmax = Some(vmax.map_or(hi, |v| v.min(hi)));
        }
    }
    (vmin, vmax)
}

fn outside(range: (f64, f64), window: (Option<f64>, Option<f64>)) -> bool {
    window.0.is_some_and(|lo| range.0 < lo) || window.1.is_some_and(|hi| range.1 > hi)
}

fn show_window(window: (Option<f64>, Option<f64>)) -> String {
    match window {
        (Some(lo), Some(hi)) => format!("{lo:.4} V to {hi:.4} V"),
        (Some(lo), None) => format!("at least {lo:.4} V"),
        (None, Some(hi)) => format!("at most {hi:.4} V"),
        (None, None) => "unbounded".to_string(),
    }
}

impl ErcRule for FeedbackDividerToleranceRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-POWER-013"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for fb_net in &board.nets {
            for fb in fb_net.endpoints.iter().filter(|e| {
                endpoint_has_any_capability(board, e.component, e.pin, &[PinCapability::Feedback])
            }) {
                let Some(regulator) = board.component(fb.component) else {
                    continue;
                };
                let Some((vref_min, vref_max)) = regulator
                    .part
                    .as_ref()
                    .and_then(|p| p.operating_conditions.as_ref())
                    .and_then(|o| Some((o.feedback_vref_min_v?, o.feedback_vref_max_v?)))
                else {
                    continue;
                };
                if !(vref_min > 0.0 && vref_max >= vref_min) {
                    continue;
                }
                let Some((top, bottom)) = divider_legs(board, fb_net) else {
                    continue;
                };
                let Some(out_net) = board.net(top.far_net) else {
                    continue;
                };
                let window = load_window(board, out_net, fb.component);
                if window == (None, None) {
                    continue;
                }
                let divider = Divider {
                    vref_min_v: vref_min,
                    vref_max_v: vref_max,
                    r_top_ohm: top.ohms,
                    r_bottom_ohm: bottom.ohms,
                    tol_top: top.tol,
                    tol_bottom: bottom.tol,
                };
                let range = worst_case_vout(&divider);
                if !outside(range, window) {
                    continue;
                }
                let tight = worst_case_vout(&Divider {
                    tol_top: TIGHT_TOLERANCE,
                    tol_bottom: TIGHT_TOLERANCE,
                    ..divider
                });
                let advice = if outside(tight, window) {
                    format!(
                        "0.1 % resistors still give {:.4} V to {:.4} V; the reference accuracy or the window must change",
                        tight.0, tight.1
                    )
                } else {
                    format!(
                        "0.1 % resistors give {:.4} V to {:.4} V, inside the window",
                        tight.0, tight.1
                    )
                };
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "regulator output can leave the load's supply window at worst case",
                    )
                    .location(Location::from_span(file.to_string(), fb.source_span))
                    .expected(format!(
                        "worst-case output of net `{}` inside {}",
                        out_net.name,
                        show_window(window)
                    ))
                    .found(format!(
                        "divider on {} gives {:.4} V to {:.4} V ({:.2} % top, {:.2} % bottom resistor, reference {:.4} V to {:.4} V); {advice}",
                        regulator.describe_pin(&board.pin(fb.component, fb.pin).map_or_default(|p| p.name.clone())),
                        range.0,
                        range.1,
                        top.tol * 100.0,
                        bottom.tol * 100.0,
                        vref_min,
                        vref_max,
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn div(tol: f64) -> Divider {
        Divider {
            vref_min_v: 0.796,
            vref_max_v: 0.804,
            r_top_ohm: 10_000.0,
            r_bottom_ohm: 20_000.0,
            tol_top: tol,
            tol_bottom: tol,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-5
    }

    // Vref 0.8 V +/-0.5 %, Rtop 10k, Rbottom 20k, nominal 0.8 * 1.5 = 1.2 V.
    // 1 %: max = 0.804 * (1 + 10.1k / 19.8k) = 0.804 * 1.510101 = 1.214121 V
    //      min = 0.796 * (1 + 9.9k / 20.2k)  = 0.796 * 1.490099 = 1.186118 V
    #[test]
    fn one_percent_corners_match_hand_arithmetic() {
        let (lo, hi) = worst_case_vout(&div(0.01));
        assert!(close(lo, 1.186_118), "{lo}");
        assert!(close(hi, 1.214_121), "{hi}");
    }

    // 0.1 %: max = 0.804 * (1 + 10.01k / 19.98k) = 0.804 * 1.501001 = 1.206805 V
    //        min = 0.796 * (1 + 9.99k / 20.02k)  = 0.796 * 1.499001 = 1.193205 V
    #[test]
    fn tenth_percent_corners_match_hand_arithmetic() {
        let (lo, hi) = worst_case_vout(&div(0.001));
        assert!(close(lo, 1.193_205), "{lo}");
        assert!(close(hi, 1.206_805), "{hi}");
    }

    #[test]
    fn tolerance_needs_a_percent_sign() {
        assert_eq!(parse_tolerance("1%"), Some(0.01));
        assert_eq!(parse_tolerance("±0.1 %"), Some(0.001));
        assert_eq!(parse_tolerance("1"), None);
        assert_eq!(parse_tolerance("0%"), None);
        assert_eq!(parse_tolerance("abc%"), None);
    }
}
