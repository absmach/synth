// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::capability::PinCapability;
use crate::part::{Part, PartId};
use crate::registry::Registry;

/// Errors surfaced when loading a registry directory.
#[derive(Debug, Error)]
pub enum LoadError {
    #[error("could not read directory {path}: {source}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not read part file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not parse part file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("part {path}: id `{found}` does not match filename `{expected}`")]
    IdMismatch {
        path: PathBuf,
        found: String,
        expected: String,
    },

    #[error("part {path}: duplicate pin name `{name}`")]
    DuplicatePinName { path: PathBuf, name: String },

    #[error("part {path}: duplicate pin number `{number}`")]
    DuplicatePinNumber { path: PathBuf, number: String },

    #[error(
        "part {path}: pin `{pin}` declares capability `{capability:?}` which requires electrical \
         type `{required:?}` but pin has `{actual:?}`"
    )]
    CapabilityElectricalMismatch {
        path: PathBuf,
        pin: String,
        capability: PinCapability,
        required: crate::ElectricalType,
        actual: crate::ElectricalType,
    },

    #[error("registry has duplicate part id `{id}` at {path:?}")]
    DuplicatePartId { id: String, path: PathBuf },

    #[error(
        "part {path}: kicad_symbol `{kicad_symbol}` does not declare \
         pin number `{registry_number}` (registry pin `{registry_pin}`). \
         Schematic and PCB would talk to different physical pins."
    )]
    KicadPinMismatch {
        path: PathBuf,
        kicad_symbol: String,
        registry_pin: String,
        registry_number: String,
    },

    #[error(
        "part {path}: invalid operating conditions (min_voltage_v {min} > max_voltage_v {max})"
    )]
    InvalidOperatingConditions { path: PathBuf, min: f64, max: f64 },

    #[error("part {path}: invalid footprint dimensions ({message})")]
    InvalidFootprintDimensions { path: PathBuf, message: String },
}

/// Recursively load every `*.synth.toml` file under `root` into a
/// [`Registry`]. Files are validated structurally (no duplicate pin
/// names/numbers; declared capabilities are compatible with the
/// declared electrical type; the part `id` matches the filename
/// stem) at load time. Logical conflicts across parts (e.g. duplicate
/// `PartId`) are caught at insert time.
pub fn load_dir(root: &Path) -> Result<Registry, LoadError> {
    let sources = collect_sources(root)?;
    load_from_sources(&sources)
}

/// Gather every `*.synth.toml` file (recursively) under `root` as
/// `(path, contents)` pairs, sorted for deterministic ordering.
fn collect_sources(root: &Path) -> Result<Vec<(PathBuf, String)>, LoadError> {
    let mut paths = Vec::new();
    walk(root, &mut paths)?;
    paths.sort();

    let mut sources = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = std::fs::read_to_string(&path).map_err(|source| LoadError::ReadFile {
            path: path.clone(),
            source,
        })?;
        sources.push((path, bytes));
    }
    Ok(sources)
}

/// Load a registry from in-memory `(filename, toml_contents)` pairs
/// without touching the filesystem. Used by WASM targets where
/// `std::fs` is unavailable: the seed registry is bundled into the
/// binary via `include_str!`.
///
/// The `filename` is used purely for diagnostic messages and the
/// id-matches-filename validation; it does not have to exist on
/// disk. The convention is to pass the bare basename
/// (`"rp2350.synth.toml"`), but any string with `.synth.toml`
/// suffix works.
pub fn load_from_strs(sources: &[(&str, &str)]) -> Result<Registry, LoadError> {
    let owned: Vec<(PathBuf, String)> = sources
        .iter()
        .map(|(name, body)| (PathBuf::from(name), (*body).to_string()))
        .collect();
    load_from_sources(&owned)
}

/// Every seed `*.synth.toml`, generated at build time as
/// `(repo-relative path, contents)` pairs by `build.rs` (absolute
/// `include_str!` paths; a proc-macro dir-embedder silently embeds
/// nothing across `../..` segments).
mod seed_embed {
    include!(concat!(env!("OUT_DIR"), "/seed_registry.rs"));
}

/// The seed (Tier-1) registry compiled into the binary, parsed once.
/// Lets CLI runs outside a synth checkout resolve every shipped part
/// without a `registry/parts` directory on disk.
///
/// # Panics
/// Only if the embedded seed fails to parse — impossible for the
/// shipped tree (load-tested at build time by the seed tests); a
/// panic here is a build bug, not a runtime condition.
pub fn embedded_registry() -> &'static Registry {
    static EMBEDDED: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
    EMBEDDED.get_or_init(|| {
        load_from_strs(seed_embed::SEED_FILES)
            .unwrap_or_else(|e| panic!("embedded seed registry must always parse: {e}"))
    })
}

fn load_from_sources(sources: &[(PathBuf, String)]) -> Result<Registry, LoadError> {
    let mut registry = Registry::new();
    for (path, bytes) in sources {
        let part: Part = toml::from_str(bytes).map_err(|source| LoadError::Parse {
            path: path.clone(),
            source,
        })?;
        validate_part(path, &part)?;

        if registry.lookup(part.id.as_str()).is_some() {
            return Err(LoadError::DuplicatePartId {
                id: part.id.as_str().to_string(),
                path: path.clone(),
            });
        }
        registry.insert(part);
    }
    Ok(registry)
}

/// A non-fatal condition observed while loading a tiered registry
/// (Phase 15, R15.1).
#[derive(Debug, Clone)]
pub enum LoadWarning {
    /// A user (Tier-2) part shadows a shipped (Tier-1) part of the same
    /// `id`. The user entry wins, but the divergence is always reported
    /// so it stays visible. Maps to diagnostic code `W-SYNTH-REG-001`.
    Shadow { id: String, user_path: PathBuf },
}

/// Outcome of [`load_tiered`]: the merged registry plus any warnings
/// (e.g. shadowed parts) that should be surfaced to the user.
#[derive(Debug)]
pub struct LoadResult {
    pub registry: Registry,
    pub warnings: Vec<LoadWarning>,
}

/// Resolve the Tier-2 (per-user) registry directory (Phase 15, R15.1).
///
/// Honours `SYNTH_USER_REGISTRY_DIR` (explicit override), then the XDG
/// base directory spec (`$XDG_DATA_HOME/synth/registry/parts`, falling
/// back to `~/.local/share/synth/registry/parts` on Linux). Returns
/// `None` when no base directory can be determined (e.g. no `HOME` on a
/// non-XDG platform).
pub fn user_registry_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("SYNTH_USER_REGISTRY_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return Some(
                PathBuf::from(xdg)
                    .join("synth")
                    .join("registry")
                    .join("parts"),
            );
        }
    }
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|home| {
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("synth")
                .join("registry")
                .join("parts")
        })
}

/// Resolve the *installed* Tier-1 (shipped) registry directory — the
/// XDG location `synth registry install` materializes the embedded
/// seed into. Honours `SYNTH_SHIPPED_REGISTRY_DIR` (explicit
/// override), then the XDG base directory spec
/// (`$XDG_DATA_HOME/synth/registry/shipped`, falling back to
/// `~/.local/share/synth/registry/shipped` on Linux). Distinct from
/// the Tier-2 [`user_registry_dir`] path so the two tiers never
/// alias each other. Returns `None` when no base directory can be
/// determined.
#[must_use]
pub fn shipped_registry_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("SYNTH_SHIPPED_REGISTRY_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return Some(
                PathBuf::from(xdg)
                    .join("synth")
                    .join("registry")
                    .join("shipped"),
            );
        }
    }
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|home| {
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("synth")
                .join("registry")
                .join("shipped")
        })
}

/// Materialize the embedded seed registry into `target` (creating
/// parent directories). Tier-1 semantics: shipped files are
/// overwritten in place — the binary's copy is authoritative — while
/// files the user added alongside survive untouched (those belong in
/// the Tier-2 overlay). Returns the number of files written.
///
/// # Errors
/// Propagates filesystem errors from creating directories or
/// writing files.
pub fn write_embedded_seed(target: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(target)?;
    for (rel, contents) in seed_embed::SEED_FILES {
        let dest = target.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, contents)?;
    }
    Ok(seed_embed::SEED_FILES.len())
}

/// Load a two-tier registry (Phase 15, R15.1): Tier 1 (shipped, read-only)
/// followed by Tier 2 (per-user). All existing per-file validation runs
/// identically on both tiers. An `id` collision resolves to the **user**
/// entry shadowing the shipped one; the divergence is recorded as a
/// [`LoadWarning::Shadow`] (`W-SYNTH-REG-001`) instead of failing, so
/// local extensions stay usable while divergence is always visible.
///
/// Pass `strict = true` to promote shadowing into a hard error (CI /
/// production exports via `--strict-registry`).
pub fn load_tiered(
    global_dir: &Path,
    user_dir: &Path,
    strict: bool,
) -> Result<LoadResult, LoadError> {
    let mut warnings = Vec::new();
    let global_sources = collect_sources(global_dir)?;
    let mut registry = load_from_sources(&global_sources)?;

    // Remember shipped `id`s so we can flag user shadowing.
    let shipped_ids: std::collections::HashSet<String> =
        registry.ids().map(|id| id.as_str().to_string()).collect();

    if user_dir.exists() {
        let user_sources = collect_sources(user_dir)?;
        // Track user-internal `id`s to surface intra-tier collisions.
        let mut seen_user: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (path, bytes) in &user_sources {
            let part: Part = toml::from_str(bytes).map_err(|source| LoadError::Parse {
                path: path.clone(),
                source,
            })?;
            validate_part(path, &part)?;

            let is_shadow = shipped_ids.contains(part.id.as_str());
            let dupe_user = !seen_user.insert(part.id.as_str().to_string());
            if is_shadow {
                if strict {
                    return Err(LoadError::DuplicatePartId {
                        id: part.id.as_str().to_string(),
                        path: path.clone(),
                    });
                }
                warnings.push(LoadWarning::Shadow {
                    id: part.id.as_str().to_string(),
                    user_path: path.clone(),
                });
            }
            if dupe_user {
                warnings.push(LoadWarning::Shadow {
                    id: part.id.as_str().to_string(),
                    user_path: path.clone(),
                });
            }
            registry.insert(part);
        }
    }

    Ok(LoadResult { registry, warnings })
}

/// Merge the Tier-2 (per-user) registry directory on top of an
/// already-loaded global [`Registry`] — the in-memory twin of
/// [`load_tiered`]'s user-overlay stage. Shadowing semantics are
/// identical: user entries win, `strict` promotes a shadow to a hard
/// error.
pub fn load_user_overlay(
    global: Registry,
    user_dir: &Path,
    strict: bool,
) -> Result<LoadResult, LoadError> {
    let shipped_ids: std::collections::HashSet<String> =
        global.ids().map(|id| id.as_str().to_string()).collect();
    let mut registry = global;
    let mut warnings = Vec::new();
    let mut seen_user: std::collections::HashSet<String> = std::collections::HashSet::new();

    if user_dir.exists() {
        let user_sources = collect_sources(user_dir)?;
        for (path, bytes) in &user_sources {
            let part: Part = toml::from_str(bytes).map_err(|source| LoadError::Parse {
                path: path.clone(),
                source,
            })?;
            validate_part(path, &part)?;

            let is_shadow = shipped_ids.contains(part.id.as_str());
            let dupe_user = !seen_user.insert(part.id.as_str().to_string());
            if is_shadow || dupe_user {
                if strict && is_shadow {
                    return Err(LoadError::DuplicatePartId {
                        id: part.id.as_str().to_string(),
                        path: path.clone(),
                    });
                }
                warnings.push(LoadWarning::Shadow {
                    id: part.id.as_str().to_string(),
                    user_path: path.clone(),
                });
            }
            registry.insert(part);
        }
    }

    Ok(LoadResult { registry, warnings })
}

/// Generate a minimal, valid `.synth.toml` skeleton for a part that is
/// not yet in the registry (Phase 15, R15.6). Every generated pin is
/// `required = false` so the design can be validated and exported while
/// the agent/user fills in real electrical types and capabilities from a
/// datasheet. The skeleton carries `[provenance] source = "authored"`
/// with an empty `reviewed_by`, so it is flagged `W-SYNTH-PART-UNVERIFIED`
/// until reviewed.
///
/// The returned string always parses via [`load_from_strs`].
pub fn create_part_stub(id: &str, pins: &[String]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "id = {}", toml_string(id));
    out.push_str("kind = \"ic\"\n");
    out.push_str("version = 1\n\n");
    out.push_str("[provenance]\n");
    out.push_str("source = \"authored\"\n");
    out.push_str("reviewed_by = \"\"\n\n");
    for (i, name) in pins.iter().enumerate() {
        out.push_str("[[pins]]\n");
        let _ = writeln!(out, "name = {}", toml_string(name));
        let _ = writeln!(out, "number = \"{}\"", i + 1);
        out.push_str("electrical_type = \"bidirectional\"\n");
        out.push_str("required = false\n\n");
    }
    out
}

/// Minimal TOML basic-string escaping (double quote + backslash).
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), LoadError> {
    let entries = std::fs::read_dir(dir).map_err(|source| LoadError::ReadDir {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| LoadError::ReadDir {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.ends_with(".synth.toml"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn validate_part(path: &Path, part: &Part) -> Result<(), LoadError> {
    // id must match filename stem (without the `.synth.toml` suffix).
    let expected_id = path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_suffix(".synth.toml"))
        .unwrap_or("");
    if part.id.as_str() != expected_id {
        return Err(LoadError::IdMismatch {
            path: path.to_path_buf(),
            found: part.id.as_str().to_string(),
            expected: expected_id.to_string(),
        });
    }

    let mut seen_names = HashSet::new();
    let mut seen_numbers = HashSet::new();
    for pin in &part.pins {
        if !seen_names.insert(&pin.name) {
            return Err(LoadError::DuplicatePinName {
                path: path.to_path_buf(),
                name: pin.name.clone(),
            });
        }
        if !seen_numbers.insert(&pin.number.0) {
            return Err(LoadError::DuplicatePinNumber {
                path: path.to_path_buf(),
                number: pin.number.0.clone(),
            });
        }
        for &cap in &pin.capabilities {
            if let Some(required) = cap.required_electrical_type() {
                if pin.electrical_type != required {
                    return Err(LoadError::CapabilityElectricalMismatch {
                        path: path.to_path_buf(),
                        pin: pin.name.clone(),
                        capability: cap,
                        required,
                        actual: pin.electrical_type,
                    });
                }
            }
        }
    }

    // Pin-number invariant: if the part references a KiCad stock
    // symbol, every registry pin number must exist in that symbol.
    // Skipped when KiCad isn't installed locally (CI without KiCad);
    // the schematic exporter then falls back to a synthesized
    // rectangle and there's no real symbol to disagree with.
    if let Some(lib_id) = part.kicad_symbol.as_deref() {
        if let Some(kicad_numbers) = crate::kicad_pin_check::kicad_pin_numbers(lib_id) {
            for pin in &part.pins {
                if !kicad_numbers.contains(&pin.number.0) {
                    return Err(LoadError::KicadPinMismatch {
                        path: path.to_path_buf(),
                        kicad_symbol: lib_id.to_string(),
                        registry_pin: pin.name.clone(),
                        registry_number: pin.number.0.clone(),
                    });
                }
            }
        }
    }

    if let Some(ref dims) = part.footprint_dimensions {
        if dims.width_mm <= 0.0 || dims.height_mm <= 0.0 {
            return Err(LoadError::InvalidFootprintDimensions {
                path: path.to_path_buf(),
                message: format!(
                    "width_mm ({}) and height_mm ({}) must be positive",
                    dims.width_mm, dims.height_mm
                ),
            });
        }
    }

    if let Some(ref op) = part.operating_conditions {
        if let (Some(min), Some(max)) = (op.min_voltage_v, op.max_voltage_v) {
            if min > max {
                return Err(LoadError::InvalidOperatingConditions {
                    path: path.to_path_buf(),
                    min,
                    max,
                });
            }
        }
    }

    let _ = PartId(part.id.as_str().to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .unwrap()
    }

    #[test]
    fn seed_registry_loads() {
        let dir = workspace_root().join("registry").join("parts");
        let registry = load_dir(&dir).expect("seed registry must load");
        assert!(!registry.is_empty(), "expected at least one seed part");
        assert!(
            registry.lookup("rp2350").is_some(),
            "rp2350 should be in the seed registry"
        );
    }

    #[test]
    fn kicad_pin_mismatch_is_caught_at_load() {
        // Synthetic part that claims pin number `99` while
        // referencing `Device:R` (which has pins 1 and 2). Only
        // exercised when KiCad's bundled symbol library is
        // available locally — silently skipped on CI without
        // KiCad (the underlying verifier returns None).
        if crate::kicad_pin_check::kicad_pin_numbers("Device:R").is_none() {
            eprintln!("skipping pin-mismatch test: KiCad symbol library not present");
            return;
        }
        let toml = r#"
id = "bogus_resistor"
kind = "resistor"
kicad_symbol = "Device:R"
[[pins]]
name = "p1"
number = "99"
electrical_type = "passive"
"#;
        let err = load_from_strs(&[("bogus_resistor.synth.toml", toml)])
            .expect_err("pin 99 should not match Device:R");
        match err {
            LoadError::KicadPinMismatch {
                registry_number, ..
            } => assert_eq!(registry_number, "99"),
            other => panic!("expected KicadPinMismatch, got {other:?}"),
        }
    }

    #[test]
    #[allow(clippy::similar_names)] // `usb_dp` and `usb_dn` are the actual pin names
    fn rp2350_has_usb_pins_with_correct_electrical_types() {
        let dir = workspace_root().join("registry").join("parts");
        let registry = load_dir(&dir).expect("seed registry must load");
        let rp2350 = registry.lookup("rp2350").expect("rp2350");

        let usb_dp = rp2350.find_pin("usb_dp").expect("usb_dp pin");
        assert_eq!(
            usb_dp.electrical_type,
            crate::ElectricalType::DifferentialPositive
        );
        assert!(usb_dp.capabilities.contains(&PinCapability::UsbDp));

        let usb_dn = rp2350.find_pin("usb_dn").expect("usb_dn pin");
        assert_eq!(
            usb_dn.electrical_type,
            crate::ElectricalType::DifferentialNegative
        );
        assert!(usb_dn.capabilities.contains(&PinCapability::UsbDn));
    }

    #[test]
    fn parses_footprint_dimensions_and_operating_conditions() {
        let toml = r#"
id = "test_ic"
kind = "ic"

[footprint_dimensions]
width_mm = 5.0
height_mm = 5.0
courtyard_margin_mm = 0.25

[operating_conditions]
min_voltage_v = 1.8
max_voltage_v = 3.6
max_current_ma = 100.0

[[pins]]
name = "vcc"
number = "1"
electrical_type = "power_input"
voltage_max_v = 3.6

[[pins]]
name = "in"
number = "2"
electrical_type = "input"
unit = "A"
voltage_max_v = 5.0
"#;
        let reg = load_from_strs(&[("test_ic.synth.toml", toml)])
            .expect("valid part with dimensions & conditions");
        let part = reg.lookup("test_ic").unwrap();
        let dims = part.footprint_dimensions.as_ref().unwrap();
        assert_eq!(dims.width_mm, 5.0);
        assert_eq!(dims.height_mm, 5.0);
        assert_eq!(dims.courtyard_margin_mm, Some(0.25));

        let op = part.operating_conditions.as_ref().unwrap();
        assert_eq!(op.min_voltage_v, Some(1.8));
        assert_eq!(op.max_voltage_v, Some(3.6));
        assert_eq!(op.max_current_ma, Some(100.0));

        let pin_in = part.find_pin("in").unwrap();
        assert_eq!(pin_in.unit.as_deref(), Some("A"));
        assert_eq!(pin_in.voltage_max_v, Some(5.0));
    }

    #[test]
    fn create_part_stub_is_valid_and_unverified() {
        let stub = create_part_stub("mystery_ic", &["vcc".into(), "gnd".into(), "sda".into()]);
        let reg = load_from_strs(&[("mystery_ic.synth.toml", &stub)])
            .expect("stub must parse and validate");
        let part = reg.lookup("mystery_ic").expect("stub part present");
        assert_eq!(part.pins.len(), 3, "stub should carry the supplied pins");
        assert!(!part.pins[0].required, "stub pins are not required");
        assert!(
            part.is_unverified(),
            "authored stub without reviewed_by is unverified"
        );
    }

    #[test]
    fn tiered_load_shadows_global_with_warning() {
        let tmp = std::env::temp_dir().join(format!("synth_tiered_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let global = tmp.join("global");
        let user = tmp.join("user");
        std::fs::create_dir_all(&global).unwrap();
        std::fs::create_dir_all(&user).unwrap();

        // Shipped part `rp2350`.
        std::fs::write(
            global.join("rp2350.synth.toml"),
            "id = \"rp2350\"\nkind = \"ic\"\n[[pins]]\nname = \"a\"\nnumber = \"1\"\nelectrical_type = \"input\"\n",
        )
        .unwrap();
        // User override of `rp2350` + a brand-new `local_led`.
        std::fs::write(
            user.join("rp2350.synth.toml"),
            "id = \"rp2350\"\nkind = \"ic\"\n[[pins]]\nname = \"b\"\nnumber = \"1\"\nelectrical_type = \"input\"\n",
        )
        .unwrap();
        std::fs::write(
            user.join("local_led.synth.toml"),
            "id = \"local_led\"\nkind = \"led\"\n[[pins]]\nname = \"k\"\nnumber = \"1\"\nelectrical_type = \"output\"\n",
        )
        .unwrap();

        let res = load_tiered(&global, &user, false).expect("tiered load succeeds");
        assert_eq!(res.warnings.len(), 1, "one shadow expected");
        assert!(matches!(res.warnings[0], LoadWarning::Shadow { ref id, .. } if id == "rp2350"));
        // User entry wins: pin name should be the override's `b`.
        let rp = res.registry.lookup("rp2350").expect("rp2350 present");
        assert_eq!(rp.pins[0].name, "b");
        assert!(res.registry.lookup("local_led").is_some());

        // Strict mode promotes the shadow to a hard error.
        assert!(load_tiered(&global, &user, true).is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn provenance_seed_is_verified_unless_reviewed_by_empty() {
        let verified = "id = \"v1\"\nkind = \"ic\"\n[[pins]]\nname = \"a\"\nnumber = \"1\"\nelectrical_type = \"input\"\n";
        let reg = load_from_strs(&[("v1.synth.toml", verified)]).unwrap();
        assert!(
            !reg.lookup("v1").unwrap().is_unverified(),
            "no provenance = seed = verified"
        );

        let unverified = "id = \"v2\"\nkind = \"ic\"\n[provenance]\nsource = \"authored\"\nreviewed_by = \"\"\n[[pins]]\nname = \"a\"\nnumber = \"1\"\nelectrical_type = \"input\"\n";
        let reg = load_from_strs(&[("v2.synth.toml", unverified)]).unwrap();
        assert!(
            reg.lookup("v2").unwrap().is_unverified(),
            "empty reviewed_by = unverified"
        );

        let reviewed = "id = \"v3\"\nkind = \"ic\"\n[provenance]\nsource = \"authored\"\nreviewed_by = \"alice\"\n[[pins]]\nname = \"a\"\nnumber = \"1\"\nelectrical_type = \"input\"\n";
        let reg = load_from_strs(&[("v3.synth.toml", reviewed)]).unwrap();
        assert!(
            !reg.lookup("v3").unwrap().is_unverified(),
            "reviewed_by set = verified"
        );
    }

    #[test]
    fn embedded_seed_registry_is_available_without_disk() {
        let reg = embedded_registry();
        assert!(!reg.is_empty(), "embedded seed must carry parts");
        assert!(
            reg.lookup("rp2350").is_some(),
            "embedded seed must contain rp2350"
        );
    }

    #[test]
    fn user_overlay_merges_over_embedded_registry() {
        let tmp = std::env::temp_dir().join(format!("synth_overlay_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("local_led.synth.toml"),
            "id = \"local_led\"\nkind = \"led\"\n[[pins]]\nname = \"k\"\nnumber = \"1\"\nelectrical_type = \"output\"\n",
        )
        .unwrap();

        let res = load_user_overlay(embedded_registry().clone(), &tmp, false)
            .expect("overlay must merge");
        assert!(res.warnings.is_empty());
        assert!(res.registry.lookup("local_led").is_some());
        assert!(
            res.registry.lookup("rp2350").is_some(),
            "embedded parts survive the overlay"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn shipped_registry_dir_respects_env_override() {
        std::env::set_var("SYNTH_SHIPPED_REGISTRY_DIR", "/tmp/synth_shipped_test");
        let got = shipped_registry_dir().expect("override honoured");
        assert_eq!(got, PathBuf::from("/tmp/synth_shipped_test"));
        std::env::remove_var("SYNTH_SHIPPED_REGISTRY_DIR");
    }

    #[test]
    fn write_embedded_seed_is_idempotent_and_completes() {
        let tmp = std::env::temp_dir().join(format!("synth_install_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let target = tmp.join("parts");

        let first = write_embedded_seed(&target).expect("first install");
        assert!(first > 50, "seed has ~100 parts, wrote {first}");
        let rp = target.join("mcus/rp2350.synth.toml");
        assert!(rp.exists(), "subdirectory layout preserved");

        // Idempotent: second install overwrites in place, same count.
        let second = write_embedded_seed(&target).expect("second install");
        assert_eq!(first, second);

        // User-added files alongside survive (Tier-2 semantics).
        let extra = target.join("mcus/my_part.synth.toml");
        std::fs::write(&extra, "id = \"my_part\"\nkind = \"ic\"\n").unwrap();
        let _ = write_embedded_seed(&target);
        assert!(extra.exists(), "non-shipped files must survive install");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn user_registry_dir_respects_env_override() {
        std::env::set_var("SYNTH_USER_REGISTRY_DIR", "/tmp/synth_user_test");
        let got = user_registry_dir().expect("override honoured");
        assert_eq!(got, PathBuf::from("/tmp/synth_user_test"));
        std::env::remove_var("SYNTH_USER_REGISTRY_DIR");
    }
}
