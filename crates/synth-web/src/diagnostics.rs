// SPDX-License-Identifier: Apache-2.0

//! Diagnostics panel. Read-only list with severity, code, title,
//! and `file:line:col` location. Each diagnostic collapses to a one-line
//! header by default; click the header to expand the message, the
//! expected/found pair, and the source location. No in-browser source
//! jumping in V1 — agents and humans get the location string and use
//! their own editor's go-to-line.

use leptos::prelude::*;
use synth_diagnostics::{Diagnostic, Severity};

use crate::state::BoardView;

#[component]
pub fn DiagnosticsList(
    state: ReadSignal<BoardView>,
    selected: RwSignal<crate::state::SelectedEntity>,
) -> impl IntoView {
    view! {
        <div class="diagnostics-wrap">
            {move || {
                let view_state = state.get();
                let board_ref = view_state.board.as_ref();
                if view_state.diagnostics.is_empty() {
                    view! { <div class="empty">"No diagnostics — design is clean."</div> }
                        .into_any()
                } else {
                    let items: Vec<_> = view_state
                        .diagnostics
                        .iter()
                        .map(|d| render_diagnostic(d, board_ref, selected).into_any())
                        .collect();
                    view! { <ul class="diagnostics">{items}</ul> }.into_any()
                }
            }}
        </div>
    }
}

fn render_diagnostic(
    d: &Diagnostic,
    board: Option<&synth_ir::Board>,
    selected: RwSignal<crate::state::SelectedEntity>,
) -> impl IntoView {
    let severity_class = match d.severity {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
        Severity::Fatal => "fatal",
    };
    let location_text = d
        .location
        .as_ref()
        .map(|l| format!("{}:{}-{}", l.file, l.span.byte_start, l.span.byte_end));
    let code = d.code.clone();
    let title = d.title.clone();
    let severity_text = format!("{:?}", d.severity).to_lowercase();
    let message = d.message.clone();
    let expected = d.expected.clone();
    let found = d.found.clone();
    let has_body =
        message.is_some() || (expected.is_some() && found.is_some()) || location_text.is_some();

    // Cross-probing target resolution
    let target_entity = board.and_then(|b| {
        for entity in &d.entities {
            match entity {
                synth_diagnostics::EntityRef::Component { id } => {
                    if let Some(comp) = b.components.iter().find(|c| c.refdes == *id) {
                        return Some(crate::state::SelectedEntity::Component(comp.id));
                    }
                }
                synth_diagnostics::EntityRef::Pin { component, .. } => {
                    if let Some(comp) = b.components.iter().find(|c| c.refdes == *component) {
                        return Some(crate::state::SelectedEntity::Component(comp.id));
                    }
                }
                synth_diagnostics::EntityRef::Net { name } => {
                    if let Some(net) = b.nets.iter().find(|n| n.name == *name) {
                        return Some(crate::state::SelectedEntity::Net(net.id));
                    }
                }
                _ => {}
            }
        }
        None
    });

    let on_click = move |_| {
        if let Some(target) = target_entity {
            selected.set(target);
        }
    };

    view! {
        <li class={format!("diagnostic diagnostic-{severity_class}")} on:click=on_click>
            <details>
                <summary class="diag-header">
                    <span class="severity">{severity_text}</span>
                    <span class="code">{code}</span>
                    <span class="title">{title}</span>
                </summary>
                {has_body.then(|| view! {
                    <div class="diag-body">
                        {message.map(|m| view! { <div class="message">{m}</div> })}
                        {expected
                            .zip(found)
                            .map(|(e, f)| view! {
                                <div class="exp-found">
                                    <span class="expected">"expected: "{e}</span>
                                    <span class="found">"  found: "{f}</span>
                                </div>
                            })}
                        {location_text.map(|l| view! { <div class="location">{l}</div> })}
                    </div>
                })}
            </details>
        </li>
    }
}
