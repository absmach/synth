// SPDX-License-Identifier: Apache-2.0

//! Client-side state and the SSE subscription.
//!
//! [`BoardView`] is the wire payload the server sends on every
//! recompile. We deserialize it directly into the `synth-ir` and
//! `synth-diagnostics` types — no two-language schema drift.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use synth_diagnostics::Diagnostic;
use synth_ir::Board;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{EventSource, MessageEvent};

/// The full state the browser tracks: latest [`Board`], latest
/// diagnostics, and the SSE connection status.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BoardView {
    /// The source path the CLI is watching, for display.
    #[serde(default)]
    pub source_path: String,
    /// Last server-side compilation result. `None` when the file
    /// could not be parsed at all.
    #[serde(default)]
    pub board: Option<Board>,
    /// Every diagnostic from every stage (parse, lower, ERC).
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    /// Transport state. Updated locally by the SSE handler; the
    /// server never sets this field on the wire.
    #[serde(skip)]
    pub connection: ConnectionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionState {
    #[default]
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectedEntity {
    #[default]
    None,
    Component(synth_ir::ComponentId),
    Net(synth_ir::NetId),
}

/// Subscribe to `/events` (Server-Sent Events) from the
/// `synth preview` server and route each message into a reactive
/// signal. Returns a read-only handle the components can subscribe to.
pub fn subscribe_to_events() -> ReadSignal<BoardView> {
    let (state, set_state) = signal(BoardView::default());

    let Ok(source) = EventSource::new("/events") else {
        // No-op on construction failure; the badge will stay
        // "connecting" forever, which is the right visual signal.
        return state;
    };

    let on_open = Closure::<dyn FnMut()>::new(move || {
        set_state.update(|s| s.connection = ConnectionState::Connected);
    });
    source.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();

    let on_error = Closure::<dyn FnMut()>::new(move || {
        set_state.update(|s| s.connection = ConnectionState::Disconnected);
    });
    source.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    on_error.forget();

    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
        let Some(data) = evt.data().as_string() else {
            return;
        };
        match serde_json::from_str::<BoardView>(&data) {
            Ok(mut view) => {
                view.connection = ConnectionState::Connected;
                set_state.set(view);
            }
            Err(e) => {
                web_sys::console::warn_1(&format!("synth-web: malformed SSE payload: {e}").into());
            }
        }
    });
    source.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();

    state
}
