// SPDX-License-Identifier: Apache-2.0

//! Integer-nanometer geometry primitives for the Synth EDA compiler.
//!
//! ## Why integer nanometers
//!
//! Per implementation plan §9.4 (Phase 7): every coordinate that
//! flows through the placer and router is an `i64` measured in
//! nanometers. `f64` math accumulates rounding error at the
//! 1e-15 level; for a 100 mm board that's ~1e-13 m of drift per
//! operation — invisible to the placer but enough to flip a "pad
//! 1 nm inside courtyard" check between equally-correct evaluations
//! on different machines.
//!
//! Integer math has none of that ambiguity. A placement that's
//! valid on one machine is bit-identical on another. The PRD is
//! explicit: "a placement that's valid with f64 epsilon and
//! invalid with integer math is not a placement the DRC will
//! accept."
//!
//! ## Units
//!
//! The compiler talks to humans in millimetres (KiCad's native
//! display unit, the convention in PCB datasheets). It talks to
//! itself in nanometers (this crate). Conversion functions live
//! at the boundary:
//!
//! ```
//! use synth_geometry::{Point, mm_to_nm};
//! let pad_center = Point::from_mm(12.5, -3.2);
//! assert_eq!(pad_center.x_nm, mm_to_nm(12.5));
//! ```
//!
//! ## What's here in Phase 7 slice 1A
//!
//! - [`Point`], [`Rect`] — axis-aligned primitives with bbox /
//!   contains / intersect ops.
//! - [`Rotation`] — re-exported here so the placer doesn't have
//!   to depend on `synth-layout` for the schematic-side enum.
//!   Mirrors the same four cardinal angles.
//! - mm ↔ nm conversion helpers.
//!
//! Future slices add: `Polygon` (for non-axis-aligned courtyards
//! and keepouts), `Transform` (translate + rotate), `Layer`
//! (Top/Bottom/Inner/Mask/Silk).

#![forbid(unsafe_code)]
#![allow(
    // `i64 ↔ f64` lossy casts are deliberate at the mm/nm
    // boundary — PCB coordinates fit in 53 bits of f64 mantissa
    // (a 1 km × 1 km design at nm resolution is 2^53 nm), so the
    // cast is lossless for any realistic input. The integer
    // arithmetic guarantee applies *inside* the geometry kernel.
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    // `dx_nm` / `dy_nm`, `half_w_nm` / `half_h_nm` etc. are the
    // natural names for paired axis values; the similar-names
    // lint is noise here.
    clippy::similar_names,
)]

pub mod sim_oracles;

pub use sim_oracles::{
    calculate_microstrip_z0, calculate_stripline_z0, estimate_component_thermal,
    ComponentThermalEstimate, MicrostripParams, SiImpedanceResult,
};

use serde::{Deserialize, Serialize};

/// Number of nanometers in one millimetre.
pub const NM_PER_MM: i64 = 1_000_000;

/// Convert a millimetre value to nanometers, rounding to the
/// nearest integer. Values outside the `i64` range saturate;
/// realistic PCB coordinates (< 1 m) are 12 orders of magnitude
/// below the limit, so saturation is unreachable in practice.
#[must_use]
pub fn mm_to_nm(mm: f64) -> i64 {
    (mm * NM_PER_MM as f64).round() as i64
}

/// Convert a nanometer count back to millimetres. Lossy by
/// design (i64 → f64); only use at display / export boundaries.
#[must_use]
pub fn nm_to_mm(nm: i64) -> f64 {
    nm as f64 / NM_PER_MM as f64
}

/// A point in PCB coordinates. `y` increases downward on the
/// schematic; on the PCB it follows KiCad's convention (y also
/// downward, but the placer treats axes opaquely).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    pub x_nm: i64,
    pub y_nm: i64,
}

impl Point {
    #[must_use]
    pub fn new(x_nm: i64, y_nm: i64) -> Self {
        Self { x_nm, y_nm }
    }

    /// Build a point from millimetre inputs. Convenience for
    /// fixture data and tests; production code should use `new`
    /// with already-converted integers.
    #[must_use]
    pub fn from_mm(x_mm: f64, y_mm: f64) -> Self {
        Self::new(mm_to_nm(x_mm), mm_to_nm(y_mm))
    }

    /// Translate by a vector (also in nanometers).
    #[must_use]
    pub fn translate(self, dx_nm: i64, dy_nm: i64) -> Self {
        Self::new(
            self.x_nm.saturating_add(dx_nm),
            self.y_nm.saturating_add(dy_nm),
        )
    }
}

/// Axis-aligned rectangle defined by its `min` (bottom-left) and
/// `max` (top-right) corners. Empty / inverted rectangles
/// (`min.x > max.x` or `min.y > max.y`) are treated as empty by
/// `contains` and `intersects`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    #[must_use]
    pub fn new(min: Point, max: Point) -> Self {
        Self { min, max }
    }

    /// Construct a rectangle from a centre point and half-extents.
    #[must_use]
    pub fn from_center_half_extents(center: Point, half_w_nm: i64, half_h_nm: i64) -> Self {
        Self::new(
            Point::new(center.x_nm - half_w_nm, center.y_nm - half_h_nm),
            Point::new(center.x_nm + half_w_nm, center.y_nm + half_h_nm),
        )
    }

    #[must_use]
    pub fn width_nm(&self) -> i64 {
        (self.max.x_nm - self.min.x_nm).max(0)
    }

    #[must_use]
    pub fn height_nm(&self) -> i64 {
        (self.max.y_nm - self.min.y_nm).max(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.min.x_nm > self.max.x_nm || self.min.y_nm > self.max.y_nm
    }

    /// True when `p` lies inside the rectangle (boundary
    /// inclusive).
    #[must_use]
    pub fn contains(&self, p: Point) -> bool {
        if self.is_empty() {
            return false;
        }
        p.x_nm >= self.min.x_nm
            && p.x_nm <= self.max.x_nm
            && p.y_nm >= self.min.y_nm
            && p.y_nm <= self.max.y_nm
    }

    /// True when `self` and `other` share any point. Two
    /// rectangles touching along an edge are considered to
    /// intersect (boundary inclusive — the placer treats
    /// touching courtyards as a collision).
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        self.min.x_nm <= other.max.x_nm
            && self.max.x_nm >= other.min.x_nm
            && self.min.y_nm <= other.max.y_nm
            && self.max.y_nm >= other.min.y_nm
    }

    /// Translate this rect by `(dx, dy)` nanometers.
    #[must_use]
    pub fn translate(&self, dx_nm: i64, dy_nm: i64) -> Self {
        Self::new(
            self.min.translate(dx_nm, dy_nm),
            self.max.translate(dx_nm, dy_nm),
        )
    }
}

/// Cardinal-rotation enum mirroring `synth_layout::Rotation`.
/// Placer-side parts may sit on the Top or Bottom layer and any
/// of the four cardinal orientations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rotation {
    Zero,
    Ninety,
    OneEighty,
    TwoSeventy,
}

impl Rotation {
    /// Degrees (counter-clockwise convention) — matches KiCad's
    /// `(at x y angle)` form for symbol instances.
    #[must_use]
    pub fn degrees(self) -> i32 {
        match self {
            Self::Zero => 0,
            Self::Ninety => 90,
            Self::OneEighty => 180,
            Self::TwoSeventy => 270,
        }
    }

    /// Rotate a footprint-local offset into board coordinates
    /// using KiCad's `.kicad_pcb` footprint convention: the file
    /// angle is counter-clockwise *as displayed*, and board Y
    /// increases downward, so a positive quarter turn maps local
    /// `(x, y)` to `(y, -x)`. Verified against `kicad-cli pcb
    /// export dxf`: a pad at local `(2, 0)` on a footprint placed
    /// at `(at 20 20 90)` lands at board `(20, 18)`. Every PCB-
    /// side consumer of footprint pad offsets (router grid,
    /// placer scoring, DRC pad positions) must use this one
    /// helper so internal geometry agrees with what pcbnew
    /// renders from the exported file.
    #[must_use]
    pub fn rotate_offset(self, x: i64, y: i64) -> (i64, i64) {
        match self {
            Self::Zero => (x, y),
            Self::Ninety => (y, -x),
            Self::OneEighty => (-x, -y),
            Self::TwoSeventy => (-y, x),
        }
    }

    /// True when this rotation swaps a footprint-local rectangle's
    /// width and height (quarter turns).
    #[must_use]
    pub fn swaps_extents(self) -> bool {
        matches!(self, Self::Ninety | Self::TwoSeventy)
    }
}

/// PCB copper / silk layer identity. Phase 7 supports Top and
/// Bottom for component placement; inner layers (signal routing)
/// land in Phase 8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Layer {
    Top,
    Inner1,
    Inner2,
    Bottom,
}

impl Layer {
    #[must_use]
    pub fn index(self, total_layers: usize) -> usize {
        if total_layers <= 2 {
            match self {
                Self::Top => 0,
                _ => 1,
            }
        } else {
            match self {
                Self::Top => 0,
                Self::Inner1 => 1,
                Self::Inner2 => 2,
                Self::Bottom => 3,
            }
        }
    }

    #[must_use]
    pub fn from_index(idx: usize, total_layers: usize) -> Self {
        if total_layers <= 2 {
            match idx {
                0 => Self::Top,
                _ => Self::Bottom,
            }
        } else {
            match idx {
                0 => Self::Top,
                1 => Self::Inner1,
                2 => Self::Inner2,
                _ => Self::Bottom,
            }
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Top => "F.Cu",
            Self::Inner1 => "In1.Cu",
            Self::Inner2 => "In2.Cu",
            Self::Bottom => "B.Cu",
        }
    }

    #[must_use]
    pub fn name_for_stackup(self, total_layers: usize) -> &'static str {
        if total_layers <= 2 {
            match self {
                Self::Top => "F.Cu",
                _ => "B.Cu",
            }
        } else {
            self.name()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mm_nm_roundtrip_is_lossless_for_representable_values() {
        for mm in [0.0, 1.0, 12.5, -3.2, 100.0] {
            assert_eq!(nm_to_mm(mm_to_nm(mm)), mm);
        }
    }

    #[test]
    fn rotate_offset_matches_kicad_file_convention() {
        // Ground truth from `kicad-cli pcb export dxf`: a pad at
        // local (2, 0) on a footprint at `(at 20 20 90)` lands on
        // board (20, 18).
        assert_eq!(Rotation::Ninety.rotate_offset(2, 0), (0, -2));
        assert_eq!(Rotation::Ninety.rotate_offset(0, 2), (2, 0));
        assert_eq!(Rotation::TwoSeventy.rotate_offset(2, 0), (0, 2));
        assert_eq!(Rotation::Zero.rotate_offset(3, -4), (3, -4));
        assert_eq!(Rotation::OneEighty.rotate_offset(3, -4), (-3, 4));
        // Quarter turns compose: Ninety applied twice is OneEighty.
        let (x, y) = Rotation::Ninety.rotate_offset(5, 1);
        assert_eq!(
            Rotation::Ninety.rotate_offset(x, y),
            Rotation::OneEighty.rotate_offset(5, 1)
        );
    }

    #[test]
    fn swaps_extents_only_for_quarter_turns() {
        assert!(!Rotation::Zero.swaps_extents());
        assert!(Rotation::Ninety.swaps_extents());
        assert!(!Rotation::OneEighty.swaps_extents());
        assert!(Rotation::TwoSeventy.swaps_extents());
    }

    #[test]
    fn point_from_mm_uses_nm_storage() {
        let p = Point::from_mm(2.5, -1.25);
        assert_eq!(p.x_nm, 2_500_000);
        assert_eq!(p.y_nm, -1_250_000);
    }

    #[test]
    fn rect_contains_boundary_inclusive() {
        let r = Rect::new(Point::from_mm(0.0, 0.0), Point::from_mm(10.0, 10.0));
        assert!(r.contains(Point::from_mm(5.0, 5.0)));
        assert!(r.contains(Point::from_mm(0.0, 0.0)));
        assert!(r.contains(Point::from_mm(10.0, 10.0)));
        assert!(!r.contains(Point::from_mm(11.0, 5.0)));
    }

    #[test]
    fn rect_intersects_touching_is_true() {
        let a = Rect::new(Point::from_mm(0.0, 0.0), Point::from_mm(5.0, 5.0));
        let b = Rect::new(Point::from_mm(5.0, 0.0), Point::from_mm(10.0, 5.0));
        // Touching along x=5.
        assert!(a.intersects(&b));
        let c = Rect::new(Point::from_mm(6.0, 0.0), Point::from_mm(10.0, 5.0));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn rect_from_center_half_extents() {
        let r =
            Rect::from_center_half_extents(Point::from_mm(10.0, 5.0), mm_to_nm(2.0), mm_to_nm(1.0));
        assert_eq!(r.min, Point::from_mm(8.0, 4.0));
        assert_eq!(r.max, Point::from_mm(12.0, 6.0));
    }

    #[test]
    fn rotation_degrees_matches_kicad_convention() {
        assert_eq!(Rotation::Zero.degrees(), 0);
        assert_eq!(Rotation::Ninety.degrees(), 90);
        assert_eq!(Rotation::OneEighty.degrees(), 180);
        assert_eq!(Rotation::TwoSeventy.degrees(), 270);
    }
}
