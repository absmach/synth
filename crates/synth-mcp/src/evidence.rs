// SPDX-License-Identifier: Apache-2.0

//! Canonical design-state hash and evidence files.
//!
//! A gate result only means something together with the revision it
//! evaluated. [`design_hash`] gives a design one identity: a SHA-256 over
//! every input that changes what the compiler produces, namely
//!
//! - the `.synth` source and every module it imports,
//! - both layout sidecars (`*.schematic.layout.toml`, `*.placement.layout.toml`),
//! - the resolved registry part of every component the design uses,
//! - the manufacturer profile and stackup.
//!
//! The hash is portable. Text is normalised to LF before hashing, entries are
//! hashed in sorted relative-path order, and each is framed as
//! `path\0content\0`. Inputs are enumerated rather than found by walking a
//! directory, so output directories, `evidence.json` and any `.synth/` state
//! can never change a design's identity.
//!
//! The per-file hashes come back with the total, so a mismatch can name
//! what changed ([`compare`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use synth_diagnostics::{Gate, GateReport, Verdict};

/// Wire identifier of an [`Evidence`] document.
pub const EVIDENCE_SCHEMA: &str = "synth.evidence.v1";

/// File name `synth export` writes its evidence to, inside the output directory.
pub const EVIDENCE_FILE: &str = "evidence.json";

/// One input covered by a [`DesignHash`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HashedFile {
    /// Relative, `/`-separated path. Entries that are not files on disk
    /// (registry parts, the fabrication profile) live under a virtual
    /// prefix: `registry/<part>.synth.toml`, `fabrication.json`.
    pub path: String,
    /// SHA-256 of the normalised content.
    pub sha256: String,
}

/// The design-state hash and the inputs it covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignHash {
    pub sha256: String,
    pub files: Vec<HashedFile>,
}

/// Convert CRLF and lone CR to LF, so a checkout on Windows hashes like one
/// on Linux.
#[must_use]
pub fn normalize_newlines(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            out.push(b'\n');
            if bytes.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    out
}

/// Hash a set of `(relative path, content)` entries canonically.
///
/// Paths are normalised to `/` separators and sorted; content is newline
/// normalised; each entry is framed as `path\0content\0`. A later duplicate
/// path replaces an earlier one.
#[must_use]
pub fn hash_entries<I>(entries: I) -> DesignHash
where
    I: IntoIterator<Item = (String, Vec<u8>)>,
{
    use sha2::Digest as _;
    let sorted: BTreeMap<String, Vec<u8>> = entries
        .into_iter()
        .map(|(path, content)| (path.replace('\\', "/"), normalize_newlines(&content)))
        .collect();
    let mut total = sha2::Sha256::new();
    let mut files = Vec::with_capacity(sorted.len());
    for (path, content) in &sorted {
        total.update(path.as_bytes());
        total.update([0]);
        total.update(content);
        total.update([0]);
        files.push(HashedFile {
            path: path.clone(),
            sha256: hex(&sha2::Sha256::digest(content)),
        });
    }
    DesignHash {
        sha256: hex(&total.finalize()),
        files,
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Whether a path names `.synth/` tool state, which is never design input.
fn is_tool_state(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, Component::Normal(name) if name == ".synth"))
}

/// An import loader that remembers every file it served.
struct RecordingLoader<'a> {
    inner: &'a dyn synth_ir::ImportLoader,
    seen: std::cell::RefCell<BTreeMap<String, String>>,
}

impl synth_ir::ImportLoader for RecordingLoader<'_> {
    fn load(&self, relative_path: &str) -> Result<String, synth_ir::ImportLoadError> {
        let text = self.inner.load(relative_path)?;
        self.seen
            .borrow_mut()
            .insert(relative_path.to_string(), text.clone());
        Ok(text)
    }
}

/// The key a file on disk is recorded under: relative to the design's
/// directory when it lies inside it, else its file name.
fn relative_key(design_dir: &Path, path: &Path) -> String {
    path.strip_prefix(design_dir)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new(path.file_name().unwrap_or(path.as_os_str())))
        .to_string_lossy()
        .replace('\\', "/")
}

/// Compute the design-state hash of a design.
///
/// `file_name` is the design's path (it need not exist when `source` was
/// passed inline), `board` the lowered design when compilation got that far,
/// and `sidecars` the layout sidecars the caller resolved. A sidecar that
/// does not exist is simply absent from the hash, so creating one changes it.
#[must_use]
pub fn design_hash(
    file_name: &str,
    source: &str,
    board: Option<&synth_ir::Board>,
    sidecars: &[PathBuf],
) -> DesignHash {
    let design = Path::new(file_name);
    let design_dir = design
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    entries.push((
        relative_key(&design_dir, design),
        source.as_bytes().to_vec(),
    ));

    // Imported modules: resolve once more with a loader that records what it
    // served, so the hash covers exactly the files compilation read.
    let parse = synth_parser::parse(source, file_name.to_string());
    if let Some(ast) = parse.ast.as_ref() {
        let fs = synth_ir::FsImportLoader {
            root: design_dir.clone(),
        };
        let recorder = RecordingLoader {
            inner: &fs,
            seen: std::cell::RefCell::default(),
        };
        let _ = synth_ir::resolve_imports(ast, &recorder, file_name);
        for (path, text) in recorder.seen.into_inner() {
            if !is_tool_state(Path::new(&path)) {
                entries.push((path, text.into_bytes()));
            }
        }
    }

    let mut seen_sidecars = BTreeSet::new();
    for sidecar in sidecars {
        if is_tool_state(sidecar) || !seen_sidecars.insert(sidecar.clone()) {
            continue;
        }
        if let Ok(bytes) = std::fs::read(sidecar) {
            entries.push((relative_key(&design_dir, sidecar), bytes));
        }
    }

    if let Some(board) = board {
        let mut parts = BTreeSet::new();
        for component in &board.components {
            if let Some(part) = component.part.as_ref()
                && parts.insert(part.id.as_str().to_string())
            {
                entries.push((
                    format!("registry/{}.synth.toml", part.id.as_str()),
                    synth_registry::part_to_toml(part, &[]).into_bytes(),
                ));
            }
        }
        let manufacturer = board
            .manufacturer
            .clone()
            .unwrap_or_else(|| "jlc-standard".to_string());
        let fabrication = serde_json::json!({
            "manufacturer": manufacturer,
            "layers": board.layers,
            "profile": synth_drc::ManufacturerProfile::from_name(&manufacturer),
            "stackup": board.stackup,
        });
        entries.push((
            "fabrication.json".to_string(),
            serde_json::to_vec(&fabrication).unwrap_or_default(),
        ));
    }

    hash_entries(entries)
}

/// The design-state hash of a design on disk, computed exactly as every MCP
/// report computes it, and whether the design is empty (no components or
/// nets).
///
/// # Errors
///
/// The design cannot be read.
pub fn design_hash_of(
    design: &Path,
    registry: Option<&Path>,
) -> Result<(DesignHash, bool), String> {
    let mut args = serde_json::json!({ "file_path": design.display().to_string() });
    if let Some(dir) = registry {
        args["registry_path"] = serde_json::json!(dir.display().to_string());
    }
    let report = crate::tools::call_tool("synth_validate", &args, None)?;
    let hash = DesignHash {
        sha256: report["design_hash"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        files: serde_json::from_value(report["design_hash_files"].clone()).unwrap_or_default(),
    };
    let empty = report["gates"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|g| g["name"] == "design" && g["status"] == "empty");
    Ok((hash, empty))
}

/// How recorded evidence relates to the design as it is now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Staleness {
    pub stale: bool,
    pub recorded_hash: String,
    pub current_hash: String,
    /// Inputs whose content changed.
    pub changed: Vec<String>,
    /// Inputs that exist now but were not recorded.
    pub added: Vec<String>,
    /// Inputs that were recorded but no longer exist.
    pub removed: Vec<String>,
}

/// Compare recorded inputs against the current design hash.
#[must_use]
pub fn compare(recorded_hash: &str, recorded: &[HashedFile], current: &DesignHash) -> Staleness {
    let before: BTreeMap<&str, &str> = recorded
        .iter()
        .map(|f| (f.path.as_str(), f.sha256.as_str()))
        .collect();
    let now: BTreeMap<&str, &str> = current
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.sha256.as_str()))
        .collect();
    let changed = now
        .iter()
        .filter(|(p, h)| before.get(*p).is_some_and(|b| b != *h))
        .map(|(p, _)| (*p).to_string())
        .collect();
    let added = now
        .keys()
        .filter(|p| !before.contains_key(*p))
        .map(|p| (*p).to_string())
        .collect();
    let removed = before
        .keys()
        .filter(|p| !now.contains_key(*p))
        .map(|p| (*p).to_string())
        .collect();
    Staleness {
        stale: recorded_hash != current.sha256,
        recorded_hash: recorded_hash.to_string(),
        current_hash: current.sha256.clone(),
        changed,
        added,
        removed,
    }
}

/// What `synth export` records about the revision it evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub schema_version: String,
    pub design_hash: String,
    pub files: Vec<HashedFile>,
    pub tool_version: String,
    pub generated_at: String,
    pub verdict: Verdict,
    pub gates: Vec<Gate>,
}

impl Evidence {
    #[must_use]
    pub fn new(hash: &DesignHash, report: &GateReport) -> Self {
        Self {
            schema_version: EVIDENCE_SCHEMA.to_string(),
            design_hash: hash.sha256.clone(),
            files: hash.files.clone(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            verdict: report.verdict,
            gates: report.gates.clone(),
        }
    }

    /// Read an evidence document.
    ///
    /// # Errors
    ///
    /// The file cannot be read, is not JSON of this shape, or carries a
    /// different schema version.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read evidence '{}': {e}", path.display()))?;
        let evidence: Self = serde_json::from_str(&text)
            .map_err(|e| format!("evidence '{}' is not valid: {e}", path.display()))?;
        if evidence.schema_version != EVIDENCE_SCHEMA {
            return Err(format!(
                "evidence '{}' has schema '{}', expected '{EVIDENCE_SCHEMA}'",
                path.display(),
                evidence.schema_version
            ));
        }
        Ok(evidence)
    }

    /// Compare this evidence against the design as it is now.
    #[must_use]
    pub fn staleness(&self, current: &DesignHash) -> Staleness {
        compare(&self.design_hash, &self.files, current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESIGN: &str = "board \"demo\" {\n  layers 2\n}\n";

    #[test]
    fn crlf_and_lf_hash_identically() {
        let lf = hash_entries([("a.synth".into(), b"one\ntwo\n".to_vec())]);
        let crlf = hash_entries([("a.synth".into(), b"one\r\ntwo\r\n".to_vec())]);
        let cr = hash_entries([("a.synth".into(), b"one\rtwo\r".to_vec())]);
        assert_eq!(lf, crlf);
        assert_eq!(lf, cr);
    }

    /// A fixed vector: the same bytes must hash the same on every platform.
    #[test]
    fn canonical_test_vector() {
        use sha2::Digest as _;
        let hash = hash_entries([
            ("b.toml".into(), b"x = 1\r\n".to_vec()),
            ("a.synth".into(), b"board a {}\r\n".to_vec()),
        ]);
        assert_eq!(
            hash.files
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>(),
            ["a.synth", "b.toml"]
        );
        // sha256("a.synth\0board a {}\n\0b.toml\0x = 1\n\0")
        let expected = hex(&sha2::Sha256::digest(
            b"a.synth\0board a {}\n\0b.toml\0x = 1\n\0",
        ));
        assert_eq!(hash.sha256, expected);
    }

    #[test]
    fn windows_separators_are_normalised() {
        let a = hash_entries([("sub\\m.synth".into(), b"m".to_vec())]);
        let b = hash_entries([("sub/m.synth".into(), b"m".to_vec())]);
        assert_eq!(a, b);
    }

    #[test]
    fn editing_a_sidecar_changes_the_hash_and_evidence_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("demo.synth");
        std::fs::write(&design, DESIGN).unwrap();
        let sidecar = dir.path().join("demo.placement.layout.toml");
        std::fs::write(&sidecar, "schema_version = 1\n").unwrap();
        let name = design.display().to_string();
        let sidecars = vec![sidecar.clone()];

        let first = design_hash(&name, DESIGN, None, &sidecars);
        assert!(
            first
                .files
                .iter()
                .any(|f| f.path == "demo.placement.layout.toml")
        );

        // Writing evidence (or any output) beside the design is not an input.
        std::fs::write(dir.path().join(EVIDENCE_FILE), "{}").unwrap();
        std::fs::create_dir_all(dir.path().join(".synth")).unwrap();
        std::fs::write(dir.path().join(".synth/state.json"), "{}").unwrap();
        assert_eq!(design_hash(&name, DESIGN, None, &sidecars), first);

        std::fs::write(&sidecar, "schema_version = 1\n# moved\n").unwrap();
        let second = design_hash(&name, DESIGN, None, &sidecars);
        assert_ne!(second.sha256, first.sha256);
        let staleness = compare(&first.sha256, &first.files, &second);
        assert!(staleness.stale);
        assert_eq!(staleness.changed, vec!["demo.placement.layout.toml"]);
    }

    #[test]
    fn imported_modules_are_covered() {
        let dir = tempfile::tempdir().unwrap();
        let module = dir.path().join("power.synth");
        std::fs::write(&module, "board \"power\" { layers 2 }\n").unwrap();
        let source = "import \"power.synth\"\nboard \"demo\" {\n  layers 2\n}\n";
        let name = dir.path().join("demo.synth").display().to_string();
        let first = design_hash(&name, source, None, &[]);
        assert!(
            first.files.iter().any(|f| f.path == "power.synth"),
            "imports are part of the design: {:?}",
            first.files
        );
        std::fs::write(&module, "board \"power\" { layers 4 }\n").unwrap();
        let second = design_hash(&name, source, None, &[]);
        assert_eq!(
            compare(&first.sha256, &first.files, &second).changed,
            vec!["power.synth"]
        );
    }

    #[test]
    fn staleness_lists_added_and_removed_inputs() {
        let old = hash_entries([
            ("a.synth".into(), b"a".to_vec()),
            ("gone.toml".into(), b"g".to_vec()),
        ]);
        let new = hash_entries([
            ("a.synth".into(), b"a".to_vec()),
            ("new.toml".into(), b"n".to_vec()),
        ]);
        let s = compare(&old.sha256, &old.files, &new);
        assert!(s.stale);
        assert!(s.changed.is_empty());
        assert_eq!(s.added, vec!["new.toml"]);
        assert_eq!(s.removed, vec!["gone.toml"]);
    }

    #[test]
    fn evidence_round_trips_and_detects_staleness() {
        let dir = tempfile::tempdir().unwrap();
        let hash = hash_entries([("a.synth".into(), b"a".to_vec())]);
        let report = GateReport::new(vec![Gate::pass("erc")]);
        let evidence = Evidence::new(&hash, &report);
        let path = dir.path().join(EVIDENCE_FILE);
        std::fs::write(&path, serde_json::to_string(&evidence).unwrap()).unwrap();
        let read = Evidence::read(&path).unwrap();
        assert_eq!(read, evidence);
        assert!(!read.staleness(&hash).stale);
        let edited = hash_entries([("a.synth".into(), b"b".to_vec())]);
        assert_eq!(read.staleness(&edited).changed, vec!["a.synth"]);
    }
}
