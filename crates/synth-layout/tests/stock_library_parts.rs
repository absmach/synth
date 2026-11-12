// SPDX-License-Identifier: Apache-2.0

//! Shipped parts that once named KiCad symbols and footprints KiCad does not
//! ship (absmach/synth#96). A name that resolves to nothing does not fail a
//! check, it turns it into `unknown`, so the pin checks skipped silently. Each
//! part here is qualified against the stock libraries and every check that
//! compares it with them has to run and pass.
//!
//! Its own test binary: `registry_qualification` points `KICAD_FOOTPRINT_DIR`
//! at a fixture library for the whole process.

use std::path::{Path, PathBuf};

use synth_layout::qualify_facts::InstalledKicad;
use synth_registry::qualify::{qualify_part, CheckStatus};

const DEFAULT_FOOTPRINT_DIR: &str = "/usr/share/kicad/footprints";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

/// Whether the stock libraries are installed; a test that needs them fails
/// without them unless `SYNTH_ALLOW_MISSING_KICAD=1` says to skip.
fn stock_libraries_installed() -> bool {
    let footprint = "RF_Module.pretty/ESP32-C6-MINI-1.kicad_mod";
    let footprint_found = std::env::var("KICAD_FOOTPRINT_DIR")
        .ok()
        .map(PathBuf::from)
        .into_iter()
        .chain([PathBuf::from(DEFAULT_FOOTPRINT_DIR)])
        .any(|dir| dir.join(footprint).exists());
    let missing = if !footprint_found {
        Some(format!(
            "KICAD_FOOTPRINT_DIR, or {DEFAULT_FOOTPRINT_DIR}, with {footprint}"
        ))
    } else if synth_layout::kicad_lib_loader::physical_pins("Memory_EEPROM:24LC512").is_none() {
        Some("the stock KiCad symbol libraries".to_string())
    } else {
        None
    };
    match missing {
        None => true,
        Some(what) if std::env::var("SYNTH_ALLOW_MISSING_KICAD").as_deref() == Ok("1") => {
            eprintln!("skipped on purpose (SYNTH_ALLOW_MISSING_KICAD=1): needs {what}");
            false
        }
        Some(what) => {
            panic!("this test needs {what}; set it up, or set SYNTH_ALLOW_MISSING_KICAD=1 to skip")
        }
    }
}

#[test]
fn parts_that_named_missing_libraries_qualify_against_the_stock_ones() {
    if !stock_libraries_installed() {
        return;
    }
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("shipped registry loads");
    let facts = InstalledKicad;
    for id in [
        "esp32_c3_mini",
        "at24c512_eeprom",
        "m24m02_eeprom",
        "mpu6050_imu",
        // #87: a six-pad balun declared only pads 1-4.
        "balun_2g4",
    ] {
        let part = registry
            .lookup(id)
            .unwrap_or_else(|| panic!("no shipped part `{id}`"));
        let q = qualify_part(part, &facts, &facts);
        // Provenance is #51's, and the 3D model is whether KiCad ships one:
        // neither compares the part with the libraries it names.
        let library_checks: Vec<_> = q
            .checks
            .iter()
            .filter(|c| !matches!(c.name.as_str(), "provenance" | "three_d_model"))
            .collect();
        assert!(
            !library_checks.is_empty(),
            "{id}: no library checks ran\n{:#?}",
            q.checks
        );
        for check in library_checks {
            assert!(
                matches!(check.status, CheckStatus::Pass | CheckStatus::NotApplicable),
                "{id}: `{}` is {:?} against the stock libraries\n{check:#?}",
                check.name,
                check.status
            );
        }
        assert!(
            !q.has_structural_defect(),
            "{id}: {:?}",
            q.structural_defects().collect::<Vec<_>>()
        );
    }
}
