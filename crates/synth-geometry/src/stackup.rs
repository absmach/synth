// SPDX-License-Identifier: Apache-2.0

//! Stackup → per-layer physical geometry.
//!
//! A board's declared `stackup { … }` block is an ordered list of physical
//! layers, outermost copper first (`copper … insulator … copper …`). The
//! signal-integrity solvers in [`crate::sim_oracles`] need a narrower view:
//! for each copper layer, the dielectric height and relative permittivity of
//! the reference plane it works against, plus the copper thickness. This
//! module flattens one into the other.
//!
//! Two things make that more than a rename:
//!
//! - **Every copper layer, not just the outer one.** A declared stackup has
//!   inner copper too, and an inner layer is a *stripline* (dielectric on
//!   both sides) where an outer layer is a *microstrip* (dielectric below or
//!   above). The two use different closed forms.
//!   [`outer_microstrip_params`](crate::sim_oracles::outer_microstrip_params)
//!   only ever described the top layer; a [`StackupProfile`] describes all of
//!   them.
//! - **A default when none is declared.** Most boards declare no `stackup`.
//!   [`StackupProfile::for_board`] falls back to a standard 2-, 4- or
//!   6-layer FR-4 stack so downstream physics has something finite to work
//!   with. [`StackupProfile::declared`] returns `None` in that case, which is
//!   what preserves the existing contract that a board without a stackup is
//!   not impedance-checked.
//!
//! The profile stores integer nanometres and millionths-of-εr so it keeps the
//! crate's determinism guarantee: `f64` appears only inside the solver calls,
//! never in the stored geometry.

use serde::{Deserialize, Serialize};
use synth_ir::{Board, DielectricConstant, Length, Stackup, StackupLayer};

use crate::Layer;
use crate::sim_oracles::{
    CoupledGeometry, MicrostripParams, SiImpedanceResult, UnreachableZ0, calculate_microstrip_z0,
    calculate_stripline_z0, coupled_microstrip_zdiff_ohms, derive_coupled_microstrip,
    derive_microstrip_width_mm,
};

/// How a copper layer's characteristic impedance is referenced: an outer layer
/// with dielectric on one side (microstrip), or an inner layer between two
/// reference planes (stripline).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceModel {
    Microstrip,
    Stripline,
}

/// Where a [`StackupProfile`]'s numbers came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StackupSource {
    /// Flattened from the board's `stackup { … }` block.
    Declared,
    /// A standard profile substituted because the board declared none.
    Default,
}

/// The transmission-line reference for one copper layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CopperLayerGeometry {
    /// The copper layer this geometry describes.
    pub layer: Layer,
    pub trace_model: TraceModel,
    /// Dielectric thickness between this layer and its reference plane.
    ///
    /// For a stripline (an inner layer) this is the average of the
    /// dielectric above and below, the usual single-height approximation to
    /// an off-centre conductor.
    pub dielectric_height: Length,
    /// Relative permittivity of that dielectric (millionths).
    pub er: DielectricConstant,
    /// Copper foil thickness of this layer.
    pub copper_thickness: Length,
}

/// A board's resolved physical stack, one entry per copper layer, ordered
/// outermost-first (F.Cu first, B.Cu last).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StackupProfile {
    pub source: StackupSource,
    pub layers: Vec<CopperLayerGeometry>,
    /// Sum of every copper and dielectric thickness (the board thickness).
    pub total_thickness: Length,
}

/// A stackup entry before it is classified as microstrip or stripline. The
/// dielectric directly below and above a copper layer are what decide which.
#[derive(Debug, Clone, Copy)]
struct RawLayer {
    copper: Length,
    below: Option<(Length, DielectricConstant)>,
    above: Option<(Length, DielectricConstant)>,
}

impl StackupProfile {
    /// Flatten the board's declared stackup, or `None` when it declares none.
    ///
    /// A declared stackup has already passed `E-SYNTH-STACKUP-*` validation, so
    /// a failure here means a hand-built `Board` whose stackup is malformed;
    /// callers that must have a profile should use [`Self::for_board`].
    #[must_use]
    pub fn declared(board: &Board) -> Option<Self> {
        board.stackup.as_ref().and_then(Self::from_stackup)
    }

    /// The board's declared stackup, or a standard profile for its layer count
    /// when it declares none.
    #[must_use]
    pub fn for_board(board: &Board) -> Self {
        Self::declared(board).unwrap_or_else(|| Self::default_for_layers(board.layers))
    }

    /// Flatten a declared [`Stackup`] into per-copper-layer geometry.
    ///
    /// Returns `None` when the stack has fewer than two copper layers or is
    /// not the alternating copper/insulator form the language guarantees
    /// (copper first, insulator, copper, … ending in copper).
    #[must_use]
    pub fn from_stackup(stackup: &Stackup) -> Option<Self> {
        let total = stackup.copper_count();
        if total < 2 {
            return None;
        }
        let mut raw = Vec::with_capacity(total);
        for k in 0..total {
            let copper_idx = 2 * k;
            let StackupLayer::Copper { thickness, .. } = stackup.layers.get(copper_idx)? else {
                return None;
            };
            let below = match stackup.layers.get(copper_idx + 1) {
                Some(StackupLayer::Insulator { thickness, er, .. }) => Some((*thickness, *er)),
                Some(_) => return None,
                None => None,
            };
            let above = if copper_idx == 0 {
                None
            } else {
                match stackup.layers.get(copper_idx - 1) {
                    Some(StackupLayer::Insulator { thickness, er, .. }) => Some((*thickness, *er)),
                    _ => return None,
                }
            };
            raw.push(RawLayer {
                copper: *thickness,
                below,
                above,
            });
        }
        let mut profile = Self::assemble(&raw, total, StackupSource::Declared)?;
        profile.total_thickness = stackup.total_thickness();
        Some(profile)
    }

    /// A standard FR-4 stack for a 2-, 4- or 6-layer board.
    ///
    /// Values follow the JLC standard process the registry profiles already
    /// assume: 1 oz outer copper, 0.5 oz inner copper, εr ≈ 4.3–4.6. Layer
    /// counts outside `{2, 4, 6}` are clamped to the nearest supported count,
    /// since that is the set the exporter writes layer names for.
    ///
    /// # Panics
    /// Never: the default stacks are well-formed by construction.
    #[must_use]
    pub fn default_for_layers(layers: u32) -> Self {
        let raw = match layers {
            0..=2 => default_two_layer(),
            3..=4 => default_four_layer(),
            _ => default_six_layer(),
        };
        let total = raw.len();
        Self::assemble(&raw, total, StackupSource::Default)
            .expect("a default stackup is well-formed by construction")
    }

    /// Classify raw entries into microstrip/stripline geometry.
    fn assemble(raw: &[RawLayer], total: usize, source: StackupSource) -> Option<Self> {
        let mut layers = Vec::with_capacity(total);
        let mut total_thickness = Length::ZERO;
        for (k, entry) in raw.iter().enumerate() {
            let (trace_model, height, er) = if k == 0 {
                let (h, er) = entry.below?;
                (TraceModel::Microstrip, h, er)
            } else if k + 1 == total {
                let (h, er) = entry.above?;
                (TraceModel::Microstrip, h, er)
            } else {
                let (h_a, er_a) = entry.above?;
                let (h_b, er_b) = entry.below?;
                (
                    TraceModel::Stripline,
                    Length((h_a.0.saturating_add(h_b.0)) / 2),
                    DielectricConstant((er_a.0.saturating_add(er_b.0)) / 2),
                )
            };
            total_thickness = Length(
                total_thickness
                    .0
                    .saturating_add(entry.copper.0)
                    .saturating_add(height.0),
            );
            layers.push(CopperLayerGeometry {
                layer: Layer::from_index(k, total),
                trace_model,
                dielectric_height: height,
                er,
                copper_thickness: entry.copper,
            });
        }
        Some(Self {
            source,
            layers,
            total_thickness,
        })
    }

    /// Geometry for one copper layer.
    #[must_use]
    pub fn geometry(&self, layer: Layer) -> Option<&CopperLayerGeometry> {
        self.layers.iter().find(|g| g.layer == layer)
    }

    /// Number of copper layers the profile describes.
    #[must_use]
    pub fn copper_count(&self) -> usize {
        self.layers.len()
    }

    /// Microstrip parameters for the top (F.Cu) copper layer, in the shape the
    /// `sim_oracles` solvers take.
    ///
    /// `width_mm` is a placeholder; the width-aware solvers override it. This
    /// is the entry point the KiCad exporter's controlled-impedance derivation
    /// uses, so it reproduces `sim_oracles::outer_microstrip_params` exactly for
    /// a declared stackup.
    #[must_use]
    pub fn outer_microstrip_params(&self) -> Option<MicrostripParams> {
        let g = self.layers.first()?;
        (g.trace_model == TraceModel::Microstrip).then(|| MicrostripParams {
            width_mm: MicrostripParams::default().width_mm,
            height_mm: g.dielectric_height.to_mm(),
            thickness_mm: g.copper_thickness.to_mm(),
            er: g.er.to_f64(),
        })
    }

    /// Microstrip parameters for any outer layer, or `None` for an inner one.
    #[must_use]
    pub fn microstrip_params(&self, layer: Layer) -> Option<MicrostripParams> {
        let g = self.geometry(layer)?;
        (g.trace_model == TraceModel::Microstrip).then(|| MicrostripParams {
            width_mm: 0.0,
            height_mm: g.dielectric_height.to_mm(),
            thickness_mm: g.copper_thickness.to_mm(),
            er: g.er.to_f64(),
        })
    }

    /// Characteristic impedance of a trace of `width_nm` on `layer`.
    #[must_use]
    pub fn z0(&self, layer: Layer, width_nm: i64) -> Option<SiImpedanceResult> {
        let g = self.geometry(layer)?;
        let width_mm = Length(width_nm).to_mm();
        Some(match g.trace_model {
            TraceModel::Microstrip => calculate_microstrip_z0(&MicrostripParams {
                width_mm,
                height_mm: g.dielectric_height.to_mm(),
                thickness_mm: g.copper_thickness.to_mm(),
                er: g.er.to_f64(),
            }),
            TraceModel::Stripline => calculate_stripline_z0(
                width_mm,
                g.dielectric_height.to_mm(),
                g.copper_thickness.to_mm(),
                g.er.to_f64(),
            ),
        })
    }

    /// Trace width on `layer` that reaches `target_ohms`, respecting the
    /// manufacturer's minimum width.
    ///
    /// # Errors
    /// [`UnreachableZ0`] with the achievable impedance range when no legal
    /// width on this layer reaches the target.
    pub fn width_for_z0(
        &self,
        layer: Layer,
        target_ohms: f64,
        min_width_nm: i64,
    ) -> Result<Length, UnreachableZ0> {
        let g = self.geometry(layer).ok_or(UnreachableZ0 {
            lowest_ohms: 0.0,
            highest_ohms: 0.0,
        })?;
        let min_width_mm = Length(min_width_nm).to_mm();
        match g.trace_model {
            TraceModel::Microstrip => {
                let base = MicrostripParams {
                    width_mm: 0.0,
                    height_mm: g.dielectric_height.to_mm(),
                    thickness_mm: g.copper_thickness.to_mm(),
                    er: g.er.to_f64(),
                };
                derive_microstrip_width_mm(target_ohms, &base, min_width_mm).map(Length::from_mm)
            }
            TraceModel::Stripline => self.derive_stripline_width(layer, target_ohms, min_width_mm),
        }
    }

    /// Differential-pair width and gap on an **outer** `layer` for
    /// `target_ohms`. Inner-layer coupled stripline is not modelled yet, and
    /// returns `UnreachableZ0`.
    ///
    /// # Errors
    /// [`UnreachableZ0`] when the target is out of range or the layer is not
    /// an outer layer.
    pub fn coupled_for_z0(
        &self,
        layer: Layer,
        target_ohms: f64,
        min_width_nm: i64,
        min_gap_nm: i64,
    ) -> Result<CoupledGeometry, UnreachableZ0> {
        let Some(base) = self.microstrip_params(layer) else {
            return Err(UnreachableZ0 {
                lowest_ohms: 0.0,
                highest_ohms: 0.0,
            });
        };
        derive_coupled_microstrip(
            target_ohms,
            &base,
            Length(min_width_nm).to_mm(),
            Length(min_gap_nm).to_mm(),
        )
    }

    /// Bisection inverse of [`calculate_stripline_z0`] for the width.
    fn derive_stripline_width(
        &self,
        layer: Layer,
        target_ohms: f64,
        min_width_mm: f64,
    ) -> Result<Length, UnreachableZ0> {
        let g = self.geometry(layer).expect("layer checked by caller");
        let height_mm = g.dielectric_height.to_mm();
        let thickness_mm = g.copper_thickness.to_mm();
        let er = g.er.to_f64();
        let z0_at =
            |width_mm: f64| calculate_stripline_z0(width_mm, height_mm, thickness_mm, er).z0_ohms;
        let narrow = min_width_mm.max(0.1 * height_mm);
        let wide = (2.0 * height_mm).max(narrow);
        let highest_ohms = z0_at(narrow);
        let lowest_ohms = z0_at(wide);
        if !(lowest_ohms..=highest_ohms).contains(&target_ohms) {
            return Err(UnreachableZ0 {
                lowest_ohms,
                highest_ohms,
            });
        }
        // Z0 falls monotonically as width grows, so bisect for the crossing.
        let (mut lo, mut hi) = (narrow, wide);
        for _ in 0..60 {
            let mid = f64::midpoint(lo, hi);
            if z0_at(mid) > target_ohms {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Ok(Length::from_mm(f64::midpoint(lo, hi)))
    }

    /// Differential impedance a given pair geometry produces on `layer`, or
    /// `None` on an inner layer (coupled stripline is not modelled yet).
    #[must_use]
    pub fn coupled_zdiff(&self, layer: Layer, width_nm: i64, gap_nm: i64) -> Option<f64> {
        let base = self.microstrip_params(layer)?;
        Some(coupled_microstrip_zdiff_ohms(
            &MicrostripParams {
                width_mm: Length(width_nm).to_mm(),
                ..base
            },
            Length(gap_nm).to_mm(),
        ))
    }
}

/// Standard 2-layer FR-4: 1.6 mm total, 1 oz copper both sides.
fn default_two_layer() -> Vec<RawLayer> {
    let copper = Length::from_mm(0.035);
    let core = (Length::from_mm(1.53), er(4.4));
    vec![
        RawLayer {
            copper,
            below: Some(core),
            above: None,
        },
        RawLayer {
            copper,
            below: None,
            above: Some(core),
        },
    ]
}

/// Standard 4-layer FR-4: 1.6 mm total, 1 oz outer / 0.5 oz inner copper.
fn default_four_layer() -> Vec<RawLayer> {
    let outer = Length::from_mm(0.035);
    let inner = Length::from_mm(0.0152);
    let prepreg = (Length::from_mm(0.2104), er(4.4));
    let core = (Length::from_mm(1.065), er(4.6));
    vec![
        RawLayer {
            copper: outer,
            below: Some(prepreg),
            above: None,
        },
        RawLayer {
            copper: inner,
            below: Some(core),
            above: Some(prepreg),
        },
        RawLayer {
            copper: inner,
            below: Some(prepreg),
            above: Some(core),
        },
        RawLayer {
            copper: outer,
            below: None,
            above: Some(prepreg),
        },
    ]
}

/// Standard 6-layer FR-4: 1.6 mm total, symmetric around the centre.
fn default_six_layer() -> Vec<RawLayer> {
    let outer = Length::from_mm(0.035);
    let inner = Length::from_mm(0.0152);
    let outer_di = (Length::from_mm(0.18), er(4.3));
    let inner_di = (Length::from_mm(0.36), er(4.6));
    vec![
        RawLayer {
            copper: outer,
            below: Some(outer_di),
            above: None,
        },
        RawLayer {
            copper: inner,
            below: Some(inner_di),
            above: Some(outer_di),
        },
        RawLayer {
            copper: inner,
            below: Some(inner_di),
            above: Some(inner_di),
        },
        RawLayer {
            copper: inner,
            below: Some(outer_di),
            above: Some(inner_di),
        },
        RawLayer {
            copper: inner,
            below: Some(outer_di),
            above: Some(outer_di),
        },
        RawLayer {
            copper: outer,
            below: None,
            above: Some(outer_di),
        },
    ]
}

fn er(value: f64) -> DielectricConstant {
    DielectricConstant((value * 1_000_000.0) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_diagnostics::Span;

    fn copper(thickness_mm: f64) -> StackupLayer {
        StackupLayer::Copper {
            thickness: Length::from_mm(thickness_mm),
            source_span: Span::new(0, 0),
        }
    }

    fn insulator(thickness_mm: f64, er: f64) -> StackupLayer {
        StackupLayer::Insulator {
            thickness: Length::from_mm(thickness_mm),
            er: DielectricConstant((er * 1_000_000.0) as i64),
            material: Some("FR4".to_string()),
            source_span: Span::new(0, 0),
        }
    }

    fn four_layer_stackup() -> Stackup {
        Stackup {
            layers: vec![
                copper(0.035),
                insulator(0.2104, 4.4),
                copper(0.0152),
                insulator(1.065, 4.6),
                copper(0.0152),
                insulator(0.2104, 4.4),
                copper(0.035),
            ],
            source_span: Span::new(0, 0),
        }
    }

    #[test]
    fn declared_stackup_flattens_to_one_entry_per_copper_layer() {
        let profile = StackupProfile::from_stackup(&four_layer_stackup()).expect("flatten");
        assert_eq!(profile.source, StackupSource::Declared);
        assert_eq!(profile.copper_count(), 4);
        assert_eq!(
            profile.layers.iter().map(|g| g.layer).collect::<Vec<_>>(),
            vec![Layer::Top, Layer::Inner1, Layer::Inner2, Layer::Bottom]
        );
    }

    #[test]
    fn outer_layers_are_microstrip_and_inner_layers_stripline() {
        let profile = StackupProfile::from_stackup(&four_layer_stackup()).expect("flatten");
        assert_eq!(
            profile.geometry(Layer::Top).unwrap().trace_model,
            TraceModel::Microstrip
        );
        assert_eq!(
            profile.geometry(Layer::Bottom).unwrap().trace_model,
            TraceModel::Microstrip
        );
        assert_eq!(
            profile.geometry(Layer::Inner1).unwrap().trace_model,
            TraceModel::Stripline
        );
        assert_eq!(
            profile.geometry(Layer::Inner2).unwrap().trace_model,
            TraceModel::Stripline
        );
    }

    #[test]
    fn outer_entry_matches_the_existing_outer_microstrip_params() {
        let stackup = four_layer_stackup();
        let existing = crate::sim_oracles::outer_microstrip_params(&stackup).expect("outer");
        let via_profile = StackupProfile::from_stackup(&stackup)
            .expect("flatten")
            .outer_microstrip_params()
            .expect("outer");
        assert_eq!(existing.height_mm, via_profile.height_mm);
        assert_eq!(existing.thickness_mm, via_profile.thickness_mm);
        assert_eq!(existing.er, via_profile.er);
        assert_eq!(existing.width_mm, via_profile.width_mm);
    }

    #[test]
    fn stripline_height_averages_the_two_dielectrics() {
        let profile = StackupProfile::from_stackup(&four_layer_stackup()).expect("flatten");
        let inner1 = profile.geometry(Layer::Inner1).unwrap();
        // (0.2104 + 1.065) / 2 = 0.6377 mm
        assert!((inner1.dielectric_height.to_mm() - 0.6377).abs() < 1e-9);
        // (4.4 + 4.6) / 2 = 4.5
        assert!((inner1.er.to_f64() - 4.5).abs() < 1e-9);
    }

    #[test]
    fn total_thickness_matches_the_declared_stackup() {
        let stackup = four_layer_stackup();
        let profile = StackupProfile::from_stackup(&stackup).expect("flatten");
        assert_eq!(profile.total_thickness, stackup.total_thickness());
    }

    #[test]
    fn microstrip_width_reproduces_the_target_on_the_outer_layer() {
        let profile = StackupProfile::default_for_layers(4);
        for target in [50.0, 60.0] {
            let width = profile
                .width_for_z0(Layer::Top, target, crate::mm_to_nm(0.127))
                .expect("reachable");
            let z0 = profile.z0(Layer::Top, width.0).expect("z0").z0_ohms;
            assert!((z0 - target).abs() < 0.05, "{target} gave {z0}");
        }
    }

    #[test]
    fn stripline_width_reproduces_the_target_on_an_inner_layer() {
        let profile = StackupProfile::default_for_layers(4);
        for target in [45.0, 50.0, 55.0] {
            let width = profile
                .width_for_z0(Layer::Inner1, target, crate::mm_to_nm(0.1))
                .expect("reachable");
            let z0 = profile.z0(Layer::Inner1, width.0).expect("z0").z0_ohms;
            assert!((z0 - target).abs() < 0.05, "{target} gave {z0}");
        }
    }

    #[test]
    fn unreachable_targets_report_an_ordered_range() {
        let profile = StackupProfile::default_for_layers(4);
        let err = profile
            .width_for_z0(Layer::Inner1, 1.0, crate::mm_to_nm(0.1))
            .unwrap_err();
        assert!(err.lowest_ohms <= err.highest_ohms, "{err:?}");
    }

    #[test]
    fn coupled_geometry_reproduces_the_differential_target() {
        let profile = StackupProfile::default_for_layers(4);
        for target in [90.0, 100.0] {
            let pair = profile
                .coupled_for_z0(
                    Layer::Top,
                    target,
                    crate::mm_to_nm(0.127),
                    crate::mm_to_nm(0.127),
                )
                .expect("reachable");
            let zdiff = profile
                .coupled_zdiff(
                    Layer::Top,
                    crate::mm_to_nm(pair.width_mm),
                    crate::mm_to_nm(pair.gap_mm),
                )
                .expect("outer");
            assert!((zdiff - target).abs() < 0.05, "{target} gave {zdiff}");
        }
    }

    #[test]
    fn coupled_geometry_is_unavailable_on_inner_layers() {
        let profile = StackupProfile::default_for_layers(4);
        assert!(
            profile
                .coupled_for_z0(
                    Layer::Inner1,
                    90.0,
                    crate::mm_to_nm(0.127),
                    crate::mm_to_nm(0.127)
                )
                .is_err()
        );
    }

    #[test]
    fn defaults_cover_two_four_and_six_layers() {
        assert_eq!(StackupProfile::default_for_layers(2).copper_count(), 2);
        assert_eq!(StackupProfile::default_for_layers(4).copper_count(), 4);
        assert_eq!(StackupProfile::default_for_layers(6).copper_count(), 6);
        assert_eq!(StackupProfile::default_for_layers(3).copper_count(), 4);
        assert_eq!(
            StackupProfile::default_for_layers(2).source,
            StackupSource::Default
        );
    }

    #[test]
    fn profile_round_trips_through_json() {
        let profile = StackupProfile::default_for_layers(4);
        let json = serde_json::to_string(&profile).expect("serialize");
        let back: StackupProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(profile, back);
    }
}
