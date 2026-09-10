// SPDX-License-Identifier: Apache-2.0
//! Pass 1: LED indicator clusters.

use std::collections::HashSet;
use synth_ir::{Board, ComponentId};

use super::Pattern;
use crate::{is_led, Cluster, ClusterKind, ClusterMember, MemberSide};

pub(crate) struct LedIndicator;

impl Pattern for LedIndicator {
    /// LED indicator clusters. Anchor is the LED, member is the
    /// current-limit resistor wired to its anode net. Both will be
    /// rendered as a vertical chain (R on top, LED below) — set
    /// `anchor_vertical: true` so the placer knows.
    fn recognize(board: &Board, claimed: &mut HashSet<ComponentId>) -> Vec<Cluster> {
        let mut clusters: Vec<Cluster> = Vec::new();
        for component in &board.components {
            if claimed.contains(&component.id) {
                continue;
            }
            if !is_led(component) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let Some(anode_idx) = part.pins.iter().position(|p| p.name == "anode") else {
                continue;
            };
            let pid = synth_ir::PinId(anode_idx as u32);
            let mut members: Vec<ClusterMember> = Vec::new();
            'outer: for (_net_id, net) in board.nets_containing(component.id, pid) {
                for endpoint in &net.endpoints {
                    if endpoint.component == component.id || claimed.contains(&endpoint.component) {
                        continue;
                    }
                    let Some(other) = board.component(endpoint.component) else {
                        continue;
                    };
                    let Some(other_part) = other.part.as_ref() else {
                        continue;
                    };
                    if other_part.kind == "resistor" {
                        members.push(ClusterMember {
                            id: endpoint.component,
                            side: MemberSide::Above,
                        });
                        claimed.insert(endpoint.component);
                        break 'outer; // one limit resistor per LED
                    }
                }
            }
            claimed.insert(component.id);
            members.sort_by_key(|m| m.id.0);
            clusters.push(Cluster {
                kind: ClusterKind::LedIndicator,
                anchor: component.id,
                anchor_vertical: true,
                members,
            });
        }
        clusters
    }
}
