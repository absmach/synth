// SPDX-License-Identifier: Apache-2.0

//! Clean-room EasyEDA → KiCad footprint converter (Phase 15, R15.4).
//!
//! This module parses the JSON document that EasyEDA/LCSC serve for a component
//! (the same `shape` primitive list the editor renders) and emits a KiCad
//! `.kicad_mod` footprint plus a `<id>.synth.toml` skeleton.
//!
//! IMPORTANT: this is original, clean-room code. It reimplements the documented
//! EasyEDA coordinate/layer model from scratch — it does NOT derive from
//! `easyeda2kicad.py` (which is AGPL-3.0). Only the public wire format
//! (endpoint shapes, primitive `type` strings) is relied upon.
//!
//! Licensing (R15.11): raw LCSC/EasyEDA CAD documents are fetched at import
//! time and converted immediately; they are NEVER persisted or redistributed
//! by Synth. Only the derived `.kicad_mod` / `.synth.toml` artifacts are
//! written. See `registry/CREDITS.md` for the full licensing posture.

use std::fmt::Write as _;

use serde::Deserialize;

/// 1 EasyEDA canvas unit = 10 mil = 0.254 mm.
const UNIT_TO_MM: f64 = 0.254;

/// EasyEDA uses a Y-down screen coordinate system; KiCad footprints are Y-up.
/// We convert units and flip Y so the imported part orients correctly.
fn mx(x: f64) -> f64 {
    x * UNIT_TO_MM
}
fn my(y: f64) -> f64 {
    -y * UNIT_TO_MM
}

/// Top-level EasyEDA component document.
#[derive(Debug, Deserialize)]
pub struct EasyEdaComponent {
    #[serde(default)]
    pub head: serde_json::Value,
    #[serde(default)]
    pub canvas: serde_json::Value,
    /// Ordered list of drawing/footprint primitives.
    #[serde(default)]
    pub shape: Vec<serde_json::Value>,
    /// Older API envelope sometimes nests the geometry under `data`.
    #[serde(default)]
    pub data: serde_json::Value,
}

impl EasyEdaComponent {
    /// Return the effective primitive list, tolerating both envelope shapes.
    pub fn primitives(&self) -> &[serde_json::Value] {
        if !self.shape.is_empty() {
            &self.shape
        } else if let Some(arr) = self.data.get("shape").and_then(|v| v.as_array()) {
            arr
        } else {
            &self.shape
        }
    }
}

/// Parse a raw EasyEDA component JSON document.
pub fn parse_easyeda(json: &str) -> anyhow::Result<EasyEdaComponent> {
    let comp: EasyEdaComponent = serde_json::from_str(json)?;
    Ok(comp)
}

/// Map an EasyEDA layer name to a KiCad layer token.
fn kicad_layer(ez: &str) -> &'static str {
    match ez {
        "TopLayer" => "F.Cu",
        "BottomLayer" => "B.Cu",
        "BottomSilkLayer" => "B.SilkS",
        "TopSolderLayer" | "TopSolderMaskLayer" => "F.Mask",
        "BottomSolderLayer" | "BottomSolderMaskLayer" => "B.Mask",
        "TopPasteMaskLayer" => "F.Paste",
        "BottomPasteMaskLayer" => "B.Paste",
        "TopLayerNote" | "TopNotes" | "Docu" | "TopDocument" => "F.Fab",
        "BottomDocument" => "B.Fab",
        "EdgeCuts" => "Edge.Cuts",
        "Multi-Layer" | "MultiLayer" => "*.Cu",
        _ => "F.SilkS",
    }
}

/// Map an EasyEDA pad shape to a KiCad pad shape token.
fn kicad_pad_shape(ez: &str, w: f64, h: f64) -> &'static str {
    match ez {
        "OVAL" => "oval",
        "ELLIPSE" => {
            if (w - h).abs() < 0.01 {
                "circle"
            } else {
                "oval"
            }
        }
        "POLYGON" => "roundrect",
        _ => "rect",
    }
}

fn f(v: &serde_json::Value, key: &str) -> f64 {
    v.get(key)
        .and_then(serde_json::Value::as_f64)
        .or_else(|| {
            v.get(key)
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0.0)
}

fn s(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .unwrap_or_default()
}

/// Read a bare numeric `Value` (used for array elements like `[x, y]`).
fn num(v: &serde_json::Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0.0)
}

/// Emit a KiCad `(pad ...)` for an EasyEDA PAD primitive.
#[allow(clippy::many_single_char_names)]
fn emit_pad(p: &serde_json::Value, out: &mut String) {
    let num = s(p, "pinNumber");
    let x = mx(f(p, "x"));
    let y = my(f(p, "y"));
    let w = (f(p, "width") * UNIT_TO_MM).max(0.01);
    let h = (f(p, "height") * UNIT_TO_MM).max(0.01);
    let hole_r = f(p, "holeR");
    let layer = s(p, "layer");
    let shape = kicad_pad_shape(&s(p, "shape"), w, h);

    if hole_r > 0.0 {
        // Through-hole pad.
        let drill = (hole_r * 2.0 * UNIT_TO_MM).max(0.1);
        let _ = writeln!(
            out,
            "    (pad \"{num}\" thru_hole {shape} (at {x:.3} {y:.3}) (size {w:.3} {h:.3}) (drill {drill:.3}) (layers \"*.Cu\" \"*.Mask\"))"
        );
    } else {
        // SMD pad — place on the primitive's copper layer.
        let prefix = if layer == "BottomLayer" { "B" } else { "F" };
        let _ = writeln!(
            out,
            "    (pad \"{num}\" smd {shape} (at {x:.3} {y:.3}) (size {w:.3} {h:.3}) (layers \"{prefix}.Cu\" \"{prefix}.Paste\" \"{prefix}.Mask\"))"
        );
    }
}

/// Emit a KiCad `(pad "" np_thru_hole ...)` for a non-plated mounting HOLE.
#[allow(clippy::many_single_char_names)]
fn emit_hole(p: &serde_json::Value, out: &mut String) {
    let x = mx(f(p, "x"));
    let y = my(f(p, "y"));
    let r = f(p, "r").max(f(p, "holeR"));
    let d = (r * 2.0 * UNIT_TO_MM).max(0.1);
    let _ = writeln!(
        out,
        "    (pad \"\" np_thru_hole circle (at {x:.3} {y:.3}) (size {d:.3} {d:.3}) (drill {d:.3}) (layers \"*.Cu\"))"
    );
}

/// Emit an `(fp_line ...)` for a TRACK primitive (or a poly-line from points).
fn emit_track(p: &serde_json::Value, out: &mut String) {
    let layer = kicad_layer(&s(p, "layer"));
    let width = (f(p, "width") * UNIT_TO_MM).max(0.01);
    // Build segment list from the explicit `points` array of [x, y] pairs.
    if let Some(arr) = p.get("points").and_then(|v| v.as_array()) {
        let mut prev: Option<(f64, f64)> = None;
        for pair in arr {
            if let Some(coords) = pair.as_array() {
                if coords.len() < 2 {
                    continue;
                }
                let x = mx(num(&coords[0]));
                let y = my(num(&coords[1]));
                if let Some((px, py)) = prev {
                    let _ = writeln!(
                        out,
                        "    (fp_line (start {px:.3} {py:.3}) (end {x:.3} {y:.3}) (layer \"{layer}\") (width {width:.3}))"
                    );
                }
                prev = Some((x, y));
            }
        }
    } else {
        // Fallback: a degenerate single-point segment — rare for TRACK.
        let x = mx(f(p, "x"));
        let y = my(f(p, "y"));
        let _ = writeln!(
            out,
            "    (fp_line (start {x:.3} {y:.3}) (end {x:.3} {y:.3}) (layer \"{layer}\") (width {width:.3}))"
        );
    }
}

/// Emit an `(fp_rect ...)` for a RECT primitive.
fn emit_rect(p: &serde_json::Value, out: &mut String) {
    let layer = kicad_layer(&s(p, "layer"));
    let w = (f(p, "width") * UNIT_TO_MM).max(0.001);
    let h = (f(p, "height") * UNIT_TO_MM).max(0.001);
    let cx = mx(f(p, "x"));
    let cy = my(f(p, "y"));
    let x1 = cx - w / 2.0;
    let y1 = cy - h / 2.0;
    let x2 = cx + w / 2.0;
    let y2 = cy + h / 2.0;
    let width = 0.1;
    let _ = writeln!(
        out,
        "    (fp_rect (start {x1:.3} {y1:.3}) (end {x2:.3} {y2:.3}) (layer \"{layer}\") (width {width:.3}) (fill none))"
    );
}

/// Emit an `(fp_circle ...)` for a CIRCLE primitive.
fn emit_circle(p: &serde_json::Value, out: &mut String) {
    let layer = kicad_layer(&s(p, "layer"));
    let cx = mx(f(p, "x"));
    let cy = my(f(p, "y"));
    let r = (f(p, "r") * UNIT_TO_MM).max(0.001);
    let width = 0.1;
    let _ = writeln!(
        out,
        "    (fp_circle (center {cx:.3} {cy:.3}) (end {cx:.3} {cy2:.3}) (layer \"{layer}\") (width {width:.3}))",
        cy2 = cy + r
    );
}

/// Emit an `(fp_text user ...)` for a TEXT primitive.
fn emit_text(p: &serde_json::Value, out: &mut String) {
    let layer = kicad_layer(&s(p, "layer"));
    let x = mx(f(p, "x"));
    let y = my(f(p, "y"));
    let text = s(p, "text").replace('"', "'");
    let _ = writeln!(
        out,
        "    (fp_text user \"{text}\" (at {x:.3} {y:.3}) (layer \"{layer}\") (effects (font (size 1.0 1.0))))"
    );
}

/// State accumulated while walking primitives.
struct FootprintBuilder {
    pads: Vec<String>,
    holes: Vec<String>,
    lines: Vec<String>,
    rects: Vec<String>,
    circles: Vec<String>,
    texts: Vec<String>,
}

impl FootprintBuilder {
    fn has_thru_hole(&self) -> bool {
        self.pads.iter().any(|p| p.contains("thru_hole"))
    }
}

/// Convert a parsed EasyEDA component into a KiCad `.kicad_mod` string.
pub fn to_kicad_mod(comp: &EasyEdaComponent, lib_id: &str) -> String {
    let mut b = FootprintBuilder {
        pads: Vec::new(),
        holes: Vec::new(),
        lines: Vec::new(),
        rects: Vec::new(),
        circles: Vec::new(),
        texts: Vec::new(),
    };

    for prim in comp.primitives() {
        let ty = s(prim, "type").to_uppercase();
        let mut line = String::new();
        match ty.as_str() {
            "PAD" => emit_pad(prim, &mut line),
            "HOLE" => emit_hole(prim, &mut line),
            "TRACK" | "LINE" => emit_track(prim, &mut line),
            "RECT" => emit_rect(prim, &mut line),
            "CIRCLE" => emit_circle(prim, &mut line),
            "TEXT" | "TEXTNOTE" => emit_text(prim, &mut line),
            // VIA / ARC / SVGNODE / IMAGE are skipped (footprints carry no vias;
            // arcs/images are decorative and out of scope for a first cut).
            _ => continue,
        }
        if !line.is_empty() {
            match ty.as_str() {
                "PAD" => b.pads.push(line),
                "HOLE" => b.holes.push(line),
                "TRACK" | "LINE" => b.lines.push(line),
                "RECT" => b.rects.push(line),
                "CIRCLE" => b.circles.push(line),
                "TEXT" | "TEXTNOTE" => b.texts.push(line),
                _ => {}
            }
        }
    }

    let attr = if b.has_thru_hole() {
        "through_hole"
    } else {
        "smd"
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "(footprint \"{lib_id}\" (version 20211014) (generator synth) (layer \"F.Cu\")"
    );
    let _ = writeln!(out, "  (attr {attr})");
    let _ = writeln!(
        out,
        "  (fp_text reference \"REF**\" (at 0 0) (layer \"F.SilkS\") (effects (font (size 1.0 1.0))))"
    );
    let _ = writeln!(
        out,
        "  (fp_text value \"{lib_id}\" (at 0 0) (layer \"F.Fab\") (effects (font (size 1.0 1.0))))"
    );
    for line in &b.lines {
        let _ = writeln!(out, "{line}");
    }
    for r in &b.rects {
        let _ = writeln!(out, "{r}");
    }
    for c in &b.circles {
        let _ = writeln!(out, "{c}");
    }
    for t in &b.texts {
        let _ = writeln!(out, "{t}");
    }
    for h in &b.holes {
        let _ = writeln!(out, "{h}");
    }
    for p in &b.pads {
        let _ = writeln!(out, "{p}");
    }
    let _ = writeln!(out, ")");
    out
}

/// Extract the ordered pad pin numbers from a component (for the `.synth.toml` skeleton).
pub fn extract_pins(comp: &EasyEdaComponent) -> Vec<String> {
    let mut pins = Vec::new();
    for prim in comp.primitives() {
        if s(prim, "type").eq_ignore_ascii_case("PAD") {
            let num = s(prim, "pinNumber");
            if !num.is_empty() && !pins.contains(&num) {
                pins.push(num);
            }
        }
    }
    pins.sort_by_key(|p| p.parse::<u32>().unwrap_or(u32::MAX));
    pins
}

/// Generate a `<id>.synth.toml` skeleton for an imported part. The part is
/// marked `provenance.source = "imported"` with an empty `reviewed_by`, so the
/// `UnverifiedPartRule` (W-SYNTH-PART-UNVERIFIED) still flags it until a human
/// reviews the pinout.
pub fn generate_part_toml(
    id: &str,
    lcsc: &str,
    mpn: &str,
    description: &str,
    sourced_from: &str,
    pins: &[String],
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "id = \"{id}\"");
    let _ = writeln!(out, "kind = \"ic\"");
    let desc = description.replace('"', "'");
    let _ = writeln!(out, "description = \"{desc}\"");
    if !mpn.is_empty() {
        let _ = writeln!(out, "mpn = \"{mpn}\"");
    }
    let _ = writeln!(out, "lcsc_pn = \"{lcsc}\"");
    let _ = writeln!(out, "kicad_footprint = \"{id}:{id}\"");
    let _ = writeln!(out);
    for pin in pins {
        let _ = writeln!(out, "[[pins]]");
        let _ = writeln!(out, "name = \"{pin}\"");
        let _ = writeln!(out, "number = \"{pin}\"");
        let _ = writeln!(
            out,
            "electrical_type = \"bidirectional\"  # TODO: confirm from datasheet"
        );
        let _ = writeln!(out, "required = false");
    }
    let _ = writeln!(
        out,
        "  # NOTE: pins default to bidirectional; set true electrical"
    );
    let _ = writeln!(out, "  # types from the datasheet before fabrication.");
    let _ = writeln!(out);
    let _ = writeln!(out, "[provenance]");
    // R15.3: LCSC/EasyEDA imports are `generated` (converted from CAD data),
    // distinct from `imported` (`synth part import kicad`, stock library).
    let _ = writeln!(out, "source = \"generated\"");
    let _ = writeln!(out, "generator = \"synth-part-import-lcsc 0.1\"");
    let src = sourced_from.replace('"', "'");
    let _ = writeln!(out, "datasheet_url = \"{src}\"");
    let _ = writeln!(out, "upstream_lcsc_pn = \"{lcsc}\"");
    let _ = writeln!(out, "reviewed_by = \"\"");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-authored EasyEDA component document covering the primitives the
    /// converter supports. Mirrors the documented `shape` primitive layout;
    /// it is NOT derived from any AGPL tooling.
    const FIXTURE: &str = r#"{
      "head": { "c_para": {} },
      "canvas": { "width": 100, "height": 100 },
      "shape": [
        { "type": "PAD", "x": 0, "y": 50, "width": 20, "height": 10, "shape": "RECT", "layer": "TopLayer", "pinNumber": "1", "net": 1 },
        { "type": "PAD", "x": 0, "y": -50, "width": 20, "height": 10, "shape": "RECT", "layer": "TopLayer", "pinNumber": "2", "net": 2 },
        { "type": "RECT", "x": 0, "y": 0, "width": 60, "height": 120, "layer": "TopSilkLayer" },
        { "type": "TRACK", "x": 0, "y": 0, "width": 5, "layer": "TopLayer", "points": [[10, 10], [20, 10], [20, 20]] },
        { "type": "CIRCLE", "x": 0, "y": 0, "r": 15, "layer": "TopSilkLayer" },
        { "type": "TEXT", "x": 0, "y": 0, "text": "U1", "layer": "TopSilkLayer" },
        { "type": "HOLE", "x": 30, "y": 0, "r": 8 }
      ]
    }"#;

    #[test]
    fn parses_and_converts_fixture() {
        let comp = parse_easyeda(FIXTURE).expect("fixture parses");
        let modl = to_kicad_mod(&comp, "my_part");
        assert!(
            modl.contains("(footprint \"my_part\""),
            "missing footprint head"
        );
        assert!(modl.contains("(pad \"1\" smd rect"), "missing pad 1");
        assert!(modl.contains("(pad \"2\" smd rect"), "missing pad 2");
        assert!(modl.contains("(fp_rect (start"), "missing silk rect");
        assert!(modl.contains("(fp_line (start"), "missing track line");
        assert!(modl.contains("(fp_circle (center"), "missing circle");
        assert!(modl.contains("(fp_text user \"U1\""), "missing text");
        assert!(
            modl.contains("(pad \"\" np_thru_hole circle"),
            "missing hole"
        );
        assert!(
            modl.contains("(attr smd)"),
            "should be smd (no thru-hole pads)"
        );
    }

    #[test]
    fn extracts_pins_in_order() {
        let comp = parse_easyeda(FIXTURE).unwrap();
        let pins = extract_pins(&comp);
        assert_eq!(pins, vec!["1".to_string(), "2".to_string()]);
    }

    #[test]
    fn generates_unverified_imported_toml() {
        let comp = parse_easyeda(FIXTURE).unwrap();
        let pins = extract_pins(&comp);
        let toml = generate_part_toml(
            "my_part",
            "C12345",
            "ACME-1",
            "a test part",
            "https://lcsc.com/product/C12345",
            &pins,
        );
        assert!(toml.contains("id = \"my_part\""));
        assert!(toml.contains("kicad_footprint = \"my_part:my_part\""));
        assert!(toml.contains("lcsc_pn = \"C12345\""));
        assert!(toml.contains("source = \"generated\""));
        assert!(toml.contains("upstream_lcsc_pn = \"C12345\""));
        assert!(toml.contains("datasheet_url = \"https://lcsc.com/product/C12345\""));
        assert!(toml.contains("reviewed_by = \"\""));
        assert!(toml.contains("[[pins]]"));
        assert!(toml.contains("electrical_type = \"bidirectional\""));
    }
}
