// SPDX-License-Identifier: Apache-2.0

//! Functional module extraction for human-quality PCB placement.
//!
//! Groups ICs, power supplies, crystals, and connectors into composite
//! sub-blocks with relative local offsets (dx, dy). When placing an anchor
//! component, its associated passives land in tight relative positions
//! around the anchor rather than floating to distant top-row grid cells.

use std::collections::{HashMap, HashSet};
use synth_geometry::{mm_to_nm, Point, Rotation};
use synth_ir::{Board, ComponentId};

/// A child component bound to a module anchor with a relative nanometer offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleMember {
    pub id: ComponentId,
    pub offset_nm: Point,
    pub rotation: Rotation,
}

/// A functional module consisting of an anchor component and its relative child passives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionalModule {
    pub anchor_id: ComponentId,
    pub members: Vec<ModuleMember>,
}

fn compute_dynamic_module_offsets(
    anchor_id: ComponentId,
    child_id: ComponentId,
    courtyard_lookup: &HashMap<ComponentId, (f64, f64)>,
    idx: usize,
) -> (f64, f64) {
    let (aw, ah) = courtyard_lookup
        .get(&anchor_id)
        .copied()
        .unwrap_or((8.0, 8.0));
    let (cw, ch) = courtyard_lookup
        .get(&child_id)
        .copied()
        .unwrap_or((2.0, 2.0));

    // Generous clearance margin: 5.5mm for THT/large ICs (>15mm), 3.0mm for standard SMD
    let margin = if ah > 15.0 || aw > 15.0 { 5.5 } else { 3.0 };
    let dx = aw / 2.0 + cw / 2.0 + margin;
    let dy = ah / 2.0 + ch / 2.0 + margin;

    let patterns = [
        (dx, 0.0),
        (-dx, 0.0),
        (0.0, dy),
        (0.0, -dy),
        (dx, dy / 2.0),
        (-dx, dy / 2.0),
        (dx, -dy / 2.0),
        (-dx, -dy / 2.0),
    ];
    patterns[idx % patterns.len()]
}

/// Extract all functional modules from `board`.
/// Returns the list of modules and the set of component IDs claimed as child members.
#[allow(
    clippy::too_many_lines,
    clippy::implicit_hasher,
    clippy::explicit_counter_loop
)]
pub fn extract_functional_modules(
    board: &Board,
    courtyard_lookup: &HashMap<ComponentId, (f64, f64)>,
) -> (Vec<FunctionalModule>, HashSet<ComponentId>) {
    let mut modules = Vec::new();
    let mut claimed_children = HashSet::new();

    // 1. IC Decoupling Modules
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        if part.required_decoupling.is_empty() {
            continue;
        }

        let (_anchor_w_mm, _anchor_h_mm) = courtyard_lookup
            .get(&component.id)
            .copied()
            .unwrap_or((10.0, 10.0));

        let mut members = Vec::new();
        let mut caps_for_ic = Vec::new();

        for rule in &part.required_decoupling {
            let Some(pin_idx) = part.pins.iter().position(|p| p.name == rule.net) else {
                continue;
            };
            let mut count_for_rule = 0_u32;
            'outer: for net in &board.nets {
                let mentions_pin = net
                    .endpoints
                    .iter()
                    .any(|ep| ep.component == component.id && ep.pin.0 as usize == pin_idx);
                if !mentions_pin {
                    continue;
                }
                for endpoint in &net.endpoints {
                    if endpoint.component == component.id
                        || claimed_children.contains(&endpoint.component)
                    {
                        continue;
                    }
                    if let Some(other) =
                        board.components.iter().find(|c| c.id == endpoint.component)
                    {
                        if other.part.as_ref().is_some_and(|p| p.kind == "capacitor") {
                            caps_for_ic.push(other.id);
                            claimed_children.insert(other.id);
                            count_for_rule += 1;
                            if count_for_rule >= rule.count {
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }

        for (angle_idx, cap_id) in caps_for_ic.into_iter().enumerate() {
            let (dx_mm, dy_mm) =
                compute_dynamic_module_offsets(component.id, cap_id, courtyard_lookup, angle_idx);
            let rot = match angle_idx % 4 {
                0 => Rotation::OneEighty,
                1 => Rotation::Zero,
                2 => Rotation::Ninety,
                _ => Rotation::TwoSeventy,
            };
            members.push(ModuleMember {
                id: cap_id,
                offset_nm: Point::new(mm_to_nm(dx_mm), mm_to_nm(dy_mm)),
                rotation: rot,
            });
        }

        if !members.is_empty() {
            modules.push(FunctionalModule {
                anchor_id: component.id,
                members,
            });
        }
    }

    // 2. Crystal Modules (Crystal + Load Caps)
    for component in &board.components {
        if component.kind != "crystal" || claimed_children.contains(&component.id) {
            continue;
        }
        let mut members = Vec::new();
        let mut cap_idx = 0;

        for net in &board.nets {
            if !net.endpoints.iter().any(|ep| ep.component == component.id) {
                continue;
            }
            for endpoint in &net.endpoints {
                if endpoint.component == component.id
                    || claimed_children.contains(&endpoint.component)
                {
                    continue;
                }
                if let Some(other) = board.components.iter().find(|c| c.id == endpoint.component) {
                    if other.kind == "capacitor" {
                        let (dx_mm, dy_mm) = compute_dynamic_module_offsets(
                            component.id,
                            other.id,
                            courtyard_lookup,
                            cap_idx,
                        );
                        members.push(ModuleMember {
                            id: other.id,
                            offset_nm: Point::new(mm_to_nm(dx_mm), mm_to_nm(dy_mm)),
                            rotation: Rotation::Zero,
                        });
                        claimed_children.insert(other.id);
                        cap_idx += 1;
                    }
                }
            }
        }

        if !members.is_empty() {
            modules.push(FunctionalModule {
                anchor_id: component.id,
                members,
            });
        }
    }

    // 3. USB Connector Modules (Connector + ESD Diodes + Series Resistors)
    for component in &board.components {
        if component.kind != "connector" || claimed_children.contains(&component.id) {
            continue;
        }
        let mut members = Vec::new();
        let mut child_idx = 0;

        for net in &board.nets {
            if !net.endpoints.iter().any(|ep| ep.component == component.id) {
                continue;
            }
            for endpoint in &net.endpoints {
                if endpoint.component == component.id
                    || claimed_children.contains(&endpoint.component)
                {
                    continue;
                }
                if let Some(other) = board.components.iter().find(|c| c.id == endpoint.component) {
                    if matches!(other.kind.as_str(), "diode" | "resistor") {
                        let (dx_mm, dy_mm) = compute_dynamic_module_offsets(
                            component.id,
                            other.id,
                            courtyard_lookup,
                            child_idx,
                        );
                        members.push(ModuleMember {
                            id: other.id,
                            offset_nm: Point::new(mm_to_nm(dx_mm), mm_to_nm(dy_mm)),
                            rotation: Rotation::Zero,
                        });
                        claimed_children.insert(other.id);
                        child_idx += 1;
                    }
                }
            }
        }

        if !members.is_empty() {
            modules.push(FunctionalModule {
                anchor_id: component.id,
                members,
            });
        }
    }

    // 4. Voltage Regulator Modules (Regulator + Input/Output Caps)
    for component in &board.components {
        if component.kind != "regulator" || claimed_children.contains(&component.id) {
            continue;
        }
        let mut members = Vec::new();
        let mut cap_idx = 0;

        for net in &board.nets {
            if !net.endpoints.iter().any(|ep| ep.component == component.id) {
                continue;
            }
            for endpoint in &net.endpoints {
                if endpoint.component == component.id
                    || claimed_children.contains(&endpoint.component)
                {
                    continue;
                }
                if let Some(other) = board.components.iter().find(|c| c.id == endpoint.component) {
                    if other.kind == "capacitor" {
                        let (dx_mm, dy_mm) = compute_dynamic_module_offsets(
                            component.id,
                            other.id,
                            courtyard_lookup,
                            cap_idx,
                        );
                        members.push(ModuleMember {
                            id: other.id,
                            offset_nm: Point::new(mm_to_nm(dx_mm), mm_to_nm(dy_mm)),
                            rotation: Rotation::Zero,
                        });
                        claimed_children.insert(other.id);
                        cap_idx += 1;
                    }
                }
            }
        }

        if !members.is_empty() {
            modules.push(FunctionalModule {
                anchor_id: component.id,
                members,
            });
        }
    }

    // 5. Orphan Passive Net Clustering (bind remaining passives to their net IC anchor)
    for component in &board.components {
        let is_passive = matches!(
            component.kind.as_str(),
            "resistor" | "capacitor" | "inductor" | "diode"
        );
        if !is_passive || claimed_children.contains(&component.id) {
            continue;
        }
        for net in &board.nets {
            let mentions_passive = net.endpoints.iter().any(|ep| ep.component == component.id);
            if !mentions_passive {
                continue;
            }
            for endpoint in &net.endpoints {
                if endpoint.component == component.id
                    || claimed_children.contains(&endpoint.component)
                {
                    continue;
                }
                if let Some(ic) = board.components.iter().find(|c| c.id == endpoint.component) {
                    let is_multi_pin = ic.part.as_ref().is_none_or(|p| p.pins.len() >= 3);
                    if is_multi_pin {
                        let idx = modules
                            .iter()
                            .find(|m| m.anchor_id == ic.id)
                            .map_or(0, |m| m.members.len());
                        let (dx_mm, dy_mm) = compute_dynamic_module_offsets(
                            ic.id,
                            component.id,
                            courtyard_lookup,
                            idx,
                        );

                        if let Some(m) = modules.iter_mut().find(|m| m.anchor_id == ic.id) {
                            m.members.push(ModuleMember {
                                id: component.id,
                                offset_nm: Point::new(mm_to_nm(dx_mm), mm_to_nm(dy_mm)),
                                rotation: Rotation::Zero,
                            });
                        } else {
                            modules.push(FunctionalModule {
                                anchor_id: ic.id,
                                members: vec![ModuleMember {
                                    id: component.id,
                                    offset_nm: Point::new(mm_to_nm(dx_mm), mm_to_nm(dy_mm)),
                                    rotation: Rotation::Zero,
                                }],
                            });
                        }
                        claimed_children.insert(component.id);
                        break;
                    }
                }
            }
            if claimed_children.contains(&component.id) {
                break;
            }
        }
    }

    (modules, claimed_children)
}
