// SPDX-License-Identifier: Apache-2.0

//! `synth preview <file>` — live, read-only browser viewer.
//!
//! Architecture (plan §7):
//!
//! - axum HTTP server on a configurable port (default 8080).
//! - `notify-debouncer-mini` watches the input file. On any change
//!   the source is re-read and the compiler pipeline runs (parse →
//!   lower; ERC follows once Phase 5 grows it).
//! - The latest [`BoardView`] is held in a shared [`tokio::sync::watch`]
//!   channel; SSE consumers subscribe to it and receive a new event
//!   per recompile.
//! - The Leptos+WASM bundle is served as static files from the
//!   `--assets-dir` directory (Trunk's `dist/` output).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use axum::extract::{Json, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::Router;
use futures::stream::Stream;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult};
use serde::{Deserialize, Serialize};
use synth_diagnostics::Diagnostic;
use synth_ir::Board;
use synth_registry::Registry;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

/// Wire payload sent to the browser. Mirrors `synth_web::BoardView`
/// — synth-cli does not depend on synth-web directly, so this is
/// intentionally an unlinked reference.
#[derive(Debug, Clone, Default, Serialize)]
struct BoardView {
    source_path: String,
    board: Option<Board>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Clone)]
struct AppState {
    rx: watch::Receiver<BoardView>,
    /// The `.synth` source file being watched. Sidecar overrides are
    /// written adjacent to it (`<source>.synth.layout.toml`, §7.7.6).
    input: PathBuf,
}

/// Strict wire schema for `POST /api/v1/layout/save`.
///
/// Mirrors the `<design>.synth.layout.toml` sidecar schema (§7.7.6) —
/// refdes-keyed per-component absolute positions — and rejects any
/// field outside it (`deny_unknown_fields`) so the endpoint only ever
/// accepts exactly the sidecar shape.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutSaveRequest {
    components: HashMap<String, LayoutSavePlacement>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutSavePlacement {
    x: f64,
    y: f64,
    #[serde(default)]
    rotation: u32,
}

/// `<source>.synth.layout.toml` next to the `.synth` file (§7.7.6).
fn sidecar_path_for(source_path: &Path) -> PathBuf {
    let mut name = source_path.as_os_str().to_os_string();
    name.push(".layout.toml");
    PathBuf::from(name)
}

/// Persist a refdes-keyed drag-offset layout to the sidecar file
/// adjacent to `source_path`. Returns the path written.
///
/// Pure (takes a path, not the whole server), so the §7.7.8 roundtrip
/// test can exercise it directly without spinning up axum.
pub fn save_layout_sidecar(
    source_path: &Path,
    layout: &synth_layout::sidecar::SidecarLayout,
) -> anyhow::Result<PathBuf> {
    let sidecar_path = sidecar_path_for(source_path);
    layout
        .save_to_file(&sidecar_path)
        .with_context(|| format!("writing sidecar {}", sidecar_path.display()))?;
    Ok(sidecar_path)
}

/// `POST /api/v1/layout/save` — persist browser drag offsets (refdes-keyed
/// absolute positions matching the sidecar schema) to
/// `<source>.synth.layout.toml` adjacent to the watched `.synth` file.
/// The `.synth` source itself is never touched.
async fn layout_save_handler(
    State(state): State<AppState>,
    Json(req): Json<LayoutSaveRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let layout = synth_layout::sidecar::SidecarLayout {
        components: req
            .components
            .into_iter()
            .map(|(refdes, p)| {
                (
                    refdes,
                    synth_layout::sidecar::SidecarPlacement {
                        x: p.x,
                        y: p.y,
                        rotation: p.rotation,
                        source: synth_layout::sidecar::OverrideSource::HumanDrag,
                        priority: synth_layout::sidecar::OverridePriority::Hard,
                        timestamp: None,
                    },
                )
            })
            .collect(),
        ..Default::default()
    };
    let path = save_layout_sidecar(&state.input, &layout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({
        "sidecar_path": path.display().to_string(),
        "components": layout.components.len(),
    })))
}

pub async fn run_preview(
    input: PathBuf,
    registry_dir: Option<PathBuf>,
    assets_dir: PathBuf,
    port: u16,
) -> anyhow::Result<()> {
    // Load the registry once. We do not yet hot-reload registry edits.
    let registry_path = registry_dir.unwrap_or_else(|| PathBuf::from("registry").join("parts"));
    let registry = synth_registry::load_dir(&registry_path)
        .with_context(|| format!("loading registry from {}", registry_path.display()))?;
    let registry = Arc::new(registry);

    let initial = compile_once(&input, &registry);
    let (tx, rx) = watch::channel(initial);

    // File watcher: re-compile on every debounced event.
    let watch_input = input.clone();
    let watch_registry = Arc::clone(&registry);
    let watch_tx = tx.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(150),
        move |res: DebounceEventResult| match res {
            Ok(_events) => {
                let next = compile_once(&watch_input, &watch_registry);
                let _ = watch_tx.send(next);
            }
            Err(errs) => {
                eprintln!("synth preview: watcher error(s): {errs:?}");
            }
        },
    )?;
    debouncer
        .watcher()
        .watch(&input, RecursiveMode::NonRecursive)
        .with_context(|| format!("watching {}", input.display()))?;

    // HTTP server. `no-cache` headers force the browser to revalidate
    // every request — critical during dev so a Trunk rebuild is
    // immediately visible on plain reload, no Ctrl+Shift+R needed.
    let state = AppState {
        rx,
        input: input.clone(),
    };
    let no_cache = SetResponseHeaderLayer::overriding(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    let router = Router::new()
        .route("/events", get(sse_handler))
        .route("/api/v1/layout/save", post(layout_save_handler))
        .fallback_service(ServeDir::new(assets_dir.clone()))
        .layer(no_cache)
        .with_state(state);

    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("binding 127.0.0.1:{port}"))?;
    let bound = listener.local_addr()?;

    println!("synth preview");
    println!("  source     {}", input.display());
    println!("  registry   {}", registry_path.display());
    println!("  assets     {}", assets_dir.display());
    println!("  serving at http://{bound}/");
    println!("  Ctrl-C to stop.");

    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        println!("\nsynth preview: shutting down");
    };

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
        .context("axum server error")?;

    // Keep the watcher alive until shutdown.
    drop(debouncer);
    Ok(())
}

fn compile_once(input: &Path, registry: &Registry) -> BoardView {
    let mut view = BoardView {
        source_path: input.display().to_string(),
        ..Default::default()
    };
    let Ok(source) = std::fs::read_to_string(input) else {
        return view;
    };
    let filename = input.to_string_lossy().into_owned();
    let parse = synth_parser::parse(&source, filename.clone());
    view.diagnostics.extend(parse.diagnostics);
    if let Some(ast) = parse.ast.as_ref() {
        let import_root = input
            .parent()
            .map_or_else(|| std::path::PathBuf::from("."), Path::to_path_buf);
        let loader = synth_ir::FsImportLoader { root: import_root };
        let resolved = synth_ir::resolve_imports(ast, &loader, &filename);
        view.diagnostics.extend(resolved.diagnostics);
        let lowered = synth_ir::lower(&resolved.program, registry, &filename);
        view.diagnostics.extend(lowered.diagnostics);
        view.board = lowered.board;
        if let Some(ref board) = view.board {
            view.diagnostics
                .extend(synth_validate::run_erc(board, &filename));
        }
    }
    view
}

async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    use futures::StreamExt as _;

    // Push the current state immediately so a freshly-connected
    // browser doesn't wait for the first file save.
    let rx = state.rx.clone();
    let stream = futures::stream::unfold((rx, true), |(mut rx, first)| async move {
        if !first && rx.changed().await.is_err() {
            return None;
        }
        let snapshot = rx.borrow().clone();
        let json = serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string());
        Some((Ok(Event::default().data(json)), (rx, false)))
    });

    Sse::new(stream.boxed()).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .unwrap()
    }

    /// Parse + lower a fixture `.synth` file into a [`Board`].
    fn board_for(rel: &str) -> Board {
        let path = workspace_root().join(rel);
        let source = std::fs::read_to_string(&path).unwrap();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let ast = synth_parser::parse(&source, filename.clone()).ast.unwrap();
        let registry =
            synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
        synth_ir::lower(&ast, &registry, &filename).board.unwrap()
    }

    /// §7.7.8 gate: "drag in web → reload → position matches bit-exact".
    /// Covers the whole save path without a live server: base layout →
    /// apply a drag offset → build the refdes-keyed sidecar → save to
    /// `<source>.synth.layout.toml` → reload via `layout_with_sidecar`
    /// (the same entry point `synth preview` uses on startup) → assert
    /// the repositioned centre and rotation round-trip exactly.
    #[test]
    fn sidecar_save_reload_roundtrip_is_bit_exact() {
        use synth_layout::sidecar::{SidecarLayout, SidecarPlacement};
        use synth_layout::Rotation;

        let board = board_for("fixtures/layout/led_indicator.synth");
        let base = synth_layout::layout(&board);

        // Pick the first placed component and "drag" it by a non-trivial
        // delta; record the rotation the placer chose for it.
        let first = &base.components[0];
        let id = first.id;
        let refdes = board
            .components
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .refdes
            .clone();
        let (bx, by) = first.center_mm;
        let (dx, dy) = (5.08, -7.62);
        let target = (bx + dx, by + dy);
        let rotation = match first.rotation {
            Rotation::Zero => 0,
            Rotation::Ninety => 90,
            Rotation::OneEighty => 180,
            Rotation::TwoSeventy => 270,
        };

        // Build the refdes-keyed sidecar (the exact payload shape the
        // browser POSTs) and save it next to a throwaway `.synth` file.
        let sidecar = SidecarLayout {
            components: HashMap::from([(
                refdes,
                SidecarPlacement {
                    x: target.0,
                    y: target.1,
                    rotation,
                    source: synth_layout::sidecar::OverrideSource::HumanDrag,
                    priority: synth_layout::sidecar::OverridePriority::Hard,
                    timestamp: None,
                },
            )]),
            ..Default::default()
        };
        let dir = std::env::temp_dir().join(format!("synth-sidecar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source_path = dir.join("led_indicator.synth");
        let src = workspace_root().join("fixtures/layout/led_indicator.synth");
        std::fs::write(&source_path, std::fs::read_to_string(&src).unwrap()).unwrap();

        let sidecar_path = save_layout_sidecar(&source_path, &sidecar).unwrap();
        assert_eq!(sidecar_path, sidecar_path_for(&source_path));
        assert!(
            sidecar_path.exists(),
            "sidecar file must be written to disk"
        );

        // Reload + apply via the same path `synth preview` uses on startup.
        let reloaded = synth_layout::layout_with_sidecar(&board, Some(&sidecar_path));
        let placement = reloaded.placement(id).unwrap();
        assert_eq!(
            placement.center_mm, target,
            "reloaded position must match the dragged position bit-exact"
        );
        assert_eq!(
            placement.rotation, first.rotation,
            "reloaded rotation must round-trip exactly"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
