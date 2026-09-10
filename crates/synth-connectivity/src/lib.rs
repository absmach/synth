// SPDX-License-Identifier: Apache-2.0

//! Authoritative connectivity derivation for Synth.
//!
//! This crate provides a single source of truth for electrical connectivity
//! across the Synth pipeline: ERC validation, placement, routing, DRC,
//! and netlist export all consume the same [`Connectivity`] derivation.
//!
//! Unlike GUI EDA tools, Synth's connectivity is explicit
//! in the IR — `connect` statements create explicit net endpoints. We
//! don't need geometric union-find over wires because the netlist is
//! already derived by the compiler. This crate wraps the IR netlist with
//! convenient query methods for downstream passes.

#![forbid(unsafe_code)]

pub mod netlist;

use synth_ir::{Board, NetId};

pub use netlist::{Connectivity, Net, NetClass, Terminal};

/// Build the authoritative connectivity from a board's explicit netlist.
/// This is the single connectivity derivation used by ERC, placement, routing,
/// DRC, and export.
pub fn build_connectivity(board: &Board) -> Connectivity {
    let mut conn = Connectivity::new();

    for net in &board.nets {
        conn.add_net(net.id, net.name.clone());

        // Record terminals for this net (topology-only: component + pin)
        for endpoint in &net.endpoints {
            conn.add_terminal(
                net.id,
                Terminal {
                    component: endpoint.component,
                    pin: endpoint.pin,
                    position: None, // Position available after placement via layout
                },
            );
        }
    }

    conn
}

/// Build project-level connectivity for hierarchical/multi-board designs.
/// Stitches together multiple board connectivities via global nets,
/// power nets, and explicit cross-board connections.
pub fn build_project_connectivity(boards: &[&Board]) -> Connectivity {
    let mut conn = Connectivity::new();

    for (board_idx, board) in boards.iter().enumerate() {
        let ns = format!("b{board_idx}_");

        for net in &board.nets {
            let global_net_id = NetId((board_idx as u32) << 24 | net.id.0);
            conn.add_net(global_net_id, format!("{ns}{}", net.name));

            for endpoint in &net.endpoints {
                conn.add_terminal(
                    global_net_id,
                    Terminal {
                        component: endpoint.component,
                        pin: endpoint.pin,
                        position: None,
                    },
                );
            }
        }
    }

    conn.merge_global_nets(boards);

    conn
}

/// Build connectivity with pin positions from a placement layout.
/// Positions (in mm) come from the layout's component placements;
/// net topology is unchanged from [`build_connectivity`].
pub fn build_connectivity_with_layout(
    board: &Board,
    layout: &synth_layout::Layout,
) -> Connectivity {
    let mut conn = Connectivity::new();

    for net in &board.nets {
        conn.add_net(net.id, net.name.clone());

        for endpoint in &net.endpoints {
            let position = layout.placement(endpoint.component).map(|p| p.center_mm);
            conn.add_terminal(
                net.id,
                Terminal {
                    component: endpoint.component,
                    pin: endpoint.pin,
                    position,
                },
            );
        }
    }

    conn
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_diagnostics::Span;
    use synth_ir::{Board, ComponentId, Net, NetEndpoint, PinId};

    fn make_test_board() -> Board {
        let net1 = Net {
            id: NetId(1),
            name: "SIGNAL".into(),
            endpoints: vec![
                NetEndpoint {
                    component: ComponentId(1),
                    pin: PinId(0),
                    source_span: Span::new(0, 0),
                },
                NetEndpoint {
                    component: ComponentId(2),
                    pin: PinId(0),
                    source_span: Span::new(0, 0),
                },
            ],
        };
        let net2 = Net {
            id: NetId(2),
            name: "3V3".into(),
            endpoints: vec![
                NetEndpoint {
                    component: ComponentId(1),
                    pin: PinId(2),
                    source_span: Span::new(0, 0),
                },
                NetEndpoint {
                    component: ComponentId(2),
                    pin: PinId(1),
                    source_span: Span::new(0, 0),
                },
            ],
        };

        Board {
            name: "test".into(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components: vec![],
            nets: vec![net1, net2],
            diff_pairs: vec![],
            keepouts: vec![],
            source_span: Span::new(0, 0),
        }
    }

    #[test]
    fn connectivity_groups_pins_on_same_net() {
        let board = make_test_board();
        let conn = build_connectivity(&board);

        // U1.PA0 and R1.1 should be on same net (SIGNAL)
        assert!(conn.pins_on_same_net((ComponentId(1), PinId(0)), (ComponentId(2), PinId(0))));

        // U1.VDD and R1.2 should be on same net (3V3)
        assert!(conn.pins_on_same_net((ComponentId(1), PinId(2)), (ComponentId(2), PinId(1))));

        // SIGNAL and 3V3 should be different
        assert!(!conn.pins_on_same_net((ComponentId(1), PinId(0)), (ComponentId(1), PinId(2))));
    }

    #[test]
    fn connectivity_terminals_recorded() {
        let board = make_test_board();
        let conn = build_connectivity(&board);

        let signal_net = conn.net_by_name("SIGNAL").expect("SIGNAL net exists");
        assert_eq!(signal_net.terminals.len(), 2);

        let vdd_net = conn.net_by_name("3V3").expect("3V3 net exists");
        assert_eq!(vdd_net.terminals.len(), 2);
    }
}
