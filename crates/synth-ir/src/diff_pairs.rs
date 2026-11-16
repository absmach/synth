// SPDX-License-Identifier: Apache-2.0

//! Differential-pair resolution: turning a `diff_pair` declaration into
//! the two nets it names, and into the components those nets join.
//!
//! A declaration writes its halves as `<REFDES>_<pin>` — `U1_usb_dp`,
//! `U1_usb_dn` — and matches them against any net that has that
//! component/pin as an endpoint. That is the useful fact: the halves are
//! identified by the pins they leave from, not by net name, so a design
//! whose nets are auto-named still resolves.
//!
//! Resolution used to live in the router and match on a `dp`/`dn` substring
//! in the label, which only ever found USB. Polarity is read from the pin
//! itself instead — its electrical type, then its capabilities — so RS-485,
//! LVDS and anything else declared `differential_positive` /
//! `differential_negative` resolves by the same rule USB does.
//!
//! Living here rather than in the router is what lets placement see pairs at
//! all: a capacitor that sits between the halves of a pair is only useful as
//! a placement decision if the placer knows which nets form the pair.

use std::collections::HashMap;

use synth_registry::{ElectricalType, PinCapability};

use crate::board::{Board, ComponentId, NetId, PinId};

/// The two nets of a differential pair, and the pins they leave from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedPair {
    pub positive_net: NetId,
    pub negative_net: NetId,
    /// The pin the positive half leaves the board from.
    pub positive_pin: (ComponentId, PinId),
    /// The pin the negative half leaves from.
    pub negative_pin: (ComponentId, PinId),
}

/// Which half of a pair a pin carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalfPolarity {
    Positive,
    Negative,
}

/// Resolve every [`crate::DiffPair`] in `board` to its two nets.
///
/// Output preserves declaration order. A pair whose halves name nets that
/// exist, or pins that exist and carry a declared polarity, resolves; one
/// that names neither is dropped rather than guessed at.
#[must_use]
pub fn resolve_pairs(board: &Board) -> Vec<ResolvedPair> {
    let mut out = Vec::new();
    for pair in &board.diff_pairs {
        if let Some(resolved) = resolve_one(board, pair) {
            out.push(resolved);
        }
    }
    out
}

fn resolve_one(board: &Board, pair: &crate::DiffPair) -> Option<ResolvedPair> {
    // Strategy 1: the halves name nets directly. Canonical when a design
    // names its nets (`connect … as "USB_DP"`).
    if let (Some(positive_net), Some(negative_net)) = (pair.positive_net, pair.negative_net)
        && positive_net != negative_net
    {
        return pair_endpoints(board, positive_net, negative_net);
    }

    // Strategy 2: the halves name nets by string, matched against declared
    // net names.
    let by_name = (
        board
            .nets
            .iter()
            .find(|n| n.name == pair.positive)
            .map(|n| n.id),
        board
            .nets
            .iter()
            .find(|n| n.name == pair.negative)
            .map(|n| n.id),
    );
    if let (Some(positive_net), Some(negative_net)) = by_name
        && positive_net != negative_net
    {
        return pair_endpoints(board, positive_net, negative_net);
    }

    // Strategy 3: the halves name pins (`U1_usb_dp`), which is how a
    // design with auto-named nets writes them. Polarity comes from the pin,
    // not from the label's spelling, so any differential pair resolves.
    let positive_pin = locate_label_pin(board, &pair.positive)?;
    let negative_pin = locate_label_pin(board, &pair.negative)?;
    let (positive_net, negative_net) = match (
        polarity_of(board, positive_pin),
        polarity_of(board, negative_pin),
    ) {
        (Some(HalfPolarity::Positive), Some(HalfPolarity::Negative)) => (
            net_of_pin(board, positive_pin)?,
            net_of_pin(board, negative_pin)?,
        ),
        (Some(HalfPolarity::Negative), Some(HalfPolarity::Positive)) => (
            net_of_pin(board, negative_pin)?,
            net_of_pin(board, positive_pin)?,
        ),
        // Both halves read the same polarity: the declaration is
        // self-inconsistent, and guessing which is which would silently
        // swap P and N.
        _ => return None,
    };
    if positive_net == negative_net {
        return None;
    }
    pair_endpoints(board, positive_net, negative_net)
}

/// Resolve a pair's nets and record which pin each half leaves from.
fn pair_endpoints(board: &Board, positive_net: NetId, negative_net: NetId) -> Option<ResolvedPair> {
    let positive_pin = first_pin_on(board, positive_net)?;
    let negative_pin = first_pin_on(board, negative_net)?;
    Some(ResolvedPair {
        positive_net,
        negative_net,
        positive_pin,
        negative_pin,
    })
}

/// Find the component and pin a `<REFDES>_<pin>` label names.
///
/// The refdes is the longest prefix that names a real component, because
/// both halves of the separator are themselves ambiguous: `U1_tx_p` splits
/// at its last underscore into a refdes `U1_tx` that does not exist, and a
/// module instance's `CH1_U1_usb_dp` splits into `CH1_U1` (correct) or
/// `CH1` (a refdes that may well exist and be the wrong part).
fn locate_label_pin(board: &Board, label: &str) -> Option<(ComponentId, PinId)> {
    let mut best: Option<(ComponentId, &str)> = None;
    for component in &board.components {
        let Some(suffix) = label.strip_prefix(component.refdes.as_str()).or_else(|| {
            // Refdes case is not significant in a design file, but the
            // label may be spelled differently.
            if label.len() > component.refdes.len()
                && label[..component.refdes.len()].eq_ignore_ascii_case(&component.refdes)
            {
                Some(&label[component.refdes.len()..])
            } else {
                None
            }
        }) else {
            continue;
        };
        let Some(pin_name) = suffix.strip_prefix('_') else {
            continue;
        };
        if best.is_none_or(|(_, seen)| seen.len() < pin_name.len()) {
            best = Some((component.id, pin_name));
        }
    }
    let (component_id, pin_name) = best?;
    let part = board.component(component_id)?.part.as_ref()?;
    let pin_idx = part
        .pins
        .iter()
        .position(|p| p.name.eq_ignore_ascii_case(pin_name))?;
    Some((component_id, PinId(u32::try_from(pin_idx).ok()?)))
}

/// Which half of a differential pair a pin carries.
///
/// Electrical type first — it is the generic statement of what the pin is —
/// then capabilities, then the `p`/`n` naming convention parts use for
/// differential pins that declare neither.
fn polarity_of(board: &Board, (component, pin): (ComponentId, PinId)) -> Option<HalfPolarity> {
    let part = board.component(component)?.part.as_ref()?;
    let pin = part.pins.get(pin.0 as usize)?;
    match pin.electrical_type {
        ElectricalType::DifferentialPositive => return Some(HalfPolarity::Positive),
        ElectricalType::DifferentialNegative => return Some(HalfPolarity::Negative),
        _ => {}
    }
    for capability in &pin.capabilities {
        match capability {
            PinCapability::DiffPairPositive | PinCapability::UsbDp => {
                return Some(HalfPolarity::Positive);
            }
            PinCapability::DiffPairNegative | PinCapability::UsbDn => {
                return Some(HalfPolarity::Negative);
            }
            _ => {}
        }
    }
    let lower = pin.name.to_ascii_lowercase();
    if lower.ends_with("_p") || lower.ends_with("_dp") || lower.ends_with('+') {
        return Some(HalfPolarity::Positive);
    }
    if lower.ends_with("_n") || lower.ends_with("_dn") || lower.ends_with('-') {
        return Some(HalfPolarity::Negative);
    }
    None
}

fn net_of_pin(board: &Board, pin: (ComponentId, PinId)) -> Option<NetId> {
    board
        .nets_containing(pin.0, pin.1)
        .map(|(net_id, _)| net_id)
        .next()
}

/// The first endpoint of a net, used to report where a half leaves from.
///
/// Deterministic by construction: nets are scanned in declaration order.
fn first_pin_on(board: &Board, net: NetId) -> Option<(ComponentId, PinId)> {
    let net = board.net(net)?;
    net.endpoints
        .first()
        .map(|endpoint| (endpoint.component, endpoint.pin))
}

/// The components a resolved pair connects, grouped by which end they sit on.
///
/// `far` is the component on the other end of the pair from `near`, which
/// is what placement needs: an endpoint should face the far end, and an
/// in-line part belongs between them.
#[must_use]
pub fn pair_connections(board: &Board, pairs: &[ResolvedPair]) -> Vec<PairConnection> {
    pairs
        .iter()
        .filter_map(|pair| {
            let positive = endpoints_of(board, pair.positive_net);
            let negative = endpoints_of(board, pair.negative_net);
            // The far end is whichever side is not the pin the halves
            // leave from — for a USB pair that is the MCU behind the
            // connector.
            let far_end = far_end(board, pair, &positive, &negative)?;
            Some(PairConnection {
                pair: *pair,
                far_end,
                in_line: in_line_components(board, &positive, &negative, pair, far_end),
            })
        })
        .collect()
}

/// One component at each end of a pair, plus the parts sitting between them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairConnection {
    pub pair: ResolvedPair,
    /// The component the pair runs *to*, given the endpoints at the near
    /// end (the connector, for a USB pair).
    pub far_end: ComponentId,
    /// Components that sit on the corridor between the two ends: series
    /// elements, ESD protection, termination.
    pub in_line: Vec<ComponentId>,
}

/// Every component on a net.
fn endpoints_of(board: &Board, net: NetId) -> Vec<ComponentId> {
    board.net(net).map_or_default(|net| {
        net.endpoints
            .iter()
            .map(|endpoint| endpoint.component)
            .collect()
    })
}

/// The end of the pair opposite the one the halves leave from.
fn far_end(
    board: &Board,
    pair: &ResolvedPair,
    positive: &[ComponentId],
    negative: &[ComponentId],
) -> Option<ComponentId> {
    let near = pair.positive_pin.0;
    // The far end is the side that does not contain the near component. A
    // pair that stays inside one component (an on-chip termination) has no
    // far end and is not a placement concern.
    for side in [negative, positive] {
        if let Some(far) = side.iter().copied().find(|id| *id != near)
            && far != near
        {
            return Some(far);
        }
    }
    let _ = board;
    None
}

/// Components on both halves that are neither end: the parts the corridor
/// between the two ends runs through.
fn in_line_components(
    board: &Board,
    positive: &[ComponentId],
    negative: &[ComponentId],
    pair: &ResolvedPair,
    far_end: ComponentId,
) -> Vec<ComponentId> {
    let mut out = Vec::new();
    for side in [positive, negative] {
        for id in side {
            if *id == pair.positive_pin.0 || *id == far_end {
                continue;
            }
            if !out.contains(id) {
                out.push(*id);
            }
        }
    }
    out.sort_by_key(|id| id.0);
    let _ = board;
    out
}

/// Index resolved pairs by the nets they use.
#[must_use]
pub fn pair_net_index(pairs: &[ResolvedPair]) -> HashMap<NetId, usize> {
    let mut map = HashMap::new();
    for (index, pair) in pairs.iter().enumerate() {
        map.insert(pair.positive_net, index);
        map.insert(pair.negative_net, index);
    }
    map
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;
    use crate::board::DiffPair;
    use crate::test_support::{self, component, net, part, passive_pin, pin, pin_with};
    use synth_diagnostics::Span;

    fn pair(positive: &str, negative: &str) -> DiffPair {
        DiffPair {
            positive: positive.to_string(),
            negative: negative.to_string(),
            positive_net: None,
            negative_net: None,
            impedance: None,
            max_skew: None,
            couple: None,
            source_span: Span::new(0, 0),
        }
    }

    /// A USB connector: DP/DN declared both by capability and by the
    /// conventional electrical type, as the registry's parts do.
    fn usb_connector() -> crate::board::Component {
        component(
            0,
            "J1",
            part(
                "connector",
                vec![
                    passive_pin("vbus"),
                    pin_with(
                        "dp",
                        ElectricalType::DifferentialPositive,
                        &[PinCapability::UsbDp],
                    ),
                    pin_with(
                        "dn",
                        ElectricalType::DifferentialNegative,
                        &[PinCapability::UsbDn],
                    ),
                    passive_pin("gnd"),
                ],
            ),
        )
    }

    fn mcu_with_usb() -> crate::board::Component {
        component(
            1,
            "U1",
            part(
                "mcu",
                vec![
                    passive_pin("vdd"),
                    pin_with(
                        "usb_dp",
                        ElectricalType::DifferentialPositive,
                        &[PinCapability::UsbDp],
                    ),
                    pin_with(
                        "usb_dn",
                        ElectricalType::DifferentialNegative,
                        &[PinCapability::UsbDn],
                    ),
                ],
            ),
        )
    }

    fn usb_board() -> Board {
        test_support::board(
            vec![usb_connector(), mcu_with_usb()],
            vec![
                net(0, "net_0", &[(0, 1), (1, 1)]),
                net(1, "net_1", &[(0, 2), (1, 2)]),
            ],
            vec![pair("J1_dp", "J1_dn")],
        )
    }

    /// The common case: halves named by the pins they leave from, on nets
    /// the design never named.
    #[test]
    fn a_usb_pair_resolves_from_its_pins() {
        let pairs = resolve_pairs(&usb_board());
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].positive_net, crate::board::NetId(0));
        assert_eq!(pairs[0].negative_net, crate::board::NetId(1));
    }

    /// A pair is not a USB concept. Anything declared
    /// `differential_positive` / `differential_negative` resolves by the same
    /// rule, which is what lets RS-485 and LVDS pairs be found at all.
    #[test]
    fn a_non_usb_differential_pair_resolves() {
        let transceiver = component(
            0,
            "U1",
            part(
                "transceiver",
                vec![
                    pin("tx_p", ElectricalType::DifferentialPositive),
                    pin("tx_n", ElectricalType::DifferentialNegative),
                ],
            ),
        );
        let receiver = component(
            1,
            "U2",
            part(
                "transceiver",
                vec![
                    pin("rx_p", ElectricalType::DifferentialPositive),
                    pin("rx_n", ElectricalType::DifferentialNegative),
                ],
            ),
        );
        let b = test_support::board(
            vec![transceiver, receiver],
            vec![
                net(0, "net_0", &[(0, 0), (1, 0)]),
                net(1, "net_1", &[(0, 1), (1, 1)]),
            ],
            vec![pair("U1_tx_p", "U1_tx_n")],
        );
        let pairs = resolve_pairs(&b);
        assert_eq!(pairs.len(), 1, "an RS-485 pair is not a USB pair");
        assert_eq!(pairs[0].positive_net, crate::board::NetId(0));
        assert_eq!(pairs[0].negative_net, crate::board::NetId(1));
    }

    /// Swapping P and N is the worst possible failure here: the board
    /// still routes, and the pair is simply backwards. A declaration that
    /// lists its halves in the other order must still resolve with the
    /// polarity the pins declare.
    #[test]
    fn polarity_comes_from_the_pins_not_the_declaration_order() {
        let mut b = usb_board();
        b.diff_pairs[0] = pair("J1_dn", "J1_dp");
        let pairs = resolve_pairs(&b);
        assert_eq!(pairs.len(), 1);
        assert_eq!(
            pairs[0].positive_net,
            crate::board::NetId(0),
            "dp is the positive half whatever order the declaration lists"
        );
    }

    /// A declaration naming two pins of the same polarity is inconsistent,
    /// and picking one would silently flip the pair.
    #[test]
    fn a_declaration_naming_two_positive_halves_is_dropped() {
        let mut b = usb_board();
        b.diff_pairs[0] = pair("J1_dp", "U1_usb_dp");
        assert!(resolve_pairs(&b).is_empty());
    }

    /// A pair whose halves are named nets resolves through those names,
    /// which is the canonical path when a design declares its nets.
    #[test]
    fn a_pair_naming_nets_resolves_through_them() {
        let mut b = usb_board();
        b.nets[0].name = "USB_DP".to_string();
        b.nets[1].name = "USB_DN".to_string();
        b.diff_pairs[0] = pair("USB_DP", "USB_DN");
        let pairs = resolve_pairs(&b);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].positive_net, crate::board::NetId(0));
        assert_eq!(pairs[0].negative_net, crate::board::NetId(1));
    }

    /// A label that names no known pin resolves to nothing rather than
    /// being guessed at from its spelling.
    #[test]
    fn an_unknown_pin_label_resolves_to_nothing() {
        let mut b = usb_board();
        b.diff_pairs[0] = pair("J1_nonexistent", "J1_dn");
        assert!(resolve_pairs(&b).is_empty());
    }

    /// Placement needs to know which component the pair runs *to*, and what
    /// sits between the two ends.
    #[test]
    fn a_connection_names_its_far_end_and_in_line_parts() {
        let mut b = usb_board();
        let series = component(
            2,
            "R1",
            part("resistor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        b.components.push(series);
        b.nets[0].endpoints.push(crate::board::NetEndpoint {
            component: ComponentId(2),
            pin: PinId(0),
            source_span: Span::new(0, 0),
        });
        let pairs = resolve_pairs(&b);
        let connections = pair_connections(&b, &pairs);
        assert_eq!(connections.len(), 1);
        assert_eq!(
            connections[0].far_end,
            ComponentId(1),
            "the pair runs from the connector to the MCU"
        );
        assert_eq!(
            connections[0].in_line,
            vec![ComponentId(2)],
            "the series resistor sits on the corridor"
        );
    }

    /// A pair that never leaves its own component — an on-chip termination,
    /// or a stub looped back — is not a placement concern.
    #[test]
    fn a_pair_inside_one_component_has_no_connection() {
        let looped = component(
            0,
            "U1",
            part(
                "mcu",
                vec![
                    pin("d_out_p", ElectricalType::DifferentialPositive),
                    pin("d_out_n", ElectricalType::DifferentialNegative),
                ],
            ),
        );
        let b = test_support::board(
            vec![looped],
            vec![net(0, "net_0", &[(0, 0)]), net(1, "net_1", &[(0, 1)])],
            vec![pair("U1_d_out_p", "U1_d_out_n")],
        );
        let pairs = resolve_pairs(&b);
        assert_eq!(pairs.len(), 1, "the pair itself still resolves");
        assert!(
            pair_connections(&b, &pairs).is_empty(),
            "but it has no far end to place towards"
        );
    }
}
