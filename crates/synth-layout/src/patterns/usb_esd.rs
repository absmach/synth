// SPDX-License-Identifier: Apache-2.0
//! Pass 2: USB connector + ESD diodes.

use std::collections::HashSet;
use synth_ir::{Board, ComponentId};

use super::Pattern;
use crate::{Cluster, ClusterKind, ClusterMember, MemberSide};

pub(crate) struct UsbEsd;

impl Pattern for UsbEsd {
    fn recognize(board: &Board, claimed: &mut HashSet<ComponentId>) -> Vec<Cluster> {
        let mut clusters: Vec<Cluster> = Vec::new();
        for component in &board.components {
            if claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "connector" {
                continue;
            }
            let usb_pin_indices: Vec<usize> = part
                .pins
                .iter()
                .enumerate()
                .filter(|(_, pin)| {
                    use synth_registry::PinCapability;
                    pin.capabilities.contains(&PinCapability::UsbDp)
                        || pin.capabilities.contains(&PinCapability::UsbDn)
                })
                .map(|(idx, _)| idx)
                .collect();
            if usb_pin_indices.is_empty() {
                continue;
            }
            let mut members: Vec<ClusterMember> = Vec::new();
            for pin_idx in usb_pin_indices {
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
                        if other_part.kind == "diode" {
                            members.push(ClusterMember {
                                id: endpoint.component,
                                side: MemberSide::Left,
                            });
                            claimed.insert(endpoint.component);
                        }
                    }
                }
            }
            claimed.insert(component.id);
            members.sort_by_key(|m| m.id.0);
            clusters.push(Cluster {
                kind: ClusterKind::UsbEsd,
                anchor: component.id,
                anchor_vertical: false,
                members,
            });
        }
        clusters
    }
}
