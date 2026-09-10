// SPDX-License-Identifier: Apache-2.0

//! Engineering unit newtypes.
//!
//! Each type wraps an `i64` in a chosen base unit:
//!
//! | Type          | Base unit       | Range                      |
//! |---------------|-----------------|----------------------------|
//! | [`Length`]    | nanometres      | ±9.2 × 10⁹ m               |
//! | [`Voltage`]   | microvolts      | ±9.2 × 10¹² V              |
//! | [`Current`]   | microamps       | ±9.2 × 10¹² A              |
//! | [`Resistance`]| milliohms       | ±9.2 × 10¹⁵ Ω              |
//! | [`Impedance`] | milliohms       | ±9.2 × 10¹⁵ Ω              |
//! | [`Frequency`] | hertz           | ±9.2 × 10¹⁸ Hz             |
//! | [`Capacitance`]| femtofarads    | ±9.2 × 10³ F               |
//!
//! Floats appear only at API boundaries: the AST's `ValueWithUnit`
//! carries the literal as a decimal string, and conversions to/from
//! human-friendly units (mm, V, etc.) happen via dedicated helpers.
//! Internal arithmetic — clearances, trace widths, voltage budgets —
//! is always integer.

use serde::{Deserialize, Serialize};
use synth_ast::{Unit, ValueWithUnit};
use synth_diagnostics::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Length(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Voltage(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Current(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Resistance(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Impedance(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Frequency(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capacitance(pub i64);

/// Errors that can arise converting a parsed [`ValueWithUnit`] into a
/// typed quantity. Each carries the offending source span for
/// diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionError {
    /// The numeric literal could not be parsed as a decimal.
    InvalidLiteral { literal: String, span: Span },
    /// The unit token is not compatible with the expected quantity
    /// (e.g., `20mm` where an `Impedance` was wanted).
    WrongUnit {
        actual: Unit,
        expected_quantity: &'static str,
        span: Span,
    },
    /// Conversion would overflow the integer base.
    Overflow {
        literal: String,
        unit: Unit,
        span: Span,
    },
}

impl ConversionError {
    pub fn span(&self) -> Span {
        match self {
            Self::InvalidLiteral { span, .. }
            | Self::WrongUnit { span, .. }
            | Self::Overflow { span, .. } => *span,
        }
    }
}

// -----------------------------------------------------------------------------
// Parsing decimal literals
// -----------------------------------------------------------------------------

/// Parse a decimal literal into `(integer_part, decimal_micro_units)`
/// where `decimal_micro_units` is the fractional part expressed as a
/// multiplier in `[0, 1_000_000)`.
///
/// Returns `None` on a malformed literal. This is intentionally
/// integer-only to avoid float rounding in the IR.
fn parse_decimal_scaled(literal: &str, scale: i64) -> Option<i64> {
    // Split into sign, integer, fraction.
    let (sign, rest) = if let Some(rest) = literal.strip_prefix('-') {
        (-1_i64, rest)
    } else {
        (1, literal)
    };
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, f),
        None => (rest, ""),
    };

    let int_part: i64 = int_part.parse().ok()?;
    let int_scaled = int_part.checked_mul(scale)?;

    let frac_scaled = if frac_part.is_empty() {
        0
    } else {
        // Truncate the fraction to fit in i64 even when scale is large.
        // For decimal "3.3" with scale 1_000_000_000, we want 300_000_000.
        let mut acc: i64 = 0;
        let mut step = scale;
        for c in frac_part.chars() {
            step /= 10;
            if step == 0 {
                break;
            }
            let d = c.to_digit(10)?;
            acc = acc.checked_add(i64::from(d).checked_mul(step)?)?;
        }
        acc
    };

    sign.checked_mul(int_scaled.checked_add(frac_scaled)?)
}

// -----------------------------------------------------------------------------
// Length
// -----------------------------------------------------------------------------

const NM_PER_MM: i64 = 1_000_000;
const NM_PER_MIL: i64 = 25_400; // 1 mil = 25.4 µm = 25,400 nm

impl Length {
    pub const ZERO: Self = Length(0);

    // f64 conversions are intentional at the boundary; integer-nm
    // arithmetic remains the canonical internal representation.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    pub fn from_mm(mm: f64) -> Self {
        Length((mm * NM_PER_MM as f64) as i64)
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn to_mm(self) -> f64 {
        self.0 as f64 / NM_PER_MM as f64
    }

    fn try_from_value(v: &ValueWithUnit) -> Result<Self, ConversionError> {
        let scale = match v.unit {
            Unit::Mm => NM_PER_MM,
            Unit::Mil => NM_PER_MIL,
            other => {
                return Err(ConversionError::WrongUnit {
                    actual: other,
                    expected_quantity: "length",
                    span: v.span,
                });
            }
        };
        parse_decimal_scaled(&v.literal, scale)
            .map(Length)
            .ok_or(ConversionError::Overflow {
                literal: v.literal.clone(),
                unit: v.unit,
                span: v.span,
            })
    }
}

impl TryFrom<&ValueWithUnit> for Length {
    type Error = ConversionError;
    fn try_from(v: &ValueWithUnit) -> Result<Self, Self::Error> {
        Self::try_from_value(v)
    }
}

// -----------------------------------------------------------------------------
// Voltage
// -----------------------------------------------------------------------------

const UV_PER_V: i64 = 1_000_000;
const UV_PER_MV: i64 = 1_000;

impl Voltage {
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    pub fn from_v(v: f64) -> Self {
        Voltage((v * UV_PER_V as f64) as i64)
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn to_v(self) -> f64 {
        self.0 as f64 / UV_PER_V as f64
    }
}

impl TryFrom<&ValueWithUnit> for Voltage {
    type Error = ConversionError;
    fn try_from(v: &ValueWithUnit) -> Result<Self, Self::Error> {
        let scale = match v.unit {
            Unit::V => UV_PER_V,
            Unit::Mv => UV_PER_MV,
            other => {
                return Err(ConversionError::WrongUnit {
                    actual: other,
                    expected_quantity: "voltage",
                    span: v.span,
                });
            }
        };
        parse_decimal_scaled(&v.literal, scale)
            .map(Voltage)
            .ok_or(ConversionError::Overflow {
                literal: v.literal.clone(),
                unit: v.unit,
                span: v.span,
            })
    }
}

// -----------------------------------------------------------------------------
// Current, Resistance, Impedance, Frequency, Capacitance
// -----------------------------------------------------------------------------

const UA_PER_A: i64 = 1_000_000;
const UA_PER_MA: i64 = 1_000;
const MOHM_PER_OHM: i64 = 1_000;
const MOHM_PER_KOHM: i64 = 1_000_000;
const HZ_PER_MHZ: i64 = 1_000_000;
const HZ_PER_GHZ: i64 = 1_000_000_000;
const FF_PER_PF: i64 = 1_000;
const FF_PER_NF: i64 = 1_000_000;
const FF_PER_UF: i64 = 1_000_000_000;

impl TryFrom<&ValueWithUnit> for Current {
    type Error = ConversionError;
    fn try_from(v: &ValueWithUnit) -> Result<Self, Self::Error> {
        let scale = match v.unit {
            Unit::A => UA_PER_A,
            Unit::Ma => UA_PER_MA,
            other => {
                return Err(ConversionError::WrongUnit {
                    actual: other,
                    expected_quantity: "current",
                    span: v.span,
                });
            }
        };
        parse_decimal_scaled(&v.literal, scale)
            .map(Current)
            .ok_or(ConversionError::Overflow {
                literal: v.literal.clone(),
                unit: v.unit,
                span: v.span,
            })
    }
}

impl TryFrom<&ValueWithUnit> for Resistance {
    type Error = ConversionError;
    fn try_from(v: &ValueWithUnit) -> Result<Self, Self::Error> {
        let scale = match v.unit {
            Unit::Ohm => MOHM_PER_OHM,
            Unit::Kohm => MOHM_PER_KOHM,
            Unit::Mohm => 1, // already in milliohms
            other => {
                return Err(ConversionError::WrongUnit {
                    actual: other,
                    expected_quantity: "resistance",
                    span: v.span,
                });
            }
        };
        parse_decimal_scaled(&v.literal, scale)
            .map(Resistance)
            .ok_or(ConversionError::Overflow {
                literal: v.literal.clone(),
                unit: v.unit,
                span: v.span,
            })
    }
}

impl TryFrom<&ValueWithUnit> for Impedance {
    type Error = ConversionError;
    fn try_from(v: &ValueWithUnit) -> Result<Self, Self::Error> {
        // Impedance shares the resistance unit set.
        let r: Resistance = v.try_into()?;
        Ok(Impedance(r.0))
    }
}

impl TryFrom<&ValueWithUnit> for Frequency {
    type Error = ConversionError;
    fn try_from(v: &ValueWithUnit) -> Result<Self, Self::Error> {
        let scale = match v.unit {
            Unit::Mhz => HZ_PER_MHZ,
            Unit::Ghz => HZ_PER_GHZ,
            other => {
                return Err(ConversionError::WrongUnit {
                    actual: other,
                    expected_quantity: "frequency",
                    span: v.span,
                });
            }
        };
        parse_decimal_scaled(&v.literal, scale)
            .map(Frequency)
            .ok_or(ConversionError::Overflow {
                literal: v.literal.clone(),
                unit: v.unit,
                span: v.span,
            })
    }
}

impl TryFrom<&ValueWithUnit> for Capacitance {
    type Error = ConversionError;
    fn try_from(v: &ValueWithUnit) -> Result<Self, Self::Error> {
        let scale = match v.unit {
            Unit::Pf => FF_PER_PF,
            Unit::Nf => FF_PER_NF,
            Unit::Uf => FF_PER_UF,
            other => {
                return Err(ConversionError::WrongUnit {
                    actual: other,
                    expected_quantity: "capacitance",
                    span: v.span,
                });
            }
        };
        parse_decimal_scaled(&v.literal, scale)
            .map(Capacitance)
            .ok_or(ConversionError::Overflow {
                literal: v.literal.clone(),
                unit: v.unit,
                span: v.span,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vwu(literal: &str, unit: Unit) -> ValueWithUnit {
        ValueWithUnit {
            literal: literal.into(),
            unit,
            span: Span::new(0, 0),
        }
    }

    #[test]
    fn length_mm_to_nm() {
        assert_eq!(
            Length::try_from(&vwu("20", Unit::Mm)).unwrap(),
            Length(20_000_000)
        );
        assert_eq!(
            Length::try_from(&vwu("0.5", Unit::Mm)).unwrap(),
            Length(500_000)
        );
        assert_eq!(
            Length::try_from(&vwu("0.001", Unit::Mm)).unwrap(),
            Length(1_000)
        );
    }

    #[test]
    fn length_mil_to_nm() {
        // 1 mil = 25,400 nm
        assert_eq!(
            Length::try_from(&vwu("1", Unit::Mil)).unwrap(),
            Length(25_400)
        );
        assert_eq!(
            Length::try_from(&vwu("10", Unit::Mil)).unwrap(),
            Length(254_000)
        );
    }

    #[test]
    fn voltage_v_and_mv() {
        assert_eq!(
            Voltage::try_from(&vwu("3.3", Unit::V)).unwrap(),
            Voltage(3_300_000)
        );
        assert_eq!(
            Voltage::try_from(&vwu("100", Unit::Mv)).unwrap(),
            Voltage(100_000)
        );
    }

    #[test]
    fn resistance_ohm_kohm_mohm() {
        assert_eq!(
            Resistance::try_from(&vwu("1", Unit::Ohm)).unwrap(),
            Resistance(1_000)
        );
        assert_eq!(
            Resistance::try_from(&vwu("10", Unit::Kohm)).unwrap(),
            Resistance(10_000_000)
        );
        assert_eq!(
            Resistance::try_from(&vwu("1", Unit::Mohm)).unwrap(),
            Resistance(1)
        );
    }

    #[test]
    fn impedance_shares_resistance_units() {
        assert_eq!(
            Impedance::try_from(&vwu("90", Unit::Ohm)).unwrap(),
            Impedance(90_000)
        );
    }

    #[test]
    fn wrong_unit_errors() {
        let err = Length::try_from(&vwu("20", Unit::Ohm)).unwrap_err();
        assert!(matches!(
            err,
            ConversionError::WrongUnit {
                expected_quantity: "length",
                ..
            }
        ));
    }

    #[test]
    fn negative_lengths() {
        assert_eq!(
            Length::try_from(&vwu("-5", Unit::Mm)).unwrap(),
            Length(-5_000_000)
        );
    }

    #[test]
    fn invalid_literal() {
        let err = Length::try_from(&vwu("not_a_number", Unit::Mm)).unwrap_err();
        assert!(matches!(err, ConversionError::Overflow { .. }));
    }

    #[test]
    fn capacitance_ranges() {
        assert_eq!(
            Capacitance::try_from(&vwu("100", Unit::Nf)).unwrap(),
            Capacitance(100_000_000)
        );
        assert_eq!(
            Capacitance::try_from(&vwu("10", Unit::Pf)).unwrap(),
            Capacitance(10_000)
        );
        assert_eq!(
            Capacitance::try_from(&vwu("1", Unit::Uf)).unwrap(),
            Capacitance(1_000_000_000)
        );
    }

    #[test]
    fn frequency_mhz_ghz() {
        assert_eq!(
            Frequency::try_from(&vwu("100", Unit::Mhz)).unwrap(),
            Frequency(100_000_000)
        );
        assert_eq!(
            Frequency::try_from(&vwu("2.4", Unit::Ghz)).unwrap(),
            Frequency(2_400_000_000)
        );
    }
}
