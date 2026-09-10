// SPDX-License-Identifier: Apache-2.0

//! Authoritative netlist and connectivity data structures.
//!
//! This is the single source of truth for electrical connectivity.
//! All downstream passes (ERC, placement, routing, DRC, export)
//! consume this instead of re-deriving connectivity.
//!
//! Synth's IR carries the netlist topology explicitly — each net is a
//! list of `(component, pin)` endpoints produced by the compiler from
//! `connect` statements. There is no geometric union-find over wires:
//! positions are a layout concern and are layered on optionally.

use std::collections::{HashMap, HashSet};

use synth_ir::{Board, ComponentId, NetId, PinId};

/// A terminal (pin) on a net.
#[derive(Debug, Clone, PartialEq)]
pub struct Terminal {
    pub component: ComponentId,
    pub pin: PinId,
    /// World-space position in mm (pin pad centre). Populated only
    /// when a placement layout is available; `None` in the
    /// topology-only derivation path.
    pub position: Option<(f64, f64)>,
}

/// Net class for categorization (power, signal, clock, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NetClass {
    #[default]
    Signal,
    Power,
    Ground,
    Clock,
    Differential,
    RF,
    Analog,
    Digital,
    Mixed,
    Other,
}

/// A net in the authoritative netlist.
#[derive(Debug, Clone)]
pub struct Net {
    pub id: NetId,
    pub name: String,
    pub class: NetClass,
    pub terminals: Vec<Terminal>,
    /// Net-level attributes (impedance, diff pair, etc.)
    pub attributes: HashMap<String, String>,
}

/// Complete connectivity state for a design.
/// Built once, consumed by all passes.
#[derive(Debug, Clone)]
pub struct Connectivity {
    /// Nets by ID
    nets: HashMap<NetId, Net>,
    /// Net name → ID lookup
    name_to_id: HashMap<String, NetId>,
    /// Component-pin → net ID
    pin_to_net: HashMap<(ComponentId, PinId), NetId>,
    /// Global/power net names that should merge across hierarchy
    global_net_names: HashSet<String>,
}

impl Connectivity {
    pub fn new() -> Self {
        Self {
            nets: HashMap::new(),
            name_to_id: HashMap::new(),
            pin_to_net: HashMap::new(),
            global_net_names: [
                "VCC", "VDD", "VCC3V3", "VCC5V", "VCC1V8", "3V3", "3V", "5V", "12V", "1V8", "1V2",
                "2V5", "GND", "VSS", "GND_A", "GND_D", "AGND", "DGND", "VBAT", "VBACKUP",
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }

    /// Add a net to the connectivity.
    pub fn add_net(&mut self, id: NetId, name: String) {
        let class = classify_net(&name);
        let net = Net {
            id,
            name: name.clone(),
            class,
            terminals: Vec::new(),
            attributes: HashMap::new(),
        };
        self.nets.insert(id, net);
        self.name_to_id.insert(name, id);
    }

    /// Add a terminal (a `(component, pin)` on the net) to the connectivity.
    pub fn add_terminal(&mut self, net_id: NetId, terminal: Terminal) {
        self.pin_to_net
            .insert((terminal.component, terminal.pin), net_id);

        if let Some(net) = self.nets.get_mut(&net_id) {
            net.terminals.push(terminal);
        }
    }

    /// Get the net ID for a component pin.
    pub fn net_for_pin(&self, comp: ComponentId, pin: PinId) -> Option<NetId> {
        self.pin_to_net.get(&(comp, pin)).copied()
    }

    /// Check if two component pins are on the same net.
    pub fn pins_on_same_net(&self, a: (ComponentId, PinId), b: (ComponentId, PinId)) -> bool {
        match (self.net_for_pin(a.0, a.1), self.net_for_pin(b.0, b.1)) {
            (Some(net_a), Some(net_b)) => net_a == net_b,
            _ => false,
        }
    }

    /// Get a net by ID.
    pub fn net(&self, id: NetId) -> Option<&Net> {
        self.nets.get(&id)
    }

    /// Get a net by name.
    pub fn net_by_name(&self, name: &str) -> Option<&Net> {
        self.name_to_id.get(name).and_then(|id| self.nets.get(id))
    }

    /// Get mutable net by ID.
    pub fn net_mut(&mut self, id: NetId) -> Option<&mut Net> {
        self.nets.get_mut(&id)
    }

    /// Iterate all nets.
    pub fn nets(&self) -> impl Iterator<Item = &Net> {
        self.nets.values()
    }

    /// Iterate all net IDs.
    pub fn net_ids(&self) -> impl Iterator<Item = NetId> + '_ {
        self.nets.keys().copied()
    }

    /// Get all terminals on a net.
    pub fn terminals(&self, net_id: NetId) -> Option<&[Terminal]> {
        self.nets.get(&net_id).map(|n| n.terminals.as_slice())
    }

    /// Count terminals on a net.
    pub fn terminal_count(&self, net_id: NetId) -> usize {
        self.nets
            .get(&net_id)
            .map(|n| n.terminals.len())
            .unwrap_or(0)
    }

    /// Merge global/power nets by name across boards (for hierarchical designs).
    ///
    /// Nets whose upper-cased name is in the global set are coalesced:
    /// the first net seen becomes canonical and absorbs the terminals of
    /// every same-named net; the absorbed nets are removed. This replaces
    /// the geometric union-find of GUI EDA tools — the merge is driven by
    /// explicit net identity, not wire geometry.
    ///
    /// `boards` is accepted for signature stability with the multi-board
    /// builder; net identity alone drives the merge today.
    pub fn merge_global_nets(&mut self, _boards: &[&Board]) {
        let canonical: Vec<(String, NetId)> = self
            .global_net_names
            .iter()
            .filter_map(|g| {
                // First same-named net wins as canonical; skip singletons.
                let mut same = self
                    .nets
                    .values()
                    .filter(|n| n.name.to_uppercase() == *g)
                    .collect::<Vec<_>>();
                if same.len() < 2 {
                    return None;
                }
                same.sort_by_key(|n| n.id.0);
                let canonical_id = same[0].id;
                Some((g.clone(), canonical_id))
            })
            .collect();

        for (_name, canonical_id) in canonical {
            // Absorb all other same-named nets into the canonical one.
            let absorbed: Vec<NetId> = self
                .nets
                .values()
                .filter(|n| n.id != canonical_id)
                .filter(|n| self.global_net_names.contains(&n.name.to_uppercase()))
                .map(|n| n.id)
                .collect();

            for other in absorbed {
                let other_terminals = self
                    .nets
                    .get(&other)
                    .map(|n| n.terminals.clone())
                    .unwrap_or_default();

                for t in &other_terminals {
                    self.pin_to_net.insert((t.component, t.pin), canonical_id);
                }

                if let Some(canon) = self.nets.get_mut(&canonical_id) {
                    canon.terminals.extend(other_terminals);
                }
                self.nets.remove(&other);
            }
        }
    }

    /// Get all pins on a specific net (for ERC rules).
    pub fn pins_on_net(&self, net_id: NetId) -> Vec<(ComponentId, PinId)> {
        self.nets
            .get(&net_id)
            .map(|n| n.terminals.iter().map(|t| (t.component, t.pin)).collect())
            .unwrap_or_default()
    }

    /// Check if a net has any power output pin (for power source validation).
    pub fn net_has_power_output(&self, net_id: NetId, board: &Board) -> bool {
        self.terminals(net_id).unwrap_or_default().iter().any(|t| {
            board
                .component(t.component)
                .and_then(|c| c.part.as_ref())
                .and_then(|p| p.pins.get(t.pin.0 as usize))
                .is_some_and(|pin| {
                    pin.electrical_type == synth_registry::ElectricalType::PowerOutput
                })
        })
    }

    /// Check if a net has any power input pin.
    pub fn net_has_power_input(&self, net_id: NetId, board: &Board) -> bool {
        self.terminals(net_id).unwrap_or_default().iter().any(|t| {
            board
                .component(t.component)
                .and_then(|c| c.part.as_ref())
                .and_then(|p| p.pins.get(t.pin.0 as usize))
                .is_some_and(|pin| {
                    pin.electrical_type == synth_registry::ElectricalType::PowerInput
                })
        })
    }

    /// Get all nets of a specific class.
    pub fn nets_of_class(&self, class: NetClass) -> Vec<&Net> {
        self.nets.values().filter(|n| n.class == class).collect()
    }
}

impl Default for Connectivity {
    fn default() -> Self {
        Self::new()
    }
}

/// Classify a net by its name.
fn classify_net(name: &str) -> NetClass {
    let upper = name.to_uppercase();

    // Power nets
    if upper.starts_with("VCC")
        || upper.starts_with("VDD")
        || upper.starts_with("VCC_")
        || upper.starts_with("VDD_")
        || upper == "VBAT"
        || upper == "VBACKUP"
        || upper == "3V3"
        || upper == "3V"
        || upper == "5V"
        || upper == "12V"
        || upper == "1V8"
        || upper == "1V2"
        || upper == "2V5"
    {
        return NetClass::Power;
    }

    // Ground nets
    if upper == "GND"
        || upper == "VSS"
        || upper.starts_with("GND_")
        || upper.starts_with("VSS_")
        || upper == "AGND"
        || upper == "DGND"
    {
        return NetClass::Ground;
    }

    // Clock nets
    if upper.contains("CLK")
        || upper.contains("CLK_")
        || upper.ends_with("_CLK")
        || upper == "XTAL"
        || upper == "OSC"
    {
        return NetClass::Clock;
    }

    // Differential pairs
    if upper.contains("DIFF")
        || upper.contains("_DP")
        || upper.contains("_DN")
        || upper.contains("USB_D")
        || upper.contains("PCIE_")
        || upper.contains("SATA_")
    {
        return NetClass::Differential;
    }

    // RF nets
    if upper.contains("RF_")
        || upper.contains("ANT")
        || upper.contains("WIFI")
        || upper.contains("BT_")
        || upper.contains("LORA")
    {
        return NetClass::RF;
    }

    // Analog nets
    if upper.contains("ADC")
        || upper.contains("DAC")
        || upper.contains("AIN")
        || upper.contains("AOUT")
    {
        return NetClass::Analog;
    }

    NetClass::Signal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_classification() {
        assert_eq!(classify_net("3V3"), NetClass::Power);
        assert_eq!(classify_net("VDD"), NetClass::Power);
        assert_eq!(classify_net("GND"), NetClass::Ground);
        assert_eq!(classify_net("VSS"), NetClass::Ground);
        assert_eq!(classify_net("CLK"), NetClass::Clock);
        assert_eq!(classify_net("USB_DP"), NetClass::Differential);
        assert_eq!(classify_net("RF_ANT"), NetClass::RF);
        assert_eq!(classify_net("ADC_IN"), NetClass::Analog);
        assert_eq!(classify_net("SIGNAL"), NetClass::Signal);
    }

    #[test]
    fn pin_lookup_and_same_net() {
        let mut conn = Connectivity::new();
        conn.add_net(NetId(1), "SIGNAL".to_string());
        conn.add_net(NetId(2), "3V3".to_string());

        conn.add_terminal(
            NetId(1),
            Terminal {
                component: ComponentId(1),
                pin: PinId(0),
                position: None,
            },
        );
        conn.add_terminal(
            NetId(1),
            Terminal {
                component: ComponentId(2),
                pin: PinId(0),
                position: None,
            },
        );
        conn.add_terminal(
            NetId(2),
            Terminal {
                component: ComponentId(1),
                pin: PinId(2),
                position: None,
            },
        );

        assert_eq!(conn.net_for_pin(ComponentId(1), PinId(0)), Some(NetId(1)));
        assert!(conn.pins_on_same_net((ComponentId(1), PinId(0)), (ComponentId(2), PinId(0))));
        assert!(!conn.pins_on_same_net((ComponentId(1), PinId(0)), (ComponentId(1), PinId(2))));
        assert_eq!(conn.terminal_count(NetId(1)), 2);
    }

    #[test]
    fn global_net_merge() {
        let mut conn = Connectivity::new();
        conn.add_net(NetId(1), "3V3".to_string());
        conn.add_net(NetId(2), "3V3".to_string());
        conn.add_terminal(
            NetId(1),
            Terminal {
                component: ComponentId(1),
                pin: PinId(0),
                position: None,
            },
        );
        conn.add_terminal(
            NetId(2),
            Terminal {
                component: ComponentId(2),
                pin: PinId(0),
                position: None,
            },
        );

        conn.merge_global_nets(&[]);

        // Both pins now resolve to the canonical net id (lowest = 1).
        assert_eq!(conn.net_for_pin(ComponentId(1), PinId(0)), Some(NetId(1)));
        assert_eq!(conn.net_for_pin(ComponentId(2), PinId(0)), Some(NetId(1)));
        assert_eq!(conn.terminal_count(NetId(1)), 2);
        assert!(conn.net(NetId(2)).is_none());
    }
}
