// SPDX-License-Identifier: Apache-2.0

//! Build a KiCad footprint from an LCSC/JLCPCB **component SVG**.
//!
//! # Why this replaces the EasyEDA shape-primitive importer
//!
//! The older path parsed `dataStr.shape` from EasyEDA's component JSON.
//! That is broken in two ways, and both fail *silently*, which is the worst
//! combination available:
//!
//! 1. The endpoint returns the part's **schematic symbol**, not its
//!    footprint. For an aQFN-73 that is two columns of 37 pins — plausible
//!    looking, and geometrically meaningless as a land pattern.
//! 2. LCSC now serves those primitives as compact tilde-delimited *strings*
//!    (`"P~show~0~A8~390~310~180~..."`) rather than objects with a `type`
//!    key. A parser matching on `obj["type"]` therefore matches nothing and
//!    emits a `.kicad_mod` with **zero pads** and no error. A footprint with
//!    no pads is worse than no footprint: it satisfies the "has a real
//!    `.kicad_mod`" export gate while being unbuildable.
//!
//! The `pcbSvg` that LCSC's public product API advertises under
//! `edaSvgInfo` is the actual footprint, and it is self-describing: every pad
//! is a `<g c_partid="part_pad" …>` carrying `number`, `c_origin`,
//! `c_width`, `c_height`, `c_rotation` and `c_shape`. There is no format
//! archaeology to do and nothing to silently misread.
//!
//! Cross-checked against KiCad's own `Nordic_AQFN-73-1EP_7x7mm_P0.5mm`
//! for LCSC `C190794`: 73 perimeter pads plus one 4.85 mm exposed pad
//! (KiCad: 4.85 mm), 0.500 mm pitch, and the package string
//! `AQFN-73_L7.0-W7.0-P0.50-BL-EP4.8`.
//!
//! # Provenance
//!
//! Raw LCSC/EasyEDA CAD documents are fetched at import time and converted
//! immediately. They are never persisted or redistributed by Synth — only
//! the derived `.kicad_mod` is written. See `registry/CREDITS.md`.

use std::fmt::Write as _;

/// One EasyEDA/SVG user unit, in millimetres.
///
/// The component SVGs carry `viewBox` units of 10 mil; a 12.192 mm canvas
/// with a 48-unit viewBox confirms the factor.
pub const UNIT_TO_MM: f64 = 0.254;

/// Margin added around the pad extents to form the courtyard (mm).
const COURTYARD_MARGIN_MM: f64 = 0.25;

/// Pad outline shape as the source describes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadShape {
    Rect,
    Ellipse,
    /// Rounded rectangle, given as a corner-radius fraction of the short side.
    RoundRect(f64),
    Oval,
    Polygon,
    /// Anything the source names that this importer does not model. Treated
    /// as a rectangle rather than dropped: a pad of the wrong shape still
    /// routes and still flags DRC if it is the wrong size, whereas a missing
    /// pad makes the net unroutable and the failure harder to read.
    Other,
}

/// One pad, in millimetres, origin-centred, Y-up (KiCad convention).
#[derive(Debug, Clone, PartialEq)]
pub struct Pad {
    /// Designator as the source gives it, e.g. `A22`, `12`, `EP`.
    pub number: String,
    pub x_mm: f64,
    pub y_mm: f64,
    pub width_mm: f64,
    pub height_mm: f64,
    pub rotation_deg: f64,
    pub shape: PadShape,
    /// False for a non-plated mechanical hole.
    pub plated: bool,
}

/// A pad as read from the document, in source units.
///
/// Kept separate from [`Pad`] because the origin is not known until every pad
/// has been seen — it may have to fall back to the pad centroid — so the raw
/// values are collected first and converted once at the end.
#[derive(Debug, Clone)]
struct RawPad {
    number: String,
    origin_x: f64,
    origin_y: f64,
    width: f64,
    height: f64,
    rotation: f64,
    shape: PadShape,
    plated: bool,
}

/// A footprint recovered from a component SVG.
///
/// # Orientation is as-drawn, and that is not always the datasheet orientation
///
/// Pads keep the orientation EasyEDA draws them in, after the SVG→KiCad Y
/// flip. For most parts that is correct, but it is not guaranteed to be: the
/// land pattern recovered for the nRF52840 QIAA (C190794) matches KiCad's
/// `Nordic_AQFN-73-1EP_7x7mm_P0.5mm` only after a transpose, to within
/// 0.002 mm, while a two-pad chip antenna (C17192881) matches as-drawn. So
/// there is no single correction to apply, and applying one would silently
/// misplace pin 1 on whichever parts it does not belong to.
///
/// The importer therefore reports the geometry as recovered and leaves
/// orientation verification to the reviewer, who checks pin 1 against the
/// datasheet before the part leaves the unverified tier.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFootprint {
    pub pads: Vec<Pad>,
    /// Source package string, e.g. `AQFN-73_L7.0-W7.0-P0.50-BL-EP4.8`.
    pub package: Option<String>,
    /// Canvas extent, used to sanity-check the result.
    pub canvas_mm: (f64, f64),
    /// `part_pad` groups that carried no emittable pad geometry.
    ///
    /// Non-zero on a healthy document — the CSS in `<style>` is stripped
    /// first, but EasyEDA also emits empty container groups. It is reported so
    /// a caller can tell "73 of 73 pads" from "70 of 73, three unreadable",
    /// which would otherwise be an invisible geometry loss.
    pub skipped_groups: usize,
    /// Whether pads are anchored on the component's own origin rather than
    /// on the pad centroid.
    pub origin_from_component: bool,
}

impl ParsedFootprint {
    /// Axis-aligned bounding box of the pads, `(min_x, min_y, max_x, max_y)`.
    pub fn pad_bbox(&self) -> (f64, f64, f64, f64) {
        let mut bb = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &self.pads {
            // A rotated rectangle's extent is the diagonal bound; using it
            // keeps the courtyard conservative rather than clipping a
            // rotated pad.
            let r = (p.width_mm.hypot(p.height_mm)) / 2.0;
            bb.0 = bb.0.min(p.x_mm - r);
            bb.1 = bb.1.min(p.y_mm - r);
            bb.2 = bb.2.max(p.x_mm + r);
            bb.3 = bb.3.max(p.y_mm + r);
        }
        bb
    }
}

/// Pull a `name="value"` attribute out of one element's opening tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    // Two EasyEDA quirks make a naive `name="..."` search wrong:
    //
    //  * `plated ="Y"` has a space before the `=`.
    //  * `c_shapetype="group"` precedes `c_shape="ELLIPSE"` on the same pad,
    //    so a single `find(name)` lands on the longer attribute's prefix and
    //    reports the pad as shapeless.
    //
    // So: try every occurrence of the name, and accept one only when what
    // follows it is optional whitespace and then `="`.
    let mut from = 0usize;
    while let Some(rel) = tag[from..].find(name) {
        let at = from + rel;
        let after = at + name.len();
        let rest = &tag[after..];
        if let Some(eq) = rest.find('=') {
            if rest[..eq].trim().is_empty() {
                let quoted = rest[eq + 1..].trim_start();
                if let Some(quoted) = quoted.strip_prefix('"') {
                    if let Some(end) = quoted.find('"') {
                        return Some(quoted[..end].to_string());
                    }
                }
            }
        }
        from = after;
    }
    None
}

/// Collect every `part_pad` group in `doc`, in source units.
///
/// Returns the pads and the number of groups that carried no emittable
/// geometry, so a caller can tell "73 of 73 pads" from "70 of 73, three
/// unreadable" rather than losing the difference silently.
fn scan_raw_pads(doc: &str) -> (Vec<RawPad>, usize) {
    let mut raw: Vec<RawPad> = Vec::new();
    let mut skipped = 0usize;
    let mut rest = doc;
    while let Some(idx) = rest.find("c_partid=\"part_pad\"") {
        // Walk back to the element's `<g` so attribute scanning starts clean.
        let start = rest[..idx].rfind('<').unwrap_or(0);
        let Some(end) = rest[idx..].find('>').map(|n| n + idx) else {
            break;
        };
        let tag = &rest[start..end];
        rest = &rest[end..];

        // A container group carries the pad's identity and geometry; a child
        // shape element does not. Anything without a number or a position is
        // not a pad we can emit, so count it and move on rather than
        // discarding an otherwise-good 74-pad package.
        let (Some(number), Some(origin)) = (attr(tag, "number"), attr(tag, "c_origin")) else {
            skipped += 1;
            continue;
        };
        let Some((ox, oy)) = origin.split_once(',').and_then(|(a, b)| {
            Some((a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?))
        }) else {
            skipped += 1;
            continue;
        };
        let width = attr(tag, "c_width")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let height = attr(tag, "c_height")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let rotation = attr(tag, "c_rotation")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let shape = match attr(tag, "c_shape").as_deref() {
            Some("RECT") => PadShape::Rect,
            Some("ELLIPSE" | "OVAL") => PadShape::Ellipse,
            Some("ROUNDRECT") => PadShape::RoundRect(0.25),
            Some("POLYGON") => PadShape::Polygon,
            _ => PadShape::Other,
        };
        // EasyEDA writes `plated ="Y"` with a space before the `=`, which a
        // naive `plated="` search misses; absence means plated.
        let plated = !matches!(attr(tag, "plated").as_deref(), Some("N"));

        raw.push(RawPad {
            number,
            origin_x: ox,
            origin_y: oy,
            width,
            height,
            rotation,
            shape,
            plated,
        });
    }
    (raw, skipped)
}

/// Parse a component SVG into pads.
///
/// Returns `None` when the document carries no `part_pad` groups. Callers
/// must treat that as a hard failure, never as "an empty footprint": a pad
/// list is the entire point of the document.
pub fn parse_component_svg(svg: &str) -> Option<ParsedFootprint> {
    // viewBox gives the canvas origin and size. The source is Y-down (SVG);
    // KiCad is Y-up, so the vertical flip is anchored on the canvas.
    //
    // The search has to skip past the *opening* quote before looking for the
    // closing one, or the first `"` it finds is the one it just matched.
    const VB_KEY: &str = "viewBox=\"";
    let open = svg.find(VB_KEY)? + VB_KEY.len();
    let close = open + svg[open..].find('"')?;
    let nums: Vec<f64> = svg[open..close]
        .split_whitespace()
        .filter_map(|v| v.parse::<f64>().ok())
        .collect();
    if nums.len() != 4 {
        return None;
    }
    // Only the canvas *size* is used, for reporting: the pads are anchored on
    // the component origin, so the canvas origin itself is irrelevant.
    let (w_units, h_units) = (nums[2], nums[3]);

    // The document's <style> block contains CSS *selectors* that quote the
    // very attribute we search for, e.g.
    //     g[c_partid="part_pad"][layerid] > polyline[c_padhole] { ... }
    // A naive substring scan matches those first, walks back to the enclosing
    // <style> tag, and then cannot find `number` on it. Blank out the
    // non-rendered blocks so the scan only ever sees real elements.
    let doc = strip_non_rendered_blocks(svg);

    // Collected in source units first: the origin is only known once every
    // pad has been seen (it may have to fall back to the centroid).
    let (raw, skipped) = scan_raw_pads(&doc);

    if raw.is_empty() {
        return None;
    }

    // Origin: the component's own `c_origin` (the tag carrying `c_para`),
    // which EasyEDA treats as the part's placement point. Falling back to the
    // pad centroid matters for two-pin parts, where a missing `c_para` would
    // otherwise leave the pads off-centre by half their span.
    let origin = component_origin(&doc);
    let (cx, cy) = origin.unwrap_or_else(|| {
        // Pad counts are single digits, so the usize→f64 widening is exact.
        #[allow(clippy::cast_precision_loss)]
        let n = raw.len() as f64;
        (
            raw.iter().map(|r| r.origin_x).sum::<f64>() / n,
            raw.iter().map(|r| r.origin_y).sum::<f64>() / n,
        )
    });
    let origin_from_component = origin.is_some();

    let pads = raw
        .into_iter()
        .map(|r| Pad {
            number: r.number,
            x_mm: (r.origin_x - cx) * UNIT_TO_MM,
            // Flip: SVG grows downward, KiCad grows upward.
            y_mm: (cy - r.origin_y) * UNIT_TO_MM,
            width_mm: r.width * UNIT_TO_MM,
            height_mm: r.height * UNIT_TO_MM,
            rotation_deg: r.rotation,
            shape: r.shape,
            plated: r.plated,
        })
        .collect();

    // The package string is in the first group's `c_para`, formatted as
    // `key`value`` pairs.
    let package = svg.find("c_para=\"").and_then(|i| {
        let s = i + "c_para=\"".len();
        let e = svg[s..].find('"')? + s;
        let para = &svg[s..e];
        let key = "package`";
        let ks = para.find(key)? + key.len();
        para[ks..].find('`').map(|n| para[ks..ks + n].to_string())
    });

    Some(ParsedFootprint {
        pads,
        package,
        canvas_mm: (w_units * UNIT_TO_MM, h_units * UNIT_TO_MM),
        skipped_groups: skipped,
        origin_from_component,
    })
}

/// The component's placement origin, in source units.
///
/// Lives on the root group as `c_origin` alongside the `c_para` attribute
/// block. Anchoring pads to the *canvas* instead is a silent 6 mm error on
/// an aQFN, because EasyEDA pads the canvas well past the land pattern.
fn component_origin(doc: &str) -> Option<(f64, f64)> {
    let para = doc.find("c_para=\"")?;
    let start = doc[..para].rfind('<')?;
    let end = para + doc[para..].find('>')?;
    let origin = attr(&doc[start..end], "c_origin")?;
    let (a, b) = origin.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Blank out the contents of `<style>`/`<script>` blocks.
///
/// Only the returned copy is used, and only for scanning, so it is safe for
/// the block to shrink. Replacements are made per `char` rather than per
/// byte: the documents carry UTF-8 in vendor links (`Contributor`, `link`),
/// and overwriting a continuation byte with a space would produce invalid
/// text that any later `find` could slice through.
fn strip_non_rendered_blocks(svg: &str) -> String {
    let mut out = svg.to_string();
    for tag in ["style", "script"] {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        let mut from = 0usize;
        while let Some(rel) = out[from..].find(&open) {
            let s = from + rel;
            let Some(gt) = out[s..].find('>').map(|n| n + s) else {
                break;
            };
            let Some(rel_end) = out[gt..].find(&close) else {
                break;
            };
            let e = gt + rel_end;
            let body: String = out[gt + 1..e]
                .chars()
                .map(|c| if c.is_whitespace() { c } else { ' ' })
                .collect();
            out.replace_range(gt + 1..e, &body);
            from = gt + 1 + body.len();
        }
    }
    out
}

/// A stable pseudo-UUID for an emitted element.
///
/// KiCad requires a `uuid` per `fp_line`/`pad`, and a footprint that
/// changes on every export produces an unreviewable diff. Deriving it from
/// the element's index keeps the file byte-stable for identical input; this
/// is not a real UUID and nothing depends on its uniqueness across
/// footprints.
fn outline_uuid(kind: &str, index: usize, salt: u32) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in kind.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h ^= index as u32;
    h = h.wrapping_mul(0x0100_0193);
    h ^ salt
}

/// Render `fp` as a KiCad `.kicad_mod`.
///
/// The courtyard and fab outline are derived from the pad extents plus a
/// margin. The source's own body outline is not parsed: its path syntax is
/// not guaranteed across parts, and a courtyard that is a few tenths of a
/// millimetre generous is harmless where a mis-parsed one is not.
pub fn to_kicad_mod(fp: &ParsedFootprint, name: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "(footprint \"{name}\"");
    let _ = writeln!(out, "\t(version 20240108)");
    let _ = writeln!(out, "\t(generator \"synth-lcsc-pcb-svg\")");
    let _ = writeln!(out, "\t(generator_version \"10.0\")");
    let _ = writeln!(out, "\t(layer \"F.Cu\")");
    let _ = writeln!(
        out,
        "\t(descr \"LCSC component SVG, {} pads\")",
        fp.pads.len()
    );
    if let Some(p) = &fp.package {
        let _ = writeln!(out, "\t(tags \"{p}\")");
    }
    let _ = writeln!(out, "\t(attr smd)");

    let bb = fp.pad_bbox();
    let fab = (bb.0, bb.1, bb.2, bb.3);
    let cy = (
        fab.0 - COURTYARD_MARGIN_MM,
        fab.1 - COURTYARD_MARGIN_MM,
        fab.2 + COURTYARD_MARGIN_MM,
        fab.3 + COURTYARD_MARGIN_MM,
    );
    for (si, (layer, r, width)) in [("F.CrtYd", &cy, 0.05), ("F.Fab", &fab, 0.1)]
        .into_iter()
        .enumerate()
    {
        for (li, (sx, sy, ex, ey)) in [
            (r.0, r.1, r.2, r.1),
            (r.2, r.1, r.2, r.3),
            (r.2, r.3, r.0, r.3),
            (r.0, r.3, r.0, r.1),
        ]
        .into_iter()
        .enumerate()
        {
            let _ = writeln!(out, "\t(fp_line");
            let _ = writeln!(out, "\t\t(start {sx:.5} {sy:.5})");
            let _ = writeln!(out, "\t\t(end {ex:.5} {ey:.5})");
            let _ = writeln!(out, "\t\t(stroke");
            let _ = writeln!(out, "\t\t\t(width {width})");
            let _ = writeln!(out, "\t\t\t(type solid)");
            let _ = writeln!(out, "\t\t)");
            let _ = writeln!(out, "\t\t(layer \"{layer}\")");
            let _ = writeln!(
                out,
                "\t\t(uuid \"{:08x}-0000-4000-8000-{:012x}\")",
                outline_uuid(layer, si, li as u32),
                si * 4 + li
            );
            let _ = writeln!(out, "\t)");
        }
    }
    let _ = out.pop(); // drop the trailing newline before pads

    for (i, p) in fp.pads.iter().enumerate() {
        // An exposed pad is conventionally `EP`; the source often numbers it
        // 0 or leaves it blank, neither of which a symbol would reference.
        let number = if p.number.is_empty() || p.number == "0" {
            "EP".to_string()
        } else {
            p.number.clone()
        };
        let (shape, extra) = match p.shape {
            PadShape::Ellipse => ("circle", String::new()),
            PadShape::RoundRect(rr) => ("roundrect", format!("\n\t\t(roundrect_rratio {rr})")),
            PadShape::Polygon => ("custom", String::new()),
            _ => ("rect", String::new()),
        };
        let pad_type = if p.plated { "smd" } else { "np_thru_hole" };
        // A circle is a special case of an ellipse in KiCad: a circle pad is
        // `circle` with size = diameter, so squash the short axis away.
        let (w, h) = if shape == "circle" {
            let d = p.width_mm.max(p.height_mm);
            (d, d)
        } else {
            (p.width_mm, p.height_mm)
        };
        let _ = writeln!(out, "\n\t(pad \"{number}\" {pad_type} {shape}");
        let _ = writeln!(
            out,
            "\t\t(at {:.5} {:.5}{:.0})",
            p.x_mm, p.y_mm, p.rotation_deg
        );
        let _ = writeln!(out, "\t\t(size {w:.5} {h:.5})");
        if p.plated {
            let _ = writeln!(out, "\t\t(layers \"F.Cu\" \"F.Paste\" \"F.Mask\")");
        } else {
            let _ = writeln!(out, "\t\t(layers \"F&B.Cu\" \"*.Mask\")");
        }
        let _ = writeln!(
            out,
            "\t\t(uuid \"{:08x}-0000-4000-8000-{:012x}\")",
            outline_uuid("pad", i, 0),
            i
        );
        let _ = writeln!(out, "\t){extra}");
    }

    out.push_str("\n)\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A miniature component SVG in the source's dialect, so the parser is
    /// pinned without vendoring a vendor document (the licensing posture in
    /// `registry/CREDITS.md` forbids persisting one).
    const SVG: &str = r#"<svg width="5.08mm" viewBox="100 200 20 20">
<g c_para="package`AQFN-4_L2.0-W2.0-P0.50`3DModel`x`">
<g c_partid="part_pad" c_etype="pinpart" c_origin="102,202" layerid="1" number="1" plated ="Y" c_rotation="0" c_width="0.9843" c_height="0.9843" c_shape="ELLIPSE"></g>
<g c_partid="part_pad" c_etype="pinpart" c_origin="118,202" layerid="1" number="2" plated ="Y" c_rotation="0" c_width="0.9843" c_height="0.9843" c_shape="ELLIPSE"></g>
<g c_partid="part_pad" c_etype="pinpart" c_origin="110,210" layerid="1" number="0" c_rotation="0" c_width="9.45" c_height="9.45" c_shape="RECT"></g>
</g></svg>"#;

    #[test]
    fn extracts_pads_with_positions_and_sizes() {
        let fp = parse_component_svg(SVG).expect("pads must be found");
        assert_eq!(fp.pads.len(), 3);
        assert_eq!(fp.package.as_deref(), Some("AQFN-4_L2.0-W2.0-P0.50"));
        let p1 = &fp.pads[0];
        assert_eq!(p1.number, "1");
        assert_eq!(p1.shape, PadShape::Ellipse);
        assert!((p1.width_mm - 0.2500).abs() < 1e-3, "{}", p1.width_mm);
        // Pads are centred on the part, not on the canvas: KiCad's footprint
        // origin is the land-pattern centre, and a canvas-anchored footprint
        // sits ~6 mm off its own placement coordinate.
        assert!(!fp.origin_from_component, "fixture has no c_para");
        let cx = f64::midpoint(fp.pads[0].x_mm, fp.pads[1].x_mm);
        assert!(cx.abs() < 1e-6, "pad row must straddle x=0, got {cx}");
        // Centroid fallback: x=110 units, y=204.67 units (the large EP pulls
        // the y-centroid down from 202).
        assert!((p1.x_mm + 2.032).abs() < 1e-3, "{}", p1.x_mm);
        // y is flipped: SVG grows downward, KiCad grows upward.
        assert!((p1.y_mm - 0.677).abs() < 1e-3, "{}", p1.y_mm);
    }

    #[test]
    fn pads_anchor_on_the_component_origin_not_the_canvas() {
        // A real aQFN canvas is ~12 mm across for a 6.5 mm land pattern, so
        // anchoring on the canvas offset every pad by ~3 mm in both axes.
        let svg = r#"<svg viewBox="0 0 48 49.5">
        <g c_origin="20,20" c_para="package`AQFN-4`pre`REF?`Contributor`lcsc`"></g>
        <g c_partid="part_pad" c_origin="18,18" number="1" c_width="1" c_height="1" c_shape="RECT"></g>
        <g c_partid="part_pad" c_origin="22,18" number="2" c_width="1" c_height="1" c_shape="RECT"></g>
        </svg>"#;
        let fp = parse_component_svg(svg).unwrap();
        assert!(fp.origin_from_component);
        // Component origin (20,20) is pad-centre y; the two pads are 4 units
        // = 1.016 mm either side of it.
        assert!(
            (fp.pads[0].x_mm + 0.508).abs() < 1e-3,
            "{}",
            fp.pads[0].x_mm
        );
        assert!(
            (fp.pads[0].y_mm - 0.508).abs() < 1e-3,
            "{}",
            fp.pads[0].y_mm
        );
        assert!(
            (fp.pads[1].x_mm - 0.508).abs() < 1e-3,
            "{}",
            fp.pads[1].x_mm
        );
    }

    #[test]
    fn exposed_pad_is_named_ep() {
        let fp = parse_component_svg(SVG).unwrap();
        let kmod = to_kicad_mod(&fp, "t");
        assert!(
            kmod.contains("(pad \"EP\" smd rect"),
            "an unnumbered/0 large pad must become EP: {kmod}"
        );
    }

    #[test]
    fn a_document_with_no_pads_is_rejected_rather_than_emptied() {
        // The failure mode this module exists to prevent: emitting a
        // `.kicad_mod` with zero pads, which passes the export gate while
        // being unbuildable.
        assert!(parse_component_svg(r#"<svg viewBox="0 0 10 10"></svg>"#).is_none());
    }

    #[test]
    fn css_selectors_naming_part_pad_are_not_mistaken_for_pads() {
        // A real EasyEDA document opens with a <style> block whose selectors
        // quote `c_partid="part_pad"` verbatim. Matching those and walking
        // back to <style> used to abort the whole parse, so a 74-pad aQFN
        // parsed as zero pads.
        let svg = r#"<svg viewBox="0 0 10 10"><style type="text/css">
        g[c_partid="part_pad"][layerid] > polyline[c_padhole] {stroke:#222222;}
        g[c_partid="part_pad"] > polygon[c_padid] {stroke-linejoin: miter;}
        </style><g c_partid="part_pad" c_origin="5,5" number="1" c_width="1" c_height="1"></g></svg>"#;
        let fp = parse_component_svg(svg).expect("the real pad must still parse");
        assert_eq!(fp.pads.len(), 1);
        assert_eq!(fp.pads[0].number, "1");
    }

    #[test]
    fn a_pad_without_geometry_is_skipped_without_losing_the_others() {
        // One malformed pad must not cost us the whole package.
        let svg = r#"<svg viewBox="0 0 10 10">
        <g c_partid="part_pad"></g>
        <g c_partid="part_pad" c_origin="1,1" number="1" c_width="1" c_height="1"></g>
        <g c_partid="part_pad" c_origin="2,2" number="2" c_width="1" c_height="1"></g></svg>"#;
        let fp = parse_component_svg(svg).expect("two good pads");
        assert_eq!(fp.pads.len(), 2);
        assert_eq!(fp.skipped_groups, 1);
    }

    #[test]
    fn c_shapetype_does_not_shadow_c_shape() {
        // Every real pad carries `c_shapetype="group"` *before*
        // `c_shape="ELLIPSE"`. Matching the shorter name first turned every
        // aQFN pad into a rectangle.
        let svg = r#"<svg viewBox="0 0 10 10">
        <g c_partid="part_pad" c_origin="1,1" number="1" c_width="1" c_height="1"
           c_shapetype="group" c_shape="ELLIPSE"></g></svg>"#;
        let fp = parse_component_svg(svg).unwrap();
        assert_eq!(fp.pads[0].shape, PadShape::Ellipse);
        let kmod = to_kicad_mod(&fp, "t");
        assert!(kmod.contains("smd circle"), "got:\n{kmod}");
    }

    #[test]
    fn plating_survives_whitespace_before_the_equals_sign() {
        // EasyEDA writes `plated ="N"`, not `plated="N"`.
        let svg = r#"<svg viewBox="0 0 10 10">
        <g c_partid="part_pad" c_origin="1,1" number="1" c_width="1" c_height="1" plated ="N"></g>
        <g c_partid="part_pad" c_origin="2,2" number="2" c_width="1" c_height="1" plated ="Y"></g>
        </svg>"#;
        let fp = parse_component_svg(svg).unwrap();
        assert!(!fp.pads[0].plated, "plated =\"N\" is a through-hole pad");
        assert!(fp.pads[1].plated);
    }

    #[test]
    fn a_svg_without_a_viewbox_is_rejected() {
        assert!(parse_component_svg(r#"<svg><g c_partid="part_pad" number="1"/></svg>"#).is_none());
    }

    #[test]
    fn generated_footprint_is_structurally_valid_and_ordered() {
        let fp = parse_component_svg(SVG).unwrap();
        let kmod = to_kicad_mod(&fp, "my_part");
        assert!(kmod.starts_with("(footprint \"my_part\""));
        assert!(kmod.trim_end().ends_with(')'));
        // Balanced parens, which is what KiCad's own parser needs.
        let depth = kmod
            .bytes()
            .filter(|b| *b == b'(')
            .count()
            .checked_sub(kmod.bytes().filter(|b| *b == b')').count());
        assert_eq!(depth, Some(0), "unbalanced parens");
        assert!(kmod.contains("(layer \"F.CrtYd\")"), "courtyard required");
        assert!(kmod.contains("(layers \"F.Cu\" \"F.Paste\" \"F.Mask\")"));
        // Pads must come after the outline so a reader sees the body first.
        assert!(kmod.find("F.Fab").unwrap() < kmod.find("(pad ").unwrap());
    }

    #[test]
    fn a_circle_pad_is_emitted_with_one_diameter() {
        let fp = parse_component_svg(SVG).unwrap();
        let kmod = to_kicad_mod(&fp, "t");
        let line = kmod
            .lines()
            .find(|l| l.trim_start().starts_with("(size"))
            .expect("a size line");
        let nums: Vec<f64> = line
            .split_whitespace()
            .map(|t| t.trim_matches(|c: char| !c.is_ascii_digit() && c != '.'))
            .filter(|t| !t.is_empty())
            .filter_map(|t| t.parse().ok())
            .collect();
        assert_eq!(nums.len(), 2, "expected two dimensions in: {line}");
        assert!(
            (nums[0] - nums[1]).abs() < 1e-9,
            "a circle's size must be square: {line}"
        );
    }
}
