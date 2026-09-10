// SPDX-License-Identifier: Apache-2.0

//! Import resolution — plan §4.3 step 1.
//!
//! Walks the `import "<path>"` statements at the top of a
//! [`ProgramAst`], loads each referenced file, parses it, recurses
//! into ITS imports, and merges the resulting board's statements
//! into the importing board. The imported file's `board` *name* is
//! discarded; only its statement list contributes.
//!
//! ## Safety
//!
//! Import handling is the most filesystem-exposed surface in the
//! compiler, so per plan §12.2 we enforce:
//!
//! - **Sandboxed paths.** No absolute paths. No `..` segments.
//!   Every import must resolve under a caller-provided root.
//! - **File-size cap.** [`MAX_IMPORT_SIZE`] bytes (1 MB). Larger
//!   files emit `E-SYNTH-IMPORT-003` and are not loaded.
//! - **Bounded recursion.** [`MAX_IMPORT_DEPTH`] levels. Beyond
//!   that, `E-SYNTH-IMPORT-004`.
//! - **Cycle detection.** A file in the active load stack errors
//!   with `E-SYNTH-IMPORT-005`; an already-finished load is a no-op
//!   (the same file imported twice contributes only once).
//!
//! ## Pluggable loader
//!
//! [`ImportLoader`] abstracts filesystem access so the same code
//! path works on `wasm32-unknown-unknown` (where `std::fs` isn't
//! available) using an in-memory loader.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use synth_ast::{BoardAst, ImportAst, ProgramAst};
use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity, Span};

/// Maximum file size accepted by the import resolver. Plan §12.2.
pub const MAX_IMPORT_SIZE: usize = 1_000_000;

/// Maximum import-chain depth before we abort.
pub const MAX_IMPORT_DEPTH: usize = 16;

/// Filesystem (or in-memory) source for import resolution. Returns
/// the file contents on success, or an error to be converted into
/// a structured diagnostic.
pub trait ImportLoader {
    /// Load the content at `relative_path`. The path is guaranteed
    /// to have already passed sandbox validation by the resolver.
    fn load(&self, relative_path: &str) -> Result<String, ImportLoadError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportLoadError {
    NotFound,
    TooLarge { actual: usize },
    Io(String),
}

/// Filesystem-backed loader rooted at a directory. Reads files
/// relative to the root. Used by the CLI; not available on
/// `wasm32-unknown-unknown`.
#[derive(Debug)]
pub struct FsImportLoader {
    pub root: PathBuf,
}

impl ImportLoader for FsImportLoader {
    fn load(&self, relative_path: &str) -> Result<String, ImportLoadError> {
        let full = self.root.join(relative_path);
        let metadata = std::fs::metadata(&full).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ImportLoadError::NotFound
            } else {
                ImportLoadError::Io(e.to_string())
            }
        })?;
        let len = metadata.len() as usize;
        if len > MAX_IMPORT_SIZE {
            return Err(ImportLoadError::TooLarge { actual: len });
        }
        std::fs::read_to_string(&full).map_err(|e| ImportLoadError::Io(e.to_string()))
    }
}

/// In-memory loader for tests and the WASM build. Map of
/// `relative_path` → file contents.
#[derive(Debug, Default)]
pub struct MemoryImportLoader {
    files: HashMap<String, String>,
}

impl MemoryImportLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl Into<String>, content: impl Into<String>) {
        self.files.insert(path.into(), content.into());
    }
}

impl ImportLoader for MemoryImportLoader {
    fn load(&self, relative_path: &str) -> Result<String, ImportLoadError> {
        let content = self
            .files
            .get(relative_path)
            .ok_or(ImportLoadError::NotFound)?;
        if content.len() > MAX_IMPORT_SIZE {
            return Err(ImportLoadError::TooLarge {
                actual: content.len(),
            });
        }
        Ok(content.clone())
    }
}

#[derive(Debug)]
pub struct ResolveResult {
    /// The resulting [`ProgramAst`] with all imports inlined and
    /// the top-level `imports` field cleared.
    pub program: ProgramAst,
    /// Every diagnostic emitted during resolution, anchored to the
    /// `import "..."` source span that triggered it.
    pub diagnostics: Vec<Diagnostic>,
}

/// Recursively resolve every import in `program` using `loader`,
/// producing a single flattened [`ProgramAst`] whose `imports`
/// list is empty and whose `board.statements` has been augmented
/// with the imported files' statements (prepended in import order).
///
/// Idempotent under diamonds: if file `lib` is reachable from
/// multiple imports (e.g. `a` and `b` both import `lib`), its
/// statements are emitted exactly once. The first path to reach
/// it wins; later paths find it in the `emitted` set and skip
/// silently.
///
/// `file` is the diagnostic location anchor for the *root* file
/// (typically the path the user passed to `synth validate`).
pub fn resolve(program: &ProgramAst, loader: &dyn ImportLoader, file: &str) -> ResolveResult {
    let mut diagnostics = Vec::new();
    let mut active = HashSet::new();
    let mut emitted: HashSet<String> = HashSet::new();
    let resolved_program = resolve_inner(
        program,
        loader,
        file,
        0,
        &mut active,
        &mut emitted,
        &mut diagnostics,
    );
    ResolveResult {
        program: resolved_program,
        diagnostics,
    }
}

fn resolve_inner(
    program: &ProgramAst,
    loader: &dyn ImportLoader,
    file: &str,
    depth: usize,
    active: &mut HashSet<String>,
    emitted: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> ProgramAst {
    let mut imported_statements: Vec<synth_ast::StatementAst> = Vec::new();

    for import in &program.imports {
        if let Some(stmts) =
            load_one_import(import, loader, file, depth, active, emitted, diagnostics)
        {
            imported_statements.extend(stmts);
        }
    }

    // Imported statements come BEFORE the importing board's own
    // statements — establishes "library context" before "design body".
    let mut merged = imported_statements;
    merged.extend(program.board.statements.iter().cloned());

    ProgramAst {
        imports: Vec::new(),
        board: BoardAst {
            name: program.board.name.clone(),
            statements: merged,
            span: program.board.span,
        },
        span: program.span,
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn load_one_import(
    import: &ImportAst,
    loader: &dyn ImportLoader,
    file: &str,
    depth: usize,
    active: &mut HashSet<String>,
    emitted: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<synth_ast::StatementAst>> {
    let path = import.path.clone();

    // Sandbox: reject absolute paths and any `..` segments.
    if let Some(err) = sandbox_violation(&path) {
        diagnostics.push(
            diag(
                "E-SYNTH-IMPORT-002",
                "import path escapes sandbox",
                import.span,
                file,
                format!("`{path}` rejected: {err}"),
            )
            .build(),
        );
        return None;
    }

    if depth >= MAX_IMPORT_DEPTH {
        diagnostics.push(
            diag(
                "E-SYNTH-IMPORT-004",
                "import recursion limit exceeded",
                import.span,
                file,
                format!("depth {depth} reached at import `{path}`"),
            )
            .build(),
        );
        return None;
    }

    if active.contains(&path) {
        diagnostics.push(
            diag(
                "E-SYNTH-IMPORT-005",
                "import cycle detected",
                import.span,
                file,
                format!("`{path}` is already being loaded further up the chain"),
            )
            .build(),
        );
        return None;
    }

    // Idempotency: if this file has already been emitted via
    // another import path, skip it silently. Diamond imports
    // contribute their statements exactly once.
    if emitted.contains(&path) {
        return Some(Vec::new());
    }

    let source = match loader.load(&path) {
        Ok(s) => s,
        Err(ImportLoadError::NotFound) => {
            diagnostics.push(
                diag(
                    "E-SYNTH-IMPORT-001",
                    "imported file not found",
                    import.span,
                    file,
                    format!("`{path}` could not be loaded"),
                )
                .build(),
            );
            return None;
        }
        Err(ImportLoadError::TooLarge { actual }) => {
            diagnostics.push(
                diag(
                    "E-SYNTH-IMPORT-003",
                    "imported file exceeds the size cap",
                    import.span,
                    file,
                    format!("`{path}` is {actual} bytes, cap is {MAX_IMPORT_SIZE} bytes"),
                )
                .build(),
            );
            return None;
        }
        Err(ImportLoadError::Io(msg)) => {
            diagnostics.push(
                diag(
                    "E-SYNTH-IMPORT-001",
                    "imported file could not be read",
                    import.span,
                    file,
                    format!("`{path}`: {msg}"),
                )
                .build(),
            );
            return None;
        }
    };

    // Parse the imported file. Parser errors get prefixed with the
    // importing context so the agent can tell where they came from.
    let inner_parse = synth_parser::parse(&source, path.clone());
    for d in &inner_parse.diagnostics {
        diagnostics.push(d.clone());
    }
    let Some(inner_ast) = inner_parse.ast else {
        // Parse failure — we already pushed the parser's diagnostics
        // through. Don't synthesize an additional one.
        return None;
    };

    active.insert(path.clone());
    let resolved_inner = resolve_inner(
        &inner_ast,
        loader,
        &path,
        depth + 1,
        active,
        emitted,
        diagnostics,
    );
    active.remove(&path);
    emitted.insert(path);

    Some(resolved_inner.board.statements)
}

fn sandbox_violation(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        return Some("empty path");
    }
    let p = Path::new(path);
    if p.is_absolute() {
        return Some("absolute paths not allowed");
    }
    for component in p.components() {
        use std::path::Component;
        match component {
            Component::ParentDir => return Some("`..` segments not allowed"),
            Component::Prefix(_) | Component::RootDir => return Some("absolute paths not allowed"),
            _ => {}
        }
    }
    None
}

fn diag(code: &str, title: &str, span: Span, file: &str, found: String) -> DiagnosticBuilder {
    DiagnosticBuilder::new(code, Severity::Error, title)
        .location(Location::from_span(file.to_string(), span))
        .found(found)
        .explanation_url(format!("synth.docs/diagnostics/{code}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> ProgramAst {
        synth_parser::parse(src, "main.synth").ast.expect("parse")
    }

    #[test]
    fn empty_imports_passes_through() {
        let p = parse(r#"board "x" { layers 2 }"#);
        let r = resolve(&p, &MemoryImportLoader::new(), "main.synth");
        assert!(r.diagnostics.is_empty());
        assert_eq!(r.program.board.statements.len(), 1);
    }

    #[test]
    fn imports_get_inlined() {
        let p = parse(
            r#"
            import "lib.synth"
            board "main" { layers 4 }
        "#,
        );
        let mut loader = MemoryImportLoader::new();
        loader.insert("lib.synth", r#"board "lib" { manufacturer "jlcpcb" }"#);
        let r = resolve(&p, &loader, "main.synth");
        assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
        // imported manufacturer + own layers = 2 statements
        assert_eq!(r.program.board.statements.len(), 2);
        assert!(r.program.imports.is_empty());
        // Imported statements first, then own statements.
        assert!(matches!(
            r.program.board.statements[0],
            synth_ast::StatementAst::Manufacturer(_)
        ));
        assert!(matches!(
            r.program.board.statements[1],
            synth_ast::StatementAst::Layers(_)
        ));
    }

    #[test]
    fn missing_import_emits_import_001() {
        let p = parse(
            r#"
            import "missing.synth"
            board "x" {}
        "#,
        );
        let r = resolve(&p, &MemoryImportLoader::new(), "main.synth");
        assert!(r.diagnostics.iter().any(|d| d.code == "E-SYNTH-IMPORT-001"));
    }

    #[test]
    fn parent_dir_segment_emits_import_002() {
        let p = parse(
            r#"
            import "../etc/passwd"
            board "x" {}
        "#,
        );
        let r = resolve(&p, &MemoryImportLoader::new(), "main.synth");
        assert!(r.diagnostics.iter().any(|d| d.code == "E-SYNTH-IMPORT-002"));
    }

    #[test]
    fn absolute_path_emits_import_002() {
        let p = parse(
            r#"
            import "/etc/passwd"
            board "x" {}
        "#,
        );
        let r = resolve(&p, &MemoryImportLoader::new(), "main.synth");
        assert!(r.diagnostics.iter().any(|d| d.code == "E-SYNTH-IMPORT-002"));
    }

    #[test]
    fn oversized_file_emits_import_003() {
        let p = parse(
            r#"
            import "big.synth"
            board "x" {}
        "#,
        );
        let mut loader = MemoryImportLoader::new();
        loader.insert("big.synth", "x".repeat(MAX_IMPORT_SIZE + 1));
        let r = resolve(&p, &loader, "main.synth");
        assert!(r.diagnostics.iter().any(|d| d.code == "E-SYNTH-IMPORT-003"));
    }

    #[test]
    fn cycle_detected_emits_import_005() {
        let p = parse(
            r#"
            import "a.synth"
            board "main" {}
        "#,
        );
        let mut loader = MemoryImportLoader::new();
        loader.insert(
            "a.synth",
            r#"
            import "b.synth"
            board "a" {}
        "#,
        );
        loader.insert(
            "b.synth",
            r#"
            import "a.synth"
            board "b" {}
        "#,
        );
        let r = resolve(&p, &loader, "main.synth");
        assert!(r.diagnostics.iter().any(|d| d.code == "E-SYNTH-IMPORT-005"));
    }

    #[test]
    fn diamond_import_loads_once() {
        // a imports lib, b imports lib, main imports a and b.
        // lib's statements should appear only once.
        let p = parse(
            r#"
            import "a.synth"
            import "b.synth"
            board "main" {}
        "#,
        );
        let mut loader = MemoryImportLoader::new();
        loader.insert(
            "a.synth",
            r#"
            import "lib.synth"
            board "a" {}
        "#,
        );
        loader.insert(
            "b.synth",
            r#"
            import "lib.synth"
            board "b" {}
        "#,
        );
        loader.insert("lib.synth", r#"board "lib" { layers 4 }"#);
        let r = resolve(&p, &loader, "main.synth");
        assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
        let layers_count = r
            .program
            .board
            .statements
            .iter()
            .filter(|s| matches!(s, synth_ast::StatementAst::Layers(_)))
            .count();
        // Diamond import: the lib's `layers 4` should appear once,
        // not twice. The cache absorbs the second resolution.
        assert_eq!(layers_count, 1);
    }
}
