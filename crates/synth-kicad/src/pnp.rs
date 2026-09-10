// SPDX-License-Identifier: Apache-2.0

//! Pick-and-Place (PnP) position file CSV exporter.
//!
//! Generates a `pnp.csv` file matching JLCPCB and PCBWay automated SMT assembly formats:
//! `Designator,Val,Package,Mid X,Mid Y,Rotation,Layer`

use std::collections::HashMap;
use std::fmt::Write as _;

use synth_geometry::{nm_to_mm, Layer};
use synth_ir::Board;
use synth_place::Placement;

/// Build a Pick-and-Place CSV string from `board` and `placement`.
#[must_use]
pub fn build_pnp_csv(board: &Board, placement: &Placement) -> String {
    let mut out = String::from("Designator,Val,Package,Mid X,Mid Y,Rotation,Layer\n");

    let placements_by_id: HashMap<_, _> = placement.components.iter().map(|p| (p.id, p)).collect();

    let mut comps: Vec<_> = board.components.iter().collect();
    comps.sort_by(|a, b| a.refdes.cmp(&b.refdes));

    for component in comps {
        let Some(placement) = placements_by_id.get(&component.id) else {
            continue;
        };
        let val = component
            .value
            .as_deref()
            .or_else(|| component.part.as_ref().and_then(|p| p.mpn.as_deref()))
            .unwrap_or(component.kind.as_str());

        let pkg = component
            .part
            .as_ref()
            .and_then(|p| p.kicad_footprint.as_deref())
            .unwrap_or("");

        let mid_x = nm_to_mm(placement.center.x_nm);
        let mid_y = nm_to_mm(placement.center.y_nm);
        let rot = placement.rotation.degrees();
        let layer_str = match placement.layer {
            Layer::Top => "Top",
            Layer::Bottom | Layer::Inner1 | Layer::Inner2 => "Bottom",
        };

        let _ = writeln!(
            out,
            "\"{}\",\"{}\",\"{}\",{:.3},{:.3},{},\"{}\"",
            component.refdes, val, pkg, mid_x, mid_y, rot, layer_str
        );
    }

    out
}
