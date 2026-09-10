// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

#[test]
fn registry_integrity_check() {
    let dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&dir).expect("seed registry must load without errors");

    assert!(
        registry.len() >= 100,
        "expected at least 100 parts in seed registry, found {}",
        registry.len()
    );

    for (id, part) in registry.iter() {
        assert_eq!(
            id.as_str(),
            part.id.as_str(),
            "registry key must match part.id"
        );
        assert!(
            !part.kind.is_empty(),
            "part {} must have non-empty kind",
            part.id
        );

        // Verify footprint and footprint dimensions
        assert!(
            part.kicad_footprint
                .as_ref()
                .is_some_and(|f| !f.trim().is_empty()),
            "part {} must have non-empty kicad_footprint",
            part.id
        );
        let dims = part
            .footprint_dimensions
            .as_ref()
            .unwrap_or_else(|| panic!("part {} must specify footprint_dimensions", part.id));
        assert!(
            dims.width_mm > 0.0 && dims.height_mm > 0.0,
            "part {} footprint_dimensions must be positive (got {}x{})",
            part.id,
            dims.width_mm,
            dims.height_mm
        );

        // Verify substitutes point to valid parts in the registry
        for sub in &part.substitutes {
            assert!(
                registry.lookup(sub.as_str()).is_some(),
                "part {} lists substitute {} which does not exist in registry",
                part.id,
                sub
            );
        }
    }
}
