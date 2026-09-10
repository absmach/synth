// SPDX-License-Identifier: Apache-2.0
//! Pass 3: LDO regulator + input/output decoupling caps (LdoBlock).
//!
//! Recognition rule (§7.5.4): a `kind = regulator` with 1+ input caps
//! on `vin/gnd` and 1+ output caps on `vout/gnd`. Runs *before* the
//! generic IC-block pass so the regulator and its rail caps are
//! claimed together instead of the regulator's `required_decoupling`
//! being consumed as a plain IC decoupling cluster.

use std::collections::HashSet;
use synth_ir::{Board, ComponentId};

use super::Pattern;
use crate::{Cluster, ClusterKind, ClusterMember, MemberSide};

pub(crate) struct LdoBlock;

impl Pattern for LdoBlock {
    /// Anchor on a `kind = regulator`; claim the input caps on its
    /// `vin` net and output caps on its `vout` net as a `Below` shelf
    /// under the regulator body.
    ///
    /// Only claims up to the regulator's declared `required_decoupling`
    /// count per rail (default 1). Without this cap the LDO greedily
    /// absorbs every capacitor on the shared power rail — including
    /// the decoupling caps that belong to downstream ICs fed from that
    /// rail (e.g. an STM32's and a sensor's `vdd` decoupling, all on
    /// the same `vout` net) — leaving those ICs with no visible
    /// decoupling. Capping to the regulator's own requirement keeps
    /// its rail caps and hands the rest back to the IC clusters.
    fn recognize(board: &Board, claimed: &mut HashSet<ComponentId>) -> Vec<Cluster> {
        let mut clusters: Vec<Cluster> = Vec::new();
        for component in &board.components {
            if claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "regulator" {
                continue;
            }
            let mut members: Vec<ClusterMember> = Vec::new();
            for (pin_idx, pin) in part.pins.iter().enumerate() {
                let lower = pin.name.to_ascii_lowercase();
                if lower != "vin" && lower != "vout" {
                    continue;
                }
                // How many caps this rail is allowed to claim. Default
                // to 1; honour a declared `required_decoupling` count
                // for this exact pin (AMS1117: 1 on vin, 1 on vout).
                let allowance = part
                    .required_decoupling
                    .iter()
                    .find(|d| d.net == pin.name)
                    .map_or(1, |d| d.count.max(1) as usize);
                let pid = synth_ir::PinId(pin_idx as u32);
                let mut claimed_for_rail = 0_usize;
                'pin_outer: for (_net_id, net) in board.nets_containing(component.id, pid) {
                    for endpoint in &net.endpoints {
                        if claimed_for_rail >= allowance {
                            break 'pin_outer;
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
                            claimed_for_rail += 1;
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
                kind: ClusterKind::LdoBlock,
                anchor: component.id,
                anchor_vertical: false,
                members,
            });
        }
        clusters
    }
}
