// SPDX-License-Identifier: Apache-2.0
//! Pass 5: crystal with two load caps to GND (Crystal).
//!
//! Recognition rule (§7.5.4): a `kind = crystal` with two load
//! capacitors, one per crystal pin, returning to a shared ground
//! node. The caps fan out on a `Below` shelf under the crystal.

use std::collections::HashSet;
use synth_ir::{Board, ComponentId};

use super::Pattern;
use crate::{Cluster, ClusterKind, ClusterMember, MemberSide};

pub(crate) struct Crystal;

impl Pattern for Crystal {
    fn recognize(board: &Board, claimed: &mut HashSet<ComponentId>) -> Vec<Cluster> {
        let mut clusters: Vec<Cluster> = Vec::new();
        for component in &board.components {
            if claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "crystal" {
                continue;
            }
            let mut members: Vec<ClusterMember> = Vec::new();
            for (pin_idx, _) in part.pins.iter().enumerate() {
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
                        if other_part.kind == "capacitor" {
                            members.push(ClusterMember {
                                id: endpoint.component,
                                side: MemberSide::Below,
                            });
                            claimed.insert(endpoint.component);
                        }
                    }
                }
            }
            if members.is_empty() {
                continue;
            }
            claimed.insert(component.id);
            members.sort_by_key(|m| m.id.0);
            clusters.push(Cluster {
                kind: ClusterKind::Crystal,
                anchor: component.id,
                anchor_vertical: false,
                members,
            });
        }
        clusters
    }
}
