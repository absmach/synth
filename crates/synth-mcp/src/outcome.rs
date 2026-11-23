// SPDX-License-Identifier: Apache-2.0

//! The outcome envelope every mutating tool returns.
//!
//! Transport success (`isError: false`) says nothing about whether every
//! requested change landed. An agent that retries a partly applied patch can
//! apply a change twice, and one that sees an error after a sidecar write
//! cannot tell whether the file changed. [`Outcome`] answers both:
//!
//! - `complete` — every item applied, every write verified;
//! - `partial` — some items applied, the rest are in `failed`/`unattempted`;
//! - `refused` — nothing was written; the reason (and the offending field)
//!   says why;
//! - `uncertain` — a write may have happened but readback could not confirm
//!   it. Re-read before retrying.
//!
//! The contract behind it: validate before writing, write atomically
//! ([`atomic_write`]: temp file, fsync, rename), and verify by reading the
//! file back ([`read_back`]) rather than echoing the request.

use std::io::Write as _;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

/// What happened to a mutating request as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Complete,
    Partial,
    Refused,
    Uncertain,
}

/// An item that was tried and did not apply.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FailedItem {
    pub item: Value,
    pub reason: String,
}

/// A file the tool wrote, hashed after the write by reading it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WrittenPath {
    pub path: String,
    pub sha256: String,
}

/// The shared outcome envelope.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outcome {
    pub kind: OutcomeKind,
    pub applied: Vec<Value>,
    pub failed: Vec<FailedItem>,
    pub unattempted: Vec<Value>,
    pub written_paths: Vec<WrittenPath>,
    /// Whether readback confirmed the change.
    pub verified: bool,
    /// The input field that caused a refusal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Set when a write was attempted whose result could not be confirmed.
    #[serde(skip)]
    unconfirmed_write: bool,
}

impl Default for Outcome {
    fn default() -> Self {
        Self {
            kind: OutcomeKind::Complete,
            applied: Vec::new(),
            failed: Vec::new(),
            unattempted: Vec::new(),
            written_paths: Vec::new(),
            verified: true,
            refused_field: None,
            reason: None,
            unconfirmed_write: false,
        }
    }
}

impl Outcome {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A refusal before any write, naming the offending field.
    #[must_use]
    pub fn refused(field: Option<&str>, reason: impl Into<String>) -> Self {
        Self {
            kind: OutcomeKind::Refused,
            refused_field: field.map(str::to_string),
            reason: Some(reason.into()),
            ..Self::default()
        }
    }

    pub fn apply(&mut self, item: Value) {
        self.applied.push(item);
    }

    pub fn fail(&mut self, item: Value, reason: impl Into<String>) {
        self.failed.push(FailedItem {
            item,
            reason: reason.into(),
        });
    }

    pub fn skip(&mut self, item: Value) {
        self.unattempted.push(item);
    }

    pub fn wrote(&mut self, written: WrittenPath) {
        self.written_paths.push(written);
    }

    /// Record a write whose result readback could not confirm.
    pub fn unconfirmed(&mut self, path: &Path, reason: impl Into<String>) {
        self.unconfirmed_write = true;
        self.verified = false;
        self.fail(
            serde_json::json!({ "path": path.display().to_string() }),
            reason,
        );
    }

    /// Mark the change as not confirmed by readback.
    pub fn unverified(&mut self) {
        self.verified = false;
    }

    /// Fix `kind` from what was recorded.
    #[must_use]
    pub fn finish(mut self) -> Self {
        if self.kind == OutcomeKind::Refused && self.refused_field.is_some() {
            return self;
        }
        let wrote = !self.written_paths.is_empty();
        let missing = !self.failed.is_empty() || !self.unattempted.is_empty();
        self.kind = if self.unconfirmed_write || (wrote && !self.verified) {
            OutcomeKind::Uncertain
        } else if self.applied.is_empty() && missing && !wrote {
            OutcomeKind::Refused
        } else if missing {
            OutcomeKind::Partial
        } else {
            OutcomeKind::Complete
        };
        self
    }

    /// Serialise.
    #[must_use]
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// A refusal as a tool error: the error text, the field, and the envelope.
///
/// Returned through `Err` so transport-level `isError` stays set, while the
/// message is a JSON document an agent can parse for `outcome`.
#[must_use]
pub fn refusal(field: Option<&str>, reason: impl Into<String>) -> String {
    let reason = reason.into();
    serde_json::json!({
        "status": "refused",
        "error": reason,
        "field": field,
        "outcome": Outcome::refused(field, reason.clone()).to_value(),
    })
    .to_string()
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    let mut out = String::with_capacity(64);
    for byte in sha2::Sha256::digest(bytes) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Write `bytes` to `path` atomically: a temp file in the same directory,
/// flushed to disk, then renamed over the target. An interrupted write leaves
/// the target exactly as it was.
///
/// # Errors
///
/// Any I/O failure; the target is untouched when one occurs.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    atomic_write_with(path, bytes, || Ok(()))
}

/// [`atomic_write`] with a hook run between the flushed temp file and the
/// rename, so a test can interrupt the write at its most dangerous point.
fn atomic_write_with(
    path: &Path,
    bytes: &[u8],
    before_rename: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".synth-write-")
        .tempfile_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    before_rename()?;
    tmp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Read a written file back and confirm it holds `expected`.
///
/// # Errors
///
/// The file cannot be read, or its content differs from what was written.
pub fn read_back(path: &Path, expected: &[u8]) -> Result<WrittenPath, String> {
    let actual =
        std::fs::read(path).map_err(|e| format!("readback of '{}' failed: {e}", path.display()))?;
    if actual != expected {
        return Err(format!(
            "readback of '{}' does not match what was written",
            path.display()
        ));
    }
    Ok(WrittenPath {
        path: path.display().to_string(),
        sha256: sha256_hex(&actual),
    })
}

/// Hash a file that some other component wrote (an export artefact).
///
/// # Errors
///
/// The file cannot be read back.
pub fn hash_written(path: &Path) -> Result<WrittenPath, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("readback of '{}' failed: {e}", path.display()))?;
    Ok(WrittenPath {
        path: path.display().to_string(),
        sha256: sha256_hex(&bytes),
    })
}

/// Write atomically, read back, and record the result in `outcome`.
///
/// Returns whether the write landed and verified. A failure before the
/// rename leaves the target untouched and is recorded as a failed item; a
/// readback mismatch after it is recorded as unconfirmed.
pub fn write_verified(outcome: &mut Outcome, path: &Path, bytes: &[u8]) -> bool {
    if let Err(e) = atomic_write(path, bytes) {
        outcome.fail(
            serde_json::json!({ "path": path.display().to_string() }),
            format!("write failed, target unchanged: {e}"),
        );
        return false;
    }
    match read_back(path, bytes) {
        Ok(written) => {
            outcome.wrote(written);
            true
        }
        Err(e) => {
            outcome.unconfirmed(path, e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_write_leaves_the_target_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("demo.placement.layout.toml");
        std::fs::write(&target, b"old").unwrap();
        let err = atomic_write_with(&target, b"new", || Err(std::io::Error::other("power cut")))
            .unwrap_err();
        assert_eq!(err.to_string(), "power cut");
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        // The temp file is cleaned up, so nothing half-written is left behind.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "{leftovers:?}");
    }

    #[test]
    fn atomic_write_replaces_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("sub/part.synth.toml");
        let mut outcome = Outcome::new();
        outcome.apply(serde_json::json!("part"));
        assert!(write_verified(&mut outcome, &target, b"id = \"x\"\n"));
        let outcome = outcome.finish();
        assert_eq!(outcome.kind, OutcomeKind::Complete);
        assert!(outcome.verified);
        assert_eq!(outcome.written_paths[0].sha256, sha256_hex(b"id = \"x\"\n"));
    }

    #[test]
    fn kinds() {
        let mut partial = Outcome::new();
        partial.apply(serde_json::json!(0));
        partial.fail(serde_json::json!(1), "range");
        assert_eq!(partial.finish().kind, OutcomeKind::Partial);

        let mut none = Outcome::new();
        none.fail(serde_json::json!(0), "range");
        assert_eq!(none.finish().kind, OutcomeKind::Refused);

        let mut uncertain = Outcome::new();
        uncertain.apply(serde_json::json!(0));
        uncertain.unconfirmed(Path::new("x"), "mismatch");
        assert_eq!(uncertain.finish().kind, OutcomeKind::Uncertain);

        assert_eq!(Outcome::new().finish().kind, OutcomeKind::Complete);
        let refused = Outcome::refused(Some("patches[1]"), "bad").finish();
        assert_eq!(refused.kind, OutcomeKind::Refused);
        assert_eq!(refused.refused_field.as_deref(), Some("patches[1]"));
    }

    #[test]
    fn refusal_is_parseable() {
        let text = refusal(Some("refdes"), "Missing required 'refdes'");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["outcome"]["kind"], "refused");
        assert_eq!(v["field"], "refdes");
        assert!(text.contains("Missing required 'refdes'"));
    }
}
