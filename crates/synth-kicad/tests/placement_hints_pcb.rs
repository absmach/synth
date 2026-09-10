// SPDX-License-Identifier: Apache-2.0

//! End-to-end placement-hint plumbing tests (plan item D2): DSL
//! `placement_hint` declarations and `<design>.synth.layout.toml`
//! sidecar overrides must reach the exported `.kicad_pcb` component
//! positions. Each test exports a small hinted design and parses the
//! emitted sexp back, asserting footprint `(at x y)` placements
//! against their declared edge bands / sidecar coordinates.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn tempdir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("synth-kicad-hints-{label}-{}", std::process::id()));
    if p.exists() {
        let _ = fs::remove_dir_all(&p);
    }
    p
}

/// Source with hard edge hints on two header pairs, mirroring a
/// classic dev-kit form factor (headers down the left/right edges).
const HINTED_SOURCE: &str = r#"board "hint_edge_export" {
  layers 2

  component U1: mcu "rp2350"
  component HL1: connector "header_1x4" {
    placement_hint { edge: left  priority: hard }
  }
  component HL2: connector "header_1x4" {
    placement_hint { edge: left  priority: hard }
  }
  component HR1: connector "header_1x4" {
    placement_hint { edge: right  priority: hard }
  }
  component HR2: connector "header_1x4" {
    placement_hint { edge: right  priority: hard }
  }

  connect U1.gp0 -> HL1.p1
  connect U1.gp1 -> HL1.p2
  connect U1.gp2 -> HL2.p1
  connect U1.gp3 -> HL2.p2
  connect U1.gp0 -> HR1.p1
  connect U1.gp1 -> HR1.p2
  connect U1.gp2 -> HR2.p1
  connect U1.gp3 -> HR2.p2
}
"#;

/// One exported footprint instance: reference designator + placement.
struct FootprintAt {
    refdes: String,
    x_mm: f64,
    y_mm: f64,
}

/// Parse every `(footprint ...)` block out of a `.kicad_pcb` sexp,
/// extracting the reference designator (`"Reference"` property) and
/// the footprint-level `(at x y ...)` — the first `(at` in the block,
/// which precedes pad/property sub-sexps by emitter construction.
fn parse_footprints(pcb_text: &str) -> Vec<FootprintAt> {
    const OPEN: &str = "(footprint";
    let mut out = Vec::new();
    let mut rest = pcb_text;
    while let Some(start) = rest.find(OPEN) {
        rest = &rest[start..];
        let end = rest[OPEN.len()..]
            .find(OPEN)
            .map_or(rest.len(), |rel| rel + OPEN.len());
        if let Some(fp) = parse_one_footprint(&rest[..end]) {
            out.push(fp);
        }
        rest = &rest[end..];
    }
    out
}

/// Parse a sexp coordinate token, tolerating the closing paren that
/// butts against the last number (`(at 12 15.8)` → `15.8`).
fn coord_token(token: &str) -> Option<f64> {
    token.trim_end_matches(')').parse().ok()
}

fn parse_one_footprint(block: &str) -> Option<FootprintAt> {
    const REF_KEY: &str = "\"Reference\"";
    // First `(at X Y...)` inside the block is the footprint placement.
    const AT_KEY: &str = "(at ";

    let key_pos = block.find(REF_KEY)?;
    let after_key = &block[key_pos + REF_KEY.len()..];
    let value_start = after_key.find('"')?;
    let after_open = &after_key[value_start + 1..];
    let value_len = after_open.find('"')?;
    let refdes = after_open[..value_len].to_string();

    let at_pos = block.find(AT_KEY)?;
    let mut nums = block[at_pos + AT_KEY.len()..].split_whitespace();
    let x_mm = coord_token(nums.next()?)?;
    let y_mm = coord_token(nums.next()?)?;
    Some(FootprintAt { refdes, x_mm, y_mm })
}

/// Board outline extents from the `Edge.Cuts` `gr_line` segments.
/// The placer emits an axis-aligned rectangle with min corner at the
/// origin, but both axes are parsed properly anyway so the assertions
/// do not silently depend on that invariant.
fn parse_edge_cuts_bbox(pcb_text: &str) -> Option<(f64, f64, f64, f64)> {
    const OPEN: &str = "(gr_line";
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut found = false;

    let mut rest = pcb_text;
    while let Some(start) = rest.find(OPEN) {
        rest = &rest[start..];
        let end = rest[OPEN.len()..]
            .find(OPEN)
            .map_or(rest.len(), |rel| rel + OPEN.len());
        let block = &rest[..end];
        rest = &rest[end..];

        if !block.contains("\"Edge.Cuts\"") {
            continue;
        }
        found = true;
        for coords in coords_after(block, "(start ")
            .into_iter()
            .chain(coords_after(block, "(end "))
        {
            min_x = min_x.min(coords.0);
            min_y = min_y.min(coords.1);
            max_x = max_x.max(coords.0);
            max_y = max_y.max(coords.1);
        }
    }

    found.then_some((min_x, min_y, max_x, max_y))
}

/// Every `(x y)` coordinate pair following occurrences of `keyword`.
fn coords_after(block: &str, keyword: &str) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(rel) = block[search_from..].find(keyword) {
        let abs = search_from + rel + keyword.len();
        let mut nums = block[abs..].split_whitespace();
        if let (Some(x), Some(y)) = (
            nums.next().and_then(coord_token),
            nums.next().and_then(coord_token),
        ) {
            out.push((x, y));
        }
        search_from = abs;
    }
    out
}

fn lower_source(source: &str, filename: &str) -> synth_ir::Board {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(source, filename);
    assert!(
        parsed.diagnostics.is_empty(),
        "{filename} must parse cleanly: {:?}",
        parsed.diagnostics
    );
    let lowered = synth_ir::lower(&parsed.ast.expect("ast"), &registry, filename);
    assert!(
        lowered.diagnostics.is_empty(),
        "{filename} must lower cleanly: {:?}",
        lowered.diagnostics
    );
    lowered.board.expect("board")
}

/// Placer geometry constants mirrored from `synth-place` (private
/// there): the placer keeps a page margin off the outline, and hard
/// edge hints land hinted components near the declared edge. We assert
/// the intent (on the right side, within `EDGE_NEAR_MM` of the edge)
/// rather than the solver's exact internal band, which can't be
/// reproduced from the exported outline.
const PLACER_MARGIN_MM: f64 = 6.0;
const EDGE_NEAR_MM: f64 = 25.0;

#[test]
fn hard_edge_hints_land_on_declared_edges_in_exported_pcb() {
    let board = lower_source(HINTED_SOURCE, "hint_edge_export.synth");
    let tmp = tempdir("edges");
    let result = synth_kicad::export(&board, &tmp).expect("export");

    let pcb_text = fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written");
    let footprints = parse_footprints(&pcb_text);
    let refs: Vec<&str> = footprints.iter().map(|f| f.refdes.as_str()).collect();
    let at = |refdes: &str| {
        footprints
            .iter()
            .find(|f| f.refdes == refdes)
            .unwrap_or_else(|| {
                panic!("{refdes} missing from exported PCB; parsed footprints: {refs:?}")
            })
    };

    // Mirror the placer's usable area (outline shrunk by its 6 mm page
    // margin) and check each hinted header is on its declared side and
    // within a generous 25 mm band of that edge. The exact
    // `min(15, w/3, h/3)` band the solver targets can't be reproduced
    // from the exported outline (the outline is re-derived from the
    // placement with its own edge margin), so we assert the intent —
    // hard edge hints land near the declared edge — not the precise
    // band geometry.
    let (min_x, _, max_x, _) = parse_edge_cuts_bbox(&pcb_text).expect("Edge.Cuts outline present");
    let usable_min_x = min_x + PLACER_MARGIN_MM;
    let usable_max_x = max_x - PLACER_MARGIN_MM;
    let center_x = f64::midpoint(usable_min_x, usable_max_x);

    for hl in ["HL1", "HL2"] {
        let fp = at(hl);
        assert!(
            fp.x_mm < center_x,
            "{hl} centre x={}mm is not in the left half (center_x={center_x})",
            fp.x_mm
        );
        assert!(
            fp.x_mm <= usable_min_x + EDGE_NEAR_MM,
            "{hl} centre x={}mm too far from the left edge (usable left edge={usable_min_x})",
            fp.x_mm
        );
    }
    for hr in ["HR1", "HR2"] {
        let fp = at(hr);
        assert!(
            fp.x_mm > center_x,
            "{hr} centre x={}mm is not in the right half (center_x={center_x})",
            fp.x_mm
        );
        assert!(
            fp.x_mm >= usable_max_x - EDGE_NEAR_MM,
            "{hr} centre x={}mm too far from the right edge (usable right edge={usable_max_x})",
            fp.x_mm
        );
    }

    // Sanity: the hinted headers flank the unhinted MCU.
    assert!(at("HL1").x_mm < at("U1").x_mm, "HL1 must sit left of U1");
    assert!(at("HR1").x_mm > at("U1").x_mm, "HR1 must sit right of U1");
}

#[test]
fn human_sidecar_overrides_win_over_dsl_hints_in_exported_pcb() {
    // Same fixture, plus a *centre* hint on U1 that a recorded human
    // drag must outrank — sidecar intent beats both the DSL hint and
    // the automatic placement.
    let source = HINTED_SOURCE.replace(
        "component U1: mcu \"rp2350\"",
        concat!(
            "component U1: mcu \"rp2350\" {\n",
            "    placement_hint { region: centre  priority: hard }\n",
            "  }"
        ),
    );
    let board = lower_source(&source, "sidecar_override.synth");

    let tmp = tempdir("sidecar");
    fs::create_dir_all(&tmp).expect("temp dir created");
    let sidecar_path = tmp.join("sidecar.toml");
    fs::write(
        &sidecar_path,
        "schema_version = 2\n\n[components.U1]\nx = 30.0\ny = 12.0\nrotation = 90\nsource = \"human_drag\"\npriority = \"hard\"\n",
    )
    .expect("sidecar TOML written");

    let result =
        synth_kicad::export_with_sidecar(&board, &tmp, Some(&sidecar_path)).expect("export");

    let pcb_text = fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written");
    let footprints = parse_footprints(&pcb_text);
    let u1 = footprints
        .iter()
        .find(|f| f.refdes == "U1")
        .unwrap_or_else(|| panic!("U1 missing from exported PCB"));
    assert!(
        (u1.x_mm - 30.0).abs() < 1e-6 && (u1.y_mm - 12.0).abs() < 1e-6,
        "human drag must reach the PCB verbatim: U1 at ({}, {}) instead of (30, 12)",
        u1.x_mm,
        u1.y_mm
    );

    // Edge hints still hold for the headers alongside the override.
    let (min_x, _, _, _) = parse_edge_cuts_bbox(&pcb_text).expect("Edge.Cuts outline present");
    let usable_min_x = min_x + PLACER_MARGIN_MM;
    let hl1 = footprints
        .iter()
        .find(|f| f.refdes == "HL1")
        .unwrap_or_else(|| panic!("HL1 missing from exported PCB"));
    assert!(
        hl1.x_mm <= usable_min_x + EDGE_NEAR_MM,
        "HL1 centre x={}mm too far from the left edge (usable left edge={usable_min_x})",
        hl1.x_mm
    );
}
