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

#[test]
fn diodes_number_pins_like_kicad() {
    let dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&dir).expect("seed registry must load without errors");

    let mut checked = 0;
    for (_, part) in registry.iter() {
        let number_of = |name: &str| {
            part.pins
                .iter()
                .find(|p| p.name == name)
                .map(|p| p.number.0.as_str())
        };
        let (Some(cathode), Some(anode)) = (number_of("cathode"), number_of("anode")) else {
            continue;
        };
        assert_eq!(
            (cathode, anode),
            ("1", "2"),
            "part {} must number cathode 1, anode 2 like KiCad",
            part.id
        );
        checked += 1;
    }
    assert!(
        checked >= 10,
        "expected at least 10 diodes, checked {checked}"
    );
}

#[test]
fn d_schottky_limits_are_the_datasheet_ratings_of_its_mpn() {
    let dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&dir).expect("seed registry must load without errors");
    let part = registry
        .lookup("d_schottky")
        .expect("d_schottky is shipped");
    assert_eq!(part.mpn.as_deref(), Some("1N5819HW-7-F"));
    // Diodes Inc. 1N5819HW: VRRM 40 V, IO 1.0 A.
    let limits = part
        .operating_conditions
        .as_ref()
        .expect("operating limits");
    assert_eq!(limits.max_voltage_v, Some(40.0));
    assert_eq!(limits.max_current_ma, Some(1000.0));
}

#[test]
fn a_decoupling_rule_names_a_pin_the_part_has() {
    // A rule on a pin that does not exist is never checked, so it protects nothing.
    let dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&dir).expect("seed registry must load without errors");
    for (_, part) in registry.iter() {
        for rule in &part.required_decoupling {
            for pin in std::iter::once(&rule.net).chain(rule.to.as_ref()) {
                assert!(
                    part.find_pin(pin).is_some(),
                    "part {}: required_decoupling names pin `{pin}`, which it does not have",
                    part.id
                );
            }
        }
    }
}

#[test]
fn no_two_active_parts_share_a_manufacturer_part_number() {
    use std::collections::BTreeMap;
    // Two pairs of shipped parts predate this check and are used by existing
    // boards, so renaming either would change them: they are listed rather than
    // allowed, and a new duplicate fails.
    const PREDATING: [&str; 2] = ["rp2350a", "w25q128jvsiq"];
    let dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&dir).expect("seed registry must load without errors");
    let mut by_mpn: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (_, part) in registry.iter() {
        if part.lifecycle != synth_registry::Lifecycle::Active {
            continue;
        }
        if let Some(mpn) = part.mpn.as_deref().filter(|m| !m.trim().is_empty()) {
            by_mpn
                .entry(mpn.to_ascii_lowercase())
                .or_default()
                .push(part.id.0.clone());
        }
    }
    let duplicates: Vec<_> = by_mpn
        .iter()
        .filter(|(mpn, ids)| ids.len() > 1 && !PREDATING.contains(&mpn.as_str()))
        .collect();
    assert!(
        duplicates.is_empty(),
        "one canonical active id per physical part; mark the others obsolete with a substitute: {duplicates:?}"
    );
}

/// An obsolete entry is a retired name for a part, not a part to buy: it must
/// point at an active substitute, so the procurement identity lives on an
/// entry that is still offered.
#[test]
fn every_obsolete_part_names_an_active_substitute() {
    let dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&dir).expect("seed registry must load without errors");
    for (_, part) in registry.iter() {
        if part.lifecycle != synth_registry::Lifecycle::Obsolete {
            continue;
        }
        assert!(
            part.substitutes.iter().any(|id| registry
                .lookup(id.as_str())
                .is_some_and(|s| s.lifecycle == synth_registry::Lifecycle::Active)),
            "obsolete part {} names no active substitute: {:?}",
            part.id,
            part.substitutes
        );
    }
}

/// The sensor-logger reference board is the one a JLCPCB quote is taken from,
/// so none of its components may be an obsolete registry entry.
#[test]
fn the_sensor_logger_builds_no_obsolete_part() {
    let dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&dir).expect("seed registry must load without errors");
    let source = std::fs::read_to_string(
        workspace_root().join("examples/sensor_logger/sensor_logger.synth"),
    )
    .expect("read sensor_logger.synth");
    let mut checked = 0;
    for line in source.lines().map(str::trim) {
        if !line.starts_with("component ") {
            continue;
        }
        let Some(id) = line.split('"').nth(1) else {
            continue;
        };
        let part = registry
            .lookup(id)
            .unwrap_or_else(|| panic!("sensor_logger names unknown part {id}"));
        assert_ne!(
            part.lifecycle,
            synth_registry::Lifecycle::Obsolete,
            "sensor_logger builds obsolete part {id}; use {:?}",
            part.substitutes
        );
        checked += 1;
    }
    assert!(checked > 0, "no components found in sensor_logger.synth");
}
