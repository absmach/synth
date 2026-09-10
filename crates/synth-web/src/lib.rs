// SPDX-License-Identifier: Apache-2.0

//! Browser viewer for the Synth EDA compiler.
//!
//! The browser is **read-only**: the source-of-truth is the user's
//! `.synth` file edited in their own IDE. The Leptos app subscribes
//! to a Server-Sent Events stream from the `synth preview` CLI and
//! re-renders the schematic + diagnostics on every server-side
//! recompile.
//!
//! What this crate is *not*:
//!
//! - An editor.  No textarea, no Monaco, no autocomplete pane.
//! - A compiler. The compiler runs server-side in `synth preview`;
//!   the WASM bundle deserializes JSON only.

#![allow(clippy::wildcard_imports)] // standard Leptos prelude pattern

mod diagnostics;
mod inspector;
mod schematic;
mod state;

use leptos::prelude::*;
use wasm_bindgen::prelude::*;

pub use state::{BoardView, ConnectionState, SelectedEntity};

/// WASM entry point. Trunk calls this on page load.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SidebarTab {
    #[default]
    Diagnostics,
    Inspector,
}

#[component]
fn App() -> impl IntoView {
    let state = state::subscribe_to_events();
    let selected = RwSignal::new(SelectedEntity::None);
    let active_tab = RwSignal::new(SidebarTab::Diagnostics);

    // Auto-switch to Inspector tab when an entity is selected.
    Effect::new(move |_| {
        let sel = selected.get();
        if !matches!(sel, SelectedEntity::None) {
            active_tab.set(SidebarTab::Inspector);
        }
    });

    let view = schematic::ViewTransform::new();
    let diagnostics_open = RwSignal::new(true);
    let diag_count = move || state.get().diagnostics.len();

    view! {
        <header class="topbar">
            <div class="brand">"synth · preview"</div>
            <TitleBlock state=state />
            <div class="topbar-tools">
                <button class="zoom-btn" title="Zoom out"
                    on:click=move |_| view.zoom_button(1.0 / 1.25)>"−"</button>
                <span class="zoom-label">
                    {move || format!("{:.0}%", view.zoom.get() * 100.0)}
                </span>
                <button class="zoom-btn" title="Zoom in"
                    on:click=move |_| view.zoom_button(1.25)>"+"</button>
                <button class="reset-btn" title="Reset view"
                    on:click=move |_| view.reset()>"reset"</button>
            </div>
            <ConnectionBadge state=state />
        </header>
        <main
            class="grid"
            class=("diagnostics-collapsed", move || !diagnostics_open.get())
        >
            <section class="pane schematic">
                <h2>"Schematic"</h2>
                <schematic::Schematic state=state view=view selected=selected />
            </section>
            <section class=move || if diagnostics_open.get() {
                "pane diagnostics".to_string()
            } else {
                "pane diagnostics collapsed".to_string()
            }>
                <div class="pane-header">
                    <button
                        class="pane-toggle"
                        title=move || if diagnostics_open.get() {
                            "Collapse panel".to_string()
                        } else {
                            "Expand panel".to_string()
                        }
                        on:click=move |_| diagnostics_open.update(|v| *v = !*v)
                    >
                        {move || if diagnostics_open.get() { "›" } else { "‹" }}
                    </button>

                    <div class="sidebar-tabs">
                        <button
                            class=move || if matches!(active_tab.get(), SidebarTab::Diagnostics) {
                                "tab-btn active"
                            } else {
                                "tab-btn"
                            }
                            on:click=move |_| active_tab.set(SidebarTab::Diagnostics)
                        >
                            {move || format!("Diagnostics ({})", diag_count())}
                        </button>
                        <button
                            class=move || if matches!(active_tab.get(), SidebarTab::Inspector) {
                                "tab-btn active"
                            } else {
                                "tab-btn"
                            }
                            on:click=move |_| active_tab.set(SidebarTab::Inspector)
                        >
                            "Inspector"
                        </button>
                    </div>
                </div>

                {move || diagnostics_open.get().then(|| match active_tab.get() {
                    SidebarTab::Diagnostics => view! {
                        <diagnostics::DiagnosticsList state=state selected=selected />
                    }.into_any(),
                    SidebarTab::Inspector => view! {
                        <inspector::InspectorPanel state=state selected=selected />
                    }.into_any(),
                })}
            </section>
        </main>
    }
}

#[component]
fn TitleBlock(state: ReadSignal<BoardView>) -> impl IntoView {
    let board_name = move || {
        state
            .get()
            .board
            .as_ref()
            .map_or_else(|| "(no board)".to_string(), |b| b.name.clone())
    };
    let source_path = move || state.get().source_path.clone();
    view! {
        <div class="title-block">
            <div class="board-name">{board_name}</div>
            <div class="source-path">{source_path}</div>
        </div>
    }
}

#[component]
fn ConnectionBadge(state: ReadSignal<BoardView>) -> impl IntoView {
    let connection = move || state.get().connection;
    view! {
        <div class="connection-badge"
             class:connected=move || matches!(connection(), ConnectionState::Connected)
             class:disconnected=move || matches!(connection(), ConnectionState::Disconnected)>
            {move || match connection() {
                ConnectionState::Connecting => "connecting…",
                ConnectionState::Connected => "live",
                ConnectionState::Disconnected => "disconnected",
            }}
        </div>
    }
}
