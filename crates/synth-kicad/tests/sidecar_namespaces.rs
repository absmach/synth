// SPDX-License-Identifier: Apache-2.0

//! The two layout sidecars are separate namespaces over one schema, and a
//! design's export must follow the right one.
//!
//! The regression this file pins: one `<design>.layout.toml` used to feed
//! both the sheet layout and the PCB placer. A footprint dragged to board
//! millimetres was therefore also read as a sheet position in millimetres,
//! which dragged it far outside its group, overflowed A2, and made
//! `layout_sheets` take the §P26 multi-sheet path. The export then wrote one
//! near-empty sheet per group — the delivered PDF no longer matched the
//! reviewed preview, which is what was reported.

use std::path::{Path, PathBuf};

use synth_layout::sidecar::{
    OverridePriority, OverrideSource, SidecarKind, SidecarLayout, SidecarPlacement,
    SIDECAR_SCHEMA_VERSION,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn board_for(name: &str) -> synth_ir::Board {
    let path = workspace_root().join("fixtures").join("layout").join(name);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let filename = path.file_name().unwrap().to_string_lossy().to_string();
    let parsed = synth_parser::parse(&src, filename.clone());
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    synth_ir::lower(&parsed.ast.expect("ast"), &registry, &filename)
        .board
        .expect("board")
}

fn sidecar_with(refdes: &str, x: f64, y: f64) -> SidecarLayout {
    let mut s = SidecarLayout {
        schema_version: SIDECAR_SCHEMA_VERSION,
        ..Default::default()
    };
    s.merge_override(
        refdes.to_string(),
        SidecarPlacement {
            x,
            y,
            rotation: 0,
            source: OverrideSource::Agent,
            priority: OverridePriority::Hard,
            sheet: None,
            timestamp: None,
            relative_to: None,
            dx: 0.0,
            dy: 0.0,
        },
    );
    s
}

/// The sheet a component ends up on, named by its group, or `root`.
fn sheet_of(board: &synth_ir::Board, layout: &synth_layout::Layout) -> Option<String> {
    let sheets = synth_layout::sheets::layout_sheets(board, layout.clone());
    sheets.iter().find_map(|s| {
        let n = s.name.clone()?;
        s.layout
            .components
            .iter()
            .any(|p| {
                board
                    .component(p.id)
                    .is_some_and(|c| c.group.as_deref() == Some(&n))
            })
            .then_some(n)
    })
}

#[test]
fn schematic_sidecar_changes_the_sheet_and_the_placement_sidecar_does_not() {
    let board = board_for("led_indicator.synth");
    let dir = tempfile::tempdir().unwrap();
    let design = dir.path().join("led_indicator.synth");
    std::fs::write(&design, "board \"x\" {}").unwrap();

    let auto = synth_layout::layout(&board);
    let auto_sheet = sheet_of(&board, &auto);
    let auto_r1 = auto
        .components
        .iter()
        .find(|p| board.component(p.id).is_some_and(|c| c.refdes == "R1"))
        .map(|p| p.center_mm);

    // A board-millimetre position in the PLACEMENT sidecar — the shape a
    // `synth_place_with_hints` drag produces. Resolving the *schematic*
    // sidecar must not find it, so the sheet stays on auto-layout.
    sidecar_with("R1", 63.5, 12.0)
        .save_to_file(&SidecarKind::Placement.canonical_path(&design).unwrap())
        .unwrap();
    assert_eq!(
        synth_layout::placement_sidecar_path(&design).map(|p| p
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()),
        Some("led_indicator.placement.layout.toml".to_string())
    );
    assert_eq!(
        synth_layout::schematic_sidecar_path(&design),
        None,
        "a placement sidecar must not be resolved as a schematic sidecar"
    );
    let with_placement = synth_layout::layout_with_sidecar(
        &board,
        synth_layout::schematic_sidecar_path(&design).as_deref(),
    );
    assert_eq!(
        with_placement
            .components
            .iter()
            .find(|p| board.component(p.id).is_some_and(|c| c.refdes == "R1"))
            .map(|p| p.center_mm),
        auto_r1,
        "the placement sidecar must not move a part on the schematic sheet"
    );
    assert_eq!(sheet_of(&board, &with_placement), auto_sheet);

    // The same coordinates in the SCHEMATIC sidecar do move it — proving the
    // two files are read independently rather than one shadowing the other.
    sidecar_with("R1", 210.82, 33.02)
        .save_to_file(&SidecarKind::Schematic.canonical_path(&design).unwrap())
        .unwrap();
    let with_schematic = synth_layout::layout_with_sidecar(
        &board,
        synth_layout::schematic_sidecar_path(&design).as_deref(),
    );
    assert_eq!(
        with_schematic
            .components
            .iter()
            .find(|p| board.component(p.id).is_some_and(|c| c.refdes == "R1"))
            .map(|p| p.center_mm),
        Some((210.82, 33.02)),
        "the schematic sidecar must move a part on the sheet"
    );

    // `layout_with_sidecar` honours whatever path it is handed, which is why
    // the *resolver* is the thing under test: passing the placement file here
    // applies board millimetres to the sheet. Callers must therefore resolve
    // per space, and the pre-split world bug is exactly what happened when
    // they did not.
    let crossed = synth_layout::layout_with_sidecar(
        &board,
        synth_layout::placement_sidecar_path(&design).as_deref(),
    );
    assert_eq!(
        crossed
            .components
            .iter()
            .find(|p| board.component(p.id).is_some_and(|c| c.refdes == "R1"))
            .map(|p| p.center_mm),
        Some((63.5, 12.0)),
        "an explicitly handed path is applied as given; resolution is the guard"
    );
}

/// A deprecated shared sidecar must still place the board (that was its
/// documented meaning) while leaving the schematic on auto-layout.
#[test]
fn legacy_sidecar_feeds_placement_only() {
    let dir = tempfile::tempdir().unwrap();
    let design = dir.path().join("led_indicator.synth");
    std::fs::write(&design, "board \"x\" {}").unwrap();

    sidecar_with("R1", 63.5, 12.0)
        .save_to_file(&SidecarKind::legacy_path(&design).unwrap())
        .unwrap();

    assert_eq!(
        synth_layout::schematic_sidecar_path(&design),
        None,
        "a pre-split sidecar must not be resolved for the schematic"
    );
    assert_eq!(
        synth_layout::placement_sidecar_path(&design),
        SidecarKind::legacy_path(&design),
        "a pre-split sidecar must still resolve for placement"
    );
}

/// The end-to-end symptom: the schematic sidecar's page fit is what keeps a
/// multi-group board on one sheet, so an export that ignores it produces a
/// different, multi-sheet project.
#[test]
fn export_honours_the_schematic_sidecar_the_render_did() {
    let board = board_for("led_indicator.synth");
    let dir = tempfile::tempdir().unwrap();
    let design = dir.path().join("led_indicator.synth");
    std::fs::write(&design, "board \"x\" {}").unwrap();

    let fitted = SidecarLayout {
        schema_version: SIDECAR_SCHEMA_VERSION,
        fit_sheet: true,
        ..Default::default()
    };
    fitted
        .save_to_file(&SidecarKind::Schematic.canonical_path(&design).unwrap())
        .unwrap();

    let with_sidecar = dir.path().join("with");
    let without_sidecar = dir.path().join("without");

    let a = synth_kicad::export_schematic_only(
        &board,
        &with_sidecar,
        synth_layout::schematic_sidecar_path(&design).as_deref(),
    )
    .expect("export with sidecar");
    let b = synth_kicad::export_schematic_only(&board, &without_sidecar, None)
        .expect("export without sidecar");

    // Whatever the sheet counts are, the sidecar-bearing export must reflect
    // the sidecar — a `fit_sheet` page fit is a persisted visual decision.
    assert_eq!(
        a.sheet_count,
        synth_layout::sheets::layout_sheets(
            &board,
            synth_layout::layout_with_sidecar(
                &board,
                synth_layout::schematic_sidecar_path(&design).as_deref()
            )
        )
        .len(),
        "export sheet count must match the layout the render reviewed"
    );
    assert_ne!(
        std::fs::read(&a.schematic_path).unwrap(),
        std::fs::read(&b.schematic_path).unwrap(),
        "a persisted fit_sheet must change the exported schematic"
    );
}

/// `Sidecars::resolve_for` is the one place that decides which file belongs to
/// which coordinate space, so it is where a regression would reappear.
#[test]
fn resolve_for_pairs_each_file_with_its_own_space() {
    let dir = tempfile::tempdir().unwrap();
    let design = dir.path().join("board.synth");
    std::fs::write(&design, "board \"x\" {}").unwrap();

    // Nothing on disk yet: neither namespace resolves.
    let none = synth_kicad::Sidecars::resolve_for(&design);
    assert_eq!(none.schematic, None);
    assert_eq!(none.placement, None);

    // A placement-only design leaves the schematic on auto-layout.
    sidecar_with("L1", 63.5, 12.0)
        .save_to_file(&SidecarKind::Placement.canonical_path(&design).unwrap())
        .unwrap();
    let placement_only = synth_kicad::Sidecars::resolve_for(&design);
    assert_eq!(placement_only.schematic, None);
    assert_eq!(
        placement_only.placement,
        SidecarKind::Placement.canonical_path(&design)
    );

    // A schematic-only design leaves the PCB on auto-placement.
    std::fs::remove_file(SidecarKind::Placement.canonical_path(&design).unwrap()).unwrap();
    sidecar_with("R1", 210.82, 33.02)
        .save_to_file(&SidecarKind::Schematic.canonical_path(&design).unwrap())
        .unwrap();
    let schematic_only = synth_kicad::Sidecars::resolve_for(&design);
    assert_eq!(
        schematic_only.schematic,
        SidecarKind::Schematic.canonical_path(&design)
    );
    assert_eq!(schematic_only.placement, None);

    // Both present: each lands in its own field, never crossed over.
    sidecar_with("L1", 63.5, 12.0)
        .save_to_file(&SidecarKind::Placement.canonical_path(&design).unwrap())
        .unwrap();
    let both = synth_kicad::Sidecars::resolve_for(&design);
    assert_eq!(
        both.schematic,
        SidecarKind::Schematic.canonical_path(&design)
    );
    assert_eq!(
        both.placement,
        SidecarKind::Placement.canonical_path(&design)
    );
    assert_ne!(both.schematic, both.placement);
}
