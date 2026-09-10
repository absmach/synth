// SPDX-License-Identifier: Apache-2.0

//! Inspector panel displaying detailed properties for selected components and nets.

use leptos::prelude::*;
use synth_ir::{Component, Net};

use crate::state::{BoardView, SelectedEntity};

#[component]
pub fn InspectorPanel(
    state: ReadSignal<BoardView>,
    selected: RwSignal<SelectedEntity>,
) -> impl IntoView {
    let clear_selection = move |_| selected.set(SelectedEntity::None);

    view! {
        <div class="inspector-wrap">
            <div class="inspector-header">
                <h3>"Inspector"</h3>
                {move || {
                    let sel = selected.get();
                    if matches!(sel, SelectedEntity::None) {
                        ().into_any()
                    } else {
                        view! {
                            <button class="clear-btn" title="Clear selection" on:click=clear_selection>
                                "×"
                            </button>
                        }
                        .into_any()
                    }
                }}
            </div>

            {move || {
                let view_state = state.get();
                let Some(board) = view_state.board.as_ref() else {
                    return view! {
                        <div class="empty">"No board loaded."</div>
                    }.into_any();
                };

                match selected.get() {
                    SelectedEntity::None => view! {
                        <div class="empty">"Click a component or net to inspect properties."</div>
                    }.into_any(),

                    SelectedEntity::Component(comp_id) => {
                        if let Some(comp) = board.component(comp_id) {
                            render_component_inspector(comp).into_any()
                        } else {
                            view! { <div class="empty">"Component not found."</div> }.into_any()
                        }
                    }

                    SelectedEntity::Net(net_id) => {
                        if let Some(net) = board.nets.iter().find(|n| n.id == net_id) {
                            render_net_inspector(board, net).into_any()
                        } else {
                            view! { <div class="empty">"Net not found."</div> }.into_any()
                        }
                    }
                }
            }}
        </div>
    }
}

fn render_component_inspector(comp: &Component) -> impl IntoView {
    let refdes = comp.refdes.clone();
    let part_id = comp
        .part
        .as_ref()
        .map_or_else(|| "(no part)".to_string(), |p| p.id.as_str().to_string());
    let kind = comp
        .part
        .as_ref()
        .map_or_else(|| "unknown".to_string(), |p| p.kind.as_str().to_string());
    let description = comp.part.as_ref().and_then(|p| p.description.clone());
    let mpn = comp.part.as_ref().and_then(|p| p.mpn.clone());
    let lcsc_pn = comp.part.as_ref().and_then(|p| p.lcsc_pn.clone());
    let kicad_symbol = comp.part.as_ref().and_then(|p| p.kicad_symbol.clone());

    let pins: Vec<_> = comp
        .part
        .as_ref()
        .map(|part| {
            part.pins
                .iter()
                .map(|p| {
                    let num = p.number.0.clone();
                    let name = p.name.clone();
                    let elec = format!("{:?}", p.electrical_type);
                    let caps: Vec<_> = p.capabilities.iter().map(|c| format!("{c:?}")).collect();
                    let caps_text = if caps.is_empty() {
                        "—".to_string()
                    } else {
                        caps.join(", ")
                    };
                    view! {
                        <tr>
                            <td class="pin-num">{num}</td>
                            <td class="pin-name">{name}</td>
                            <td class="pin-elec">{elec}</td>
                            <td class="pin-caps">{caps_text}</td>
                        </tr>
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    view! {
        <div class="inspector-content">
            <div class="inspector-card">
                <div class="card-title">
                    <span class="refdes-badge">{refdes}</span>
                    <span class="kind-tag">{kind}</span>
                </div>
                <div class="attr-row">
                    <span class="attr-label">"Part ID:"</span>
                    <span class="attr-val">{part_id}</span>
                </div>
                {mpn.map(|m| view! {
                    <div class="attr-row">
                        <span class="attr-label">"MPN:"</span>
                        <span class="attr-val">{m}</span>
                    </div>
                })}
                {lcsc_pn.map(|l| view! {
                    <div class="attr-row">
                        <span class="attr-label">"LCSC:"</span>
                        <span class="attr-val">{l}</span>
                    </div>
                })}
                {kicad_symbol.map(|sym| view! {
                    <div class="attr-row">
                        <span class="attr-label">"Symbol:"</span>
                        <span class="attr-val">{sym}</span>
                    </div>
                })}
                {description.map(|d| view! {
                    <div class="description-box">{d}</div>
                })}
            </div>

            <div class="pins-section">
                <h4>{format!("Pins ({})", pins.len())}</h4>
                <table class="pins-table">
                    <thead>
                        <tr>
                            <th>"#"</th>
                            <th>"Name"</th>
                            <th>"Type"</th>
                            <th>"Capabilities"</th>
                        </tr>
                    </thead>
                    <tbody>{pins}</tbody>
                </table>
            </div>
        </div>
    }
}

fn render_net_inspector(board: &synth_ir::Board, net: &Net) -> impl IntoView {
    let name = net.name.clone();
    let count = net.endpoints.len();

    let endpoints: Vec<_> = net
        .endpoints
        .iter()
        .map(|ep| {
            let comp_refdes = board
                .component(ep.component)
                .map_or_else(|| format!("Comp#{}", ep.component.0), |c| c.refdes.clone());
            let pin_name = board
                .component(ep.component)
                .and_then(|c| c.part.as_ref())
                .and_then(|p| p.pins.get(ep.pin.0 as usize))
                .map_or_else(|| format!("Pin#{}", ep.pin.0), |p| p.name.clone());
            let pin_num = board
                .component(ep.component)
                .and_then(|c| c.part.as_ref())
                .and_then(|p| p.pins.get(ep.pin.0 as usize))
                .map(|p| p.number.0.clone())
                .unwrap_or_default();

            let num_view = if pin_num.is_empty() {
                None
            } else {
                Some(view! {
                    <span class="ep-num">{format!("(pin {pin_num})")}</span>
                })
            };

            view! {
                <li class="endpoint-item">
                    <span class="ep-comp">{comp_refdes}</span>
                    <span class="ep-sep">"."</span>
                    <span class="ep-pin">{pin_name}</span>
                    {num_view}
                </li>
            }
        })
        .collect();

    view! {
        <div class="inspector-content">
            <div class="inspector-card">
                <div class="card-title">
                    <span class="net-badge">{name}</span>
                </div>
                <div class="attr-row">
                    <span class="attr-label">"Endpoints:"</span>
                    <span class="attr-val">{count}</span>
                </div>
            </div>

            <div class="endpoints-section">
                <h4>"Connected Pins"</h4>
                <ul class="endpoints-list">{endpoints}</ul>
            </div>
        </div>
    }
}
