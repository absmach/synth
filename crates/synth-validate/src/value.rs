// SPDX-License-Identifier: Apache-2.0

//! Component value parsing for value-based ERC.
//!
//! [`synth_ir::Component::value`] carries a free-form display string
//! (`"4.7k"`, `"18pF"`, `"1M"`). These helpers expand SI prefixes and
//! trailing unit tags so rules can reason about actual magnitudes
//! instead of raw text — the foundation for checks that go beyond
//! pure topology (divider ratios, crystal load caps, derating, …).
//!
//! Recognised forms (the unit tag is matched case-insensitively):
//!
//! | Kind         | Examples                                            | Returns       |
//! | ------------ | --------------------------------------------------- | ------------- |
//! | resistance   | `100`, `100R`, `4.7k`, `1M`, `10kΩ`, `2.2kOhm`       | ohms          |
//! | capacitance  | `22`, `22p`, `18pF`, `100nF`, `0.1uF`, `10µF`, `10μF` | farads        |
//!
//! SI prefixes: `p`(1e-12) `n`(1e-9) `µ/u`(1e-6) `m`(1e-3) `k/K`(1e3)
//! `M`(1e6) `G`(1e9); an absent prefix means base units. Both the micro
//! sign (`µ`, U+00B5) and Greek mu (`μ`, U+03BC) are accepted.
//!
//! Parsing is deliberately total and local: anything unrecognised yields
//! [`None`] and the caller falls back to a topology-only check. The
//! European `4k7` convention is not supported; write `4.7k`.

/// Parse an ohmic resistance value, returning ohms, or `None` if the
/// string is not a recognisable SI quantity.
pub fn parse_resistance(input: &str) -> Option<f64> {
    parse_quantity(input, &["Ohm", "Ω", "R"])
}

/// Parse a capacitance value, returning farads, or `None` if the string
/// is not a recognisable SI quantity.
pub fn parse_capacitance(input: &str) -> Option<f64> {
    parse_quantity(input, &["F"])
}

/// Shared SI-quantity parser.
///
/// Splits the numeric head from a trailing `<prefix>[unit]` tail,
/// strips a known unit tag (longest first) when present, then maps the
/// leftover SI prefix to its multiplier. A non-prefix leftover (e.g.
/// `R0` in `1R0`) yields `None`.
fn parse_quantity(input: &str, units: &[&str]) -> Option<f64> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }

    // Numeric head: digits with an optional decimal point and sign.
    let num_end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '+' || c == '-'))
        .unwrap_or(s.len());
    let num = &s[..num_end];
    if num.is_empty() || !num.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let value: f64 = num.parse().ok()?;

    let suffix = s[num_end..].trim();
    let mut prefix = suffix;
    // Strip a trailing unit tag (matched at the end, case-insensitive),
    // leaving only a valid SI-prefix region. Longest tag first so
    // `kOhm` strips `Ohm` and leaves `k`, not `kO`.
    for unit in units {
        if suffix.len() >= unit.len()
            && suffix[suffix.len() - unit.len()..].eq_ignore_ascii_case(unit)
        {
            let before = &suffix[..suffix.len() - unit.len()];
            if before
                .chars()
                .all(|c| c.is_ascii_alphabetic() || c == 'µ' || c == 'μ')
            {
                prefix = before;
                break;
            }
        }
    }

    let multiplier = si_multiplier(prefix.trim())?;
    Some(value * multiplier)
}

/// Map an SI prefix string to its multiplier. `""` is the base unit.
fn si_multiplier(prefix: &str) -> Option<f64> {
    Some(match prefix {
        "" => 1.0,
        "p" => 1e-12,
        "n" => 1e-9,
        "u" | "µ" | "μ" => 1e-6,
        "m" => 1e-3,
        "k" | "K" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Approximate equality so floating-point products of decimal
    /// inputs (e.g. `4.7 * 1e3`) don't trip exact `==`.
    fn close(a: Option<f64>, b: Option<f64>) -> bool {
        match (a, b) {
            (Some(x), Some(y)) => {
                let scale = x.abs().max(y.abs()).max(1.0);
                (x - y).abs() < 1e-9 * scale
            }
            (None, None) => true,
            _ => false,
        }
    }

    #[test]
    fn resistance_parsing() {
        assert!(close(parse_resistance("100"), Some(100.0)));
        assert!(close(parse_resistance("100R"), Some(100.0)));
        assert!(close(parse_resistance("220"), Some(220.0)));
        assert!(close(parse_resistance("4.7k"), Some(4_700.0)));
        assert!(close(parse_resistance("4.7K"), Some(4_700.0)));
        assert!(close(parse_resistance("1M"), Some(1_000_000.0)));
        assert!(close(parse_resistance("10kΩ"), Some(10_000.0)));
        assert!(close(parse_resistance("1MΩ"), Some(1_000_000.0)));
        assert!(close(parse_resistance("2.2kOhm"), Some(2_200.0)));
        assert!(close(parse_resistance("100kR"), Some(100_000.0)));
    }

    #[test]
    fn resistance_rejects_garbage() {
        assert!(parse_resistance("").is_none());
        assert!(parse_resistance("abc").is_none());
        assert!(parse_resistance("4k7").is_none());
        assert!(parse_resistance("k").is_none());
    }

    #[test]
    fn capacitance_parsing() {
        assert!(close(parse_capacitance("22"), Some(22.0)));
        assert!(close(parse_capacitance("22p"), Some(22e-12)));
        assert!(close(parse_capacitance("18pF"), Some(18e-12)));
        assert!(close(parse_capacitance("100nF"), Some(100e-9)));
        assert!(close(parse_capacitance("0.1uF"), Some(0.1e-6)));
        assert!(close(parse_capacitance("10µF"), Some(10e-6)));
        assert!(close(parse_capacitance("10μF"), Some(10e-6)));
        assert!(close(parse_capacitance("1u"), Some(1e-6)));
        assert!(close(parse_capacitance("1F"), Some(1.0)));
    }

    #[test]
    fn capacitance_rejects_garbage() {
        assert!(parse_capacitance("").is_none());
        assert!(parse_capacitance("nope").is_none());
        assert!(parse_capacitance("18Fp").is_none());
    }
}
