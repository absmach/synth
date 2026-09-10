// SPDX-License-Identifier: Apache-2.0

//! Minimal s-expression writer for KiCad files.
//!
//! KiCad's `.kicad_sch`, `.kicad_sym`, `.kicad_pcb`, and `.kicad_mod`
//! formats are s-expressions with a fixed indentation convention:
//! tabs for hierarchy, atoms separated by single spaces, strings
//! quoted with `"…"`. The writer here renders [`Sexp`] trees to a
//! `String` in that convention so output is byte-stable.
//!
//! This module deliberately does *not* parse s-expressions. The
//! exporter is one-way; we never read KiCad files.

/// One node in an s-expression tree. `List` is a head atom followed
/// by zero or more child nodes.
#[derive(Debug, Clone)]
pub enum Sexp {
    /// A bare atom — written verbatim, no quoting. Use for keywords
    /// (`kicad_sch`, `version`) and identifiers.
    Atom(String),
    /// A string literal — written with quotes and `\"`/`\\` escaping.
    Str(String),
    /// A list whose first element is `head` followed by `children`.
    List { head: String, children: Vec<Sexp> },
    /// Raw pre-formatted s-expression text. Emitted verbatim with
    /// indentation prefixed on each line. Used when re-embedding
    /// KiCad stock library symbol definitions whose internal
    /// structure we don't want to round-trip through the parser
    /// (no parser in this crate — just an emitter).
    Raw(String),
}

impl Sexp {
    pub fn atom(s: impl Into<String>) -> Self {
        Sexp::Atom(s.into())
    }

    pub fn str(s: impl Into<String>) -> Self {
        Sexp::Str(s.into())
    }

    pub fn list(head: impl Into<String>, children: Vec<Sexp>) -> Self {
        Sexp::List {
            head: head.into(),
            children,
        }
    }

    /// Render the tree to a `String` with KiCad's indentation
    /// convention (tabs).
    pub fn to_string_pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out.push('\n');
        out
    }

    fn write(&self, out: &mut String, depth: usize) {
        match self {
            Sexp::Atom(a) => out.push_str(a),
            Sexp::Str(s) => write_quoted(out, s),
            Sexp::Raw(text) => {
                // Emit verbatim. The caller already placed the leading
                // tab/newline for the first line. The source text
                // carries its own indentation for inner lines, so we
                // just preserve line breaks unchanged.
                out.push_str(text.trim_end());
            }
            Sexp::List { head, children } => {
                out.push('(');
                out.push_str(head);
                // Any list whose children include a sub-list breaks
                // to multiple lines for KiCad's idiomatic shape.
                let multiline = children
                    .iter()
                    .any(|c| matches!(c, Sexp::List { .. } | Sexp::Raw(_)));
                if multiline {
                    for c in children {
                        out.push('\n');
                        for _ in 0..=depth {
                            out.push('\t');
                        }
                        c.write(out, depth + 1);
                    }
                    out.push('\n');
                    for _ in 0..depth {
                        out.push('\t');
                    }
                } else {
                    for c in children {
                        out.push(' ');
                        c.write(out, depth + 1);
                    }
                }
                out.push(')');
            }
        }
    }
}

fn write_quoted(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out.push('"');
}

// -----------------------------------------------------------------------------
// Builder helpers used throughout the exporter
// -----------------------------------------------------------------------------

/// `(name value)` for two-atom pairs like `(version 20231120)`.
pub fn pair(name: &str, value: Sexp) -> Sexp {
    Sexp::list(name, vec![value])
}

/// `(name "value")` for two-atom pairs whose value is a string.
pub fn str_pair(name: &str, value: impl Into<String>) -> Sexp {
    Sexp::list(name, vec![Sexp::str(value)])
}

/// Render an `f64` with the precision KiCad expects (no trailing
/// zeros beyond 4 decimal places; integer form when possible).
pub fn num(v: f64) -> Sexp {
    Sexp::Atom(format_num(v))
}

fn format_num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        // Up to 4 decimals, trailing zeros stripped.
        let s = format!("{v:.4}");
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atoms_render_verbatim() {
        assert_eq!(
            Sexp::atom("kicad_sch").to_string_pretty().trim(),
            "kicad_sch"
        );
    }

    #[test]
    fn strings_get_quoted_and_escaped() {
        let rendered = Sexp::str("path \"with\" quote").to_string_pretty();
        assert_eq!(rendered.trim(), "\"path \\\"with\\\" quote\"");
    }

    #[test]
    fn small_list_inlines() {
        let s = Sexp::list("version", vec![Sexp::atom("20231120")]);
        assert_eq!(s.to_string_pretty().trim(), "(version 20231120)");
    }

    #[test]
    fn nested_list_indents_with_tabs() {
        let s = Sexp::list(
            "kicad_sch",
            vec![
                Sexp::list("version", vec![Sexp::atom("20231120")]),
                Sexp::list("generator", vec![Sexp::str("synth-eda")]),
            ],
        );
        let rendered = s.to_string_pretty();
        // Outer list spans multiple lines because children are nested lists.
        assert!(rendered.contains("\n\t(version 20231120)"));
        assert!(rendered.contains("\n\t(generator \"synth-eda\")"));
    }

    #[test]
    fn integers_format_without_decimal() {
        assert_eq!(format_num(50.0), "50");
        assert_eq!(format_num(0.0), "0");
        assert_eq!(format_num(-25.0), "-25");
    }

    #[test]
    fn decimals_trim_trailing_zeros() {
        assert_eq!(format_num(2.54), "2.54");
        assert_eq!(format_num(2.5400), "2.54");
        assert_eq!(format_num(0.508), "0.508");
    }
}
