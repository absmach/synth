// SPDX-License-Identifier: Apache-2.0

//! Golden-fixture tests: every `fixtures/ir/*.synth` is exported to
//! a KiCad project in a temp directory; the resulting files are
//! snapshotted via insta. Re-running must produce byte-identical
//! output (the determinism guarantee from plan §6.2).

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn read_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "synth"))
        .collect();
    paths.sort();
    paths
}

#[test]
fn every_ir_fixture_exports_deterministically() {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");

    for path in read_sorted(&workspace_root().join("fixtures").join("ir")) {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let src = fs::read_to_string(&path).unwrap();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();

        let parsed = synth_parser::parse(&src, filename.clone());
        let ast = parsed.ast.expect("ast");
        let board = synth_ir::lower(&ast, &registry, &filename)
            .board
            .expect("board");

        let tmp = tempdir(&stem);
        let result = synth_kicad::export(&board, &tmp).expect("export");

        let project = fs::read_to_string(&result.project_path).unwrap();
        let schematic = fs::read_to_string(&result.schematic_path).unwrap();
        let library = fs::read_to_string(&result.library_path).unwrap();
        let bom = fs::read_to_string(&result.bom_path).unwrap();

        insta::with_settings!(
            { snapshot_suffix => format!("{stem}__project"), sort_maps => true },
            { insta::assert_snapshot!(project); }
        );
        insta::with_settings!(
            { snapshot_suffix => format!("{stem}__schematic") },
            { insta::assert_snapshot!(schematic); }
        );
        insta::with_settings!(
            { snapshot_suffix => format!("{stem}__library") },
            { insta::assert_snapshot!(library); }
        );
        insta::with_settings!(
            { snapshot_suffix => format!("{stem}__bom") },
            { insta::assert_snapshot!(bom); }
        );

        // Determinism: re-export and confirm byte-identical files.
        let tmp2 = tempdir(&format!("{stem}_rerun"));
        let result2 = synth_kicad::export(&board, &tmp2).expect("export rerun");
        assert_eq!(
            fs::read_to_string(&result2.project_path).unwrap(),
            project,
            "{stem}: .kicad_pro not deterministic"
        );
        assert_eq!(
            fs::read_to_string(&result2.schematic_path).unwrap(),
            schematic,
            "{stem}: .kicad_sch not deterministic"
        );
        assert_eq!(
            fs::read_to_string(&result2.library_path).unwrap(),
            library,
            "{stem}: .kicad_sym not deterministic"
        );
        assert_eq!(
            fs::read_to_string(&result2.bom_path).unwrap(),
            bom,
            "{stem}: bom.csv not deterministic"
        );
    }
}

/// One reference design, exported twice and checked for structure +
/// byte determinism. Kept as a shared helper so each fixture can run as
/// its own `#[test]` below — cargo-nextest then runs them in parallel
/// (a single loop test would serialize all exports, and the PCB
/// place+route on the larger designs is the slowest part of CI).
fn export_reference_deterministically(registry: &synth_registry::Registry, stem: &str) {
    let path = workspace_root()
        .join("fixtures")
        .join("kicad-reference")
        .join(format!("{stem}.synth"));
    let src = fs::read_to_string(&path).unwrap();
    let filename = format!("{stem}.synth");

    let parsed = synth_parser::parse(&src, filename.clone());
    let ast = parsed.ast.expect("ast");
    let board = synth_ir::lower(&ast, registry, &filename)
        .board
        .expect("board");

    let tmp = tempdir(&format!("ref_{stem}"));
    let result = synth_kicad::export(&board, &tmp).expect("export reference design");

    let project = fs::read_to_string(&result.project_path).unwrap();
    let schematic = fs::read_to_string(&result.schematic_path).unwrap();
    let library = fs::read_to_string(&result.library_path).unwrap();
    let pcb = fs::read_to_string(&result.pcb_path).unwrap();
    let bom = fs::read_to_string(&result.bom_path).unwrap();

    // Structural assertions on exported files
    assert!(
        schematic.contains("(kicad_sch"),
        "{stem}: schematic missing (kicad_sch"
    );
    assert!(
        schematic.contains("(lib_symbols"),
        "{stem}: schematic missing (lib_symbols"
    );
    assert!(
        schematic.contains("(symbol"),
        "{stem}: schematic missing (symbol"
    );
    assert!(pcb.contains("(kicad_pcb"), "{stem}: PCB missing (kicad_pcb");
    assert!(bom.starts_with("refdes,"), "{stem}: BOM missing header");

    // Determinism check: re-export and assert byte-for-byte identity
    let tmp2 = tempdir(&format!("ref_{stem}_rerun"));
    let result2 = synth_kicad::export(&board, &tmp2).expect("export rerun");
    assert_eq!(
        fs::read_to_string(&result2.project_path).unwrap(),
        project,
        "{stem}: .kicad_pro not deterministic"
    );
    assert_eq!(
        fs::read_to_string(&result2.schematic_path).unwrap(),
        schematic,
        "{stem}: .kicad_sch not deterministic"
    );
    assert_eq!(
        fs::read_to_string(&result2.library_path).unwrap(),
        library,
        "{stem}: .kicad_sym not deterministic"
    );
    assert_eq!(
        fs::read_to_string(&result2.pcb_path).unwrap(),
        pcb,
        "{stem}: .kicad_pcb not deterministic"
    );
    assert_eq!(
        fs::read_to_string(&result2.bom_path).unwrap(),
        bom,
        "{stem}: bom.csv not deterministic"
    );
}

macro_rules! reference_test {
    ($name:ident, $stem:literal) => {
        #[test]
        fn $name() {
            let registry =
                synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
                    .expect("seed registry must load");
            export_reference_deterministically(&registry, $stem);
        }
    };
}

reference_test!(ref_battery_charger, "ref_battery_charger");
reference_test!(ref_crystal_mcu, "ref_crystal_mcu");
reference_test!(ref_ldo_sensor, "ref_ldo_sensor");
reference_test!(ref_led_driver, "ref_led_driver");
reference_test!(ref_spi_flash, "ref_spi_flash");
reference_test!(ref_secure_element, "ref_secure_element");
reference_test!(ref_uart_bridge, "ref_uart_bridge");
reference_test!(ref_usb_cdc, "ref_usb_cdc");
reference_test!(sensor_logger, "sensor_logger");
reference_test!(secure_tracker, "secure_tracker");

fn tempdir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("synth-kicad-golden-{label}-{}", std::process::id()));
    if p.exists() {
        let _ = fs::remove_dir_all(&p);
    }
    p
}
