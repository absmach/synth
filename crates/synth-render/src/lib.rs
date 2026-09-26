// SPDX-License-Identifier: Apache-2.0

//! Deterministic SVG rasterization and pixel diffing for Synth's
//! schematic visual-feedback loop.
//!
//! Rendered PNGs become stored visual baselines that are compared
//! pixel-for-pixel across machines and sessions, so determinism is the
//! design constraint rather than a nice-to-have. Two rules follow:
//!
//! - The rendering stack is pinned exactly in the workspace manifest, and
//!   [`RENDERER_ID`] records it in every baseline so a renderer bump is
//!   detected instead of being mistaken for design drift.
//! - No system font is ever consulted. [`usvg::Options::default`] starts
//!   with an empty font database, so a schematic SVG that depends on
//!   `<text>` glyphs renders them as nothing on every machine instead of
//!   differently on each one. KiCad's schematic exporter draws its visible
//!   text as stroke-font path data, so this is not a practical limitation;
//!   [`visible_text_count`] reports any glyph that really was omitted.

// Pixel dimensions are capped at 8192 by `svg_to_png`, so the u32→f32 and
// u64→f64 conversions below are exact enough for image geometry and pixel
// counts. The precision-loss and sign-loss lints add `as`-cast noise here
// without catching a real defect.
#![allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]

use resvg::{tiny_skia, usvg};
use serde::Serialize;
use thiserror::Error;

/// Identity of the rendering stack, stored in every visual baseline and
/// compared before a baseline diff is trusted. The version half must move
/// with the exact pins in the workspace manifest — a resvg bump that moves
/// one antialiased edge by an ulp would otherwise read as design drift.
pub const RENDERER_ID: &str = "synth-render/resvg-0.48.1";

/// Per-channel-max luminance delta under which a pixel difference is
/// treated as noise rather than change (8/255).
pub const DIFF_THRESHOLD: u8 = 8;

/// A rasterized image plus the facts a caller reports about it.
#[derive(Debug, Clone)]
pub struct Rendered {
    /// Encoded PNG bytes.
    pub png: Vec<u8>,
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("width_px must be in 1..=8192, got {0}")]
    BadWidth(u32),
    #[error("SVG did not parse: {0}")]
    Parse(String),
    #[error("SVG has no drawable area")]
    Empty,
    #[error("could not allocate a {width_px}x{height_px} pixmap")]
    Allocate { width_px: u32, height_px: u32 },
    #[error("PNG encoding failed: {0}")]
    Encode(String),
    #[error("image did not decode: {0}")]
    Decode(String),
}

/// Rasterize SVG bytes to a PNG at the given pixel width; height follows
/// the SVG's aspect ratio. The page is painted white first so the PNG is
/// opaque and alpha differences cannot masquerade as "no change" in a diff.
pub fn svg_to_png(svg: &[u8], width_px: u32) -> Result<Rendered, RenderError> {
    if width_px == 0 || width_px > 8192 {
        return Err(RenderError::BadWidth(width_px));
    }

    let options = usvg::Options::default();
    let tree =
        usvg::Tree::from_data(svg, &options).map_err(|e| RenderError::Parse(e.to_string()))?;

    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return Err(RenderError::Empty);
    }
    let scale = width_px as f32 / size.width();
    let height_px = (size.height() * scale).round().max(1.0) as u32;

    let mut pixmap = tiny_skia::Pixmap::new(width_px, height_px).ok_or(RenderError::Allocate {
        width_px,
        height_px,
    })?;
    pixmap.fill(tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    let png = pixmap
        .encode_png()
        .map_err(|e| RenderError::Encode(e.to_string()))?;
    Ok(Rendered {
        png,
        width_px,
        height_px,
    })
}

/// Number of `<text>` elements in `svg` that would paint if fonts were
/// available. KiCad emits its searchable text with `opacity="0"` /
/// `stroke-opacity="0"` beside stroke-font paths (which do paint), so those
/// are not counted. A non-zero result means glyphs were silently omitted
/// from the render, which callers surface as a warning rather than an error:
/// the image is still deterministic, just missing that text.
pub fn visible_text_count(svg: &[u8]) -> usize {
    let text = String::from_utf8_lossy(svg);
    let mut count = 0;
    let mut rest = text.as_ref();
    while let Some(pos) = rest.find("<text") {
        let after = &rest[pos + 5..];
        let end = after.find('>').unwrap_or(after.len());
        let tag = &after[..end];
        let invisible = tag.contains("opacity=\"0\"")
            || tag.contains("stroke-opacity=\"0\"")
            || tag.contains("fill=\"none\"");
        if !invisible {
            count += 1;
        }
        rest = &after[end..];
    }
    count
}

/// The result of comparing two renders.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PixelDiff {
    pub changed_pixels: u64,
    pub total_pixels: u64,
    /// Pixels that are non-background in either image: the drawing, as
    /// opposed to the paper. Drift thresholds compare against this, because a
    /// schematic sheet is mostly blank page and a percentage of the page
    /// under-reports design change by an order of magnitude.
    pub content_pixels: u64,
    /// Percentage of the page in `[0, 100]`, rounded to 3 decimals.
    pub changed_pct: f64,
    /// Percentage of the content in `[0, 100]`, rounded to 3 decimals;
    /// `0.0` when both images are blank.
    pub changed_pct_of_content: f64,
    /// `[x_min, y_min, x_max, y_max]` bounding box of all changed pixels;
    /// `None` when nothing changed.
    pub changed_bbox: Option<[u32; 4]>,
}

/// Compare two PNGs pixel-for-pixel on a canvas sized to the larger of each
/// dimension (area present in only one image counts as changed). KiCad paints
/// its own background rect, so the paper is taken to be the dominant colour
/// of the "before" image rather than assumed to be white.
pub fn diff_pngs(before: &[u8], after: &[u8]) -> Result<PixelDiff, RenderError> {
    let a = image::load_from_memory(before)
        .map_err(|e| RenderError::Decode(e.to_string()))?
        .to_rgba8();
    let b = image::load_from_memory(after)
        .map_err(|e| RenderError::Decode(e.to_string()))?
        .to_rgba8();

    let width = a.width().max(b.width());
    let height = a.height().max(b.height());
    let background = dominant_color(&a);

    let mut changed: u64 = 0;
    let mut content: u64 = 0;
    let mut bbox: Option<[u32; 4]> = None;

    for y in 0..height {
        for x in 0..width {
            let pa = pixel_or_white(&a, x, y);
            let pb = pixel_or_white(&b, x, y);
            if pa != background || pb != background {
                content += 1;
            }
            let delta = pa
                .iter()
                .zip(pb.iter())
                .map(|(ca, cb)| ca.abs_diff(*cb))
                .max()
                .unwrap_or(0);
            if delta > DIFF_THRESHOLD {
                changed += 1;
                bbox = Some(match bbox {
                    None => [x, y, x, y],
                    Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
                });
            }
        }
    }

    let total = u64::from(width) * u64::from(height);
    let round3 = |v: f64| (v * 1000.0).round() / 1000.0;
    let changed_pct = if total == 0 {
        0.0
    } else {
        round3(changed as f64 / total as f64 * 100.0)
    };
    let changed_pct_of_content = if content == 0 {
        0.0
    } else {
        round3(changed as f64 / content as f64 * 100.0)
    };

    Ok(PixelDiff {
        changed_pixels: changed,
        total_pixels: total,
        content_pixels: content,
        changed_pct,
        changed_pct_of_content,
        changed_bbox: bbox,
    })
}

/// The most frequent flattened colour in an image: the paper. A render is
/// mostly background by construction, so the mode is unambiguous.
fn dominant_color(img: &image::RgbaImage) -> [u8; 3] {
    let mut counts: std::collections::HashMap<[u8; 3], u64> = std::collections::HashMap::new();
    for y in 0..img.height() {
        for x in 0..img.width() {
            *counts.entry(pixel_or_white(img, x, y)).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map_or([255, 255, 255], |(c, _)| c)
}

/// Flatten a pixel onto white outside the image bounds and under
/// transparency, so an out-of-canvas area or a transparent pixel registers as
/// a colour difference instead of disappearing.
fn pixel_or_white(img: &image::RgbaImage, x: u32, y: u32) -> [u8; 3] {
    if x >= img.width() || y >= img.height() {
        return [255, 255, 255];
    }
    let p = img.get_pixel(x, y).0;
    let alpha = u16::from(p[3]);
    let over = |c: u8| ((u16::from(c) * alpha + 255 * (255 - alpha)) / 255) as u8;
    [over(p[0]), over(p[1]), over(p[2])]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
        <rect x="10" y="10" width="30" height="30" fill="black"/>
    </svg>"#;

    #[test]
    fn renders_png_with_requested_width_and_aspect_height() {
        let r = svg_to_png(SQUARE.as_bytes(), 400).unwrap();
        assert_eq!(r.width_px, 400);
        assert_eq!(r.height_px, 200);
        assert_eq!(
            &r.png[..8],
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        );
    }

    #[test]
    fn rejects_zero_and_oversized_width() {
        assert!(matches!(
            svg_to_png(SQUARE.as_bytes(), 0),
            Err(RenderError::BadWidth(0))
        ));
        assert!(matches!(
            svg_to_png(SQUARE.as_bytes(), 9000),
            Err(RenderError::BadWidth(9000))
        ));
    }

    #[test]
    fn identical_render_has_no_drift() {
        let a = svg_to_png(SQUARE.as_bytes(), 200).unwrap();
        let b = svg_to_png(SQUARE.as_bytes(), 200).unwrap();
        let d = diff_pngs(&a.png, &b.png).unwrap();
        assert_eq!(d.changed_pixels, 0);
        assert_eq!(d.changed_pct_of_content, 0.0);
        assert!(d.changed_bbox.is_none());
    }

    #[test]
    fn moved_rect_registers_drift_with_bounding_box() {
        let moved = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
            <rect x="60" y="10" width="30" height="30" fill="black"/>
        </svg>"#;
        let a = svg_to_png(SQUARE.as_bytes(), 200).unwrap();
        let b = svg_to_png(moved.as_bytes(), 200).unwrap();
        let d = diff_pngs(&a.png, &b.png).unwrap();
        assert!(d.changed_pixels > 0);
        assert!(d.changed_pct_of_content > 0.0);
        assert!(d.changed_bbox.is_some());
    }

    #[test]
    fn invisible_kicad_text_is_not_counted_as_visible() {
        let svg = br#"<svg><text opacity="0">U1</text><path d="M0 0"/></svg>"#;
        assert_eq!(visible_text_count(svg), 0);
        let svg2 = br#"<svg><text x="1">U1</text></svg>"#;
        assert_eq!(visible_text_count(svg2), 1);
    }
}
