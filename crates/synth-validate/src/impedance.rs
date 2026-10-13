// SPDX-License-Identifier: Apache-2.0

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity};
use synth_geometry::{
    calculate_microstrip_z0, coupled_microstrip_zdiff_ohms, derive_coupled_microstrip,
    derive_microstrip_width_mm, outer_microstrip_params, MicrostripParams, UnreachableZ0,
};
use synth_ir::{Board, Length, Net};

use crate::{ErcCategory, ErcRule};

const ESTIMATE: &str = " (an estimate, about 10 percent; does not replace expert analysis)";
const ESTIMATE_PAIR: &str =
    " (an estimate from a closed-form coupled-pair approximation; does not replace expert analysis)";
const ERROR_DEVIATION: f64 = 0.20;
const WARNING_DEVIATION: f64 = 0.10;
/// Reference trace width for the stackup impedance envelope (`W-SYNTH-SI-001`).
const SI_REFERENCE_WIDTH_MM: f64 = 0.2;

struct Controlled<'a> {
    net: &'a Net,
    partner: Option<&'a Net>,
    /// Where the target was written: the `diff_pair` or the `netclass`.
    span: synth_diagnostics::Span,
    /// Human description used in the diagnostics, e.g. "pair `a`/`b`" or
    /// "net `x` in netclass `RF`".
    subject: String,
    target_ohms: f64,
    outer: Option<MicrostripParams>,
}

impl Controlled<'_> {
    fn derive(&self, outer: &MicrostripParams) -> Result<(), UnreachableZ0> {
        match self.partner {
            Some(_) => derive_coupled_microstrip(
                self.target_ohms,
                outer,
                min_trace_width_mm(),
                min_clearance().to_mm(),
            )
            .map(drop),
            None => {
                derive_microstrip_width_mm(self.target_ohms, outer, min_trace_width_mm()).map(drop)
            }
        }
    }

    fn declared(&self, board: &Board, leg: &Net) -> Option<(f64, Option<f64>)> {
        let (width, gap) = board.declared_leg_geometry(leg, min_clearance())?;
        Some((width.to_mm(), self.partner.map(|_| gap.to_mm())))
    }
}

fn impedance_at(outer: &MicrostripParams, width_mm: f64, gap_mm: Option<f64>) -> f64 {
    let params = MicrostripParams {
        width_mm,
        ..outer.clone()
    };
    match gap_mm {
        Some(gap) => coupled_microstrip_zdiff_ohms(&params, gap),
        None => calculate_microstrip_z0(&params).z0_ohms,
    }
}

/// Every controlled-impedance target on the board: a `diff_pair`'s
/// differential `impedance`, and a `netclass`'s single-ended `impedance` on
/// each of its member nets.
fn controlled(board: &Board) -> Vec<Controlled<'_>> {
    let outer = board.stackup.as_ref().and_then(outer_microstrip_params);
    let mut out = Vec::new();
    for (net, neg, dp, impedance) in board.impedance_legs() {
        let partner = (neg.id != net.id).then_some(neg);
        out.push(Controlled {
            net,
            partner,
            span: dp.source_span,
            subject: match partner {
                Some(partner) => format!("pair `{}`/`{}`", net.name, partner.name),
                None => format!("net `{}`", net.name),
            },
            target_ohms: impedance.to_ohms(),
            outer: outer.clone(),
        });
    }
    for class in &board.netclasses {
        let Some(impedance) = class.impedance else {
            continue;
        };
        for net in board
            .nets
            .iter()
            .filter(|n| n.netclass.as_deref() == Some(class.name.as_str()))
        {
            out.push(Controlled {
                net,
                partner: None,
                span: class.source_span,
                subject: format!("net `{}` in netclass `{}`", net.name, class.name),
                target_ohms: impedance.to_ohms(),
                outer: outer.clone(),
            });
        }
    }
    out
}

fn diagnostic(
    code: &str,
    severity: Severity,
    title: &str,
    file: &str,
    c: &Controlled<'_>,
) -> DiagnosticBuilder {
    let estimate = if c.partner.is_some() {
        ESTIMATE_PAIR
    } else {
        ESTIMATE
    };
    DiagnosticBuilder::new(code, severity, format!("{title}{estimate}"))
        .location(Location::from_span(file.to_string(), c.span))
        .explanation_url(format!("synth.docs/diagnostics/{code}"))
}

fn describe(geometry: Option<(f64, Option<f64>)>) -> String {
    match geometry {
        Some((width, Some(gap))) => format!("{width} mm wide with a {gap} mm gap"),
        Some((width, None)) => format!("{width} mm"),
        None => "no trace width".to_string(),
    }
}

fn min_trace_width_mm() -> f64 {
    synth_ir::Length(synth_drc::ManufacturerProfile::jlc_standard().min_trace_width_nm).to_mm()
}

fn min_clearance() -> Length {
    Length(synth_drc::ManufacturerProfile::jlc_standard().min_copper_clearance_nm)
}

pub(crate) struct DeclaredWidthRule;

impl ErcRule for DeclaredWidthRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-IMPEDANCE-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Rf
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        controlled(board)
            .into_iter()
            .filter_map(|c| {
                let legs: Vec<_> = std::iter::once(c.net).chain(c.partner).collect();
                let declared: Vec<_> = legs.iter().map(|leg| c.declared(board, leg)).collect();
                if declared.iter().all(Option::is_none) {
                    return None;
                }
                if declared.iter().any(|d| *d != declared[0]) {
                    return Some(
                        diagnostic(
                            self.code(),
                            Severity::Error,
                            "declared trace geometry differs between the legs of the pair",
                            file,
                            &c,
                        )
                        .expected(format!(
                            "both legs of {} in one class giving about {:.1} ohm",
                            c.subject, c.target_ohms
                        ))
                        .found(
                            legs.iter()
                                .zip(&declared)
                                .map(|(leg, d)| format!("`{}` declares {}", leg.name, describe(*d)))
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                        .build(),
                    );
                }
                let (width, gap) = declared[0]?;
                let outer = c.outer.as_ref()?;
                c.derive(outer).ok()?;
                let z0 = impedance_at(outer, width, gap);
                let deviation = (z0 - c.target_ohms).abs() / c.target_ohms;
                let severity = if deviation > ERROR_DEVIATION {
                    Severity::Error
                } else if deviation >= WARNING_DEVIATION {
                    Severity::Warning
                } else {
                    return None;
                };
                Some(
                    diagnostic(
                        self.code(),
                        severity,
                        "declared trace geometry is off the impedance target",
                        file,
                        &c,
                    )
                    .expected(format!(
                        "a geometry giving about {:.1} ohm on {}",
                        c.target_ohms, c.subject
                    ))
                    .found(format!(
                        "{} gives {z0:.1} ohm ({:.0} percent off)",
                        describe(Some((width, gap))),
                        deviation * 100.0
                    ))
                    .build(),
                )
            })
            .collect()
    }
}

/// `W-SYNTH-SI-001` — the stackup's outer microstrip at a reference width sits
/// outside a standard impedance envelope.
///
/// The same characteristic-impedance and propagation-delay calculation the
/// `synth_evaluate_thermal_si` oracle computes, folded into the gate so a
/// stackup whose signal layer is too thin or too thick to route normal traces
/// is flagged at validation rather than only when an agent asks.
pub(crate) struct StackupImpedanceEnvelopeRule;

impl ErcRule for StackupImpedanceEnvelopeRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-SI-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Rf
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let Some(stackup) = board.stackup.as_ref() else {
            return Vec::new();
        };
        let Some(outer) = outer_microstrip_params(stackup) else {
            return Vec::new();
        };
        // A reference width in the middle of the usual range, so the envelope
        // reflects the stackup rather than a particular design's geometry.
        let result = calculate_microstrip_z0(&MicrostripParams {
            width_mm: SI_REFERENCE_WIDTH_MM,
            ..outer
        });
        if (30.0..=100.0).contains(&result.z0_ohms) {
            return Vec::new();
        }
        vec![DiagnosticBuilder::new(
            self.code(),
            Severity::Warning,
            "stackup impedance is outside the standard envelope",
        )
        .location(Location::from_span(file.to_string(), stackup.source_span))
        .expected("an outer-layer microstrip giving 30 to 100 ohm at a 0.2 mm trace")
        .found(format!(
            "about {:.0} ohm ({:.2} ns/m delay)",
            result.z0_ohms, result.propagation_delay_ns_m
        ))
        .message(format!(
            "the stackup's outer microstrip gives about {:.0} ohm at a 0.2 mm trace \
                 ({:.2} ns/m delay), outside the 30-100 ohm envelope a normal signal layer \
                 sits in. An estimate from the declared dielectric, not a field solve.",
            result.z0_ohms, result.propagation_delay_ns_m
        ))
        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
        .build()]
    }
}

pub(crate) struct UnreachableTargetRule;

impl ErcRule for UnreachableTargetRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-IMPEDANCE-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Rf
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        controlled(board)
            .into_iter()
            .filter_map(|c| {
                let range = c.derive(c.outer.as_ref()?).err()?;
                Some(
                    diagnostic(
                        self.code(),
                        Severity::Error,
                        "impedance target is outside the range this estimate covers",
                        file,
                        &c,
                    )
                    .expected(format!(
                        "a target inside the range this estimate covers ({:.1} to {:.1} ohm on \
                         this stackup)",
                        range.lowest_ohms, range.highest_ohms
                    ))
                    .found(format!("{:.1} ohm on {}", c.target_ohms, c.subject))
                    .build(),
                )
            })
            .collect()
    }
}

pub(crate) struct InnerLayerNotVerifiedRule;

impl ErcRule for InnerLayerNotVerifiedRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-IMPEDANCE-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Rf
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        if board.stackup.as_ref().is_none_or(|s| s.copper_count() <= 2) {
            return Vec::new();
        }
        controlled(board)
            .into_iter()
            .filter(|c| c.outer.is_some())
            .map(|c| {
                diagnostic(
                    self.code(),
                    Severity::Warning,
                    "impedance not verified on inner layers",
                    file,
                    &c,
                )
                .expected("a controlled-impedance net routed on an outer layer")
                .found(format!(
                    "{} may be routed on an inner copper layer, where impedance is not \
                     verified; only outer-layer microstrip is derived and checked",
                    c.subject
                ))
                .build()
            })
            .collect()
    }
}
