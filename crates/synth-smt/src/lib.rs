// SPDX-License-Identifier: Apache-2.0

//! Lightweight, pure-Rust single-variable SMT-LIB2 assertion solver.
//!
//! Evaluates linear quantitative constraints emitted by Synth ERC rules in sub-microsecond time,
//! avoiding external C/C++ dependencies (like Z3).

#![forbid(unsafe_code)]

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SmtError {
    #[error("unsatisfiable constraint")]
    Unsatisfiable,
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("unsupported constraint form: {0}")]
    Unsupported(String),
}

/// A solved constraint outcome describing the variable name, operator, and target numeric value.
#[derive(Debug, Clone, PartialEq)]
pub struct SmtSolution {
    pub variable: String,
    pub operator: String,
    pub target_value: f64,
}

/// Solves a single-variable SMT-LIB2 assertion string and returns the minimum/target non-negative float value.
///
/// Supported assertion forms:
/// - `(assert (= VAR VALUE))` -> returns `VALUE`
/// - `(assert (>= VAR VALUE))` -> returns `VALUE`
/// - `(assert (<= VAR VALUE))` -> returns `VALUE`
/// - `(assert (> VAR VALUE))` -> returns `VALUE + 1` (or next step)
/// - `(assert (< VAR VALUE))` -> returns `VALUE - 1` (or previous step)
#[must_use]
pub fn solve_minimum(constraint: &str) -> Option<f64> {
    solve_constraint(constraint).ok().map(|s| s.target_value)
}

/// Parses and solves a single-variable SMT-LIB2 assertion string.
pub fn solve_constraint(constraint: &str) -> Result<SmtSolution, SmtError> {
    let tokens = tokenize(constraint);
    if tokens.len() < 5 {
        return Err(SmtError::ParseError("too few tokens".into()));
    }

    // Expect form: "(" "assert" "(" OP ARG1 ARG2 ")" ")"
    if tokens[0] != "(" || tokens[1] != "assert" || tokens[2] != "(" {
        return Err(SmtError::Unsupported(constraint.to_string()));
    }

    let op = &tokens[3];
    let arg1 = &tokens[4];
    if tokens.len() < 6 {
        return Err(SmtError::ParseError("missing second argument".into()));
    }
    let arg2 = &tokens[5];

    let (var, val_str) = if let Ok(_num) = arg1.parse::<f64>() {
        (arg2.clone(), arg1.clone())
    } else if let Ok(_num) = arg2.parse::<f64>() {
        (arg1.clone(), arg2.clone())
    } else {
        return Err(SmtError::Unsupported(
            "expected one variable and one number".into(),
        ));
    };

    let raw_val: f64 = val_str
        .parse()
        .map_err(|_| SmtError::ParseError(format!("invalid numeric value '{val_str}'")))?;

    let target_value = match op.as_str() {
        "=" | ">=" | "<=" => raw_val,
        ">" => raw_val + 1.0,
        "<" => raw_val - 1.0,
        _ => {
            return Err(SmtError::Unsupported(format!(
                "unsupported operator '{op}'"
            )))
        }
    };

    Ok(SmtSolution {
        variable: var,
        operator: op.clone(),
        target_value,
    })
}

fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in input.chars() {
        if ch == '(' || ch == ')' {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            tokens.push(ch.to_string());
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_solve_equality() {
        let sol = solve_constraint("(assert (= impedance 90))").unwrap();
        assert_eq!(sol.variable, "impedance");
        assert_eq!(sol.operator, "=");
        assert_eq!(sol.target_value, 90.0);
        assert_eq!(solve_minimum("(assert (= impedance 90))"), Some(90.0));
    }

    #[test]
    fn test_solve_greater_than_or_equal() {
        let sol = solve_constraint("(assert (>= decoupling_count 2))").unwrap();
        assert_eq!(sol.variable, "decoupling_count");
        assert_eq!(sol.operator, ">=");
        assert_eq!(sol.target_value, 2.0);
        assert_eq!(solve_minimum("(assert (>= decoupling_count 2))"), Some(2.0));
    }

    #[test]
    fn test_solve_rf_feed() {
        assert_eq!(solve_minimum("(assert (= impedance 50))"), Some(50.0));
    }

    #[test]
    fn test_solve_residual_score() {
        assert_eq!(solve_minimum("(assert (<= residual_score 3.0))"), Some(3.0));
    }

    #[test]
    fn test_solve_strict_inequalities() {
        assert_eq!(solve_minimum("(assert (> count 5))"), Some(6.0));
        assert_eq!(solve_minimum("(assert (< count 5))"), Some(4.0));
    }

    #[test]
    fn test_invalid_constraints() {
        assert!(solve_constraint("invalid").is_err());
        assert!(solve_constraint("(assert (= a b))").is_err());
    }
}
