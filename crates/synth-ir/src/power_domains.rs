// SPDX-License-Identifier: Apache-2.0

//! Voltage domain inference engine over `synth_ir::Board`.
//!
//! Groups nets and pins into voltage domains (e.g. 5V, 3.3V, 1.8V, Ground)
//! by identifying active power sources (regulator outputs, VBUS, rail names)
//! and propagating nominal operating voltages along connected nets and component pins.

use crate::board::{Board, NetId, PinId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use synth_registry::ElectricalType;

/// Inferred power domain for a net.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PowerDomainKind {
    /// Active power supply rail (e.g. 5.0V, 3.3V, 1.8V).
    Rail {
        nominal_v: f64,
        min_v: f64,
        max_v: f64,
    },
    /// Ground reference.
    Ground,
    /// Signal net driven by a specific voltage domain.
    Signal {
        driven_voltage: Option<f64>,
        max_tolerant_v: Option<f64>,
    },
    /// Unknown / unconstrained domain.
    Unknown,
}

impl PowerDomainKind {
    pub fn nominal_voltage(&self) -> Option<f64> {
        match self {
            PowerDomainKind::Rail { nominal_v, .. } => Some(*nominal_v),
            PowerDomainKind::Ground => Some(0.0),
            PowerDomainKind::Signal { driven_voltage, .. } => *driven_voltage,
            PowerDomainKind::Unknown => None,
        }
    }

    pub fn is_rail(&self) -> bool {
        matches!(self, PowerDomainKind::Rail { .. })
    }

    pub fn is_ground(&self) -> bool {
        matches!(self, PowerDomainKind::Ground)
    }
}

/// Map of inferred power domains for every net in a `Board`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PowerDomainMap {
    pub domains: HashMap<NetId, PowerDomainKind>,
}

impl PowerDomainMap {
    pub fn get(&self, net_id: NetId) -> Option<&PowerDomainKind> {
        self.domains.get(&net_id)
    }
}

/// Infer power domains for all nets in `board`.
pub fn infer_power_domains(board: &Board) -> PowerDomainMap {
    let mut map = PowerDomainMap::default();

    // Pass 1: Identify power output rails & ground reference nets
    for net in &board.nets {
        let mut inferred_rail: Option<f64> = None;
        let mut is_gnd = false;

        // Check net name heuristics
        let name_lower = net.name.to_lowercase();
        if matches!(
            name_lower.as_str(),
            "gnd" | "vss" | "vssa" | "vee" | "agnd" | "dgnd" | "vneg"
        ) {
            is_gnd = true;
        } else if let Some(v) = parse_voltage_from_name(&name_lower) {
            inferred_rail = Some(v);
        }

        // Check connected pin definitions
        for endpoint in &net.endpoints {
            if let Some(pin) = board.pin(endpoint.component, endpoint.pin) {
                if pin.electrical_type == ElectricalType::GroundReference {
                    is_gnd = true;
                } else if pin.electrical_type == ElectricalType::PowerOutput {
                    if let Some(v) = pin.nominal_voltage_v() {
                        inferred_rail = Some(v);
                    }
                }
            }
        }

        if is_gnd {
            map.domains.insert(net.id, PowerDomainKind::Ground);
        } else if let Some(nom_v) = inferred_rail {
            map.domains.insert(
                net.id,
                PowerDomainKind::Rail {
                    nominal_v: nom_v,
                    min_v: nom_v * 0.9,
                    max_v: nom_v * 1.1,
                },
            );
        }
    }

    // Pass 2: Infer signal net driven voltages from supply rails of driving components
    for net in &board.nets {
        if map.domains.contains_key(&net.id) {
            continue;
        }

        let mut driven_v: Option<f64> = None;
        let mut max_tol_v: Option<f64> = None;

        for endpoint in &net.endpoints {
            let Some(component) = board.component(endpoint.component) else {
                continue;
            };
            let Some(pin) = board.pin(endpoint.component, endpoint.pin) else {
                continue;
            };

            if let Some(v_max) = pin.voltage_max_v {
                max_tol_v = Some(max_tol_v.map_or(v_max, |cur| cur.min(v_max)));
            }

            if pin.electrical_type.is_output_driver() {
                // Find supply rail for this component
                if let Some(rail_v) = component_supply_rail(board, component, &map) {
                    driven_v = Some(rail_v);
                } else if let Some(nom_v) = pin.nominal_voltage_v() {
                    driven_v = Some(nom_v);
                }
            }
        }

        if driven_v.is_some() || max_tol_v.is_some() {
            map.domains.insert(
                net.id,
                PowerDomainKind::Signal {
                    driven_voltage: driven_v,
                    max_tolerant_v: max_tol_v,
                },
            );
        } else {
            map.domains.insert(net.id, PowerDomainKind::Unknown);
        }
    }

    map
}

/// Helper to parse nominal voltage from canonical rail names like `v33`, `3v3`, `v5`, `5v`, `1v8`, `vbus`.
fn parse_voltage_from_name(name: &str) -> Option<f64> {
    match name {
        "v33" | "3v3" | "+3v3" | "vcc3v3" | "vdd3v3" => Some(3.3),
        "v5" | "5v" | "+5v" | "vbus" | "vcc5" => Some(5.0),
        "1v8" | "v18" | "+1v8" => Some(1.8),
        "1v2" | "v12" => Some(1.2),
        "12v" | "+12v" => Some(12.0),
        _ => None,
    }
}

/// Helper to find the active supply rail voltage for a component based on its `PowerInput` pins.
fn component_supply_rail(
    board: &Board,
    component: &crate::board::Component,
    map: &PowerDomainMap,
) -> Option<f64> {
    let part = component.part.as_ref()?;
    for (idx, pin) in part.pins.iter().enumerate() {
        if pin.electrical_type == ElectricalType::PowerInput {
            let pid = PinId(idx as u32);
            for (_, net) in board.nets_containing(component.id, pid) {
                if let Some(PowerDomainKind::Rail { nominal_v, .. }) = map.get(net.id) {
                    return Some(*nominal_v);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_voltage_names() {
        assert_eq!(parse_voltage_from_name("3v3"), Some(3.3));
        assert_eq!(parse_voltage_from_name("5v"), Some(5.0));
        assert_eq!(parse_voltage_from_name("vbus"), Some(5.0));
        assert_eq!(parse_voltage_from_name("1v8"), Some(1.8));
        assert_eq!(parse_voltage_from_name("unknown"), None);
    }
}
