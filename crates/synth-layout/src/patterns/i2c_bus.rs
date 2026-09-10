// SPDX-License-Identifier: Apache-2.0
//! Pass 4: I2C bus — SDA/SCL pull-ups on a shared rail (I2cBus).
//!
//! Recognition rule (§7.5.4): a component carrying both an `i2c_sda`
//! and an `i2c_scl` pin, each with a pull-up resistor whose far pin
//! ties to a common power net. The pull-ups rise toward their supply
//! rail (MemberSide::Above), the usual readable bus-pull-up
//! arrangement.

use std::collections::HashSet;
use synth_ir::{Board, ComponentId, NetId};

use super::Pattern;
use crate::{Cluster, ClusterKind, ClusterMember, MemberSide};

pub(crate) struct I2cBus;

impl Pattern for I2cBus {
    fn recognize(board: &Board, claimed: &mut HashSet<ComponentId>) -> Vec<Cluster> {
        use synth_registry::PinCapability;
        let mut clusters: Vec<Cluster> = Vec::new();
        for component in &board.components {
            if claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let sda_idx = part
                .pins
                .iter()
                .position(|p| p.capabilities.contains(&PinCapability::I2cSda));
            let scl_idx = part
                .pins
                .iter()
                .position(|p| p.capabilities.contains(&PinCapability::I2cScl));
            let (Some(sda_idx), Some(scl_idx)) = (sda_idx, scl_idx) else {
                continue;
            };

            let mut members: Vec<ClusterMember> = Vec::new();
            let mut has_sda_pull = false;
            let mut has_scl_pull = false;
            for (pin_idx, want) in [(sda_idx, &mut has_sda_pull), (scl_idx, &mut has_scl_pull)] {
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
                        if is_pullup_to_power(board, other.id, net.id) {
                            members.push(ClusterMember {
                                id: other.id,
                                side: MemberSide::Above,
                            });
                            claimed.insert(other.id);
                            *want = true;
                        }
                    }
                }
            }

            // Require a pull-up on *both* the SDA and SCL nets so this
            // is genuinely an I2C bus, not a lone resistor that the
            // generic IC-block pass (running next) should own.
            if !has_sda_pull || !has_scl_pull {
                continue;
            }
            claimed.insert(component.id);
            members.sort_by_key(|m| m.id.0);
            clusters.push(Cluster {
                kind: ClusterKind::I2cBus,
                anchor: component.id,
                anchor_vertical: false,
                members,
            });
        }
        clusters
    }
}

/// True when `resistor_id` has a pin (other than the one carrying
/// `signal_net`) whose net touches a `power_input`/`power_output`
/// pin — i.e. the resistor pulls a signal line up or down to a rail.
fn is_pullup_to_power(board: &Board, resistor_id: ComponentId, signal_net: NetId) -> bool {
    use synth_registry::ElectricalType;
    let Some(part) = board.component(resistor_id).and_then(|c| c.part.as_ref()) else {
        return false;
    };
    for (pin_idx, _) in part.pins.iter().enumerate() {
        let pid = synth_ir::PinId(pin_idx as u32);
        for (_net_id, net) in board.nets_containing(resistor_id, pid) {
            if net.id == signal_net {
                continue;
            }
            let touches_rail = net.endpoints.iter().any(|ep| {
                ep.component != resistor_id
                    && board.pin(ep.component, ep.pin).is_some_and(|p| {
                        matches!(
                            p.electrical_type,
                            ElectricalType::PowerInput | ElectricalType::PowerOutput
                        )
                    })
            });
            if touches_rail {
                return true;
            }
        }
    }
    false
}
