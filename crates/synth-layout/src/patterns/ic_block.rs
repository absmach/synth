// SPDX-License-Identifier: Apache-2.0
//! Pass 6: IC + decoupling caps + reset network passives + pull-up/down resistors.

use std::collections::HashSet;
use synth_ir::{Board, ComponentId};

use super::Pattern;
use crate::{
    classify_ic_pin_layout, compute_anchor_pin_offset, Cluster, ClusterKind, ClusterMember,
    MemberSide, PinSide,
};

pub(crate) struct IcBlock;

impl Pattern for IcBlock {
    fn recognize(board: &Board, claimed: &mut HashSet<ComponentId>) -> Vec<Cluster> {
        let mut clusters: Vec<Cluster> = Vec::new();
        for component in &board.components {
            if claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let has_decoupling = !part.required_decoupling.is_empty();
            let has_reset_pin = part.pins.iter().any(|p| {
                use synth_registry::PinCapability;
                p.capabilities.contains(&PinCapability::Reset)
            });
            if !has_decoupling && !has_reset_pin {
                continue;
            }

            let mut members: Vec<ClusterMember> = Vec::new();

            // Decoupling caps, one per `decoupling.count` per pin.
            // Marked as `Below`: they hang under the IC body.
            for decoupling in &part.required_decoupling {
                let Some(pin_idx) = part.pins.iter().position(|p| p.name == decoupling.net) else {
                    continue;
                };
                let pid = synth_ir::PinId(pin_idx as u32);
                let max_claims = decoupling.count as usize;
                let mut claims_for_pin = 0_usize;
                'cap_outer: for (_net_id, net) in board.nets_containing(component.id, pid) {
                    for endpoint in &net.endpoints {
                        if claims_for_pin >= max_claims {
                            break 'cap_outer;
                        }
                        if endpoint.component == component.id
                            || claimed.contains(&endpoint.component)
                        {
                            continue;
                        }
                        let Some(other) = board.component(endpoint.component) else {
                            continue;
                        };
                        let Some(other_part) = other.part.as_ref() else {
                            continue;
                        };
                        if other_part.kind == "capacitor" {
                            members.push(ClusterMember {
                                id: endpoint.component,
                                side: MemberSide::Below,
                            });
                            claimed.insert(endpoint.component);
                            claims_for_pin += 1;
                        }
                    }
                }
            }

            // Reset network: passives on a reset-capability pin's net.
            // The passives sit on whichever side the reset pin actually
            // occupies — the stock KiCad symbol's geometry when the part
            // has one (STM32F103's NRST is on the LEFT), otherwise the
            // synthesized-model convention (Reset → Right; see
            // classify_ic_pin in synth-web). Wires from the reset pin go
            // straight out without curling around the body.
            for (pin_idx, pin) in part.pins.iter().enumerate() {
                use synth_registry::PinCapability;
                if !pin.capabilities.contains(&PinCapability::Reset) {
                    continue;
                }
                let member_side = match compute_anchor_pin_offset(part, pin_idx).2 {
                    PinSide::Left => MemberSide::Left,
                    _ => MemberSide::Right,
                };
                let pid = synth_ir::PinId(pin_idx as u32);
                for (_net_id, net) in board.nets_containing(component.id, pid) {
                    for endpoint in &net.endpoints {
                        if endpoint.component == component.id
                            || claimed.contains(&endpoint.component)
                        {
                            continue;
                        }
                        let Some(other) = board.component(endpoint.component) else {
                            continue;
                        };
                        let Some(other_part) = other.part.as_ref() else {
                            continue;
                        };
                        if matches!(
                            other_part.kind.as_str(),
                            "resistor" | "capacitor" | "switch"
                        ) {
                            members.push(ClusterMember {
                                id: endpoint.component,
                                side: member_side,
                            });
                            claimed.insert(endpoint.component);
                        }
                    }
                }
            }

            // Pull-up / pull-down resistors: resistors connected to a signal pin of the IC
            // where the other pin is tied to a power rail (VCC/GND/etc.) or another power/ground pin
            for (pin_idx, pin) in part.pins.iter().enumerate() {
                let side = classify_ic_pin_layout(pin);
                // Ignore power pins (top/bottom)
                if side == PinSide::Top || side == PinSide::Bottom {
                    continue;
                }
                let pid = synth_ir::PinId(pin_idx as u32);
                for (_net_id, net) in board.nets_containing(component.id, pid) {
                    for endpoint in &net.endpoints {
                        if endpoint.component == component.id
                            || claimed.contains(&endpoint.component)
                        {
                            continue;
                        }
                        let Some(other) = board.component(endpoint.component) else {
                            continue;
                        };
                        let Some(other_part) = other.part.as_ref() else {
                            continue;
                        };
                        if other_part.kind != "resistor" {
                            continue;
                        }

                        // Check if the other pin of this resistor connects to power
                        let mut is_pull = false;
                        for other_pin_idx in 0..2 {
                            let other_pid = synth_ir::PinId(other_pin_idx as u32);
                            for (_other_net_id, other_net) in
                                board.nets_containing(other.id, other_pid)
                            {
                                for other_ep in &other_net.endpoints {
                                    if let Some(comp) = board.component(other_ep.component) {
                                        if let Some(p) = comp.part.as_ref() {
                                            if let Some(pin) = p.pins.get(other_ep.pin.0 as usize) {
                                                let pin_name_lower = pin.name.to_lowercase();
                                                if matches!(
                                                    pin_name_lower.as_str(),
                                                    "vcc"
                                                        | "vdd"
                                                        | "gnd"
                                                        | "vss"
                                                        | "vbus"
                                                        | "vin"
                                                        | "vout"
                                                        | "3v3"
                                                        | "5v"
                                                ) {
                                                    is_pull = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if is_pull {
                            // Pull resistors conventionally stand above
                            // the driven logic pin, rising toward their
                            // supply rail. Keeping them out of the pin
                            // column also leaves bus labels readable.
                            let member_side = MemberSide::Above;
                            members.push(ClusterMember {
                                id: other.id,
                                side: member_side,
                            });
                            claimed.insert(other.id);
                        }
                    }
                }
            }

            claimed.insert(component.id);
            members.sort_by_key(|m| m.id.0);
            clusters.push(Cluster {
                kind: ClusterKind::IcBlock,
                anchor: component.id,
                anchor_vertical: false,
                members,
            });
        }
        clusters
    }
}
