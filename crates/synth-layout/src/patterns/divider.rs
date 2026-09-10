// SPDX-License-Identifier: Apache-2.0
//! Pass 7: two-resistor voltage divider (Divider).
//!
//! Recognition rule (§7.5.4): a net carrying exactly two passive
//! resistors connected R1-from-rail-to-mid and R2-from-mid-to-gnd.
//! The "mid" net has exactly two endpoints, both resistors; the
//! rail-side resistor's other pin connects to a non-resistor source
//! (connector / IC / regulator). Anchors on the rail-side resistor
//! and claims the mid-to-gnd resistor as a `Below` member.

use std::collections::HashSet;
use synth_ir::{Board, ComponentId, Net, PinId};

use super::Pattern;
use crate::{Cluster, ClusterKind, ClusterMember, MemberSide};

pub(crate) struct Divider;

impl Pattern for Divider {
    fn recognize(board: &Board, claimed: &mut HashSet<ComponentId>) -> Vec<Cluster> {
        let mut clusters: Vec<Cluster> = Vec::new();
        for component in &board.components {
            if claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "resistor" || part.pins.len() != 2 {
                continue;
            }
            let Some(cluster) = recognize_divider(board, claimed, component.id) else {
                continue;
            };
            clusters.push(cluster);
        }
        clusters
    }
}

/// Recognise a single divider anchored at `anchor_id` (a 2-pin
/// resistor), claiming its partner and returning the cluster, or
/// `None` if `anchor_id` is not the rail-side resistor of a divider.
fn recognize_divider(
    board: &Board,
    claimed: &mut HashSet<ComponentId>,
    anchor_id: ComponentId,
) -> Option<Cluster> {
    for mid_pin in 0..2usize {
        let rail_pin = 1 - mid_pin;
        let mid_pin_id = PinId(mid_pin as u32);
        let rail_pin_id = PinId(rail_pin as u32);

        // `mid_pin` must sit on the divider's mid net: exactly two
        // endpoints, both resistors (this resistor + its partner).
        let Some(mid_net) = board
            .nets_containing(anchor_id, mid_pin_id)
            .find(|(_, n)| is_two_resistor_mid_net(board, n, anchor_id))
            .map(|(nid, _)| nid)
        else {
            continue;
        };

        // The other pin's net must reach a non-resistor source (the
        // rail) — otherwise both sides are resistors and this is not a
        // rail→mid→gnd divider.
        let has_rail = board.nets_containing(anchor_id, rail_pin_id).any(|(_, n)| {
            n.endpoints.iter().any(|ep| {
                ep.component != anchor_id
                    && board
                        .component(ep.component)
                        .and_then(|c| c.part.as_ref())
                        .is_some_and(|p| p.kind != "resistor")
            })
        });
        if !has_rail {
            continue;
        }

        // The partner resistor on the mid net is the member.
        let member = board
            .net(mid_net)?
            .endpoints
            .iter()
            .find(|ep| ep.component != anchor_id)?
            .component;
        if claimed.contains(&member) {
            continue;
        }
        claimed.insert(anchor_id);
        claimed.insert(member);
        return Some(Cluster {
            kind: ClusterKind::Divider,
            anchor: anchor_id,
            anchor_vertical: false,
            members: vec![ClusterMember {
                id: member,
                side: MemberSide::Below,
            }],
        });
    }
    None
}

/// True when `net` is a "mid" net for a divider: exactly two
/// endpoints, both resistors (one of them `anchor_id`).
fn is_two_resistor_mid_net(board: &Board, net: &Net, anchor_id: ComponentId) -> bool {
    if net.endpoints.len() != 2 {
        return false;
    }
    net.endpoints.iter().any(|ep| ep.component == anchor_id)
        && net.endpoints.iter().all(|ep| {
            board
                .component(ep.component)
                .and_then(|c| c.part.as_ref())
                .is_some_and(|p| p.kind == "resistor")
        })
}
