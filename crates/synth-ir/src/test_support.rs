// SPDX-License-Identifier: Apache-2.0

//! Builders for hand-written boards in tests.
//!
//! The recognition and pair-resolution passes are tested on boards built
//! here rather than on fixtures, because the interesting cases are small and
//! shaped exactly: one capacitor on a rail, one pair of halves leaving one
//! chip. A fixture would carry a real part's pin numbering and hide the thing
//! under test behind unrelated topology.

#![allow(clippy::cast_possible_truncation)]

use crate::board::{Board, Component, Net, NetEndpoint};
use synth_diagnostics::Span;
use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin, PinNumber, RequiredDecoupling};

/// A pin with no capabilities.
pub fn pin(name: &str, electrical_type: ElectricalType) -> Pin {
    pin_with(name, electrical_type, &[])
}

/// A pin carrying capabilities.
pub fn pin_with(
    name: &str,
    electrical_type: ElectricalType,
    caps: &[synth_registry::PinCapability],
) -> Pin {
    Pin {
        name: name.to_string(),
        number: PinNumber(String::new()),
        electrical_type,
        capabilities: caps.to_vec(),
        required: false,
        footprint_only: false,
        unit: None,
        voltage_max_v: None,
        voltage_min_v: None,
        voltage_nominal_v: None,
    }
}

/// A passive terminal, as a two-pin part's pins are.
pub fn passive_pin(name: &str) -> Pin {
    pin(name, ElectricalType::Passive)
}

pub fn part(kind: &str, pins: Vec<Pin>) -> Part {
    Part {
        id: PartId(kind.to_string()),
        kind: kind.to_string(),
        description: None,
        version: 0,
        lifecycle: Lifecycle::Active,
        signed_by: Vec::new(),
        substitutes: Vec::new(),
        mpn: None,
        lcsc_pn: None,
        provenance: None,
        pins,
        required_decoupling: Vec::new(),
        kicad_symbol: None,
        kicad_footprint: None,
        footprint_dimensions: None,
        operating_conditions: None,
    }
}

/// A part that declares decoupling requirements, as `(net, count)`.
pub fn part_with_decoupling(kind: &str, pins: Vec<Pin>, rules: &[(&str, u32)]) -> Part {
    let mut p = part(kind, pins);
    p.required_decoupling = rules
        .iter()
        .map(|(net, count)| RequiredDecoupling {
            net: (*net).to_string(),
            value: "100nF".to_string(),
            count: *count,
            max_distance_mm: None,
            to: None,
            through: None,
        })
        .collect();
    p
}

pub fn component(id: u32, refdes: &str, part: Part) -> Component {
    Component {
        id: crate::board::ComponentId(id),
        refdes: refdes.to_string(),
        kind: part.kind.clone(),
        part: Some(part),
        value: None,
        dnp: false,
        properties: std::collections::BTreeMap::default(),
        placement_hint: None,
        group: None,
        sheet: None,
        source_span: Span::new(0, 0),
    }
}

/// A net from `(component, pin)` pairs, where the component is its index
/// and the pin is its index in that part's pin list.
pub fn net(id: u32, name: &str, endpoints: &[(u32, u32)]) -> Net {
    Net {
        id: crate::board::NetId(id),
        name: name.to_string(),
        auto_named: name.starts_with("net_"),
        endpoints: endpoints
            .iter()
            .map(|(component, pin)| NetEndpoint {
                component: crate::board::ComponentId(*component),
                pin: crate::board::PinId(*pin),
                source_span: Span::new(0, 0),
            })
            .collect(),
        netclass: None,
        voltage: None,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn board(
    components: Vec<Component>,
    nets: Vec<Net>,
    diff_pairs: Vec<crate::board::DiffPair>,
) -> Board {
    Board {
        name: "test".to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        company: None,
        schematic_paper: None,
        schematic_overflow: None,
        components,
        nets,
        diff_pairs,
        notes: Vec::new(),
        legends: false,
        keepouts: Vec::new(),
        stackup: None,
        netclasses: Vec::new(),
        buses: Vec::new(),
        modules: Vec::new(),
        groups: Vec::new(),
        variants: Vec::new(),
        required_processes: std::collections::BTreeSet::new(),
        source_span: Span::new(0, 0),
    }
}

/// A board with no differential pairs.
pub fn plain_board(components: Vec<Component>, nets: Vec<Net>) -> Board {
    board(components, nets, Vec::new())
}
