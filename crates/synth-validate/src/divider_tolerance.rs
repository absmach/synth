// SPDX-License-Identifier: Apache-2.0

//! Worst-case tolerance analysis for regulator feedback dividers.
//!
//! An adjustable regulator sets `Vout = Vref * (1 + Rtop / Rbottom)`. The
//! nominal value can sit inside a load's supply window while resistor
//! tolerance and reference accuracy push the real output outside it. This
//! module computes the output at the corners of those tolerances and compares
//! it with the window the loads on the output net declare.
//!
//! Three codes come out of it:
//!
//! - `E-SYNTH-POWER-013` (error): the worst case leaves the load window.
//! - `E-SYNTH-POWER-014` (warning): the worst case stays inside the window
//!   but closer to an edge than the margin threshold.
//! - `E-SYNTH-POWER-015` (warning): a divider exists but an input the
//!   analysis needs is missing, so the margin is unproven. The analysis never
//!   assumes a tolerance or a reference range; missing evidence is reported,
//!   not passed.
//!
//! Every input comes from the design or the registry:
//!
//! - Resistor tolerance: the `tolerance "1%"` property, a percent token in
//!   the value string (`value "10k 1%"`), or the tolerance letter of a Yageo
//!   or Panasonic chip-resistor MPN.
//! - Resistor drift: `tempco "100ppm"` on the resistor, applied over the
//!   regulator's `temp_range "-40..85"` when both are declared.
//! - Reference: the regulator's `feedback_vref_min_v`/`feedback_vref_max_v`,
//!   plus `feedback_leakage_max_a` (FB pin current through Rtop) and
//!   `load_regulation_pct`/`line_regulation_pct` when the part declares them.
//! - Window: each load pin's `voltage_min_v`/`voltage_max_v`, or its part's
//!   `operating_conditions` limits.

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity};
use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, NetId};
use synth_registry::{ElectricalType, OperatingConditions, PinCapability};

use crate::{ErcCategory, ErcRule, deep_erc, endpoint_has_any_capability};

/// Tolerance classes tried, loosest first, when suggesting a fix.
const TOLERANCE_CLASSES: [f64; 6] = [0.05, 0.01, 0.005, 0.0025, 0.001, 0.0005];

/// Default margin threshold for `E-SYNTH-POWER-014`, as a fraction of the
/// nominal output. A regulator overrides it with `vout_margin "1%"`.
pub const DEFAULT_MARGIN: f64 = 0.005;

/// Reference temperature resistor tolerances are quoted at, in °C.
const REFERENCE_TEMP_C: f64 = 25.0;

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

/// Effects beyond the divider corners, each one optional in the data.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Extras {
    /// Largest FB pin current magnitude, in amps. It flows through Rtop in
    /// either direction, shifting the output by up to `I * Rtop`.
    pub fb_leakage_a: f64,
    /// Load plus line regulation, as a fraction of the output.
    pub regulation: f64,
}

/// The output voltage range `(min, max)` over every corner of the divider.
///
/// The output is highest when the reference is high, the top resistor is
/// high and the bottom resistor is low. It is lowest in the opposite case.
#[must_use]
pub fn worst_case_vout(d: &Divider) -> (f64, f64) {
    worst_case_vout_with(d, &Extras::default())
}

/// [`worst_case_vout`] with FB leakage and regulation folded in.
#[must_use]
pub fn worst_case_vout_with(d: &Divider, x: &Extras) -> (f64, f64) {
    let top_lo = d.r_top_ohm * (1.0 - d.tol_top);
    let top_hi = d.r_top_ohm * (1.0 + d.tol_top);
    let low = d.vref_min_v * (1.0 + top_lo / (d.r_bottom_ohm * (1.0 + d.tol_bottom)))
        - x.fb_leakage_a * top_hi;
    let high = d.vref_max_v * (1.0 + top_hi / (d.r_bottom_ohm * (1.0 - d.tol_bottom)))
        + x.fb_leakage_a * top_hi;
    (low * (1.0 - x.regulation), high * (1.0 + x.regulation))
}

/// The nominal output, at the midpoint of the reference range.
#[must_use]
pub fn nominal_vout(d: &Divider) -> f64 {
    f64::midpoint(d.vref_min_v, d.vref_max_v) * (1.0 + d.r_top_ohm / d.r_bottom_ohm)
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

/// Split a value string such as `10k 1%` into the resistance part and an
/// optional tolerance. A value with no percent token returns itself and
/// `None`.
#[must_use]
pub fn split_value_tolerance(value: &str) -> (String, Option<f64>) {
    let mut kept = Vec::new();
    let mut tol = None;
    let tokens: Vec<&str> = value.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        // `1 %` splits the number from its sign.
        if tokens.get(i + 1) == Some(&"%")
            && let Some(t) = parse_tolerance(&format!("{token}%"))
        {
            tol = Some(t);
            i += 2;
            continue;
        }
        if let Some(t) = parse_tolerance(token) {
            tol = Some(t);
        } else {
            kept.push(token);
        }
        i += 1;
    }
    (kept.join(" "), tol)
}

/// The tolerance a chip-resistor MPN encodes, for the series whose code
/// letter is documented: Yageo `RC`/`RT`/`AC`/`AT` (`RC0603FR-0710KL`, the
/// letter after the size) and Panasonic `ERJ`/`ERA` (`ERJ-3EKF1002V`,
/// `ERA-3AEB103V`, the letter before the value code).
#[must_use]
pub fn mpn_tolerance(mpn: &str) -> Option<f64> {
    let upper = mpn.trim().to_ascii_uppercase();
    let letter = |c: char| -> Option<f64> {
        match c {
            'A' => Some(0.0005),
            'B' => Some(0.001),
            'C' => Some(0.0025),
            'D' => Some(0.005),
            'F' => Some(0.01),
            'G' => Some(0.02),
            'J' => Some(0.05),
            _ => None,
        }
    };
    for prefix in ["RC", "RT", "AC", "AT"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            // Four size digits, then the tolerance letter.
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if digits.len() == 4 {
                return rest.chars().nth(4).and_then(letter);
            }
        }
    }
    for prefix in ["ERJ-", "ERA-"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            // A size digit, a family code, then the letter before four or
            // five value digits and the packaging letter: `3EKF1002V`.
            let mut body: Vec<char> = rest.chars().collect();
            // Drop the trailing packaging letter, then the letter before the
            // value digits is the tolerance.
            if body.last().is_some_and(char::is_ascii_alphabetic) {
                body.pop();
            }
            let pos = body.iter().rposition(|c| !c.is_ascii_digit())?;
            let digits = body.len() - pos - 1;
            if (3..=5).contains(&digits) {
                return letter(body[pos]);
            }
        }
    }
    None
}

/// Parse a tempco such as `100ppm` or `25 ppm/C` into a fraction per °C.
#[must_use]
pub fn parse_tempco(input: &str) -> Option<f64> {
    let s = input.trim().to_ascii_lowercase();
    let idx = s.find("ppm")?;
    let ppm: f64 = s[..idx]
        .trim()
        .trim_start_matches(['±', '+'])
        .parse()
        .ok()?;
    (ppm > 0.0).then_some(ppm * 1e-6)
}

/// Parse a temperature range `-40..85` (optionally with `C` units) into
/// `(min, max)` °C.
#[must_use]
pub fn parse_temp_range(input: &str) -> Option<(f64, f64)> {
    let clean = input.replace(['C', 'c', '°'], "");
    let (a, b) = clean.split_once("..")?;
    let lo: f64 = a.trim().parse().ok()?;
    let hi: f64 = b.trim().parse().ok()?;
    (hi > lo).then_some((lo, hi))
}

pub(crate) struct FeedbackDividerToleranceRule;

/// What is known about one resistor of the divider.
struct Leg {
    ohms: f64,
    tol: Option<f64>,
    tempco: Option<f64>,
    far_net: NetId,
    refdes: String,
}

fn resistor_tolerance(component: &Component) -> Option<f64> {
    if let Some(t) = component
        .properties
        .get("Tolerance")
        .and_then(|t| parse_tolerance(t))
    {
        return Some(t);
    }
    if let Some(t) = component
        .value
        .as_deref()
        .and_then(|v| split_value_tolerance(v).1)
    {
        return Some(t);
    }
    component
        .properties
        .get("mpn")
        .map(String::as_str)
        .or_else(|| component.part.as_ref().and_then(|p| p.mpn.as_deref()))
        .and_then(mpn_tolerance)
}

fn leg(board: &Board, component: &Component, from: synth_ir::PinId) -> Option<Leg> {
    let far_net = deep_erc::other_pin_net(board, component, from)?;
    let value = component.value.as_deref()?;
    let (resistance, _) = split_value_tolerance(value);
    let ohms = crate::value::parse_resistance(&resistance)?;
    (ohms > 0.0).then(|| Leg {
        ohms,
        tol: resistor_tolerance(component),
        tempco: component
            .properties
            .get("tempco")
            .and_then(|t| parse_tempco(t)),
        far_net,
        refdes: component.refdes.clone(),
    })
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

type Window = (Option<f64>, Option<f64>);

/// The window `(vmin, vmax)` the loads on `net` allow, tightest bound on
/// each side. `skip` is the regulator itself.
fn load_window(board: &Board, net: &Net, skip: ComponentId) -> Window {
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

fn outside(range: (f64, f64), window: Window) -> bool {
    window.0.is_some_and(|lo| range.0 < lo) || window.1.is_some_and(|hi| range.1 > hi)
}

/// Smallest distance from the range to a window edge, in volts. Negative
/// when the range leaves the window.
fn margin_v(range: (f64, f64), window: Window) -> f64 {
    let lo = window.0.map_or(f64::INFINITY, |lo| range.0 - lo);
    let hi = window.1.map_or(f64::INFINITY, |hi| hi - range.1);
    lo.min(hi)
}

fn show_window(window: Window) -> String {
    match window {
        (Some(lo), Some(hi)) => format!("{lo:.4} V to {hi:.4} V"),
        (Some(lo), None) => format!("at least {lo:.4} V"),
        (None, Some(hi)) => format!("at most {hi:.4} V"),
        (None, None) => "unbounded".to_string(),
    }
}

fn show_pct(fraction: f64) -> String {
    let pct = fraction * 100.0;
    let s = format!("{pct:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} %")
}

/// The loosest tolerance class that keeps the worst case inside the window,
/// with the output it gives. `None` when even the tightest class fails.
fn loosest_fitting(d: &Divider, x: &Extras, window: Window) -> Option<(f64, (f64, f64))> {
    TOLERANCE_CLASSES.iter().find_map(|&class| {
        let range = worst_case_vout_with(
            &Divider {
                tol_top: class,
                tol_bottom: class,
                ..*d
            },
            x,
        );
        (!outside(range, window)).then_some((class, range))
    })
}

/// Fold resistor drift over the regulator's declared temperature range into
/// the tolerance. Returns the effective tolerance and whether drift was used.
fn with_drift(tol: f64, tempco: Option<f64>, temp: Option<(f64, f64)>) -> f64 {
    match (tempco, temp) {
        (Some(tc), Some((lo, hi))) => {
            let excursion = (hi - REFERENCE_TEMP_C)
                .abs()
                .max((REFERENCE_TEMP_C - lo).abs());
            tol + tc * excursion
        }
        _ => tol,
    }
}

/// Everything the rule found at one feedback pin.
struct Site<'a> {
    fb: &'a NetEndpoint,
    regulator: &'a Component,
    oc: Option<&'a OperatingConditions>,
    top: Leg,
    bottom: Leg,
    out_net: &'a Net,
    window: Window,
}

impl FeedbackDividerToleranceRule {
    fn missing(site: &Site<'_>) -> Vec<String> {
        let mut missing = Vec::new();
        let vref = site
            .oc
            .and_then(|o| Some((o.feedback_vref_min_v?, o.feedback_vref_max_v?)));
        if !vref.is_some_and(|(lo, hi)| lo > 0.0 && hi >= lo) {
            missing.push(format!(
                "a reference range (`feedback_vref_min_v`/`feedback_vref_max_v`) on {}'s part",
                site.regulator.refdes
            ));
        }
        for leg in [&site.top, &site.bottom] {
            if leg.tol.is_none() {
                missing.push(format!(
                    "a tolerance on {} (`tolerance \"1%\"`)",
                    leg.refdes
                ));
            }
        }
        if site.window == (None, None) {
            missing.push(format!(
                "a supply window on a load of net `{}` (`voltage_min_v`/`voltage_max_v`)",
                site.out_net.name
            ));
        }
        missing
    }

    #[allow(clippy::too_many_lines)]
    fn judge(&self, board: &Board, file: &str, site: &Site<'_>) -> Option<Diagnostic> {
        let location = Location::from_span(file.to_string(), site.fb.source_span);
        let pin_name = board
            .pin(site.fb.component, site.fb.pin)
            .map_or_default(|p| p.name.clone());
        let missing = Self::missing(site);
        if !missing.is_empty() {
            return Some(
                DiagnosticBuilder::new(
                    "E-SYNTH-POWER-015",
                    Severity::Warning,
                    "feedback divider margin is unproven: evidence is missing",
                )
                .location(location)
                .expected("every input the worst-case divider analysis needs")
                .found(format!(
                    "divider {} over {} on {} lacks {}",
                    site.top.refdes,
                    site.bottom.refdes,
                    site.regulator.describe_pin(&pin_name),
                    missing.join("; ")
                ))
                .explanation_url("synth.docs/diagnostics/E-SYNTH-POWER-015")
                .build(),
            );
        }
        let oc = site.oc?;
        let (vref_min, vref_max) = (oc.feedback_vref_min_v?, oc.feedback_vref_max_v?);
        let temp = site
            .regulator
            .properties
            .get("temp_range")
            .and_then(|t| parse_temp_range(t));
        let divider = Divider {
            vref_min_v: vref_min,
            vref_max_v: vref_max,
            r_top_ohm: site.top.ohms,
            r_bottom_ohm: site.bottom.ohms,
            tol_top: with_drift(site.top.tol?, site.top.tempco, temp),
            tol_bottom: with_drift(site.bottom.tol?, site.bottom.tempco, temp),
        };
        let extras = Extras {
            fb_leakage_a: oc.feedback_leakage_max_a.unwrap_or(0.0).abs(),
            regulation: (oc.load_regulation_pct.unwrap_or(0.0).abs()
                + oc.line_regulation_pct.unwrap_or(0.0).abs())
                / 100.0,
        };
        let range = worst_case_vout_with(&divider, &extras);
        let nominal = nominal_vout(&divider);
        let mut terms = vec![
            format!("{} top", show_pct(divider.tol_top)),
            format!("{} bottom resistor", show_pct(divider.tol_bottom)),
            format!("reference {vref_min:.4} V to {vref_max:.4} V"),
        ];
        if extras.fb_leakage_a > 0.0 {
            terms.push(format!("FB current {:.0} nA", extras.fb_leakage_a * 1e9));
        }
        if extras.regulation > 0.0 {
            terms.push(format!("regulation {}", show_pct(extras.regulation)));
        }
        if temp.is_some() && (site.top.tempco.is_some() || site.bottom.tempco.is_some()) {
            terms.push("tempco drift over temp_range".to_string());
        }
        let found_core = format!(
            "divider on {} gives {:.4} V to {:.4} V ({})",
            site.regulator.describe_pin(&pin_name),
            range.0,
            range.1,
            terms.join(", ")
        );
        let expected = format!(
            "worst-case output of net `{}` inside {}",
            site.out_net.name,
            show_window(site.window)
        );
        let advice = if let Some((class, fit)) = loosest_fitting(&divider, &extras, site.window) {
            format!(
                "{} resistors give {:.4} V to {:.4} V (±{} of {:.4} V), inside the window",
                show_pct(class),
                fit.0,
                fit.1,
                show_pct(((fit.1 - fit.0) / 2.0) / nominal),
                nominal,
            )
        } else {
            let tight = worst_case_vout_with(
                &Divider {
                    tol_top: 0.0005,
                    tol_bottom: 0.0005,
                    ..divider
                },
                &extras,
            );
            format!(
                "0.05 % resistors still give {:.4} V to {:.4} V; the reference accuracy or the window must change",
                tight.0, tight.1
            )
        };
        if outside(range, site.window) {
            return Some(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Error,
                    "regulator output can leave the load's supply window at worst case",
                )
                .location(location)
                .expected(expected)
                .found(format!("{found_core}; {advice}"))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        let threshold = site
            .regulator
            .properties
            .get("vout_margin")
            .and_then(|m| parse_tolerance(m))
            .unwrap_or(DEFAULT_MARGIN);
        let margin = margin_v(range, site.window);
        if margin < threshold * nominal {
            return Some(
                DiagnosticBuilder::new(
                    "E-SYNTH-POWER-014",
                    Severity::Warning,
                    "regulator output has a thin worst-case margin to the load's supply window",
                )
                .location(location)
                .expected(format!(
                    "{expected}, with at least {} ({:.4} V) to spare",
                    show_pct(threshold),
                    threshold * nominal
                ))
                .found(format!(
                    "{found_core}; the margin is {:.4} V ({}); {advice}",
                    margin,
                    show_pct(margin / nominal)
                ))
                .explanation_url("synth.docs/diagnostics/E-SYNTH-POWER-014")
                .build(),
            );
        }
        None
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
                // Without a resolvable two-resistor divider there is nothing
                // to analyse; the topology rules own that case.
                let Some((top, bottom)) = divider_legs(board, fb_net) else {
                    continue;
                };
                let Some(out_net) = board.net(top.far_net) else {
                    continue;
                };
                let site = Site {
                    fb,
                    regulator,
                    oc: regulator
                        .part
                        .as_ref()
                        .and_then(|p| p.operating_conditions.as_ref()),
                    window: load_window(board, out_net, fb.component),
                    top,
                    bottom,
                    out_net,
                };
                out.extend(self.judge(board, file, &site));
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

    // 50 nA through Rtop (10.01k at the high corner) adds 0.5005 mV each way;
    // 1 % regulation then scales the corners.
    #[test]
    fn leakage_and_regulation_widen_the_corners() {
        let x = Extras {
            fb_leakage_a: 50e-9,
            regulation: 0.01,
        };
        let (lo, hi) = worst_case_vout_with(&div(0.001), &x);
        assert!(close(lo, (1.193_205 - 0.000_500_5) * 0.99), "{lo}");
        assert!(close(hi, (1.206_805 + 0.000_500_5) * 1.01), "{hi}");
    }

    #[test]
    fn tolerance_needs_a_percent_sign() {
        assert_eq!(parse_tolerance("1%"), Some(0.01));
        assert_eq!(parse_tolerance("±0.1 %"), Some(0.001));
        assert_eq!(parse_tolerance("1"), None);
        assert_eq!(parse_tolerance("0%"), None);
        assert_eq!(parse_tolerance("abc%"), None);
    }

    #[test]
    fn a_value_string_can_carry_its_tolerance() {
        assert_eq!(
            split_value_tolerance("10k 1%"),
            ("10k".to_string(), Some(0.01))
        );
        assert_eq!(
            split_value_tolerance("10k 0.1 %"),
            ("10k".to_string(), Some(0.001))
        );
        assert_eq!(split_value_tolerance("10k"), ("10k".to_string(), None));
    }

    #[test]
    fn chip_resistor_mpns_encode_their_tolerance() {
        assert_eq!(mpn_tolerance("RC0603FR-0710KL"), Some(0.01));
        assert_eq!(mpn_tolerance("RT0603BRD0710KL"), Some(0.001));
        assert_eq!(mpn_tolerance("ERJ-3EKF1002V"), Some(0.01));
        assert_eq!(mpn_tolerance("ERA-3AEB103V"), Some(0.001));
        assert_eq!(mpn_tolerance("0603WAF1002T5E"), None);
    }

    #[test]
    fn tempco_and_temperature_range_parse() {
        assert!(close(parse_tempco("100ppm").unwrap(), 1e-4));
        assert!(close(parse_tempco("25 ppm/C").unwrap(), 25e-6));
        assert_eq!(parse_temp_range("-40..85"), Some((-40.0, 85.0)));
        assert_eq!(parse_temp_range("-40C..125C"), Some((-40.0, 125.0)));
        // 100 ppm over -40..85 drifts 65 °C from 25 °C: +0.65 %.
        assert!(close(
            with_drift(0.001, Some(1e-4), Some((-40.0, 85.0))),
            0.0075
        ));
    }
}
