// SPDX-License-Identifier: Apache-2.0

//! Bill of materials CSV writer.
//!
//! Columns: `refdes,value,kind,description`. Fields containing
//! commas, quotes, or newlines are RFC-4180 quoted. Output is
//! sorted by refdes for diff stability.
//!
//! Do-not-populate parts are left out: a DNP line in the BOM would
//! order a part the build must not place. (ERC still checks DNP
//! parts; the KiCad schematic still draws them with `(dnp yes)`.)

use std::collections::BTreeSet;
use std::fmt::Write as _;

use synth_ir::{Board, Variant};

/// Base BOM: every component that is not do-not-populate.
pub fn build_bom_csv(board: &Board) -> String {
    build_bom_csv_filtered(board, &BTreeSet::new())
}

/// BOM for one design variant: the base BOM with the variant's
/// do-not-populate overrides applied on top of the component `dnp`
/// flags.
pub fn build_bom_csv_for_variant(board: &Board, variant: &Variant) -> String {
    let extra: BTreeSet<&str> = variant.dnp.iter().map(String::as_str).collect();
    build_bom_csv_filtered(board, &extra)
}

#[allow(clippy::type_complexity)]
fn build_bom_csv_filtered(board: &Board, extra_dnp: &BTreeSet<&str>) -> String {
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
        .filter(|c| !c.dnp && !extra_dnp.contains(c.refdes.as_str()))
        .map(|c| {
            // Schematic-quality plan Phase A1: `value` → part `mpn` →
            // `(no value)` sentinel, never the registry part id. An
            // unorderable BOM cell must look unorderable
            // (`E-SYNTH-VALUE-001` fires for the generic case).
            let value = c
                .value
                .clone()
                .or_else(|| c.part.as_ref().and_then(|p| p.mpn.clone()))
                .unwrap_or_else(|| "(no value)".to_string());
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
    fn variant_bom_applies_dnp_overrides() {
        use synth_ir::{Component, ComponentId, Variant};
        let mk = |refdes: &str, dnp: bool| Component {
            id: ComponentId(0),
            refdes: refdes.to_string(),
            kind: "resistor".to_string(),
            part: None,
            value: None,
            dnp,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: synth_diagnostics::Span::new(0, 0),
        };
        let board = Board {
            legends: false,
            name: "b".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: vec![mk("R1", false), mk("R2", false), mk("R3", true)],
            nets: vec![],
            diff_pairs: vec![],
            notes: vec![],
            keepouts: vec![],
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: synth_diagnostics::Span::new(0, 0),
        };
        // Base BOM drops the component-level dnp part.
        let base = build_bom_csv(&board);
        assert!(base.contains("R1") && base.contains("R2") && !base.contains("R3"));
        // The variant drops its own override on top.
        let variant = Variant {
            name: "lite".to_string(),
            description: None,
            dnp: vec!["R2".to_string()],
        };
        let lite = build_bom_csv_for_variant(&board, &variant);
        assert!(lite.contains("R1") && !lite.contains("R2") && !lite.contains("R3"));
    }

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

    // Schematic-quality plan Phase A1: `value` → part `mpn` →
    // `(no value)` sentinel, never the registry part id.
    #[test]
    fn value_fallback_chain_never_uses_part_id() {
        use std::path::Path;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .unwrap();
        let registry = synth_registry::load_dir(&root.join("registry").join("parts")).unwrap();
        let src = r#"board "b" {
            component R1: resistor "r_generic_0603" value "10k"
            component R2: resistor "r_generic_0603"
            component U1: regulator "ams1117_3v3"
            connect R1.p1 -> R2.p1
            connect R1.p2 -> R2.p2
            connect U1.vin -> R1.p1
            connect U1.gnd -> R1.p2
            connect U1.vout -> R2.p1
        }"#;
        let parsed = synth_parser::parse(src, "inline.synth");
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let board = synth_ir::lower(&parsed.ast.unwrap(), &registry, "inline.synth")
            .board
            .unwrap();
        let csv = build_bom_csv(&board);
        let value_of = |refdes: &str| {
            csv.lines()
                .find(|line| line.starts_with(&format!("{refdes},")))
                .unwrap_or_else(|| panic!("{refdes} missing:\n{csv}"))
                .split(',')
                .nth(1)
                .unwrap()
                .to_string()
        };
        assert_eq!(value_of("R1"), "10k");
        assert_eq!(value_of("R2"), "(no value)");
        assert_eq!(value_of("U1"), "AMS1117-3.3");
        assert!(
            !csv.contains("r_generic_0603"),
            "part id must never leak into the value column:\n{csv}"
        );
    }

    #[test]
    fn dnp_parts_are_excluded() {
        use std::path::Path;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .unwrap();
        let registry = synth_registry::load_dir(&root.join("registry").join("parts")).unwrap();
        let src = r#"board "b" {
            component R1: resistor "r_generic_0603"
            component R2: resistor "r_generic_0603" dnp
            connect R1.p1 -> R2.p1
            connect R1.p2 -> R2.p2
        }"#;
        let parsed = synth_parser::parse(src, "inline.synth");
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let board = synth_ir::lower(&parsed.ast.unwrap(), &registry, "inline.synth")
            .board
            .unwrap();
        let csv = build_bom_csv(&board);
        assert!(csv.contains("R1"), "populated part must be listed");
        assert!(
            !csv.lines().any(|line| line.starts_with("R2,")),
            "DNP part must be left out:\n{csv}"
        );
    }
}
