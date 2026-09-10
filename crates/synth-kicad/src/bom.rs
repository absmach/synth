// SPDX-License-Identifier: Apache-2.0

//! Bill of materials CSV writer.
//!
//! Columns: `refdes,value,kind,description`. Fields containing
//! commas, quotes, or newlines are RFC-4180 quoted. Output is
//! sorted by refdes for diff stability.

use std::fmt::Write as _;

use synth_ir::Board;

#[allow(clippy::type_complexity)]
pub fn build_bom_csv(board: &Board) -> String {
    let mut rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    )> = board
        .components
        .iter()
        .map(|c| {
            let value = c
                .value
                .clone()
                .or_else(|| c.part.as_ref().map(|p| p.id.as_str().to_string()))
                .unwrap_or_default();
            let description = c
                .part
                .as_ref()
                .and_then(|p| p.description.clone())
                .unwrap_or_default();
            let kicad_symbol = c
                .part
                .as_ref()
                .and_then(|p| p.kicad_symbol.clone())
                .unwrap_or_default();
            let kicad_footprint = c
                .part
                .as_ref()
                .and_then(|p| p.kicad_footprint.clone())
                .unwrap_or_default();
            let lcsc_pn = c
                .part
                .as_ref()
                .and_then(|p| p.lcsc_pn.clone())
                .unwrap_or_default();
            let mpn = c
                .part
                .as_ref()
                .and_then(|p| p.mpn.clone())
                .unwrap_or_default();
            (
                c.refdes.clone(),
                value,
                c.kind.clone(),
                description,
                kicad_symbol,
                kicad_footprint,
                lcsc_pn,
                mpn,
            )
        })
        .collect();
    rows.sort();

    let mut out = String::new();
    out.push_str("refdes,value,kind,description,kicad_symbol,kicad_footprint,lcsc_pn,mpn\n");
    for (refdes, value, kind, description, kicad_symbol, kicad_footprint, lcsc_pn, mpn) in rows {
        write_field(&mut out, &refdes);
        out.push(',');
        write_field(&mut out, &value);
        out.push(',');
        write_field(&mut out, &kind);
        out.push(',');
        write_field(&mut out, &description);
        out.push(',');
        write_field(&mut out, &kicad_symbol);
        out.push(',');
        write_field(&mut out, &kicad_footprint);
        out.push(',');
        write_field(&mut out, &lcsc_pn);
        out.push(',');
        write_field(&mut out, &mpn);
        out.push('\n');
    }
    out
}

/// RFC-4180 quoting: fields containing comma, quote, CR, or LF are
/// surrounded by `"..."` with embedded `"` doubled to `""`.
fn write_field(out: &mut String, s: &str) {
    let needs_quoting = s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r');
    if needs_quoting {
        out.push('"');
        for c in s.chars() {
            if c == '"' {
                out.push_str("\"\"");
            } else {
                out.push(c);
            }
        }
        out.push('"');
    } else {
        // Use the write_str trait method to satisfy clippy::format_push_string.
        out.write_str(s).expect("write to String never fails");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_doubles_internal_quotes() {
        let mut s = String::new();
        write_field(&mut s, r#"a "b" c"#);
        assert_eq!(s, r#""a ""b"" c""#);
    }

    #[test]
    fn no_quoting_when_simple() {
        let mut s = String::new();
        write_field(&mut s, "simple");
        assert_eq!(s, "simple");
    }

    #[test]
    fn comma_triggers_quoting() {
        let mut s = String::new();
        write_field(&mut s, "a,b");
        assert_eq!(s, "\"a,b\"");
    }
}
