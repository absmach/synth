// SPDX-License-Identifier: Apache-2.0

//! Pick-and-Place (PnP) position file CSV exporter.
//!
//! Generates a `pnp.csv` file matching JLCPCB and PCBWay automated SMT assembly formats:
//! `Designator,Val,Package,Mid X,Mid Y,Rotation,Layer`
//!
//! Do-not-populate parts are left out: the assembly house must not
//! place them. (The footprint is still on the PCB and ERC still
//! checks the part.)

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
        if component.dnp {
            continue;
        }
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
            Layer::Bottom | Layer::Inner1 | Layer::Inner2 | Layer::Inner3 | Layer::Inner4 => {
                "Bottom"
            }
        };

        let _ = writeln!(
            out,
            "\"{}\",\"{}\",\"{}\",{:.3},{:.3},{},\"{}\"",
            component.refdes, val, pkg, mid_x, mid_y, rot, layer_str
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_place::{ComponentPlacement, Placement};

    #[test]
    fn dnp_parts_are_excluded() {
        use std::path::Path;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .unwrap();
        let registry = synth_registry::load_dir(&root.join("registry").join("parts")).unwrap();
        let src = r#"board "b" {
            component R1: resistor "r_generic_0603"
            component R2: resistor "r_generic_0603" dnp
            connect R1.p1 -> R2.p1
            connect R1.p2 -> R2.p2
        }"#;
        let parsed = synth_parser::parse(src, "inline.synth");
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let board = synth_ir::lower(&parsed.ast.unwrap(), &registry, "inline.synth")
            .board
            .unwrap();
        let placement = Placement {
            board_outline: synth_geometry::Rect::new(
                synth_geometry::Point::new(0, 0),
                synth_geometry::Point::new(10_000_000, 10_000_000),
            ),
            components: board
                .components
                .iter()
                .map(|c| ComponentPlacement {
                    id: c.id,
                    center: synth_geometry::Point::new(1_000_000, 1_000_000),
                    rotation: synth_geometry::Rotation::Zero,
                    layer: Layer::Top,
                })
                .collect(),
        };
        let csv = build_pnp_csv(&board, &placement);
        assert!(csv.contains("\"R1\""), "populated part must be listed");
        assert!(!csv.contains("\"R2\""), "DNP part must be left out:\n{csv}");
    }
}
