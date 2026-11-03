// SPDX-License-Identifier: Apache-2.0

//! Repeated-circuit detection for the placement review.
//!
//! Two output drivers, three identical input stages, four LED rows: a board
//! often holds several copies of one small circuit, and a reader expects the
//! copies to be laid out alike. This module finds them by structure alone,
//! with no part named: the board is cut at the large ICs, at the connectors
//! and at the supply rails, the small parts that remain and share a signal net
//! form an *island*, and each island gets a fingerprint from its part types,
//! its internal wiring and the roles of the pins it is wired to outside.
//! Islands with equal fingerprints are copies.
//!
//! What it decides is membership and the part-for-part correspondence between
//! two copies. Where the parts sit is the caller's business.

use std::collections::{HashMap, HashSet};

use synth_ir::{Board, ComponentId};

/// A part with more pins than this is a hub (an MCU, a regulator, a sensor): the
/// board is cut there, so circuits wired to one hub do not merge into one. A
/// connector or jack is a hub whatever its pin count, since two circuits on two
/// pins of one small connector are still two circuits.
const HUB_PINS: usize = 5;

/// Nets with more endpoints than this are distribution rails, not signals.
const RAIL_FANOUT: usize = 8;

/// Colour-refinement rounds. Enough to tell apart circuits a few parts across.
const ROUNDS: usize = 4;

/// One copy of a repeated circuit.
#[derive(Debug, Clone)]
pub(crate) struct Island {
    /// The part the copy is positioned from: its connector, else its largest part.
    pub anchor: ComponentId,
    /// Every part in the copy, with a label that matches across copies.
    pub parts: Vec<(ComponentId, u64)>,
}

fn hash_of<T: std::hash::Hash>(value: &T) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn is_hub(board: &Board, id: ComponentId) -> bool {
    board.component(id).is_some_and(|c| {
        matches!(c.kind.as_str(), "connector" | "jack")
            || c.part
                .as_ref()
                .is_some_and(|part| part.pins.len() > HUB_PINS)
    })
}

/// A supply or ground net, by the project's own classification (the power
/// domains and the pin types), or one so widely shared that it distributes
/// power whatever it is called.
fn is_rail(board: &Board, domains: &synth_ir::PowerDomainMap, net: &synth_ir::Net) -> bool {
    crate::is_supply_net(board, domains, net) || net.endpoints.len() > RAIL_FANOUT
}

/// A bus function a general-purpose pin can be multiplexed to. Which of the
/// several I2C, SPI or UART pins a circuit is wired to does not change what the
/// circuit is, so these do not separate one pin's role from another's.
fn is_bus_option(capability: synth_registry::PinCapability) -> bool {
    use synth_registry::PinCapability as C;
    matches!(
        capability,
        C::SpiMosi
            | C::SpiMiso
            | C::SpiSck
            | C::SpiCs
            | C::I2cSda
            | C::I2cScl
            | C::UartTx
            | C::UartRx
    )
}

/// The role a list of pin capabilities gives a pin: the sorted set of every
/// capability that is not a bus option (so `gpio` and `gpio` + `analog_input`
/// differ, and the order in the part file does not matter), or the first
/// capability when a pin has nothing else.
fn capability_role(capabilities: &[synth_registry::PinCapability]) -> Option<String> {
    let mut functions: Vec<String> = capabilities
        .iter()
        .filter(|c| !is_bus_option(**c))
        .map(|c| format!("{c:?}"))
        .collect();
    functions.sort();
    functions.dedup();
    if functions.is_empty() {
        capabilities.first().map(|c| format!("{c:?}"))
    } else {
        Some(functions.join("+"))
    }
}

/// What a pin is for, as the part declares it: its capability role, else its
/// electrical type. Two copies of a circuit wired to a hub are the same circuit
/// only if they are wired to the same kind of pin of the same hub part.
fn pin_role(board: &Board, endpoint: &synth_ir::NetEndpoint) -> String {
    board.pin(endpoint.component, endpoint.pin).map_or_else(
        || endpoint.pin.0.to_string(),
        |pin| {
            capability_role(&pin.capabilities)
                .unwrap_or_else(|| format!("{:?}", pin.electrical_type))
        },
    )
}

/// The hub part and pin role of an endpoint outside the island.
fn boundary_role(board: &Board, endpoint: &synth_ir::NetEndpoint) -> (String, String) {
    let part = board
        .component(endpoint.component)
        .and_then(|c| c.part.as_ref())
        .map_or(String::new(), |p| p.id.0.clone());
    (part, pin_role(board, endpoint))
}

fn pin_name(board: &Board, endpoint: &synth_ir::NetEndpoint) -> String {
    board
        .component(endpoint.component)
        .and_then(|c| c.part.as_ref())
        .and_then(|p| p.pins.get(endpoint.pin.0 as usize))
        .map_or_else(|| endpoint.pin.0.to_string(), |pin| pin.name.clone())
}

/// Union-find root, with path halving.
fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// Every group of two or more islands with the same fingerprint, each group
/// ordered by its first part's refdes so the first copy is the reference.
pub(crate) fn find_repeated_circuits(board: &Board) -> Vec<Vec<Island>> {
    let domains = synth_ir::infer_power_domains(board);
    // Small parts, linked through non-rail nets.
    let small: Vec<ComponentId> = board
        .components
        .iter()
        .filter(|c| c.part.is_some() && !is_hub(board, c.id))
        .map(|c| c.id)
        .collect();
    let index: HashMap<ComponentId, usize> =
        small.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut parent: Vec<usize> = (0..small.len()).collect();
    let signal_nets: Vec<&synth_ir::Net> = board
        .nets
        .iter()
        .filter(|net| !is_rail(board, &domains, net))
        .collect();
    for net in &signal_nets {
        let members: Vec<usize> = net
            .endpoints
            .iter()
            .filter_map(|ep| index.get(&ep.component).copied())
            .collect();
        for pair in members.windows(2) {
            let (a, b) = (find(&mut parent, pair[0]), find(&mut parent, pair[1]));
            if a != b {
                parent[a] = b;
            }
        }
    }
    let mut islands: HashMap<usize, Vec<ComponentId>> = HashMap::new();
    for (i, id) in small.iter().enumerate() {
        let root = find(&mut parent, i);
        islands.entry(root).or_default().push(*id);
    }
    let mut islands: Vec<Vec<ComponentId>> = islands
        .into_values()
        .filter(|parts| parts.len() >= 2)
        .collect();
    for parts in &mut islands {
        parts.sort_by_key(|id| id.0);
    }
    islands.sort_by_key(|parts| parts[0].0);

    let mut by_fingerprint: HashMap<u64, Vec<Island>> = HashMap::new();
    let mut order: Vec<u64> = Vec::new();
    for parts in islands {
        let island: HashSet<ComponentId> = parts.iter().copied().collect();
        let mut label: HashMap<ComponentId, u64> = parts
            .iter()
            .map(|id| {
                let part = board
                    .component(*id)
                    .and_then(|c| c.part.as_ref())
                    .map_or(String::new(), |p| p.id.0.clone());
                (*id, hash_of(&part))
            })
            .collect();
        // The nets that touch the island, and how each leaves it.
        let touching: Vec<&synth_ir::Net> = board
            .nets
            .iter()
            .filter(|net| {
                net.endpoints
                    .iter()
                    .any(|ep| island.contains(&ep.component))
            })
            .collect();
        for _ in 0..ROUNDS {
            let net_label: Vec<u64> = touching
                .iter()
                .map(|net| {
                    let mut inside: Vec<(String, u64)> = net
                        .endpoints
                        .iter()
                        .filter(|ep| island.contains(&ep.component))
                        .map(|ep| (pin_name(board, ep), label[&ep.component]))
                        .collect();
                    inside.sort();
                    // Leaving the island by a rail is one thing, by a signal
                    // another; which rail is not part of the circuit.
                    let leaves = u8::from(is_rail(board, &domains, net));
                    // Where it leaves a signal net is part of the circuit: the
                    // kind of hub and the role of its pin, not which pin.
                    let mut outside: Vec<(String, String)> = net
                        .endpoints
                        .iter()
                        .filter(|ep| !island.contains(&ep.component))
                        .map(|ep| boundary_role(board, ep))
                        .collect();
                    outside.sort();
                    outside.dedup();
                    if leaves == 1 {
                        outside.clear();
                    }
                    hash_of(&(inside, leaves, outside))
                })
                .collect();
            let mut next = label.clone();
            for id in &parts {
                let mut around: Vec<(String, u64)> = Vec::new();
                for (net, nl) in touching.iter().zip(&net_label) {
                    for ep in net.endpoints.iter().filter(|ep| ep.component == *id) {
                        around.push((pin_name(board, ep), *nl));
                    }
                }
                around.sort();
                next.insert(*id, hash_of(&(label[id], around)));
            }
            label = next;
        }
        let mut sorted: Vec<u64> = label.values().copied().collect();
        sorted.sort_unstable();
        let fingerprint = hash_of(&sorted);
        // Anchor: a connector if there is one, else the part with the most pins.
        let anchor = parts
            .iter()
            .copied()
            .max_by_key(|id| {
                let c = board.component(*id);
                let connector = c.is_some_and(|c| matches!(c.kind.as_str(), "connector" | "jack"));
                let pins = c.and_then(|c| c.part.as_ref()).map_or(0, |p| p.pins.len());
                (u8::from(connector), pins, std::cmp::Reverse(id.0))
            })
            .unwrap_or(parts[0]);
        by_fingerprint
            .entry(fingerprint)
            .or_insert_with(|| {
                order.push(fingerprint);
                Vec::new()
            })
            .push(Island {
                anchor,
                parts: parts.iter().map(|id| (*id, label[id])).collect(),
            });
    }
    order
        .into_iter()
        .filter_map(|fp| by_fingerprint.remove(&fp))
        .filter(|group| group.len() >= 2)
        .collect()
}

impl Island {
    /// The parts of this copy that carry `label`, other than `skip`. Parts
    /// serving different pins of the same hub can carry one label (two equal
    /// resistors on two pins of one connector), so a label names a set; the
    /// caller pairs the sets of two copies.
    pub(crate) fn parts_with_label(&self, label: u64, skip: ComponentId) -> Vec<ComponentId> {
        self.parts
            .iter()
            .filter(|(id, l)| *l == label && *id != skip)
            .map(|(id, _)| *id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use synth_registry::PinCapability as C;

    use super::capability_role;

    #[test]
    fn pins_that_differ_in_a_non_bus_capability_have_different_roles() {
        assert_ne!(
            capability_role(&[C::Gpio]),
            capability_role(&[C::Gpio, C::AnalogInput])
        );
        assert_ne!(
            capability_role(&[C::Gpio, C::I2cSda]),
            capability_role(&[C::Gpio, C::AnalogInput, C::I2cSda])
        );
    }

    #[test]
    fn pins_that_differ_only_in_bus_options_share_a_role() {
        assert_eq!(
            capability_role(&[C::Gpio, C::SpiMosi, C::UartTx, C::I2cSda]),
            capability_role(&[C::Gpio, C::SpiSck, C::UartTx])
        );
    }

    #[test]
    fn the_order_of_capabilities_does_not_change_the_role() {
        assert_eq!(
            capability_role(&[C::AnalogInput, C::Gpio]),
            capability_role(&[C::Gpio, C::AnalogInput])
        );
    }

    #[test]
    fn a_bus_only_pin_keeps_its_first_capability_as_its_role() {
        assert_eq!(capability_role(&[C::I2cSda]), Some("I2cSda".to_string()));
        assert_eq!(capability_role(&[]), None);
    }
}
