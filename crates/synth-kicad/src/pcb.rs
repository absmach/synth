// SPDX-License-Identifier: Apache-2.0

//! `.kicad_pcb` generator.
//!
//! Given a [`synth_place::Placement`] plus the source [`Board`], emit
//! a KiCad-10 PCB file that pcbnew opens without conversion.
//!
//! Phase 7 slice 1B scope: footprint *instances* with `(lib_id ...)`
//! references to KiCad's bundled footprint libraries. KiCad resolves
//! the lib_id through the user's global fp-lib-table (which already
//! points at `/usr/share/kicad/footprints/*.pretty` on a stock
//! install). Unlike the schematic, we do **not** embed footprint
//! definitions inline — pcbnew is fine with external lookups, and
//! KiCad's footprint cache rebuilds on first project open.
//!
//! Phase 7+ adds: real net assignment per pad (Phase 8), tracks +
//! vias (Phase 8 routing), zones / copper pours (Phase 9 DRC), 3D
//! model paths (already attached via `(model ...)` inside each
//! footprint file).
//!
//! ## Output structure
//!
//! ```text
//! (kicad_pcb
//!   (version 20260206)
//!   (generator "synth-eda")
//!   (generator_version "10.0")
//!   (general (thickness 1.6) ...)
//!   (paper "A4")          ; sized to fit the placement
//!   (layers ...)          ; standard 2-layer stackup
//!   (setup ...)           ; minimum required block
//!   (net 0 "")            ; KiCad requires net 0 = no-connection
//!   (gr_line ... layer "Edge.Cuts")  ; board outline
//!   (footprint "Lib:Name" (layer "F.Cu") (at x y) ...)  ; one per component
//! )
//! ```

use std::collections::HashMap;
use std::fmt::Write as _;

use synth_geometry::{mm_to_nm, nm_to_mm, Layer, Point, Rotation};
use synth_ir::{Board, ComponentId, Stackup, StackupLayer};
use synth_pcb::{Routing, Segment};
use synth_place::{ComponentPlacement, Placement};
use uuid::Uuid;

use crate::sexp::{num, pair, str_pair, Sexp};
use crate::silk_labels;
use crate::uuid_v5::derive_entity_uuid;

/// Board thickness in millimetres. JLC standard 1.6 mm.
const DEFAULT_THICKNESS_MM: f64 = 1.6;

/// Edge.Cuts stroke width in millimetres. KiCad's default is 0.05;
/// 0.1 mm is more visible without affecting fabrication.
const EDGE_CUT_WIDTH_MM: f64 = 0.1;

/// Build the full `.kicad_pcb` s-expression.
///
/// `project` namespaces deterministic uuids the same way the
/// schematic exporter does — re-running on identical IR + identical
/// placement produces byte-identical output.
/// A declared power plane could not be fanned out without a via-in-pad.
///
/// The plane is opt-in and its stitching vias are placed beside the pad on
/// purpose, so the board never depends on a filled/capped via-in-pad process.
/// When no side location is legal, export stops with this instead of quietly
/// emitting a via-in-pad the board was not designed around.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaneFanoutError {
    /// The plane net whose pad could not be fanned out.
    pub net: String,
    /// A human-readable explanation, including the pad position.
    pub detail: String,
}

/// `label_overrides` is the design's say over reference labels, by designator,
/// from the placement sidecar.
pub fn build_pcb(
    board: &Board,
    placement: &Placement,
    routing: &Routing,
    project: &Uuid,
    label_overrides: &HashMap<String, synth_layout::sidecar::SidecarLabel>,
    fixed_vias: &[synth_layout::sidecar::SidecarVia],
) -> Result<Sexp, PlaneFanoutError> {
    let placements_by_id: HashMap<ComponentId, &ComponentPlacement> =
        placement.components.iter().map(|p| (p.id, p)).collect();

    // Slice 1B: assign every board.net a positive PCB net id
    // (net 0 stays the unconnected net per KiCad convention),
    // and build a per-pad lookup so footprint instances can
    // declare `(net N "name")` on each pad. The ratsnest
    // depends on this; without per-pad nets, pcbnew shows no
    // "rubber band" connectivity hints.
    let (net_table, pad_net_lookup) = build_net_assignments(board);

    let mut children = vec![
        pair("version", Sexp::atom("20260206")),
        str_pair("generator", "synth-eda"),
        str_pair("generator_version", "10.0"),
        Sexp::list(
            "general",
            vec![
                Sexp::list("thickness", vec![board_thickness(board.stackup.as_ref())]),
                Sexp::list("legacy_teardrops", vec![Sexp::atom("no")]),
            ],
        ),
        Sexp::list("paper", vec![Sexp::str(paper_for(placement))]),
        build_layers(board.layers),
        build_setup_with(
            board.stackup.as_ref(),
            &synth_drc::ManufacturerProfile::from_name(
                board.manufacturer.as_deref().unwrap_or_default(),
            ),
        ),
        // Net 0 = unconnected (KiCad convention). Real nets get
        // ids 1..N in IR declaration order — same ordering KiCad
        // uses when it generates from an imported netlist.
        Sexp::list("net", vec![Sexp::atom("0"), Sexp::str("")]),
    ];
    for (net_id, name) in &net_table {
        children.push(Sexp::list(
            "net",
            vec![Sexp::atom(net_id.to_string()), Sexp::str(name)],
        ));
    }
    children.extend(build_netclasses(&net_table, board));

    // Edge.Cuts board outline. Four gr_line segments tracing
    // `placement.board_outline`. Slice 1B uses an axis-aligned
    // rectangle; later slices accept user-defined outlines.
    children.extend(build_edge_cuts(placement, project));

    // Footprint instances. Components with a real `kicad_footprint`
    // reference it directly; components without one (e.g. a brand-new
    // module that has no official KiCad library yet) get a synthesized
    // fallback footprint built from their `footprint_dimensions` + pin
    // list, so they still appear — netted — on the board instead of
    // being dropped. The router stamps the *same* synthesized pad
    // geometry (`kicad_footprint_loader::synth_part_pads`), so traces
    // planned against the part actually terminate on real copper.
    let labels = silk_labels::place_labels(
        &label_parts(board, &placements_by_id),
        placement.board_outline,
        &label_overrides_by_id(board, label_overrides),
    );
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let Some(placement) = placements_by_id.get(&component.id) else {
            continue;
        };
        let synth_lib_id = format!("synth:{}", part.id);
        let lib_id = part.kicad_footprint.as_deref().unwrap_or(&synth_lib_id);
        children.push(build_footprint_instance(
            component,
            part,
            placement,
            lib_id,
            project,
            &pad_net_lookup,
            labels[&component.id],
        ));
    }

    // Phase 8: emit routed traces as `(segment ...)` blocks
    // after every footprint. Each segment carries the same
    // PCB net id stamped on the pads it connects, so KiCad's
    // ratsnest contracts as routed nets get traces.
    let net_id_lookup: HashMap<synth_ir::NetId, u32> = board
        .nets
        .iter()
        .enumerate()
        .map(|(idx, net)| (net.id, (idx as u32) + 1))
        .collect();
    for (idx, seg) in routing.segments.iter().enumerate() {
        children.push(build_segment(
            seg,
            idx,
            &net_id_lookup,
            project,
            board.layers,
        ));
    }
    for (idx, via) in routing.vias.iter().enumerate() {
        children.push(build_via(via, idx, &net_id_lookup, project));
    }
    for (idx, via) in fixed_vias.iter().enumerate() {
        let Some((pcb_net, _)) = net_table.iter().find(|(_, name)| *name == via.net) else {
            return Err(PlaneFanoutError {
                net: via.net.clone(),
                detail: format!(
                    "the placement sidecar fixes a via at ({}, {}) mm on a net the board does not have",
                    via.x, via.y
                ),
            });
        };
        children.push(build_ground_stitch_via(
            mm_to_nm(via.x),
            mm_to_nm(via.y),
            *pcb_net,
            project,
            // Past every stitch via the exporter itself places, so the uuids stay distinct.
            FIXED_VIA_UUID_BASE + idx,
        ));
    }

    // Ground Plane Copper Flood Zone: Inject continuous GND copper zones on F.Cu, B.Cu, and inner layers
    // Ground plane: use the same topology classifier as netclasses so opaque
    // generated names such as `net_2` do not prevent plane creation. Every
    // zone carries an explicit priority (see `GROUND_ZONE_PRIORITY`), so a
    // rail plane declared through a `netclass` never ties with a ground plane
    // and leaves either net with isolated islands.
    let power_domains = synth_ir::infer_power_domains(board);
    let plane_nets: Vec<(u32, &str, synth_ir::NetId)> = board
        .nets
        .iter()
        .filter(|net| net.endpoints.len() >= 3)
        .filter_map(|net| {
            let domain = power_domains.get(net.id)?;
            let name_lower = net.name.to_ascii_lowercase();
            let explicit_ground = name_lower.contains("gnd")
                || name_lower.contains("vss")
                || name_lower == "0v"
                || net.endpoints.iter().any(|endpoint| {
                    board
                        .component(endpoint.component)
                        .and_then(|component| component.part.as_ref())
                        .and_then(|part| part.pins.get(endpoint.pin.0 as usize))
                        .is_some_and(|pin| {
                            let pin_name = pin.name.to_ascii_lowercase();
                            pin_name.contains("gnd") || pin_name.contains("vss") || pin_name == "0v"
                        })
                });
            if !(domain.is_ground() || explicit_ground) {
                return None;
            }
            net_id_lookup
                .get(&net.id)
                .copied()
                .map(|pcb_net| (pcb_net, net.name.as_str(), net.id))
        })
        .collect();

    // Ground and a declared power plane cannot both be a full-board flood on
    // one layer: KiCad fills the higher-priority zone across the whole layer
    // and starves the other. So on a board with inner layers the two are put on
    // disjoint layers — ground on the outer copper, power on the inner — and
    // each gets a plane of its own. Without a declared power plane, ground
    // keeps every layer exactly as before. On a board with no inner layers a
    // declared power plane has nowhere to go and is not emitted.
    let has_power_plane = board.netclasses.iter().any(|class| class.plane);
    let (gnd_layers, power_layers): (Vec<&'static str>, Vec<&'static str>) =
        if has_power_plane && board.layers >= 4 {
            (vec!["F.Cu", "B.Cu"], inner_plane_layers(board.layers))
        } else {
            (ground_plane_layers(board.layers), Vec::new())
        };

    for (plane_index, (pcb_net, net_name, net_id)) in plane_nets.iter().enumerate() {
        let region = ground_zone_region(board, placement, *net_id, &plane_nets);
        for layer in &gnd_layers {
            // The first plane keeps the layer-only key so single-ground uuids stay unchanged.
            let zone_key = if plane_index == 0 {
                (*layer).to_string()
            } else {
                format!("{net_name}/{layer}")
            };
            children.push(build_zone(
                placement,
                layer,
                *pcb_net,
                net_name,
                project,
                &zone_key,
                ground_zone_priority(plane_index, plane_nets.len()),
                region.as_deref(),
            ));
        }
        // A zone on each copper layer is only a plane, not an electrical
        // connection between planes.  Add a small, deterministic fence of
        // through vias so the planes form one physical ground network. The
        // external routers preserve these source vias while replacing their
        // own routed segments and vias.
        if board.layers >= 4 && plane_index == 0 {
            let outline = placement.board_outline;
            let inset = mm_to_nm(1.5);
            let holes: Vec<(Point, i64)> = board
                .components
                .iter()
                .filter(|component| component.kind == "mounting_hole")
                .filter_map(|component| {
                    let centre = placements_by_id.get(&component.id)?.center;
                    let width_mm = component
                        .part
                        .as_ref()?
                        .footprint_dimensions
                        .as_ref()?
                        .width_mm;
                    Some((centre, mm_to_nm(width_mm / 2.0)))
                })
                .collect();
            // Left corners slide right and right corners slide left.
            let points = [
                (outline.min.x_nm + inset, outline.min.y_nm + inset, 1),
                (outline.max.x_nm - inset, outline.min.y_nm + inset, -1),
                (outline.max.x_nm - inset, outline.max.y_nm - inset, -1),
                (outline.min.x_nm + inset, outline.max.y_nm - inset, 1),
            ];
            for (stitch_index, (x_nm, y_nm, inward)) in points.into_iter().enumerate() {
                let Point { x_nm, y_nm } =
                    clear_of_mounting_holes(Point::new(x_nm, y_nm), inward, &holes);
                children.push(build_ground_stitch_via(
                    x_nm,
                    y_nm,
                    *pcb_net,
                    project,
                    plane_index * 4 + stitch_index,
                ));
            }
        }
    }

    // A screw head over copper shorts it or crushes the mask, so each mounting
    // hole keeps tracks, vias and pour out of the area under the head.
    for component in board
        .components
        .iter()
        .filter(|component| component.kind == "mounting_hole")
    {
        let Some(placed) = placements_by_id.get(&component.id) else {
            continue;
        };
        let Some(width_mm) = component
            .part
            .as_ref()
            .and_then(|part| part.footprint_dimensions.as_ref())
            .map(|dimensions| dimensions.width_mm)
        else {
            continue;
        };
        children.push(build_mounting_hole_keepout(
            placed.center,
            mm_to_nm(width_mm / 2.0),
            &ground_plane_layers(board.layers),
            project,
            &component.refdes,
        ));
    }

    // Declared planes (`netclass "PWR" { plane }`): flood a zone for each
    // member net on the dedicated power layers, so a rail gets a low-impedance
    // path of its own. The plane sits on the inner layers, away from ground, so
    // each of the net's own pads gets a stitching via reached by a short
    // fanout stub: without one the plane exists but connects nothing, because
    // the pads are on the outer copper and the plane is not. The via is placed
    // beside the pad, not in it, so the board does not silently depend on
    // via-in-pad. Opt-in: no plane is emitted for a class that does not ask,
    // and none at all on a board with no inner layer to put it on.
    // Every pad on the board, so a generated stitching via can be kept clear of
    // one: a via dropped on another net's pad is a short. Computed once, and
    // only when a plane is actually going to be emitted.
    let mut obstacles: Vec<(synth_geometry::Point, i64)> =
        if board.netclasses.iter().any(|class| class.plane) && !power_layers.is_empty() {
            all_pad_obstacles(board, placement)
        } else {
            Vec::new()
        };
    // One rail per layer. Pouring every plane net on every power layer puts
    // two rails at the same priority on the same copper, which is the
    // starvation described above with ground swapped out for a second rail:
    // on a four-layer board with a core rail and an I/O rail it left the I/O
    // rail's thirty pads stranded.
    //
    // Rails are dealt across the layers with a stride, so no two share one
    // while there are enough layers to go round. Dealing contiguous blocks
    // instead reintroduces the bug as soon as the counts do not divide: three
    // rails over four layers would give the first and third the same pair.
    // With a stride they take In1/In4, In2 and In3. Only when the rails
    // outnumber the layers does a layer carry two, and then each rail still
    // gets exactly one.
    let rails: Vec<(u32, &synth_ir::Net)> = board
        .netclasses
        .iter()
        .filter(|class| class.plane)
        .flat_map(|class| {
            board
                .nets
                .iter()
                .filter(move |net| net.netclass.as_deref() == Some(class.name.as_str()))
        })
        .filter_map(|net| net_id_lookup.get(&net.id).copied().map(|id| (id, net)))
        .collect();

    let mut plane_via_index = 0usize;
    let mut plane_stub_index = 0usize;
    if !power_layers.is_empty() {
        for (rail_index, (pcb_net, net)) in rails.iter().copied().enumerate() {
            let layers: Vec<&'static str> = if rails.len() >= power_layers.len() {
                vec![power_layers[rail_index % power_layers.len()]]
            } else {
                power_layers
                    .iter()
                    .skip(rail_index)
                    .step_by(rails.len())
                    .copied()
                    .collect()
            };
            for layer in layers {
                let zone_key = format!("plane/{}/{layer}", net.name);
                children.push(build_zone(
                    placement,
                    layer,
                    pcb_net,
                    net.name.as_str(),
                    project,
                    &zone_key,
                    POWER_ZONE_PRIORITY,
                    None,
                ));
            }
            for (pad, origin, layer) in plane_pad_targets(board, placement, net) {
                let Some(via) = plane_via_offset(pad, origin, placement.board_outline, &obstacles)
                else {
                    return Err(PlaneFanoutError {
                        net: net.name.clone(),
                        detail: format!(
                            "no legal stitching-via position beside the pad at ({:.2}, {:.2}) mm \
                             on {layer:?}: every candidate is outside the board or within \
                             clearance of another pad or via. Move the part, widen the board, or \
                             allow via-in-pad explicitly.",
                            nm_to_mm(pad.x_nm),
                            nm_to_mm(pad.y_nm),
                        ),
                    });
                };
                obstacles.push((via, PLANE_VIA_RADIUS_NM));
                children.push(build_plane_via(
                    via.x_nm,
                    via.y_nm,
                    pcb_net,
                    project,
                    plane_via_index,
                ));
                plane_via_index += 1;
                children.push(build_segment(
                    &Segment {
                        net: net.id,
                        layer,
                        start: pad,
                        end: via,
                        width_nm: PLANE_STUB_WIDTH_NM,
                    },
                    plane_stub_index,
                    &net_id_lookup,
                    project,
                    board.layers,
                ));
                plane_stub_index += 1;
            }
        }
    }

    Ok(Sexp::list("kicad_pcb", children))
}

fn ground_plane_layers(layer_count: u32) -> Vec<&'static str> {
    match layer_count {
        4 => vec!["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"],
        6 => vec!["F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu"],
        _ => vec!["F.Cu", "B.Cu"],
    }
}

/// The inner layers a declared power plane floods: the layers ground gives up
/// when a power plane is declared. Empty for a board with no inner layer.
fn inner_plane_layers(layer_count: u32) -> Vec<&'static str> {
    match layer_count {
        6 => vec!["In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu"],
        4 => vec!["In1.Cu", "In2.Cu"],
        _ => Vec::new(),
    }
}

/// Emit a single `(via ...)` block.
fn build_via(
    via: &synth_pcb::Via,
    idx: usize,
    net_id_lookup: &HashMap<synth_ir::NetId, u32>,
    project: &Uuid,
) -> Sexp {
    let pcb_net = net_id_lookup.get(&via.net).copied().unwrap_or(0);
    let via_uuid = derive_entity_uuid(project, "via", &format!("{idx}"));
    Sexp::list(
        "via",
        vec![
            Sexp::list(
                "at",
                vec![num(nm_to_mm(via.at.x_nm)), num(nm_to_mm(via.at.y_nm))],
            ),
            Sexp::list("size", vec![num(nm_to_mm(via.pad_diameter_nm))]),
            Sexp::list("drill", vec![num(nm_to_mm(via.drill_nm))]),
            Sexp::list("layers", vec![Sexp::str("F.Cu"), Sexp::str("B.Cu")]),
            Sexp::list("net", vec![Sexp::atom(pcb_net.to_string())]),
            str_pair("uuid", via_uuid.to_string()),
        ],
    )
}

/// Board-space targets for a declared power plane: one
/// `(pad centre, footprint origin, pad layer)` per pad the net connects.
///
/// The plane is on the inner layers while its pads are on the outer copper, so
/// each pad needs a via through the plane, reached by a short stub. This
/// resolves a pad's board position the same way the exporter places the
/// footprint: the pad's local offset, rotated by the placement and translated
/// by the footprint origin.
#[allow(clippy::similar_names)] // dx_nm/dy_nm are the natural names for an axis pair
fn plane_pad_targets(
    board: &Board,
    placement: &Placement,
    net: &synth_ir::Net,
) -> Vec<(synth_geometry::Point, synth_geometry::Point, Layer)> {
    let mut out = Vec::new();
    for endpoint in &net.endpoints {
        let Some(component) = board.component(endpoint.component) else {
            continue;
        };
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let Some(placed) = placement
            .components
            .iter()
            .find(|placed| placed.id == component.id)
        else {
            continue;
        };
        let Some(pin) = part.pins.get(endpoint.pin.0 as usize) else {
            continue;
        };
        let Some(pads) = synth_layout::kicad_footprint_loader::synth_part_pads(part) else {
            continue;
        };
        let origin = synth_place::footprint_origin(Some(part), placed);
        for pad in pads
            .iter()
            .filter(|pad| !pad.is_npth && pad.number == pin.number.0)
        {
            let (dx_nm, dy_nm) = placed
                .rotation
                .rotate_offset(mm_to_nm(pad.center_mm.0), mm_to_nm(pad.center_mm.1));
            let center = synth_geometry::Point::new(origin.x_nm + dx_nm, origin.y_nm + dy_nm);
            out.push((center, origin, placed.layer));
        }
    }
    out
}

/// Stitching-via pad radius in nanometres (a 0.6 mm via).
const PLANE_VIA_RADIUS_NM: i64 = 300_000;

/// Fabrication copper clearance, in nanometres, used to keep a generated via
/// legal against the nearest pad or other via.
const PLANE_VIA_FAB_CLEARANCE_NM: i64 = 200_000;

/// Keep a plane stitching via at least this far inside Edge.Cuts, in
/// nanometres (via radius plus the fabrication edge clearance).
const PLANE_EDGE_MARGIN_NM: i64 = 1_000_000;

/// Every copper pad on the board as `(centre, circumscribed radius)`, in board
/// space. Used to keep a generated plane stitching via off a neighbouring pad:
/// a via dropped onto another net's pad is a short, and the exporter has to
/// avoid it rather than rely on the router's DRC to catch it later.
#[allow(clippy::similar_names)] // dx_nm/dy_nm are the natural names for an axis pair
fn all_pad_obstacles(board: &Board, placement: &Placement) -> Vec<(synth_geometry::Point, i64)> {
    let mut out = Vec::new();
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let Some(placed) = placement
            .components
            .iter()
            .find(|placed| placed.id == component.id)
        else {
            continue;
        };
        let Some(pads) = synth_layout::kicad_footprint_loader::synth_part_pads(part) else {
            continue;
        };
        let origin = synth_place::footprint_origin(Some(part), placed);
        for pad in pads.iter().filter(|pad| !pad.is_npth) {
            let (dx_nm, dy_nm) = placed
                .rotation
                .rotate_offset(mm_to_nm(pad.center_mm.0), mm_to_nm(pad.center_mm.1));
            let (w_mm, h_mm) = pad.size_mm;
            let radius_nm = mm_to_nm(((w_mm / 2.0).powi(2) + (h_mm / 2.0).powi(2)).sqrt());
            out.push((
                synth_geometry::Point::new(origin.x_nm + dx_nm, origin.y_nm + dy_nm),
                radius_nm,
            ));
        }
    }
    out
}

/// Squared distance from the segment `a`–`b` to the point `p`, in nanometres
/// squared.
///
/// Used to test a whole fanout stub against an obstacle, not only its endpoint:
/// the minimum distance from the segment to the obstacle centre is what a
/// clearance check needs.
fn segment_clearance_nm_sq(
    a: synth_geometry::Point,
    b: synth_geometry::Point,
    p: synth_geometry::Point,
) -> i64 {
    let abx = b.x_nm - a.x_nm;
    let aby = b.y_nm - a.y_nm;
    let apx = p.x_nm - a.x_nm;
    let apy = p.y_nm - a.y_nm;
    let denom = abx * abx + aby * aby;
    let (cx, cy) = if denom == 0 {
        (a.x_nm, a.y_nm)
    } else {
        // Integer projection: t is clamped to [0, denom], so the closest point
        // stays on the segment. i128 avoids the i64 overflow of t * ab.
        let t = (apx * abx + apy * aby).clamp(0, denom);
        let tx = (i128::from(t) * i128::from(abx)) / i128::from(denom);
        let ty = (i128::from(t) * i128::from(aby)) / i128::from(denom);
        (
            a.x_nm + i64::try_from(tx).unwrap_or(0),
            a.y_nm + i64::try_from(ty).unwrap_or(0),
        )
    };
    let dx = p.x_nm - cx;
    let dy = p.y_nm - cy;
    dx * dx + dy * dy
}

/// Where a power pad's stitching via goes.
///
/// The preferred spot is [`PLANE_VIA_OFFSET_NM`] from the pad, away from the
/// footprint origin along the dominant axis so the direction is stable, and the
/// via sits beside the pad rather than in it so the board does not depend on
/// via-in-pad. That preference is only a first choice: 1 mm along one axis can
/// land outside Edge.Cuts or on a neighbouring pad, so candidate positions are
/// tried (four axes, three distances) and the first that stays inside the board
/// and clear of every pad and already-placed via is used. `None` when no side
/// location is legal — a pad boxed in on every side — so the caller can stop
/// rather than silently fall back to via-in-pad.
fn plane_via_offset(
    pad: synth_geometry::Point,
    origin: synth_geometry::Point,
    outline: synth_geometry::Rect,
    obstacles: &[(synth_geometry::Point, i64)],
) -> Option<synth_geometry::Point> {
    let inset = PLANE_EDGE_MARGIN_NM;
    let usable = synth_geometry::Rect::new(
        synth_geometry::Point::new(outline.min.x_nm + inset, outline.min.y_nm + inset),
        synth_geometry::Point::new(outline.max.x_nm - inset, outline.max.y_nm - inset),
    );

    let dx = pad.x_nm - origin.x_nm;
    let dy = pad.y_nm - origin.y_nm;
    let sx = if dx >= 0 { 1 } else { -1 };
    let sy = if dy >= 0 { 1 } else { -1 };
    let axes: [(i64, i64); 4] = if dx.abs() >= dy.abs() {
        [(sx, 0), (0, sy), (0, -sy), (-sx, 0)]
    } else {
        [(0, sy), (sx, 0), (-sx, 0), (0, -sy)]
    };
    let distances = [
        PLANE_VIA_OFFSET_NM,
        PLANE_VIA_OFFSET_NM * 3 / 4,
        PLANE_VIA_OFFSET_NM / 2,
    ];
    // The whole fanout stub is checked, not only the via endpoint: a stub from
    // the pad to a nearby candidate can cut across another pad even when the via
    // itself is clear. The stub body is held to its own half-width, the via at
    // its end to the via radius.
    let stub_half_nm = PLANE_STUB_WIDTH_NM / 2;
    let clear_of_obstacles = |candidate: synth_geometry::Point| -> bool {
        obstacles
            .iter()
            .filter(|(centre, _)| *centre != pad)
            .all(|(centre, radius_nm)| {
                let seg_need = stub_half_nm + PLANE_VIA_FAB_CLEARANCE_NM + radius_nm;
                let via_need = PLANE_VIA_RADIUS_NM + PLANE_VIA_FAB_CLEARANCE_NM + radius_nm;
                let vdx = candidate.x_nm - centre.x_nm;
                let vdy = candidate.y_nm - centre.y_nm;
                segment_clearance_nm_sq(pad, candidate, *centre) >= seg_need * seg_need
                    && vdx * vdx + vdy * vdy >= via_need * via_need
            })
    };
    for distance in distances {
        for (ux, uy) in axes {
            let candidate =
                synth_geometry::Point::new(pad.x_nm + ux * distance, pad.y_nm + uy * distance);
            if usable.contains(candidate) && clear_of_obstacles(candidate) {
                return Some(candidate);
            }
        }
    }
    // Boxed in on every side: report it rather than quietly place a via-in-pad.
    None
}

/// A through via beside a power pad, joining the outer pad (through the fanout
/// stub) to the inner power plane.
fn build_plane_via(x_nm: i64, y_nm: i64, pcb_net: u32, project: &Uuid, idx: usize) -> Sexp {
    let via_uuid = derive_entity_uuid(project, "plane-via", &idx.to_string());
    Sexp::list(
        "via",
        vec![
            Sexp::list("at", vec![num(nm_to_mm(x_nm)), num(nm_to_mm(y_nm))]),
            Sexp::list("size", vec![num(GROUND_VIA_RADIUS_MM * 2.0)]),
            Sexp::list("drill", vec![num(0.3)]),
            Sexp::list("layers", vec![Sexp::str("F.Cu"), Sexp::str("B.Cu")]),
            Sexp::list("net", vec![Sexp::atom(pcb_net.to_string())]),
            str_pair("uuid", via_uuid.to_string()),
        ],
    )
}

/// How far a via keeps from a mounting hole's edge, in mm. A screw head and its
/// washer sit over the copper round the hole, so a via closer than this is
/// under them. Covers the head of an M3 screw (about 5.5 mm across) with room
/// for the via's own pad.
const MOUNTING_HOLE_KEEPOUT_MM: f64 = 1.5;

/// Radius of the pad of a ground fence via (the via is 0.6 mm across).
const GROUND_VIA_RADIUS_MM: f64 = 0.3;

/// A copper keepout under a mounting hole's screw head: a round rule area on
/// every copper layer that refuses tracks, vias and pour, as wide as the hole
/// plus `MOUNTING_HOLE_KEEPOUT_MM`. Pads stay allowed, so the hole's own part
/// is not flagged. `layers` are the board's copper layers.
fn build_mounting_hole_keepout(
    centre: Point,
    radius_nm: i64,
    layers: &[&str],
    project: &Uuid,
    refdes: &str,
) -> Sexp {
    const SIDES: u32 = 24;
    let reach = nm_to_mm(radius_nm) + MOUNTING_HOLE_KEEPOUT_MM;
    let points = (0..SIDES)
        .map(|k| {
            let angle = std::f64::consts::TAU * f64::from(k) / f64::from(SIDES);
            Sexp::list(
                "xy",
                vec![
                    num(nm_to_mm(centre.x_nm) + reach * angle.cos()),
                    num(nm_to_mm(centre.y_nm) + reach * angle.sin()),
                ],
            )
        })
        .collect();
    let zone_uuid = derive_entity_uuid(project, "mounting-hole-keepout", refdes);
    Sexp::list(
        "zone",
        vec![
            Sexp::list("net", vec![Sexp::atom("0")]),
            str_pair("net_name", ""),
            Sexp::list(
                "layers",
                layers.iter().map(|layer| Sexp::str(*layer)).collect(),
            ),
            str_pair("uuid", zone_uuid.to_string()),
            str_pair("name", format!("{refdes} screw head")),
            Sexp::list("hatch", vec![Sexp::atom("edge"), num(0.5)]),
            Sexp::list(
                "keepout",
                vec![
                    Sexp::list("tracks", vec![Sexp::atom("not_allowed")]),
                    Sexp::list("vias", vec![Sexp::atom("not_allowed")]),
                    Sexp::list("pads", vec![Sexp::atom("allowed")]),
                    Sexp::list("copperpour", vec![Sexp::atom("not_allowed")]),
                    Sexp::list("footprints", vec![Sexp::atom("allowed")]),
                ],
            ),
            Sexp::list("polygon", vec![Sexp::list("pts", points)]),
        ],
    )
}

/// Slide a corner fence via along the board edge, toward the inside (`inward`
/// is +1 for a left-hand corner and -1 for a right-hand one), until it is out of
/// every mounting hole's zone: the hole's radius plus the keepout. A via that is
/// already clear is returned as it is. `holes` are `(centre, radius)`.
fn clear_of_mounting_holes(via: Point, inward: i64, holes: &[(Point, i64)]) -> Point {
    let mut via = via;
    for &(centre, radius) in holes {
        // The whole pad must be outside the zone, not only its centre.
        let limit = nm_to_mm(radius) + MOUNTING_HOLE_KEEPOUT_MM + GROUND_VIA_RADIUS_MM;
        let dx = nm_to_mm(via.x_nm - centre.x_nm);
        let dy = nm_to_mm(via.y_nm - centre.y_nm);
        if dx.hypot(dy) >= limit {
            continue;
        }
        // Along the edge the via must be at least this far from the centre.
        let reach = (limit * limit - dy * dy).sqrt();
        via.x_nm = centre.x_nm + inward * (mm_to_nm(reach) + 1);
    }
    via
}

/// Uuid index of the first sidecar-fixed via, clear of the stitch vias the exporter derives.
const FIXED_VIA_UUID_BASE: usize = 1_000_000;

fn build_ground_stitch_via(x_nm: i64, y_nm: i64, pcb_net: u32, project: &Uuid, idx: usize) -> Sexp {
    let via_uuid = derive_entity_uuid(project, "ground-stitch-via", &idx.to_string());
    Sexp::list(
        "via",
        vec![
            Sexp::list("at", vec![num(nm_to_mm(x_nm)), num(nm_to_mm(y_nm))]),
            Sexp::list("size", vec![num(0.6)]),
            Sexp::list("drill", vec![num(0.3)]),
            Sexp::list("layers", vec![Sexp::str("F.Cu"), Sexp::str("B.Cu")]),
            Sexp::list("net", vec![Sexp::atom(pcb_net.to_string())]),
            str_pair("uuid", via_uuid.to_string()),
        ],
    )
}

/// Priority of a generated ground plane: distinct per ground net, and every
/// ground above the power planes.
///
/// Distinct so that split grounds on one layer are filled in a deterministic
/// order — the first declared ground wins and the rest are carved around it —
/// rather than in an order that leaves an arbitrary one with isolated islands.
/// Above zero so that, where a rail plane shares a layer, ground is filled
/// first.
fn ground_zone_priority(index: usize, count: usize) -> u8 {
    u8::try_from(count.saturating_sub(index))
        .unwrap_or(1)
        .max(1)
}

/// Margin added around a split ground's pad bounding box, in millimetres.
const POUR_REGION_MARGIN_MM: f64 = 8.0;

/// The sub-region a ground pour covers.
///
/// A single ground floods the whole board. Split grounds each cover the
/// bounding box of their own pads plus a margin, so two pours on one layer sit
/// over their own parts instead of overlapping everywhere. `None` means the
/// whole board.
fn ground_zone_region(
    board: &Board,
    placement: &Placement,
    net_id: synth_ir::NetId,
    grounds: &[(u32, &str, synth_ir::NetId)],
) -> Option<Vec<synth_geometry::Point>> {
    if grounds.len() < 2 {
        return None;
    }
    let outline = placement.board_outline;
    let inset = mm_to_nm(0.5);
    let board_polygon = vec![
        synth_geometry::Point::new(outline.min.x_nm + inset, outline.min.y_nm + inset),
        synth_geometry::Point::new(outline.max.x_nm - inset, outline.min.y_nm + inset),
        synth_geometry::Point::new(outline.max.x_nm - inset, outline.max.y_nm - inset),
        synth_geometry::Point::new(outline.min.x_nm + inset, outline.max.y_nm - inset),
    ];
    let Some(index) = grounds.iter().position(|(_, _, id)| *id == net_id) else {
        return Some(board_polygon);
    };

    // Two grounds: split the board along the perpendicular bisector of their
    // pad centroids, so each pour covers its own side rather than both
    // overlapping everywhere. This is a true partition for the common
    // analog/digital case; a centroid that cannot be found falls back to the
    // whole board.
    if grounds.len() == 2 {
        let other = 1 - index;
        let (Some(mine), Some(theirs)) = (
            net_centroid(board, placement, net_id),
            net_centroid(board, placement, grounds[other].2),
        ) else {
            return Some(board_polygon);
        };
        let d = synth_geometry::Point::new(theirs.x_nm - mine.x_nm, theirs.y_nm - mine.y_nm);
        if d.x_nm == 0 && d.y_nm == 0 {
            return Some(board_polygon);
        }
        let mid = synth_geometry::Point::new(
            (mine.x_nm + theirs.x_nm) / 2,
            (mine.y_nm + theirs.y_nm) / 2,
        );
        return Some(clip_half_plane(&board_polygon, mid, d));
    }

    // More than two grounds: a bounding box of this net's own pads.
    let net = board.net(net_id)?;
    let centres: Vec<synth_geometry::Point> = plane_pad_targets(board, placement, net)
        .into_iter()
        .map(|(centre, _, _)| centre)
        .collect();
    if centres.len() < 2 {
        return Some(board_polygon);
    }
    let margin = mm_to_nm(POUR_REGION_MARGIN_MM);
    let (mut min_x, mut min_y) = (i64::MAX, i64::MAX);
    let (mut max_x, mut max_y) = (i64::MIN, i64::MIN);
    for point in &centres {
        min_x = min_x.min(point.x_nm);
        max_x = max_x.max(point.x_nm);
        min_y = min_y.min(point.y_nm);
        max_y = max_y.max(point.y_nm);
    }
    let (min_x, min_y) = (
        (min_x - margin).max(outline.min.x_nm + inset),
        (min_y - margin).max(outline.min.y_nm + inset),
    );
    let (max_x, max_y) = (
        (max_x + margin).min(outline.max.x_nm - inset),
        (max_y + margin).min(outline.max.y_nm - inset),
    );
    Some(vec![
        synth_geometry::Point::new(min_x, min_y),
        synth_geometry::Point::new(max_x, min_y),
        synth_geometry::Point::new(max_x, max_y),
        synth_geometry::Point::new(min_x, max_y),
    ])
}

/// Board-space centroid of a net's pads, or `None` when none resolve.
fn net_centroid(
    board: &Board,
    placement: &Placement,
    net_id: synth_ir::NetId,
) -> Option<synth_geometry::Point> {
    let net = board.net(net_id)?;
    let centres = plane_pad_targets(board, placement, net);
    if centres.is_empty() {
        return None;
    }
    let (mut sx, mut sy) = (0_i64, 0_i64);
    for (centre, _, _) in &centres {
        sx += centre.x_nm;
        sy += centre.y_nm;
    }
    #[allow(clippy::cast_possible_wrap)] // a board has far fewer pads than i64::MAX
    let count = centres.len() as i64;
    Some(synth_geometry::Point::new(sx / count, sy / count))
}

/// Sutherland–Hodgman clip of a convex polygon by the half-plane
/// `(p - mid)·d <= 0`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn clip_half_plane(
    polygon: &[synth_geometry::Point],
    mid: synth_geometry::Point,
    d: synth_geometry::Point,
) -> Vec<synth_geometry::Point> {
    let side = |p: synth_geometry::Point| -> i64 {
        (p.x_nm - mid.x_nm) * d.x_nm + (p.y_nm - mid.y_nm) * d.y_nm
    };
    let mut out = Vec::new();
    for i in 0..polygon.len() {
        let current = polygon[i];
        let next = polygon[(i + 1) % polygon.len()];
        let (sc, sn) = (side(current), side(next));
        if sc <= 0 {
            out.push(current);
        }
        if (sc <= 0) != (sn <= 0) {
            let denom = sc - sn;
            if denom != 0 {
                let t = sc as f64 / denom as f64;
                out.push(synth_geometry::Point::new(
                    current.x_nm + ((next.x_nm - current.x_nm) as f64 * t) as i64,
                    current.y_nm + ((next.y_nm - current.y_nm) as f64 * t) as i64,
                ));
            }
        }
    }
    out
}

/// Priority of a declared power plane (`netclass "PWR" { plane }`).
const POWER_ZONE_PRIORITY: u8 = 0;

/// Distance from a power pad to its stitching via, in nanometres.
const PLANE_VIA_OFFSET_NM: i64 = 1_000_000; // 1 mm

/// Width of the pad-to-via fanout stub, in nanometres.
const PLANE_STUB_WIDTH_NM: i64 = 250_000; // 0.25 mm

/// Emit a `(zone ...)` S-expression for a copper flood on one layer.
///
/// Used for both the automatic ground planes and the opt-in power planes.
/// `region` restricts the pour to a polygon — a split ground floods only over
/// its own side — and defaults to the whole board rectangle.
#[allow(clippy::too_many_arguments)] // a zone is a wide record; its fields are read in order
fn build_zone(
    placement: &Placement,
    layer_name: &str,
    net_id: u32,
    net_name: &str,
    project: &Uuid,
    zone_key: &str,
    priority: u8,
    region: Option<&[synth_geometry::Point]>,
) -> Sexp {
    // Keep the zone just inside Edge.Cuts.  A polygon whose vertices lie
    // exactly on the board boundary can be entirely removed by KiCad's edge
    // clearance during refill, leaving every ground pad reported as
    // unconnected.  The inset is smaller than the normal board margin and
    // still covers all ordinary ground pads, including edge connectors.
    let inset_nm = mm_to_nm(0.5);
    let vertices: Vec<synth_geometry::Point> = if let Some(points) = region {
        points.to_vec()
    } else {
        let r = placement.board_outline;
        let (min_x, min_y) = (r.min.x_nm + inset_nm, r.min.y_nm + inset_nm);
        let (max_x, max_y) = (r.max.x_nm - inset_nm, r.max.y_nm - inset_nm);
        vec![
            synth_geometry::Point::new(min_x, min_y),
            synth_geometry::Point::new(max_x, min_y),
            synth_geometry::Point::new(max_x, max_y),
            synth_geometry::Point::new(min_x, max_y),
        ]
    };
    let zone_uuid = derive_entity_uuid(project, "zone", zone_key);
    let pts = vertices
        .iter()
        .map(|p| Sexp::list("xy", vec![num(nm_to_mm(p.x_nm)), num(nm_to_mm(p.y_nm))]))
        .collect();

    Sexp::list(
        "zone",
        vec![
            Sexp::list("net", vec![Sexp::atom(net_id.to_string())]),
            str_pair("net_name", net_name),
            Sexp::list("layer", vec![Sexp::str(layer_name)]),
            str_pair("uuid", zone_uuid.to_string()),
            Sexp::list("priority", vec![Sexp::atom(priority.to_string())]),
            Sexp::list("hatch", vec![Sexp::atom("edge"), num(0.5)]),
            Sexp::list(
                "connect_pads",
                // The RP2350 QFN has 0.2 mm-wide perimeter pads. A generic
                // 0.3 mm plane clearance leaves no legal thermal connection
                // to those pads, so use a fabrication-safe but QFN-aware
                // clearance for the generated ground plane.
                vec![Sexp::atom("yes"), Sexp::list("clearance", vec![num(0.15)])],
            ),
            Sexp::list(
                "fill",
                vec![
                    Sexp::atom("yes"),
                    Sexp::list("thermal_gap", vec![num(0.2)]),
                    Sexp::list("thermal_bridge_width", vec![num(0.25)]),
                    // A pour can be cut into islands by traces and keepouts;
                    // an island touches no pad, so keeping it only adds copper
                    // that KiCad reports as an unconnected zone.
                    Sexp::list("island_removal_mode", vec![Sexp::atom("0")]),
                ],
            ),
            Sexp::list("polygon", vec![Sexp::list("pts", pts)]),
        ],
    )
}
fn build_segment(
    seg: &Segment,
    idx: usize,
    net_id_lookup: &HashMap<synth_ir::NetId, u32>,
    project: &Uuid,
    board_layers: u32,
) -> Sexp {
    let pcb_net = net_id_lookup.get(&seg.net).copied().unwrap_or(0);
    let layer_name = seg.layer.name_for_stackup(board_layers as usize);
    let seg_uuid = derive_entity_uuid(project, "segment", &format!("{idx}"));
    Sexp::list(
        "segment",
        vec![
            Sexp::list(
                "start",
                vec![num(nm_to_mm(seg.start.x_nm)), num(nm_to_mm(seg.start.y_nm))],
            ),
            Sexp::list(
                "end",
                vec![num(nm_to_mm(seg.end.x_nm)), num(nm_to_mm(seg.end.y_nm))],
            ),
            Sexp::list("width", vec![num(nm_to_mm(seg.width_nm))]),
            Sexp::list("layer", vec![Sexp::str(layer_name)]),
            Sexp::list("net", vec![Sexp::atom(pcb_net.to_string())]),
            str_pair("uuid", seg_uuid.to_string()),
        ],
    )
}

/// Walk every net in `board` and produce:
///
/// 1. The ordered list of `(pcb_net_id, net_name)` pairs that
///    appear as top-level `(net N "name")` declarations.
///    `pcb_net_id` starts at 1; net id 0 is reserved for the
///    KiCad-required "unconnected" net.
/// 2. A `(component, pad_number_str) → pcb_net_id` lookup that
///    the footprint emitter consumes to stamp `(net N "name")`
///    on the matching pads.
///
/// `(component, pad_number_string) → (pcb_net_id, net_name)`.
type PadNetLookup = HashMap<(ComponentId, String), (u32, String)>;

/// Both outputs are deterministic — net id assignment follows
/// IR net declaration order.
fn build_net_assignments(board: &Board) -> (Vec<(u32, String)>, PadNetLookup) {
    let mut table: Vec<(u32, String)> = Vec::with_capacity(board.nets.len());
    let mut lookup: HashMap<(ComponentId, String), (u32, String)> = HashMap::new();
    let is_gnd = |name: &str| {
        name.to_lowercase().contains("gnd") || name.to_lowercase().contains("vss") || name == "0v"
    };
    let sole_ground = board.nets.iter().filter(|net| is_gnd(&net.name)).count() == 1;
    for (idx, net) in board.nets.iter().enumerate() {
        let pcb_net_id = (idx as u32) + 1;
        let net_name = if sole_ground && is_gnd(&net.name) {
            "GND".to_string()
        } else {
            net.name.clone()
        };
        table.push((pcb_net_id, net_name.clone()));
        for endpoint in &net.endpoints {
            let Some(component) = board.component(endpoint.component) else {
                continue;
            };
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let Some(pin) = part.pins.get(endpoint.pin.0 as usize) else {
                continue;
            };
            let pin_raw = pin.number.0.clone();
            lookup.insert(
                (component.id, pin_raw.clone()),
                (pcb_net_id, net_name.clone()),
            );
            let pin_norm = pin_raw
                .trim_start_matches(['p', 'P', 'i', 'N', 'I', 'N'])
                .to_string();
            if !pin_norm.is_empty() && pin_norm != pin_raw {
                lookup.insert((component.id, pin_norm), (pcb_net_id, net_name.clone()));
            }
        }
    }

    // Physical-pin reconciliation: the registry declares *logical*
    // pins only, but the footprint carries every physical pad. Fan the
    // rails out to the undeclared power pads (VDD/VDDA/VSS/VSSA...) so
    // the ratsnest and DRC see a fully-connected part — matching the
    // schematic exporter's `build_fanned_power_symbol`. See
    // `crate::pin_reconcile` for the reasoning.
    for reconciled in crate::pin_reconcile::reconcile_all(board) {
        for leg in &reconciled.power_legs {
            // Map the rail net id from the IR to its PCB net id. The
            // IR net may be declared after the leg's component in the
            // table above, so look it up by id rather than index.
            let Some(pcb_net_id) = board
                .nets
                .iter()
                .position(|n| n.id == leg.net)
                .map(|idx| (idx as u32) + 1)
            else {
                continue;
            };
            let net_name = board
                .net(leg.net)
                .map_or_else(|| format!("net_{}", leg.net.0), |n| n.name.clone());
            lookup.insert(
                (leg.component, leg.pin_number.clone()),
                (pcb_net_id, net_name),
            );
        }
    }

    // USB-C pin-group mapping: the receptacle has physically duplicated VBUS,
    // GND, and USB2 differential pads. The Synth registry exposes one logical
    // pin for each function, so the PCB exporter must fan that logical net out
    // to every physical pad in the connector group. Without this, a valid
    // `connect J1.vbus -> ...` leaves A9/B4/B9 as no-connect pads and external
    // routers see a boxed-in VBUS terminal (often the last unrouted net).
    let mut mirror_additions = Vec::new();
    let usb_pin_groups: &[&[&str]] = &[
        &["A4", "B4", "A9", "B9"],   // VBUS
        &["A1", "B1", "A12", "B12"], // GND
        &["A6", "B6"],               // USB D+
        &["A7", "B7"],               // USB D-
    ];

    for ((comp, pin), net_info) in &lookup {
        for group in usb_pin_groups {
            if group.iter().any(|alias| *alias == pin) {
                for alias in *group {
                    mirror_additions.push(((*comp, (*alias).to_string()), net_info.clone()));
                }
            }
        }

        if pin == "A1" || pin == "B12" || pin == "B1" || pin == "A12" {
            for shield in ["SH", "SH1", "SH2", "SH3", "SH4"] {
                mirror_additions.push(((*comp, shield.to_string()), net_info.clone()));
            }
        }
    }
    for (k, v) in mirror_additions {
        lookup.entry(k).or_insert(v);
    }

    (table, lookup)
}

/// Pick the smallest KiCad-standard paper size that comfortably
/// fits the placement plus a frame margin. Slice 1B keeps this
/// simple: A4 if it fits, otherwise A3, etc.
fn paper_for(placement: &Placement) -> &'static str {
    let w_mm = nm_to_mm(placement.board_outline.width_nm());
    let h_mm = nm_to_mm(placement.board_outline.height_nm());
    if w_mm <= 297.0 && h_mm <= 210.0 {
        "A4"
    } else if w_mm <= 420.0 && h_mm <= 297.0 {
        "A3"
    } else {
        "A2"
    }
}

/// Canonical KiCad 10 layer stackup for 2-, 4-, and 6-layer boards with
/// strictly unique layer IDs.
fn build_layers(layer_count: u32) -> Sexp {
    let mut layers = vec![layer(0, "F.Cu", "signal", None)];
    if layer_count >= 6 {
        // KiCad 10 uses even IDs for copper layers in this stack model.
        layers.push(layer(4, "In1.Cu", "power", None));
        layers.push(layer(6, "In2.Cu", "power", None));
        layers.push(layer(8, "In3.Cu", "power", None));
        layers.push(layer(10, "In4.Cu", "power", None));
        layers.push(layer(2, "B.Cu", "signal", None));
    } else if layer_count == 4 {
        layers.push(layer(4, "In1.Cu", "power", None));
        layers.push(layer(6, "In2.Cu", "power", None));
        layers.push(layer(2, "B.Cu", "signal", None));
    } else {
        layers.push(layer(31, "B.Cu", "signal", None));
    }
    layers.extend(vec![
        layer(32, "B.Adhes", "user", Some("B.Adhesive")),
        layer(33, "F.Adhes", "user", Some("F.Adhesive")),
        layer(34, "B.Paste", "user", None),
        layer(35, "F.Paste", "user", None),
        layer(36, "B.SilkS", "user", Some("B.Silkscreen")),
        layer(37, "F.SilkS", "user", Some("F.Silkscreen")),
        layer(38, "B.Mask", "user", None),
        layer(39, "F.Mask", "user", None),
        layer(40, "Dwgs.User", "user", Some("User.Drawings")),
        layer(41, "Cmts.User", "user", Some("User.Comments")),
        layer(42, "Eco1.User", "user", Some("User.Eco1")),
        layer(43, "Eco2.User", "user", Some("User.Eco2")),
        layer(44, "Edge.Cuts", "user", None),
        layer(45, "Margin", "user", None),
        layer(46, "B.CrtYd", "user", Some("B.Courtyard")),
        layer(47, "F.CrtYd", "user", Some("F.Courtyard")),
        layer(48, "B.Fab", "user", None),
        layer(49, "F.Fab", "user", None),
    ]);
    Sexp::list("layers", layers)
}

fn layer(id: i32, name: &str, kind: &str, alias: Option<&str>) -> Sexp {
    // KiCad's layer syntax is `(<id> "<name>" <kind>)` — the
    // numeric id sits in the head slot. Build accordingly.
    let mut children = vec![Sexp::str(name), Sexp::atom(kind)];
    if let Some(a) = alias {
        children.push(Sexp::str(a));
    }
    Sexp::list(id.to_string(), children)
}

/// Minimum `(setup ...)` block KiCad 10 will accept. Default trace
/// widths and clearances; the via minima come from the manufacturer profile.
///
/// `via_min_size` was a fixed 0.60 mm while the project file, the router and
/// Synth's own check all worked to the profile's 0.56 mm (a 0.30 mm drill and a
/// 0.13 mm ring each side). `kicad-cli` applies the board file's figure, so
/// every via the router placed at the legal 0.56 mm was reported as an error
/// and the board was refused.
fn build_setup(profile: &synth_drc::ManufacturerProfile) -> Sexp {
    let via_min_size =
        synth_geometry::nm_to_mm(profile.min_drill_diameter_nm + 2 * profile.min_annular_ring_nm);
    let via_min_drill = synth_geometry::nm_to_mm(profile.min_drill_diameter_nm);
    Sexp::list(
        "setup",
        vec![
            Sexp::list("last_trace_width", vec![num(0.127)]),
            Sexp::list("trace_clearance", vec![num(0.127)]),
            Sexp::list("pad_to_mask_clearance", vec![num(0.0)]),
            Sexp::list("solder_mask_min_width", vec![num(0.0)]),
            Sexp::list(
                "allow_soldermask_bridges_in_footprints",
                vec![Sexp::atom("yes")],
            ),
            // Keep these minima aligned with the router's default signal
            // width and the KiCad board defaults used during native DRC.
            Sexp::list("trace_min", vec![num(0.127)]),
            // 0.127 mm is the default profile's copper clearance; the
            // router enforces this same value when choosing grid moves.
            Sexp::list("clearance_min", vec![num(0.127)]),
            Sexp::list("via_min_size", vec![num(via_min_size)]),
            Sexp::list("via_min_drill", vec![num(via_min_drill)]),
        ],
    )
}

/// [`build_setup`], with the declared stackup (if any) as its first entry.
fn build_setup_with(stackup: Option<&Stackup>, profile: &synth_drc::ManufacturerProfile) -> Sexp {
    let mut setup = build_setup(profile);
    if let (Some(stackup), Sexp::List { children, .. }) = (stackup, &mut setup) {
        children.insert(0, build_stackup(stackup));
    }
    setup
}

/// Board thickness: the declared stack's total, else the fixed default.
fn board_thickness(stackup: Option<&Stackup>) -> Sexp {
    stackup.map_or_else(
        || num(DEFAULT_THICKNESS_MM),
        |s| Sexp::atom(fixed_6(s.total_thickness().0)),
    )
}

/// KiCad `(stackup ...)` for a declared stack. Dielectrics are typed
/// `dielectric` because the language does not say core or prepreg.
fn build_stackup(stackup: &Stackup) -> Sexp {
    let copper_count = stackup.copper_count();
    let (mut copper_seen, mut dielectric_seen) = (0, 0);
    let layers = stackup
        .layers
        .iter()
        .map(|layer| match layer {
            StackupLayer::Copper { thickness, .. } => {
                let name = match copper_seen {
                    0 => "F.Cu".to_string(),
                    n if n + 1 == copper_count => "B.Cu".to_string(),
                    n => format!("In{n}.Cu"),
                };
                copper_seen += 1;
                Sexp::list(
                    "layer",
                    vec![
                        Sexp::str(name),
                        str_pair("type", "copper"),
                        Sexp::list("thickness", vec![Sexp::atom(fixed_6(thickness.0))]),
                    ],
                )
            }
            StackupLayer::Insulator {
                thickness,
                er,
                material,
                ..
            } => {
                dielectric_seen += 1;
                let mut children = vec![
                    Sexp::str(format!("dielectric {dielectric_seen}")),
                    str_pair("type", "dielectric"),
                    Sexp::list("thickness", vec![Sexp::atom(fixed_6(thickness.0))]),
                ];
                children.extend(material.as_ref().map(|m| str_pair("material", m.as_str())));
                children.push(Sexp::list("epsilon_r", vec![Sexp::atom(fixed_6(er.0))]));
                Sexp::list("layer", children)
            }
        })
        .collect();
    Sexp::list("stackup", layers)
}

/// An integer in millionths (nanometres of a millimetre value, or a
/// micro-unit ratio) as a decimal with trailing zeros trimmed. `num`
/// stops at 4 decimals, which would round a 1.4 mil copper foil.
#[allow(clippy::cast_precision_loss)]
fn fixed_6(micro: i64) -> String {
    let s = format!("{:.6}", micro as f64 / 1e6);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Top-level `(net_class ...)` definitions conforming to KiCad 10 grammar.
///
/// Declared classes (`netclass "PWR" { … }` in source) are emitted
/// verbatim — one `net_class` per declaration, carrying the nets
/// joined to it via `class "PWR"` — so the width/clearance contract
/// the designer wrote is what KiCad DRC enforces. Nets without a
/// join fall back to the heuristic `Default` / `Power` buckets; a
/// heuristic bucket whose name matches a declared class merges into
/// it instead of emitting a duplicate.
///
/// Power nets are identified by topological inference via [`synth_ir::infer_power_domains`]
/// — this correctly handles nets whose names are opaque (`net_0`, `net_1`, …) but whose
/// connected pins carry `PowerOutput` / `GroundReference` electrical types.
///
/// One declared `netclass` with its KiCad rules and joined nets,
/// in source declaration order.
struct DeclaredGroup {
    name: String,
    trace_width_mm: f64,
    clearance_mm: f64,
    via_diameter_mm: f64,
    via_drill_mm: f64,
    pair: Option<(f64, f64)>,
    nets: Vec<String>,
}

pub(crate) fn declared_pair_geometry(
    board: &Board,
    class: &synth_ir::NetClass,
) -> Option<(f64, f64)> {
    let min_gap = synth_drc::ManufacturerProfile::jlc_standard().min_copper_clearance_nm;
    let (width, gap) = board.netclass_pair_geometry(&class.name, synth_ir::Length(min_gap))?;
    Some((width.to_mm(), gap.to_mm()))
}

#[derive(Clone, Copy, PartialEq)]
struct DerivedGeometry {
    impedance: synth_ir::Impedance,
    width_mm: f64,
    gap_mm: Option<f64>,
}

/// A declared class's trace width for the router.
///
/// What the author wrote, or — when the class declares a `current` but no width
/// — the IPC-2221 width that current needs, so the engine widens a
/// high-current rail instead of routing it at the default and failing the
/// ampacity check afterwards. Falls back to the profile default.
///
/// The width is sized against the board's own manufacturer profile and declared
/// stackup — the same inputs the ampacity DRC uses — so a custom profile (a
/// different allowed temperature rise) or a thinner stackup is not under-sized
/// and then failed after routing. The thinnest copper in the stackup is the
/// worst case the router can pick, so a rail stays wide enough on every layer.
fn declared_trace_width_mm(class: &synth_ir::NetClass, board: &Board) -> f64 {
    if let Some(width) = class.trace_width {
        return width.to_mm();
    }
    if let Some(current) = class.current {
        let profile = synth_drc::ManufacturerProfile::from_name(
            board.manufacturer.as_deref().unwrap_or("jlc-standard"),
        );
        let delta_t_c =
            f64::from(i32::try_from(profile.ampacity_delta_t_milli_c).unwrap_or(10_000)) / 1000.0;
        let thickness_nm = synth_geometry::StackupProfile::for_board(board)
            .layers
            .iter()
            .map(|layer| layer.copper_thickness.0)
            .min()
            .unwrap_or_else(|| mm_to_nm(0.035));
        let width_nm = synth_drc::required_width_nm(current.0, thickness_nm, delta_t_c);
        return nm_to_mm(width_nm);
    }
    0.127
}

fn derived_controlled_geometry(board: &Board) -> Vec<(synth_ir::NetId, DerivedGeometry)> {
    // Declared stackup only: deriving controlled geometry from a substituted
    // default would silently change the exported board for a design that
    // declares none. `StackupProfile::declared` keeps that contract explicit.
    let Some(outer) = synth_geometry::StackupProfile::declared(board)
        .and_then(|profile| profile.outer_microstrip_params())
    else {
        return Vec::new();
    };
    let profile = synth_drc::ManufacturerProfile::jlc_standard();
    let min_width_mm = synth_ir::Length(profile.min_trace_width_nm).to_mm();
    let min_gap_mm = synth_ir::Length(profile.min_copper_clearance_nm).to_mm();
    board
        .impedance_legs()
        .filter(|(pos, neg, ..)| {
            board.declared_trace_width(pos).is_none() && board.declared_trace_width(neg).is_none()
        })
        .filter_map(|(pos, neg, _, impedance)| {
            if pos.id == neg.id {
                let width_mm = synth_geometry::derive_microstrip_width_mm(
                    impedance.to_ohms(),
                    &outer,
                    min_width_mm,
                )
                .ok()?;
                let geometry = DerivedGeometry {
                    impedance,
                    width_mm,
                    gap_mm: None,
                };
                return Some(vec![(pos.id, geometry)]);
            }
            let pair = synth_geometry::derive_coupled_microstrip(
                impedance.to_ohms(),
                &outer,
                min_width_mm,
                min_gap_mm,
            )
            .ok()?;
            let geometry = DerivedGeometry {
                impedance,
                width_mm: pair.width_mm,
                gap_mm: Some(pair.gap_mm),
            };
            Some(vec![(pos.id, geometry), (neg.id, geometry)])
        })
        .flatten()
        .collect()
}

/// The clearance a net class takes when the author declared none: the board's
/// manufacturer minimum, from the profile the project file and the router
/// resolve, so the classes written here say what the project file says.
fn default_class_clearance_mm(board: &Board) -> f64 {
    synth_geometry::nm_to_mm(crate::export::manufacturer_profile(board).min_copper_clearance_nm)
}

fn build_netclasses(net_table: &[(u32, String)], board: &Board) -> Vec<Sexp> {
    // Topological power-domain map: classifies every net by connected pin types,
    // not by string name. This is the authoritative source for Power/GND class assignment.
    let domain_map = synth_ir::infer_power_domains(board);
    let is_rp2350_board = board.components.iter().any(|component| {
        component
            .part
            .as_ref()
            .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("rp2350"))
    });

    // Secondary name-based power classifier: fires when the topological domain map
    // cannot classify a net (no connected pins with ElectricalType information).
    // This covers boards with incomplete registry data and unit tests with synthetic nets.
    let is_power_name = |name: &str| {
        let n = name.to_ascii_lowercase();
        n.contains("vbus")
            || n.contains("vbat")
            || n.contains("vsys")
            || n.contains("vin")
            || n.contains("vout")
            || n.contains("vcc")
            || n.contains("vdd")
            || n.contains("3v3")
            || n.contains("5v")
            || n.contains("gnd")
            || n.contains("vss")
            || n.contains("0v")
            || n.contains("power")
    };

    let mut default_nets = Vec::new();
    let mut power_nets = Vec::new();
    let controlled = derived_controlled_geometry(board);
    let mut controlled_groups: Vec<(DerivedGeometry, Vec<String>)> = Vec::new();

    // Declared classes in source order. A join naming an undeclared
    // class cannot arrive here through lowering (it is
    // `E-SYNTH-NAME-006` and lowers to no join), but a hand-built
    // board may carry one — those nets fall back to the heuristic
    // buckets below.
    let mut declared: Vec<DeclaredGroup> = board
        .netclasses
        .iter()
        .map(|nc| DeclaredGroup {
            name: nc.name.clone(),
            trace_width_mm: declared_trace_width_mm(nc, board),
            clearance_mm: nc.clearance.map_or(0.127, synth_ir::Length::to_mm),
            via_diameter_mm: nc.effective_via_diameter().to_mm(),
            via_drill_mm: nc.effective_via_drill().to_mm(),
            pair: declared_pair_geometry(board, nc),
            nets: Vec::new(),
        })
        .collect();

    for (pcb_net_id, name) in net_table {
        if name.is_empty() {
            continue;
        }
        // pcb_net_id is 1-indexed; IR NetId is 0-indexed.
        let ir_net_idx = pcb_net_id.saturating_sub(1);
        if let Some(&(_, geometry)) = controlled.iter().find(|(id, _)| id.0 == ir_net_idx) {
            match controlled_groups.iter_mut().find(|g| g.0 == geometry) {
                Some(group) => group.1.push(name.clone()),
                None => controlled_groups.push((geometry, vec![name.clone()])),
            }
            continue;
        }
        // Explicit join first: the designer stated the class.
        if let Some(class) = board
            .net(synth_ir::NetId(ir_net_idx))
            .and_then(|n| n.netclass.as_deref())
        {
            if let Some(group) = declared.iter_mut().find(|g| g.name == class) {
                group.nets.push(name.clone());
                continue;
            }
            // Undeclared join on a hand-built board: heuristic fallback.
        }
        // Primary: topological inference via power domain map (works with opaque net names).
        // Fallback: name-based heuristic (works for boards with semantic names but missing pins).
        let is_power_net = domain_map
            .get(synth_ir::NetId(ir_net_idx))
            .map_or_else(|| is_power_name(name), |d| d.is_rail() || d.is_ground());

        // A heuristic bucket whose name matches a declared class
        // merges into it so KiCad never sees two classes with one name.
        let mut push_heuristic = |bucket: &str, net: &String| -> bool {
            if let Some(group) = declared.iter_mut().find(|g| g.name == bucket) {
                group.nets.push(net.clone());
                false
            } else {
                true
            }
        };
        if is_power_net {
            if push_heuristic("Power", name) {
                power_nets.push(name.clone());
            }
        } else if push_heuristic("Default", name) {
            default_nets.push(name.clone());
        }
    }

    let mut classes = Vec::new();

    // Default Net Class (signals). Skipped when the designer declared
    // their own "Default" — its heuristic members already joined it.
    if !declared.iter().any(|g| g.name == "Default") {
        let mut default_args = vec![
            Sexp::str("Default"),
            Sexp::str("Default net class"),
            Sexp::list("clearance", vec![num(default_class_clearance_mm(board))]),
            Sexp::list("trace_width", vec![num(0.127)]),
            Sexp::list("via_dia", vec![num(0.60)]),
            Sexp::list("via_drill", vec![num(0.30)]),
        ];
        for name in default_nets {
            default_args.push(Sexp::list("add_net", vec![Sexp::str(&name)]));
        }
        classes.push(Sexp::list("net_class", default_args));
    }

    // Declared classes, in source order (even empty ones — the
    // declaration itself is design intent KiCad should show).
    for group in &declared {
        let mut args = vec![
            Sexp::str(&group.name),
            Sexp::str(format!("Synth netclass {}", group.name)),
            Sexp::list("clearance", vec![num(group.clearance_mm)]),
            Sexp::list("trace_width", vec![num(group.trace_width_mm)]),
            Sexp::list("via_dia", vec![num(group.via_diameter_mm)]),
            Sexp::list("via_drill", vec![num(group.via_drill_mm)]),
        ];
        if let Some((width_mm, gap_mm)) = group.pair {
            args.push(Sexp::list("diff_pair_width", vec![num(width_mm)]));
            args.push(Sexp::list("diff_pair_gap", vec![num(gap_mm)]));
        }
        for name in &group.nets {
            args.push(Sexp::list("add_net", vec![Sexp::str(name)]));
        }
        classes.push(Sexp::list("net_class", args));
    }

    // Power Net Class (PDN). Skipped when "Power" was declared —
    // same merge as Default above.
    if !power_nets.is_empty() && !declared.iter().any(|g| g.name == "Power") {
        let power_width = if is_rp2350_board { 0.127 } else { 0.50 };
        // The project file writes this class at the manufacturer minimum, which
        // is what the router honours. A larger value here is the one KiCad's
        // DRC enforces, so copper the router placed legally is reported.
        let power_clearance = default_class_clearance_mm(board);
        let mut power_args = vec![
            Sexp::str("Power"),
            Sexp::str("Power delivery network"),
            Sexp::list("clearance", vec![num(power_clearance)]),
            Sexp::list("trace_width", vec![num(power_width)]),
            Sexp::list("via_dia", vec![num(0.80)]),
            Sexp::list("via_drill", vec![num(0.40)]),
        ];
        for name in power_nets {
            power_args.push(Sexp::list("add_net", vec![Sexp::str(&name)]));
        }
        classes.push(Sexp::list("net_class", power_args));
    }

    for (geometry, nets) in controlled_groups {
        let ohms = geometry.impedance.to_ohms();
        let width = num(geometry.width_mm);
        let mut args = match geometry.gap_mm {
            Some(gap_mm) => vec![
                Sexp::str(format!("ZDIFF_{ohms}OHM")),
                Sexp::str(format!(
                    "Differential {ohms} ohm outer-layer microstrip pair, width and gap estimated from the stackup"
                )),
                Sexp::list("clearance", vec![num(gap_mm)]),
                Sexp::list("trace_width", vec![width.clone()]),
                Sexp::list("diff_pair_width", vec![width]),
                Sexp::list("diff_pair_gap", vec![num(gap_mm)]),
            ],
            None => vec![
                Sexp::str(format!("Z0_{ohms}OHM")),
                Sexp::str(format!(
                    "Single-ended {ohms} ohm outer-layer microstrip, width estimated from the stackup"
                )),
                Sexp::list("clearance", vec![num(0.2)]),
                Sexp::list("trace_width", vec![width]),
            ],
        };
        args.extend([
            Sexp::list("via_dia", vec![num(0.60)]),
            Sexp::list("via_drill", vec![num(0.30)]),
        ]);
        for name in nets {
            args.push(Sexp::list("add_net", vec![Sexp::str(&name)]));
        }
        classes.push(Sexp::list("net_class", args));
    }

    classes
}

fn build_edge_cuts(placement: &Placement, project: &Uuid) -> Vec<Sexp> {
    let r = placement.board_outline;
    let min_x = nm_to_mm(r.min.x_nm);
    let min_y = nm_to_mm(r.min.y_nm);
    let max_x = nm_to_mm(r.max.x_nm);
    let max_y = nm_to_mm(r.max.y_nm);
    let corners = [
        ((min_x, min_y), (max_x, min_y), "top"),
        ((max_x, min_y), (max_x, max_y), "right"),
        ((max_x, max_y), (min_x, max_y), "bottom"),
        ((min_x, max_y), (min_x, min_y), "left"),
    ];
    corners
        .iter()
        .map(|(start, end, tag)| {
            let line_uuid = derive_entity_uuid(project, "edge_cut", tag);
            Sexp::list(
                "gr_line",
                vec![
                    Sexp::list("start", vec![num(start.0), num(start.1)]),
                    Sexp::list("end", vec![num(end.0), num(end.1)]),
                    Sexp::list(
                        "stroke",
                        vec![
                            Sexp::list("width", vec![num(EDGE_CUT_WIDTH_MM)]),
                            Sexp::list("type", vec![Sexp::atom("solid")]),
                        ],
                    ),
                    Sexp::list("layer", vec![Sexp::str("Edge.Cuts")]),
                    str_pair("uuid", line_uuid.to_string()),
                ],
            )
        })
        .collect()
}

/// Half of a part's unrotated footprint width and height, in millimetres.
/// Falls back to a size by pin count when the part has no dimensions.
fn footprint_half_extent_mm(part: &synth_registry::Part) -> (f64, f64) {
    let (w, h) = part.footprint_dimensions.as_ref().map_or_else(
        || {
            if part.pins.len() <= 2 {
                (2.0, 2.0)
            } else if part.pins.len() <= 8 {
                (3.0, 3.0)
            } else {
                (7.62, 36.0)
            }
        },
        |dims| (dims.width_mm, dims.height_mm),
    );
    (w / 2.0, h / 2.0)
}

/// Everything the label placer needs about each placed part.
fn label_parts(
    board: &Board,
    placements_by_id: &HashMap<ComponentId, &ComponentPlacement>,
) -> Vec<silk_labels::LabelPart> {
    board
        .components
        .iter()
        .filter_map(|component| {
            let part = component.part.as_ref()?;
            let placement = placements_by_id.get(&component.id)?;
            let (_, (court_w, court_h)) = synth_layout::pcb_courtyard_geometry_for_part(part);
            let (w, h) = if placement.rotation.swaps_extents() {
                (court_h, court_w)
            } else {
                (court_w, court_h)
            };
            let (half_w, half_h) = (mm_to_nm(w) / 2, mm_to_nm(h) / 2);
            let c = placement.center;
            let (side, hidden) = part_label_hints(part);
            Some(silk_labels::LabelPart {
                id: component.id,
                refdes: component.refdes.clone(),
                origin: synth_place::footprint_origin(Some(part), placement),
                rotation: placement.rotation,
                courtyard: synth_geometry::Rect::new(
                    synth_geometry::Point::new(c.x_nm - half_w, c.y_nm - half_h),
                    synth_geometry::Point::new(c.x_nm + half_w, c.y_nm + half_h),
                ),
                half_extent_mm: footprint_half_extent_mm(part),
                side,
                hidden,
            })
        })
        .collect()
}

/// The label side and visibility a part file asks for.
fn part_label_hints(part: &synth_registry::Part) -> (Option<silk_labels::Side>, bool) {
    use synth_registry::LabelSide;
    let dims = part.footprint_dimensions.as_ref();
    let side = dims.and_then(|d| d.label_side).map(|s| match s {
        LabelSide::Above => silk_labels::Side::Above,
        LabelSide::Below => silk_labels::Side::Below,
        LabelSide::Left => silk_labels::Side::Left,
        LabelSide::Right => silk_labels::Side::Right,
    });
    (side, dims.and_then(|d| d.label_hidden).unwrap_or(false))
}

/// The placement sidecar's label entries, keyed by component instead of
/// designator. A designator that names no part is ignored.
fn label_overrides_by_id(
    board: &Board,
    overrides: &HashMap<String, synth_layout::sidecar::SidecarLabel>,
) -> HashMap<ComponentId, silk_labels::LabelOverride> {
    board
        .components
        .iter()
        .filter_map(|component| {
            let o = overrides.get(&component.refdes)?;
            Some((component.id, label_override(o)))
        })
        .collect()
}

/// A sidecar label entry as the placer sees it. A missing `dx` or `dy` counts
/// as 0; with neither there is no offset, so `angle` and `priority` have
/// nothing to apply to.
fn label_override(entry: &synth_layout::sidecar::SidecarLabel) -> silk_labels::LabelOverride {
    let offset_mm = (entry.dx.is_some() || entry.dy.is_some())
        .then(|| (entry.dx.unwrap_or(0.0), entry.dy.unwrap_or(0.0)));
    silk_labels::LabelOverride {
        offset_mm,
        angle_deg: entry.angle,
        hide: entry.hide,
        hard: entry.priority == synth_layout::sidecar::OverridePriority::Hard,
    }
}

fn build_footprint_instance(
    component: &synth_ir::Component,
    part: &synth_registry::Part,
    placement: &ComponentPlacement,
    lib_id: &str,
    project: &Uuid,
    pad_net_lookup: &PadNetLookup,
    ref_label: silk_labels::LabelPos,
) -> Sexp {
    // Route the origin through the same helper the external placement
    // contract uses (`synth_place::to_external`), so the exported `(at x y)`
    // and the published placement JSON cannot drift apart.
    let origin = synth_place::footprint_origin(Some(part), placement);
    let x = nm_to_mm(origin.x_nm);
    let y = nm_to_mm(origin.y_nm);
    let angle = f64::from(placement.rotation.degrees());
    let layer_name = match placement.layer {
        Layer::Top => "F.Cu",
        _ => "B.Cu",
    };
    // KiCad reads a pad's angle as its absolute orientation, so a top-side
    // footprint's rotation has to be added to every pad (absmach/synth#199).
    // The bottom side keeps the library angles: its pads are not mirrored
    // here either, which is a separate gap.
    let pad_angle = if placement.layer == Layer::Top {
        angle
    } else {
        0.0
    };
    let fp_uuid = derive_entity_uuid(project, "footprint", &component.refdes);

    // Resolve the bundled/user footprint body up front so we can decide whether
    // the module is fully inlined (no `(footprint "lib:name")` reference, which
    // would otherwise force KiCad to look the library up in fp-lib-table).
    let inlined_body = synth_layout::kicad_footprint_loader::inline_body(lib_id);

    let mut at_args = vec![num(x), num(y)];
    if placement.rotation != Rotation::Zero {
        at_args.push(num(angle));
    }

    let (half_w, half_h) = footprint_half_extent_mm(part);
    let text_margin = silk_labels::DEFAULT_MARGIN_MM;

    // The reference label's position comes from the label placer; the hidden
    // value text keeps its fixed offset.
    let silk_labels::LabelPos {
        local_x_mm: ref_local_x,
        local_y_mm: ref_local_y,
        angle_deg: ref_text_angle,
        hidden: ref_hidden,
    } = ref_label;

    let (val_local_x, val_local_y, val_text_angle) = match placement.rotation {
        Rotation::Zero => (0.0, half_h + text_margin, 0.0),
        Rotation::Ninety => (half_w + text_margin, 0.0, -90.0),
        Rotation::OneEighty => (0.0, -(half_h + text_margin), 180.0),
        Rotation::TwoSeventy => (-(half_w + text_margin), 0.0, 90.0),
    };

    let silk_layer = match placement.layer {
        Layer::Top => "F.SilkS",
        _ => "B.SilkS",
    };

    let fab_layer = match placement.layer {
        Layer::Top => "F.Fab",
        _ => "B.Fab",
    };

    // Deterministic UUIDs for the Reference/Value text fields. Without
    // them, KiCad's `--save-board` DRC gate fills in random v4 UUIDs on
    // every export, breaking byte-identical re-exports (plan §6.2).
    let ref_prop_uuid = derive_entity_uuid(
        project,
        "fp_property",
        &format!("{}:Reference", component.refdes),
    );
    let val_prop_uuid = derive_entity_uuid(
        project,
        "fp_property",
        &format!("{}:Value", component.refdes),
    );
    // Empty Datasheet/Description slots, mirroring exactly what KiCad's
    // `--save-board` backfills for instances that lack them (empty value,
    // `(at 0 0 0)`, fab layer, hidden, 1.27 mm font). Without these, the
    // gate fills them with fresh random v4 UUIDs on every export and the
    // plan §6.2 byte-identical guarantee breaks. The native
    // `lib_footprint_mismatch` check tolerates them: it already tolerates
    // the identical blocks KiCad itself backfills (the gate is clean).
    let datasheet_prop_uuid = derive_entity_uuid(
        project,
        "fp_property",
        &format!("{}:Datasheet", component.refdes),
    );
    let description_prop_uuid = derive_entity_uuid(
        project,
        "fp_property",
        &format!("{}:Description", component.refdes),
    );
    let mpn_prop_uuid =
        derive_entity_uuid(project, "fp_property", &format!("{}:MPN", component.refdes));
    let lcsc_prop_uuid = derive_entity_uuid(
        project,
        "fp_property",
        &format!("{}:LCSC", component.refdes),
    );
    let doc_property = |name: &str, uuid: &Uuid| {
        Sexp::list(
            "property",
            vec![
                Sexp::str(name),
                Sexp::str(""),
                Sexp::list("at", vec![num(0.0), num(0.0), num(0.0)]),
                Sexp::list("layer", vec![Sexp::str(fab_layer)]),
                Sexp::list("hide", vec![Sexp::atom("yes")]),
                str_pair("uuid", uuid.to_string()),
                Sexp::list(
                    "effects",
                    vec![Sexp::list(
                        "font",
                        vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                    )],
                ),
            ],
        )
    };
    let assignment_property = |name: &str, value: &str, uuid: &Uuid| {
        let mut property = doc_property(name, uuid);
        if let Sexp::List { children, .. } = &mut property {
            if let Some(Sexp::Str(current)) = children.get_mut(1) {
                *current = value.to_string();
            }
        }
        property
    };

    let mut reference_property = vec![
        Sexp::str("Reference"),
        Sexp::str(&component.refdes),
        Sexp::list(
            "at",
            vec![num(ref_local_x), num(ref_local_y), num(ref_text_angle)],
        ),
        Sexp::list("layer", vec![Sexp::str(silk_layer)]),
        str_pair("uuid", ref_prop_uuid.to_string()),
    ];
    if ref_hidden {
        reference_property.push(Sexp::list("hide", vec![Sexp::atom("yes")]));
    }
    reference_property.push(Sexp::list(
        "effects",
        vec![Sexp::list(
            "font",
            vec![Sexp::list("size", vec![num(1.0), num(1.0)])],
        )],
    ));

    let mut children = vec![
        Sexp::list("layer", vec![Sexp::str(layer_name)]),
        str_pair("uuid", fp_uuid.to_string()),
        Sexp::list("at", at_args),
        Sexp::list("property", reference_property),
        Sexp::list(
            "property",
            vec![
                Sexp::str("Value"),
                // Schematic-quality plan Phase A1: same `value` → `mpn` →
                // `(no value)` chain as the schematic emitter, never the
                // registry part id.
                Sexp::str(
                    component
                        .value
                        .as_deref()
                        .or_else(|| component.part.as_ref().and_then(|p| p.mpn.as_deref()))
                        .unwrap_or("(no value)"),
                ),
                Sexp::list(
                    "at",
                    vec![num(val_local_x), num(val_local_y), num(val_text_angle)],
                ),
                Sexp::list("layer", vec![Sexp::str(fab_layer)]),
                str_pair("uuid", val_prop_uuid.to_string()),
                Sexp::list("hide", vec![Sexp::atom("yes")]),
                Sexp::list(
                    "effects",
                    vec![Sexp::list(
                        "font",
                        vec![Sexp::list("size", vec![num(1.0), num(1.0)])],
                    )],
                ),
            ],
        ),
        doc_property("Datasheet", &datasheet_prop_uuid),
        doc_property("Description", &description_prop_uuid),
        assignment_property(
            "MPN",
            component
                .part
                .as_ref()
                .and_then(|part| part.mpn.as_deref())
                .unwrap_or(""),
            &mpn_prop_uuid,
        ),
        assignment_property(
            "LCSC",
            component
                .part
                .as_ref()
                .and_then(|part| part.lcsc_pn.as_deref())
                .unwrap_or(""),
            &lcsc_prop_uuid,
        ),
    ];

    let has_model = if let Some(body) = inlined_body.as_deref() {
        // Strip the outer `(footprint "name" ...)` head so the geometry is
        // embedded self-contained under the module. Leaving the head in place
        // makes pcbnew try to resolve a footprint *library* named after `name`
        // (e.g. a user-generated `<id>.pretty`), which is not registered in the
        // project's fp-lib-table and produces a spurious DRC error.
        let body = strip_footprint_head(body);
        let body = add_footprint_angle_to_pads(&body, pad_angle);
        let body_with_nets = inject_pad_nets(&body, component.id, pad_net_lookup);
        let body_with_nets = ensure_body_uuids(&body_with_nets, project, lib_id);
        let has_model = body_with_nets.contains("(model ");
        children.push(Sexp::Raw(body_with_nets));
        has_model
    } else {
        // No inline body (no bundled KiCad footprint): synthesize a
        // fallback footprint from the part's `footprint_dimensions` +
        // pin list. Pad positions come from the shared
        // `synth_part_pads` helper so they match exactly what the
        // router stamped — traces terminate on real, netted copper
        // rather than dangling in space.
        if let Some(synth_pads) = synth_layout::kicad_footprint_loader::synth_part_pads(part) {
            for pad in &synth_pads {
                if let Some((net_id, net_name)) =
                    pad_net_lookup.get(&(component.id, pad.number.clone()))
                {
                    // Deterministic pad UUID (see `ensure_body_uuids`):
                    // without it KiCad's `--save-board` gate fills a random
                    // v4 UUID per export.
                    let pad_uuid = derive_entity_uuid(
                        project,
                        "pad",
                        &format!("{}:{}", component.refdes, pad.number),
                    );
                    children.push(Sexp::list(
                        "pad",
                        vec![
                            Sexp::str(&pad.number),
                            Sexp::atom("smd"),
                            Sexp::atom("rect"),
                            Sexp::list("at", pad_at_args(pad.center_mm, pad_angle)),
                            Sexp::list("size", vec![num(pad.size_mm.0), num(pad.size_mm.1)]),
                            Sexp::list(
                                "layers",
                                vec![Sexp::str("F.Cu"), Sexp::str("F.Paste"), Sexp::str("F.Mask")],
                            ),
                            Sexp::list(
                                "net",
                                vec![Sexp::atom(net_id.to_string()), Sexp::str(net_name)],
                            ),
                            str_pair("uuid", pad_uuid.to_string()),
                        ],
                    ));
                }
            }
        }
        false
    };

    // Every footprint instance needs a `(footprint "lib:name")` head so KiCad
    // can parse the module. For inlined (user-imported) footprints the geometry
    // is embedded inline, so a real library lookup is unnecessary — but the
    // reference must still be present. The exporter registers the user footprint
    // directory in the project's fp-lib-table (see `export_with_sidecars`) so the
    // reference resolves without a spurious "library not found" warning.
    children.insert(0, Sexp::str(lib_id));
    if !has_model {
        if let Some(model_sexp) = build_3d_model_sexp(lib_id) {
            children.push(model_sexp);
        }
    }

    Sexp::list("footprint", children)
}

/// Build a standalone `.kicad_mod` file body for a part that has no
/// real footprint (no bundled/user `.kicad_mod`) — the same pad
/// geometry [`build_footprint_instance`]'s fallback branch embeds
/// inline, but as a proper library module (full head, no per-board
/// net assignments) so it can be written to disk under
/// `<project>.pretty/` and registered in `fp-lib-table`. Without this,
/// the embedded instance's `(footprint "synth:<id>" ...)` lib_id
/// resolves to nothing, and native `kicad-cli pcb drc` flags
/// `lib_footprint_issues` even though the geometry renders fine.
///
/// Returns `None` when the part has no pins/dimensions to synthesize
/// from (mirrors [`synth_layout::kicad_footprint_loader::synth_part_pads`]).
///
/// Deliberately carries *only* `version`/`generator`/`layer` plus
/// pads — no `attr`/`fp_text` — because `build_footprint_instance`'s
/// fallback branch embeds only pads too. `kicad-cli pcb drc`'s
/// `lib_footprint_mismatch` check compares this file's content
/// against the embedded instance verbatim (modulo the fields
/// `INSTANCE_OVERRIDES` strips: version/generator/generator_version/
/// layer/descr/tags), so any field present in one but not the other
/// trips it.
#[must_use]
pub fn build_synthesized_footprint_module(
    part: &synth_registry::Part,
    lib_id: &str,
) -> Option<Sexp> {
    let pads = synth_layout::kicad_footprint_loader::synth_part_pads(part)?;

    let mut children = vec![
        Sexp::str(lib_id),
        pair("version", Sexp::atom("20260206")),
        str_pair("generator", "synth-eda"),
        str_pair("generator_version", "10.0"),
        Sexp::list("layer", vec![Sexp::str("F.Cu")]),
    ];

    for pad in &pads {
        children.push(Sexp::list(
            "pad",
            vec![
                Sexp::str(&pad.number),
                Sexp::atom("smd"),
                Sexp::atom("rect"),
                Sexp::list("at", vec![num(pad.center_mm.0), num(pad.center_mm.1)]),
                Sexp::list("size", vec![num(pad.size_mm.0), num(pad.size_mm.1)]),
                Sexp::list(
                    "layers",
                    vec![Sexp::str("F.Cu"), Sexp::str("F.Paste"), Sexp::str("F.Mask")],
                ),
            ],
        ));
    }

    Some(Sexp::list("footprint", children))
}

fn build_3d_model_sexp(lib_id: &str) -> Option<Sexp> {
    let (lib, name) = lib_id.split_once(':')?;

    let path = format!("${{KICAD10_3DMODEL_DIR}}/{lib}.3dshapes/{name}.step");
    Some(Sexp::list(
        "model",
        vec![
            Sexp::str(&path),
            Sexp::list(
                "at",
                vec![Sexp::list("xyz", vec![num(0.0), num(0.0), num(0.0)])],
            ),
            Sexp::list(
                "scale",
                vec![Sexp::list("xyz", vec![num(1.0), num(1.0), num(1.0)])],
            ),
            Sexp::list(
                "rotate",
                vec![Sexp::list("xyz", vec![num(0.0), num(0.0), num(0.0)])],
            ),
        ],
    ))
}

/// Post-process the inlined footprint body, finding each
/// `(pad "<number>" ...)` block and injecting `(net X "name")`
/// before its closing paren when the (component, pad-number)
/// pair is in the IR netlist. KiCad accepts net declarations
/// anywhere inside a pad block.
///
/// The scanner is byte-level — we don't have a full sexp parser
/// in this crate, but a balanced-paren walk is enough because
/// pad blocks never contain quoted strings whose contents
/// include parens at sufficient nesting to fool us.
/// Return the interior of a KiCad `(footprint "name" …)` s-expression with the
/// outer `(footprint "name"` head removed, so it can be embedded directly under
/// a `(module …)` without leaving a dangling library reference.
/// Footprint-body children that carry a `(uuid ...)` in KiCad 10.
/// Anything else (e.g. `model`, `attr`, `descr`) must NOT gain one.
const BODY_UUID_HEADS: &[&str] = &[
    "pad",
    "property",
    "fp_text",
    "fp_line",
    "fp_rect",
    "fp_circle",
    "fp_arc",
    "fp_poly",
    "fp_curve",
];

/// Fill in deterministic UUIDs for inlined footprint-body blocks that
/// lack them (stock-library drawing primitives, pads and properties
/// predate mandatory UUIDs).
///
/// Without this, KiCad's `--save-board` DRC gate assigns a fresh random v4
/// UUID to every such block on each export, so two exports of the same
/// board differ and the plan §6.2 byte-identical guarantee breaks. UUIDs
/// are v5 over `(project, "fp_body:{lib_id}:{kind}:{key}#{n}")`, hence
/// stable across runs, thread counts, and machines. Blocks that already
/// carry `(uuid ...)` are left untouched.
fn ensure_body_uuids(body: &str, project: &Uuid, lib_id: &str) -> String {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    ensure_level(body, project, lib_id, "", &mut seen, false)
}

/// One level of [`ensure_body_uuids`]: walk the top-level blocks of `text`,
/// add deterministic UUIDs where missing, and recurse into each block's
/// children (footprint `fp_text` nests a `(point ...)` designator lock that
/// KiCad fills the same way). `path` is the `/`-joined ancestor chain so
/// nested names cannot collide with top-level ones; `seen` counts each
/// full path for duplicate siblings.
///
/// Nested levels only ever gain UUIDs on `point`: pad-nested `(property
/// pad_prop_mechanical)` must stay exactly as the stock library ships it —
/// KiCad 10.0's pad parser rejects anything else there.
fn ensure_level(
    text: &str,
    project: &Uuid,
    lib_id: &str,
    path: &str,
    seen: &mut std::collections::HashMap<String, usize>,
    nested: bool,
) -> String {
    // `point` blocks only ever occur nested (fp_text designator locks).
    const NESTED_UUID_HEADS: &[&str] = &["point"];
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 256);
    let mut i = 0;
    // Byte-slices are copied verbatim (never char-split), so non-ASCII in
    // values passes through untouched; `(` inside string literals does not
    // start a block.
    let mut seg_start = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i = skip_string(bytes, i);
            continue;
        }
        if bytes[i] != b'(' {
            i += 1;
            continue;
        }
        let Some(end) = matching_close_quoted(text, i + 1) else {
            break;
        };
        let block = &text[i..=end];
        let head = block
            .trim_start_matches('(')
            .split(|c: char| c.is_ascii_whitespace() || c == '(' || c == ')')
            .next()
            .unwrap_or("");
        let key = first_quoted_after(block, head).unwrap_or("").to_string();
        let full_path = format!("{path}/{head}:{key}");
        let mut block = block.to_string();
        // NB: only a *direct* `(uuid ...)` child counts — a nested one
        // (e.g. an fp_text whose `point` child already has one) must not
        // suppress the parent's own UUID.
        let whitelisted =
            (!nested && BODY_UUID_HEADS.contains(&head)) || NESTED_UUID_HEADS.contains(&head);
        if whitelisted && !has_direct_uuid(&block) {
            let counter = seen.entry(full_path.clone()).or_insert(0);
            let uuid = derive_entity_uuid(
                project,
                "fp_body",
                &format!("{lib_id}:{full_path}#{counter}"),
            );
            *counter += 1;
            block.pop();
            let _ = write!(block, " (uuid \"{uuid}\")");
            block.push(')');
        }
        // Recurse into children: everything between the `(head ...args`
        // opener and the final `)`. Leaf blocks (no inner `(`) come back
        // unchanged.
        if let Some(range) = inner_range(&block) {
            let fixed = ensure_level(
                &block[range.clone()],
                project,
                lib_id,
                &full_path,
                seen,
                true,
            );
            block.replace_range(range, &fixed);
        }
        out.push_str(&text[seg_start..i]);
        out.push_str(&block);
        i = end + 1;
        seg_start = i;
    }
    out.push_str(&text[seg_start..]);
    out
}

/// True when `block`'s direct children include a `(uuid ...)` node.
/// (A plain substring search would false-positive on a *nested* UUID, e.g.
/// an fp_text whose `point` child already has one while it does not.)
fn has_direct_uuid(block: &str) -> bool {
    let Some(range) = inner_range(block) else {
        return false;
    };
    let inner = &block[range];
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i = skip_string(bytes, i);
            continue;
        }
        if bytes[i] != b'(' {
            i += 1;
            continue;
        }
        let Some(end) = matching_close_quoted(inner, i + 1) else {
            return false;
        };
        let head = inner[i + 1..=end]
            .split(|c: char| c.is_ascii_whitespace() || c == '(' || c == ')')
            .next()
            .unwrap_or("");
        if head == "uuid" {
            return true;
        }
        i = end + 1;
    }
    false
}

/// Byte index just past the string literal opening at `i` (which must be
/// a `"` byte): handles `\\` and `\"` escapes.
fn skip_string(bytes: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => j += 2,
            b'"' => return j + 1,
            _ => j += 1,
        }
    }
    j
}

/// Byte range of a block's children: the text between the head + inline
/// args and the closing paren, or `None` for leaf blocks. Quote-aware so
/// `")"` inside a string cannot fake a boundary.
fn inner_range(block: &str) -> Option<std::ops::Range<usize>> {
    let bytes = block.as_bytes();
    // End of the `(head` opener.
    let mut k = 1;
    while k < bytes.len() && !bytes[k].is_ascii_whitespace() && bytes[k] != b'(' && bytes[k] != b')'
    {
        k += 1;
    }
    // First nested `(` outside of strings starts the children.
    let mut in_string = false;
    let mut j = k;
    while j < bytes.len() {
        let b = bytes[j];
        if in_string {
            if b == b'\\' {
                j += 2;
                continue;
            }
            if b == b'"' {
                in_string = false;
            }
            j += 1;
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'(' => return Some(k..block.len() - 1),
            b')' => return None,
            _ => {}
        }
        j += 1;
    }
    None
}

/// Quote-aware paren matcher: byte index of the `)` closing the list
/// opened just before `from`. String literals (`"..."`, with `\\` and
/// `\"` escapes) never affect depth, so values containing parens
/// (e.g. a Description) cannot corrupt the scan.
fn matching_close_quoted(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 1_i32;
    let mut in_string = false;
    let mut j = from;
    while j < bytes.len() {
        let b = bytes[j];
        if in_string {
            if b == b'\\' {
                j += 2;
                continue;
            }
            if b == b'"' {
                in_string = false;
            }
            j += 1;
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

/// First `"quoted"` value appearing after the `(head ` opener of `block`
/// (the pad number for `(pad ...)`, the name for `(property ...)`).
fn first_quoted_after<'a>(block: &'a str, head: &str) -> Option<&'a str> {
    let after = block.get(1 + head.len()..)?;
    let q1 = after.find('"')?;
    let rest = &after[q1 + 1..];
    let q2 = rest.find('"')?;
    Some(&rest[..q2])
}

fn strip_footprint_head(body: &str) -> String {
    let s = body.trim();
    if !s.starts_with('(') {
        return body.to_string();
    }
    // Skip the opening '(' then the `footprint` atom.
    let rest = s[1..].trim_start();
    let footprint_tok = next_token(rest);
    if footprint_tok != "footprint" {
        return body.to_string();
    }
    let after = rest[footprint_tok.len()..].trim_start();
    // The footprint name is a quoted string literal.
    let name_tok = next_token(after);
    let interior = after[name_tok.len()..].trim();
    // Drop the trailing ')' that closed the original (footprint …) form.
    let interior = interior.strip_suffix(')').unwrap_or(interior).trim();
    interior.to_string()
}

/// Read the next s-expression token from the front of `s`: a quoted string
/// (respecting backslash escapes) or a whitespace/paren-delimited atom.
fn next_token(s: &str) -> &str {
    let s = s.trim_start();
    let bytes = s.as_bytes();
    if s.is_empty() {
        return "";
    }
    if bytes[0] == b'"' {
        let mut end = 1;
        while end < bytes.len() {
            if bytes[end] == b'"' && bytes[end - 1] != b'\\' {
                end += 1;
                break;
            }
            end += 1;
        }
        &s[..end.min(s.len())]
    } else {
        let end = s
            .find(|c: char| c.is_whitespace() || c == '(' || c == ')')
            .unwrap_or(s.len());
        &s[..end]
    }
}

/// `(at x y)` arguments for a pad, with the angle added when it is not zero.
fn pad_at_args(center_mm: (f64, f64), angle_deg: f64) -> Vec<Sexp> {
    let mut args = vec![num(center_mm.0), num(center_mm.1)];
    if angle_deg != 0.0 {
        args.push(num(angle_deg));
    }
    args
}

/// Add `angle_deg` to the angle of every `(pad ...)` in a footprint body.
///
/// A pad's `(at x y [angle])` is its absolute orientation in KiCad, so a
/// footprint turned by `angle_deg` needs the same turn on each pad. A pad
/// with no angle gets one; the result stays in `0..360`. The pad's own `at`
/// is the first one after its head, before any primitive.
fn add_footprint_angle_to_pads(body: &str, angle_deg: f64) -> String {
    if angle_deg == 0.0 {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len() + 64);
    let mut pos = 0;
    while let Some(idx) = body[pos..].find("(pad ") {
        let pad_start = pos + idx;
        let Some(pad_end) = balanced_close(body, pad_start) else {
            break;
        };
        let pad = &body[pad_start..pad_end];
        let Some(at_idx) = pad.find("(at ") else {
            out.push_str(&body[pos..pad_end]);
            pos = pad_end;
            continue;
        };
        let args_start = at_idx + "(at ".len();
        let args_end = args_start + pad[args_start..].find(')').unwrap_or(0);
        let mut words: Vec<&str> = pad[args_start..args_end].split_whitespace().collect();
        let own: f64 = words.get(2).and_then(|w| w.parse().ok()).unwrap_or(0.0);
        let turned = format_angle((own + angle_deg).rem_euclid(360.0));
        words.truncate(2);
        words.push(&turned);
        out.push_str(&body[pos..pad_start + args_start]);
        out.push_str(&words.join(" "));
        out.push_str(&pad[args_end..]);
        pos = pad_end;
    }
    out.push_str(&body[pos..]);
    out
}

fn format_angle(v: f64) -> String {
    match num(v) {
        Sexp::Atom(s) => s,
        _ => unreachable!("num returns an atom"),
    }
}

fn inject_pad_nets(body: &str, component: ComponentId, lookup: &PadNetLookup) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(body.len() + 256);
    let mut pos = 0;
    while let Some(idx) = body[pos..].find("(pad ") {
        let abs = pos + idx;
        out.push_str(&body[pos..abs]);
        let after = &body[abs + "(pad ".len()..];
        let pad_number = if after.starts_with('"') {
            let num_start = 1;
            if let Some(end_q) = after[num_start..].find('"') {
                after[num_start..num_start + end_q].to_string()
            } else {
                out.push_str(&body[abs..]);
                return out;
            }
        } else {
            let num_end = after
                .find(|c: char| c.is_whitespace() || c == ')')
                .unwrap_or(0);
            after[..num_end].to_string()
        };

        // Balanced-paren end of the (pad ...) block.
        let Some(block_end) = balanced_close(body, abs) else {
            out.push_str(&body[abs..]);
            return out;
        };
        let block = &body[abs..block_end];
        let pad_norm = pad_number.trim_start_matches(['p', 'P', 'i', 'n', 'I', 'N']);
        let matched = lookup
            .get(&(component, pad_number.clone()))
            .or_else(|| lookup.get(&(component, pad_norm.to_string())));
        if let Some((net_id, net_name)) = matched {
            // Strip the closing paren so we can append the net
            // declaration before it, preserving everything else.
            let close_paren_pos = block.rfind(')').unwrap_or(block.len() - 1);
            out.push_str(&block[..close_paren_pos]);
            write!(
                out,
                "\t\t(net {net_id} \"{}\")\n\t",
                escape_sexp_str(net_name)
            )
            .unwrap();
            out.push_str(&block[close_paren_pos..]);
        } else {
            // Pad has no net in the IR (NC, mechanical, or a pin the
            // design doesn't wire). Stamp (net 0 "") so KiCad DRC
            // treats un-wired pads correctly without mask aperture bridges.
            let close_paren_pos = block.rfind(')').unwrap_or(block.len() - 1);
            out.push_str(&block[..close_paren_pos]);
            write!(out, "\t\t(net 0 \"\")\n\t").unwrap();
            out.push_str(&block[close_paren_pos..]);
        }
        pos = block_end;
    }
    out.push_str(&body[pos..]);
    out
}

/// Byte-level scan: return the offset *one past* the matching
/// close paren that pairs with the `(` at `from`.
fn balanced_close(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(from) != Some(&b'(') {
        return None;
    }
    let mut depth = 0_i32;
    for (i, b) in bytes.iter().enumerate().skip(from) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Escape an s-expression string literal. KiCad's parser is
/// tolerant — most net names are plain ASCII — but `"` and `\`
/// need backslash-escaping per the format spec.
fn escape_sexp_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_diagnostics::Span;
    use synth_registry::{ElectricalType, Lifecycle, Pin, PinNumber};

    #[test]
    fn a_footprint_angle_is_added_to_every_pad_angle() {
        let body = concat!(
            "(fp_text reference \"R\" (at 0 -1.2 90))\n",
            "(pad \"1\" smd rect (at -1 0) (size 1 2))\n",
            "(pad \"2\" smd rect (at 1 0 90) (size 1 2))\n",
            "(pad \"3\" smd custom (at 0 2 270) (size 1 1) (primitives (gr_poly (pts (xy 0 0))) (at 5 5)))"
        );
        let got = add_footprint_angle_to_pads(body, 90.0);
        assert!(
            got.contains("(fp_text reference \"R\" (at 0 -1.2 90))"),
            "text untouched"
        );
        assert!(
            got.contains("(pad \"1\" smd rect (at -1 0 90)"),
            "no angle gets one"
        );
        assert!(
            got.contains("(pad \"2\" smd rect (at 1 0 180)"),
            "own angle is added"
        );
        assert!(got.contains("(at 0 2 0)"), "360 wraps to 0");
        assert!(
            got.contains("(at 5 5)"),
            "a primitive's own at is not a pad at"
        );
        assert_eq!(add_footprint_angle_to_pads(body, 0.0), body);
    }

    fn hole(x: f64, y: f64, radius: f64) -> (Point, i64) {
        (Point::new(mm_to_nm(x), mm_to_nm(y)), mm_to_nm(radius))
    }

    /// The `(xy x y)` pairs of a serialised s-expression, in order.
    fn xy_pairs(text: &str) -> Vec<(f64, f64)> {
        text.split("(xy ")
            .skip(1)
            .map(|rest| {
                let mut numbers = rest.trim_start().split([' ', ')']);
                let x = numbers.next().unwrap().parse().unwrap();
                let y = numbers.next().unwrap().parse().unwrap();
                (x, y)
            })
            .collect()
    }

    /// Under a screw head no copper may run: a rule area on every copper layer
    /// that refuses tracks, vias and pour, centred on the hole and as wide as
    /// the hole plus the head's keepout. Pads stay allowed so the part itself
    /// is not flagged.
    #[test]
    fn a_mounting_hole_gets_a_copper_keepout_the_size_of_its_screw_head() {
        let project = Uuid::nil();
        let (centre, radius) = hole(3.5, 3.45, 1.6);
        let layers = ground_plane_layers(4);

        let zone = build_mounting_hole_keepout(centre, radius, &layers, &project, "H1");
        let text = zone.to_string_pretty();

        for expected in [
            "(tracks not_allowed)",
            "(vias not_allowed)",
            "(copperpour not_allowed)",
            "(pads allowed)",
            "\"F.Cu\"",
            "\"In1.Cu\"",
            "\"In2.Cu\"",
            "\"B.Cu\"",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        let reach = 1.6 + MOUNTING_HOLE_KEEPOUT_MM;
        let points = xy_pairs(&text);
        assert!(
            points.len() >= 16,
            "a round outline, not a box: {}",
            points.len()
        );
        for (x, y) in points {
            let distance = (x - 3.5).hypot(y - 3.45);
            assert!(
                (distance - reach).abs() < 0.02,
                "({x}, {y}) is {distance:.3} mm from the centre, not {reach}"
            );
        }
    }

    #[test]
    fn the_same_hole_gives_the_same_keepout_bytes() {
        let project = Uuid::nil();
        let (centre, radius) = hole(47.0, 3.45, 1.6);
        let layers = ground_plane_layers(4);
        let a = build_mounting_hole_keepout(centre, radius, &layers, &project, "H2");
        let b = build_mounting_hole_keepout(centre, radius, &layers, &project, "H2");
        assert_eq!(a.to_string_pretty(), b.to_string_pretty());
    }

    fn gap_mm(a: Point, b: Point) -> f64 {
        nm_to_mm(a.x_nm - b.x_nm).hypot(nm_to_mm(a.y_nm - b.y_nm))
    }

    /// A ground fence via in the corner must stay out of a mounting hole's
    /// screw-head zone: the whole pad, so its centre is the hole's radius plus
    /// `MOUNTING_HOLE_KEEPOUT_MM` plus the pad's radius from the hole.
    #[test]
    fn a_corner_via_moves_along_the_edge_out_of_a_mounting_hole_zone() {
        // M3 hole 3.5 mm in from both edges; the via sits 1.5 mm in.
        let hole = hole(3.5, 3.45, 1.6);
        let corner = Point::new(mm_to_nm(1.5), mm_to_nm(1.5));
        let moved = clear_of_mounting_holes(corner, 1, &[hole]);
        assert!(
            gap_mm(moved, hole.0) >= 1.6 + MOUNTING_HOLE_KEEPOUT_MM + GROUND_VIA_RADIUS_MM - 0.001,
            "{moved:?}"
        );
        assert_eq!(moved.y_nm, corner.y_nm, "it slides along the edge only");
        assert!(
            moved.x_nm > corner.x_nm,
            "and toward the inside of the board"
        );
    }

    #[test]
    fn a_right_hand_corner_slides_the_other_way() {
        let hole = hole(47.0, 3.45, 1.6);
        let corner = Point::new(mm_to_nm(49.0), mm_to_nm(1.5));
        let moved = clear_of_mounting_holes(corner, -1, &[hole]);
        assert!(moved.x_nm < corner.x_nm);
        assert!(
            gap_mm(moved, hole.0) >= 1.6 + MOUNTING_HOLE_KEEPOUT_MM + GROUND_VIA_RADIUS_MM - 0.001
        );
    }

    #[test]
    fn a_via_clear_of_every_hole_stays_where_it_is() {
        let corner = Point::new(mm_to_nm(1.5), mm_to_nm(1.5));
        assert_eq!(clear_of_mounting_holes(corner, 1, &[]), corner);
        let far = hole(40.0, 30.0, 1.6);
        assert_eq!(clear_of_mounting_holes(corner, 1, &[far]), corner);
    }

    /// A fanout stub that crosses another pad is rejected even when the via
    /// endpoint itself is clear: the whole segment is checked, not only its end.
    #[test]
    fn plane_via_rejects_a_stub_that_crosses_another_pad() {
        let pad = Point::new(0, 0);
        let origin = Point::new(0, 0);
        let outline = synth_geometry::Rect::new(
            Point::new(-10_000_000, -10_000_000),
            Point::new(10_000_000, 10_000_000),
        );
        // A small pad 0.4 mm along +x. The +x via endpoint at 1 mm is clear of
        // it on its own, but the stub from the pad to that endpoint runs
        // straight through it.
        let blocker = (Point::new(400_000, 0), 50_000);
        let obstacles = vec![(pad, 50_000), blocker];
        let via = plane_via_offset(pad, origin, outline, &obstacles)
            .expect("an off-axis candidate is legal");
        assert_eq!(via, Point::new(0, PLANE_VIA_OFFSET_NM));
    }

    fn part_from_toml(id: &str, dims: &str) -> synth_registry::Part {
        let toml = format!(
            "id = \"{id}\"\nkind = \"resistor\"\n\n[footprint_dimensions]\nwidth_mm = 2.0\nheight_mm = 1.0\n{dims}\n"
        );
        let registry =
            synth_registry::load_from_strs(&[(&format!("{id}.synth.toml"), &toml)]).unwrap();
        registry.lookup(id).unwrap().clone()
    }

    fn entry(dx: Option<f64>, dy: Option<f64>) -> synth_layout::sidecar::SidecarLabel {
        synth_layout::sidecar::SidecarLabel {
            dx,
            dy,
            angle: 0.0,
            hide: None,
            priority: synth_layout::sidecar::OverridePriority::Soft,
        }
    }

    #[test]
    fn a_half_set_sidecar_offset_counts_the_missing_axis_as_zero() {
        assert_eq!(
            label_override(&entry(Some(2.5), None)).offset_mm,
            Some((2.5, 0.0))
        );
        assert_eq!(
            label_override(&entry(None, Some(-1.0))).offset_mm,
            Some((0.0, -1.0))
        );
    }

    #[test]
    fn a_sidecar_label_with_no_offset_has_no_position() {
        let hidden_only = synth_layout::sidecar::SidecarLabel {
            hide: Some(true),
            ..entry(None, None)
        };
        let o = label_override(&hidden_only);
        assert_eq!((o.offset_mm, o.hide), (None, Some(true)));
    }

    #[test]
    fn a_hard_priority_is_carried_to_the_placer() {
        let hard = synth_layout::sidecar::SidecarLabel {
            priority: synth_layout::sidecar::OverridePriority::Hard,
            ..entry(Some(1.0), Some(1.0))
        };
        assert!(label_override(&hard).hard);
        assert!(!label_override(&entry(Some(1.0), Some(1.0))).hard);
    }

    #[test]
    fn a_part_files_label_hints_reach_the_placer() {
        let hinted = part_from_toml("hinted", "label_side = \"left\"\nlabel_hidden = true");
        assert_eq!(
            part_label_hints(&hinted),
            (Some(silk_labels::Side::Left), true)
        );
        let plain = part_from_toml("plain", "");
        assert_eq!(part_label_hints(&plain), (None, false));
    }

    fn no_footprint_part() -> synth_registry::Part {
        synth_registry::Part {
            id: synth_registry::PartId("nofp".into()),
            kind: "ic".into(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: vec![],
            substitutes: vec![],
            mpn: None,
            lcsc_pn: None,
            pins: vec![
                Pin {
                    name: "vdd".into(),
                    number: PinNumber("1".into()),
                    electrical_type: ElectricalType::PowerInput,
                    capabilities: vec![],
                    required: true,
                    footprint_only: false,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
                Pin {
                    name: "gnd".into(),
                    number: PinNumber("2".into()),
                    electrical_type: ElectricalType::PowerInput,
                    capabilities: vec![],
                    required: true,
                    footprint_only: false,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
            ],
            required_decoupling: vec![],
            kicad_symbol: None,
            kicad_footprint: None,
            // `footprint_dimensions: None` — `synth_part_pads` falls
            // back to pin-count-based default sizing.
            footprint_dimensions: None,
            operating_conditions: None,
            provenance: None,
        }
    }

    #[test]
    fn synthesized_footprint_module_has_one_pad_per_pin() {
        let part = no_footprint_part();
        let module = build_synthesized_footprint_module(&part, "synth:nofp")
            .expect("part has pins + footprint_dimensions, must synthesize");
        let rendered = module.to_string_pretty();
        let pad_count = rendered.matches("(pad\n").count() + rendered.matches("(pad ").count();
        assert_eq!(pad_count, 2);
        assert!(rendered.starts_with("(footprint"));
        assert!(rendered.contains("\"synth:nofp\""));
    }

    /// Regression test for the KiCad `--save-board` determinism hole:
    /// every uuid-bearing footprint-body block must carry a deterministic
    /// UUID before KiCad sees the file, or the gate fills random v4 UUIDs
    /// and re-exports stop being byte-identical (plan §6.2).
    #[test]
    fn inlined_body_blocks_carry_deterministic_uuids() {
        let body = "(property \"Datasheet\" \"\" (at 0 0 0) (layer \"F.Fab\") (hide yes) (effects (font (size 1.27 1.27)))) (pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\")) (fp_line (start 0 0) (end 1 1) (stroke (width 0.1)) (layer \"F.SilkS\")) (fp_text user \"REF\" (at 0 0) (layer \"F.SilkS\") (effects (font (size 1 1))) (point (at 0 0) (size 1) (layer \"F.SilkS\")))";
        let project = Uuid::nil();
        let once = super::ensure_body_uuids(body, &project, "lib:part");
        assert_eq!(
            once.matches("(uuid \"").count(),
            5,
            "one uuid per block: {once}"
        );
        // Idempotent and deterministic: a second pass changes nothing, and
        // pre-uuid'd blocks are preserved verbatim.
        let twice = super::ensure_body_uuids(&once, &project, "lib:part");
        assert_eq!(once, twice, "uuid pass must be idempotent");
        let again = super::ensure_body_uuids(body, &project, "lib:part");
        assert_eq!(once, again, "uuid pass must be deterministic");
    }

    #[test]
    fn synthesized_pads_carry_the_footprint_angle() {
        let part = no_footprint_part();
        let component = synth_ir::Component {
            id: ComponentId(0),
            refdes: "U1".to_string(),
            kind: part.kind.clone(),
            part: Some(part.clone()),
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        };
        let placement = ComponentPlacement {
            id: ComponentId(0),
            center: Point { x_nm: 0, y_nm: 0 },
            rotation: Rotation::Ninety,
            layer: Layer::Top,
        };
        let mut pad_net_lookup: PadNetLookup = HashMap::new();
        pad_net_lookup.insert((ComponentId(0), "1".to_string()), (1, "VDD".to_string()));
        pad_net_lookup.insert((ComponentId(0), "2".to_string()), (2, "GND".to_string()));
        let instance = build_footprint_instance(
            &component,
            &part,
            &placement,
            "synth:nofp",
            &Uuid::nil(),
            &pad_net_lookup,
            silk_labels::LabelPos {
                local_x_mm: 0.0,
                local_y_mm: -4.5,
                angle_deg: 0.0,
                hidden: false,
            },
        )
        .to_string_pretty();
        let pad_ats: Vec<&str> = instance
            .lines()
            .filter(|l| l.trim_start().starts_with("(at ") && l.trim_end().ends_with(" 90)"))
            .collect();
        // Two pads, each `(at x y 90)`; the footprint's own `(at x y 90)`
        // and the property texts make up the rest of the matches.
        assert!(
            instance.matches("(pad").count() == 2 && pad_ats.len() >= 2,
            "pads must carry the 90 degree footprint angle:\n{instance}"
        );
    }

    #[test]
    fn footprint_carries_native_procurement_fields() {
        let mut part = no_footprint_part();
        part.mpn = Some("MFG-123".into());
        part.lcsc_pn = Some("C123456".into());
        let component = synth_ir::Component {
            id: ComponentId(0),
            refdes: "R1".into(),
            kind: part.kind.clone(),
            part: Some(part.clone()),
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        };
        let placement = ComponentPlacement {
            id: ComponentId(0),
            center: Point { x_nm: 0, y_nm: 0 },
            rotation: Rotation::Zero,
            layer: Layer::Top,
        };
        let rendered = build_footprint_instance(
            &component,
            &part,
            &placement,
            "synth:nofp",
            &Uuid::nil(),
            &HashMap::new(),
            silk_labels::LabelPos {
                local_x_mm: 0.0,
                local_y_mm: -4.5,
                angle_deg: 0.0,
                hidden: false,
            },
        )
        .to_string_pretty();
        assert!(rendered.contains("MPN") && rendered.contains("MFG-123"));
        assert!(rendered.contains("LCSC") && rendered.contains("C123456"));
    }

    /// Regression test for the `lib_footprint_mismatch` DRC violation:
    /// the standalone module written to `<project>.pretty/` must carry
    /// *only* the fields the embedded PCB instance's fallback branch
    /// also emits (pads, no `attr`/`fp_text`), or `kicad-cli pcb drc`
    /// flags every synthesized-footprint board.
    #[test]
    fn synthesized_footprint_module_matches_embedded_instance_pads() {
        let part = no_footprint_part();
        let lib_id = "synth:nofp";
        let module = build_synthesized_footprint_module(&part, lib_id).unwrap();
        let module_text = module.to_string_pretty();
        assert!(
            !module_text.contains("fp_text"),
            "standalone module must not carry fp_text the embedded instance lacks"
        );
        assert!(
            !module_text.contains("(attr "),
            "standalone module must not carry attr the embedded instance lacks"
        );

        let component = synth_ir::Component {
            id: ComponentId(0),
            refdes: "U1".to_string(),
            kind: part.kind.clone(),
            part: Some(part.clone()),
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        };
        let placement = ComponentPlacement {
            id: ComponentId(0),
            center: Point { x_nm: 0, y_nm: 0 },
            rotation: Rotation::Zero,
            layer: Layer::Top,
        };
        let mut pad_net_lookup: PadNetLookup = HashMap::new();
        pad_net_lookup.insert((ComponentId(0), "1".to_string()), (1, "VDD".to_string()));
        pad_net_lookup.insert((ComponentId(0), "2".to_string()), (2, "GND".to_string()));
        let project = Uuid::nil();
        let instance = build_footprint_instance(
            &component,
            &part,
            &placement,
            lib_id,
            &project,
            &pad_net_lookup,
            silk_labels::LabelPos {
                local_x_mm: 0.0,
                local_y_mm: -4.5,
                angle_deg: 0.0,
                hidden: false,
            },
        );
        let instance_text = instance.to_string_pretty();

        // Both must carry the same pad set (numbers, position, size),
        // net assignments aside. Strip each pad's `(net ...)` child
        // before comparing so this doesn't just re-test net lookup.
        let pad_open_count = |text: &str| -> usize {
            text.lines()
                .filter(|l| {
                    let t = l.trim_start();
                    t == "(pad" || t.starts_with("(pad ")
                })
                .count()
        };
        // Pad geometry lives on the lines *following* the `(pad ...)`
        // opener in the pretty-printer's multi-line form, so compare
        // the full rendered pad blocks via a coarser check: same pad
        // count and same set of pad numbers/positions/sizes appear in
        // both, independent of `(net ...)`.
        for marker in ["\"1\"", "\"2\""] {
            assert!(
                module_text.contains(marker) && instance_text.contains(marker),
                "pad {marker} must appear in both the standalone module and the embedded instance"
            );
        }
        assert_eq!(
            pad_open_count(&module_text),
            2,
            "module must open exactly 2 pads"
        );
    }

    #[test]
    fn canonical_layers_have_unique_ids_and_names() {
        for layer_count in [2, 4, 6] {
            let layers_sexp = build_layers(layer_count);
            let mut ids = std::collections::HashSet::new();
            let mut names = std::collections::HashSet::new();
            if let Sexp::List {
                head: _,
                children: items,
            } = layers_sexp
            {
                for item in items {
                    if let Sexp::List {
                        head: id_str,
                        children,
                    } = item
                    {
                        let id: i32 = id_str.parse().expect("layer ID must be integer");
                        assert!(
                            ids.insert(id),
                            "Duplicate layer ID {id} in {layer_count}-layer stackup"
                        );
                        if let Some(Sexp::Str(name)) = children.first() {
                            assert!(
                                names.insert(name.clone()),
                                "Duplicate layer name {name} in {layer_count}-layer stackup"
                            );
                        }
                    }
                }
            }
            if layer_count == 2 {
                assert!(names.contains("F.Cu") && names.contains("B.Cu"));
                assert!(!names.contains("In1.Cu"));
            } else if layer_count == 4 {
                assert!(
                    names.contains("F.Cu")
                        && names.contains("In1.Cu")
                        && names.contains("In2.Cu")
                        && names.contains("B.Cu")
                );
            } else {
                assert!(
                    names.contains("F.Cu")
                        && names.contains("In1.Cu")
                        && names.contains("In2.Cu")
                        && names.contains("In3.Cu")
                        && names.contains("In4.Cu")
                        && names.contains("B.Cu")
                );
            }
        }
    }

    #[test]
    fn six_layer_ground_planes_cover_every_copper_layer() {
        assert_eq!(
            ground_plane_layers(6),
            vec!["F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu"]
        );
        assert_eq!(
            ground_plane_layers(4),
            vec!["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]
        );
        assert_eq!(ground_plane_layers(2), vec!["F.Cu", "B.Cu"]);
    }

    #[test]
    fn fixed_6_keeps_the_sign_and_trims_zeros() {
        assert_eq!(fixed_6(35_000), "0.035");
        assert_eq!(fixed_6(35_560), "0.03556");
        assert_eq!(fixed_6(1_600_000), "1.6");
        assert_eq!(fixed_6(4_000_000), "4");
        assert_eq!(fixed_6(0), "0");
        assert_eq!(fixed_6(-500_000), "-0.5");
    }

    /// The board file's own minimum via has to be the profile's, because
    /// `kicad-cli` applies it, and the router places vias down to that figure.
    #[test]
    fn the_via_minimum_is_the_profiles_not_a_fixed_figure() {
        let jlc = build_setup(&synth_drc::ManufacturerProfile::jlc_standard()).to_string_pretty();
        assert!(jlc.contains("(via_min_size 0.56)"), "{jlc}");
        assert!(jlc.contains("(via_min_drill 0.3)"), "{jlc}");
        let pcbway =
            build_setup(&synth_drc::ManufacturerProfile::pcbway_standard()).to_string_pretty();
        assert!(pcbway.contains("(via_min_size 0.6)"), "{pcbway}");
    }

    #[test]
    fn without_a_stackup_setup_and_thickness_are_the_legacy_ones() {
        let profile = synth_drc::ManufacturerProfile::jlc_standard();
        assert_eq!(
            build_setup_with(None, &profile).to_string_pretty(),
            build_setup(&profile).to_string_pretty()
        );
        assert!(!build_setup_with(None, &profile)
            .to_string_pretty()
            .contains("stackup"));
        assert_eq!(
            board_thickness(None).to_string_pretty().trim(),
            DEFAULT_THICKNESS_MM.to_string()
        );
    }

    #[test]
    fn netclass_is_top_level_outside_setup() {
        let setup_sexp = build_setup(&synth_drc::ManufacturerProfile::jlc_standard());
        let setup_str = setup_sexp.to_string_pretty();
        assert!(
            !setup_str.contains("netclass"),
            "setup must not nest netclass"
        );
        assert!(
            !setup_str.contains("net_class"),
            "setup must not nest net_class"
        );

        // Build a minimal Board whose nets have semantic names so the power-domain
        // engine classifies them correctly (GND → Ground, VCC → Rail via name heuristic).
        let board = Board {
            schematic_overflow: None,
            schematic_paper: None,
            groups: Vec::new(),
            legends: false,
            name: "test_netclass".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: vec![],
            nets: vec![
                synth_ir::Net {
                    id: synth_ir::NetId(0),
                    name: "VCC".to_string(),
                    auto_named: false,
                    endpoints: vec![],
                    netclass: None,
                    voltage: None,
                },
                synth_ir::Net {
                    id: synth_ir::NetId(1),
                    name: "GND".to_string(),
                    auto_named: false,
                    endpoints: vec![],
                    netclass: None,
                    voltage: None,
                },
                synth_ir::Net {
                    id: synth_ir::NetId(2),
                    name: "SIG1".to_string(),
                    auto_named: false,
                    endpoints: vec![],
                    netclass: None,
                    voltage: None,
                },
                synth_ir::Net {
                    id: synth_ir::NetId(3),
                    name: "MAIN_ANT".to_string(),
                    auto_named: false,
                    endpoints: vec![],
                    netclass: None,
                    voltage: None,
                },
            ],
            diff_pairs: vec![],
            notes: vec![],
            keepouts: vec![],
            stackup: None,
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            required_processes: std::collections::BTreeSet::new(),
            source_span: Span::new(0, 0),
        };
        let net_table = vec![
            (1_u32, "VCC".to_string()),
            (2_u32, "GND".to_string()),
            (3_u32, "SIG1".to_string()),
            (4_u32, "MAIN_ANT".to_string()),
        ];
        let ncs = build_netclasses(&net_table, &board);
        let nc_str = ncs
            .iter()
            .map(super::super::sexp::Sexp::to_string_pretty)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(nc_str.contains("net_class"));
        assert!(nc_str.contains("\"Default\""));
        assert!(nc_str.contains("\"Default net class\""));
        assert!(nc_str.contains("\"Power\""));
        assert!(nc_str.contains("\"Power delivery network\""));
        assert!(!nc_str.contains("RF_50"));
        assert!(nc_str.contains("(add_net \"SIG1\")"));
        assert!(nc_str.contains("(add_net \"VCC\")"));
        assert!(nc_str.contains("(add_net \"GND\")"));
        assert!(nc_str.contains("(add_net \"MAIN_ANT\")"));
        // The router reads the project file, where an undeclared class is at the
        // manufacturer minimum. The board's own classes must say the same.
        assert_eq!(nc_str.matches("(clearance 0.127)").count(), 2, "{nc_str}");
        assert!(!nc_str.contains("(clearance 0.2)"), "{nc_str}");
    }

    /// The Default and Power classes take the clearance of the board's
    /// manufacturer, the figure the project file and the router work to, not a
    /// fixed 0.127 mm: PCBWay's minimum is 0.15 mm and OSH Park's 0.125 mm.
    #[test]
    fn inferred_classes_take_the_manufacturers_clearance() {
        let net = |id: u32, name: &str| synth_ir::Net {
            id: synth_ir::NetId(id),
            name: name.to_string(),
            auto_named: false,
            endpoints: vec![],
            netclass: None,
            voltage: None,
        };
        let net_table = vec![
            (1_u32, "VCC".to_string()),
            (2_u32, "GND".to_string()),
            (3_u32, "SIG1".to_string()),
        ];
        for (manufacturer, expected) in [
            (None, "(clearance 0.127)"),
            (Some("jlcpcb"), "(clearance 0.127)"),
            (Some("pcbway"), "(clearance 0.15)"),
            (Some("oshpark"), "(clearance 0.125)"),
        ] {
            let mut board = plain_test_board();
            board.manufacturer = manufacturer.map(str::to_string);
            board.nets = vec![net(0, "VCC"), net(1, "GND"), net(2, "SIG1")];
            let classes = build_netclasses(&net_table, &board)
                .iter()
                .map(crate::sexp::Sexp::to_string_pretty)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(classes.contains("\"Power\""), "{manufacturer:?}: {classes}");
            assert_eq!(
                classes.matches(expected).count(),
                2,
                "{manufacturer:?}: Default and Power both take {expected}\n{classes}"
            );
            assert!(
                !classes.contains("(clearance 0.2)"),
                "{manufacturer:?}: no class falls back to 0.2 mm\n{classes}"
            );
        }
    }

    #[test]
    fn declared_netclass_emitted_with_joined_nets() {
        use synth_ir::Length;
        let board = Board {
            schematic_overflow: None,
            schematic_paper: None,
            groups: Vec::new(),
            legends: false,
            name: "test_declared_netclass".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: vec![],
            nets: vec![
                synth_ir::Net {
                    id: synth_ir::NetId(0),
                    name: "+3V3".to_string(),
                    auto_named: false,
                    endpoints: vec![],
                    netclass: Some("PWR".to_string()),
                    voltage: None,
                },
                synth_ir::Net {
                    id: synth_ir::NetId(1),
                    name: "SIG".to_string(),
                    auto_named: false,
                    endpoints: vec![],
                    netclass: None,
                    voltage: None,
                },
            ],
            diff_pairs: vec![],
            notes: vec![],
            keepouts: vec![],
            stackup: None,
            netclasses: vec![synth_ir::NetClass {
                name: "PWR".to_string(),
                trace_width: Some(Length::from_mm(0.5)),
                clearance: Some(Length::from_mm(0.2)),
                via_diameter: None,
                via_drill: None,
                color: None,
                impedance: None,
                current: None,
                plane: false,
                max_length: None,
                max_skew: None,
                source_span: Span::new(0, 0),
            }],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            required_processes: std::collections::BTreeSet::new(),
            source_span: Span::new(0, 0),
        };
        let net_table = vec![(1_u32, "+3V3".to_string()), (2_u32, "SIG".to_string())];
        let ncs = build_netclasses(&net_table, &board);
        let nc_str = ncs
            .iter()
            .map(super::super::sexp::Sexp::to_string_pretty)
            .collect::<Vec<_>>()
            .join("\n");
        // Declared class renders under its own name with its own
        // rules and the joined net; nothing duplicates into the
        // heuristic Power bucket.
        assert!(nc_str.contains("\"PWR\""));
        assert!(nc_str.contains("\"Synth netclass PWR\""));
        assert!(nc_str.contains("(trace_width 0.5)"));
        assert!(nc_str.contains("(clearance 0.2)"));
        assert!(nc_str.contains("(add_net \"+3V3\")"));
        // A class declaring no via geometry exports the copper it always did.
        assert!(nc_str.contains("(via_dia 0.6)"));
        assert!(nc_str.contains("(via_drill 0.3)"));
        assert!(
            !nc_str.contains("\"Power\""),
            "heuristic Power must not duplicate PWR"
        );
        // The unjoined signal net still lands in Default.
        assert!(nc_str.contains("(add_net \"SIG\")"));
    }

    /// An empty two-layer board, for the net-class questions that turn only
    /// on what the classes declare.
    fn plain_test_board() -> Board {
        Board {
            schematic_overflow: None,
            schematic_paper: None,
            groups: Vec::new(),
            legends: false,
            name: "via_geometry".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: vec![],
            nets: vec![],
            diff_pairs: vec![],
            notes: vec![],
            keepouts: vec![],
            stackup: None,
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            required_processes: std::collections::BTreeSet::new(),
            source_span: Span::new(0, 0),
        }
    }

    /// Escape geometry reaches the exported net class, which is where a
    /// router that reads net classes picks it up. Before this the exporter
    /// wrote one constant for every class on every board, so a fine-pitch
    /// package could not ask for the via its escape needs.
    #[test]
    fn declared_via_geometry_reaches_the_exported_net_class() {
        use synth_ir::Length;
        let mut board = plain_test_board();
        board.nets = vec![synth_ir::Net {
            id: synth_ir::NetId(0),
            name: "TMDS0_P".to_string(),
            auto_named: false,
            endpoints: vec![],
            netclass: Some("ESCAPE".to_string()),
            voltage: None,
        }];
        board.netclasses = vec![synth_ir::NetClass {
            name: "ESCAPE".to_string(),
            trace_width: Some(Length::from_mm(0.2)),
            clearance: Some(Length::from_mm(0.2)),
            max_length: None,
            max_skew: None,
            via_diameter: Some(Length::from_mm(0.45)),
            via_drill: Some(Length::from_mm(0.2)),
            color: None,
            impedance: None,
            current: None,
            plane: false,
            source_span: Span::new(0, 0),
        }];

        let net_table = vec![(1_u32, "TMDS0_P".to_string())];
        let nc_str = build_netclasses(&net_table, &board)
            .iter()
            .map(super::super::sexp::Sexp::to_string_pretty)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            nc_str.contains("(via_dia 0.45)"),
            "declared via diameter must reach the net class: {nc_str}"
        );
        assert!(
            nc_str.contains("(via_drill 0.2)"),
            "declared via drill must reach the net class: {nc_str}"
        );
        assert!(
            !nc_str.contains("(via_dia 0.6)") || nc_str.matches("(via_dia 0.6)").count() <= 1,
            "the default must not shadow the declared class"
        );
    }

    #[test]
    fn a_sidecar_via_is_written_on_its_net_before_routing() {
        let mut board = plain_test_board();
        board.nets = vec![synth_ir::Net {
            id: synth_ir::NetId(0),
            name: "GND".to_string(),
            auto_named: false,
            endpoints: vec![],
            netclass: None,
            voltage: None,
        }];
        let placement = Placement {
            board_outline: synth_geometry::Rect::from_center_half_extents(
                Point::new(0, 0),
                mm_to_nm(25.0),
                mm_to_nm(25.0),
            ),
            components: vec![],
        };
        let vias = [synth_layout::sidecar::SidecarVia {
            x: 3.5,
            y: -4.25,
            net: "GND".into(),
        }];
        let text = build_pcb(
            &board,
            &placement,
            &Routing::default(),
            &Uuid::nil(),
            &HashMap::new(),
            &vias,
        )
        .expect("export")
        .to_string_pretty();
        let start = text
            .match_indices("(via")
            .map(|(at, _)| at)
            .find(|at| text[at + 4..].starts_with(char::is_whitespace))
            .expect("a via is written");
        let via = &text[start..];
        assert!(via.contains("(at 3.5 -4.25)"), "{via}");
        assert!(via.contains("(net 1)"), "{via}");

        let unknown = [synth_layout::sidecar::SidecarVia {
            x: 0.0,
            y: 0.0,
            net: "NOPE".into(),
        }];
        let error = build_pcb(
            &board,
            &placement,
            &Routing::default(),
            &Uuid::nil(),
            &HashMap::new(),
            &unknown,
        )
        .expect_err("an unknown net is refused");
        assert_eq!(error.net, "NOPE");
    }

    #[test]
    fn test_kicad_cli_roundtrip_valid_board() {
        let board = Board {
            schematic_overflow: None,
            schematic_paper: None,
            groups: Vec::new(),
            legends: false,
            name: "test_roundtrip".to_string(),
            layers: 4,
            manufacturer: Some("jlcpcb".to_string()),
            revision: Some("A".to_string()),
            company: None,
            components: vec![],
            nets: vec![],
            diff_pairs: vec![],
            notes: vec![],
            keepouts: vec![],
            stackup: None,
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            required_processes: std::collections::BTreeSet::new(),
            source_span: Span::new(0, 0),
        };
        let placement = Placement {
            board_outline: synth_geometry::Rect::from_center_half_extents(
                Point::new(0, 0),
                synth_geometry::mm_to_nm(25.0),
                synth_geometry::mm_to_nm(25.0),
            ),
            components: vec![],
        };
        let routing = Routing {
            segments: vec![],
            vias: vec![],
            diff_pair_reports: vec![],
            unrouted_nets: vec![],
            cells_expanded: 0,
            pad_escape_rejections: 0,
        };
        let project = Uuid::nil();
        let pcb_sexp = build_pcb(&board, &placement, &routing, &project, &HashMap::new(), &[])
            .expect("plane fanout");
        let pcb_str = pcb_sexp.to_string_pretty();

        let tmp_path =
            std::env::temp_dir().join(format!("test_kicad_cli_{}.kicad_pcb", std::process::id()));
        std::fs::write(&tmp_path, pcb_str).expect("write temp pcb");

        let res = std::process::Command::new("kicad-cli")
            .args(["pcb", "drc", tmp_path.to_str().unwrap()])
            .output();

        let _ = std::fs::remove_file(&tmp_path);
        if let Ok(output) = res {
            assert!(
                output.status.success(),
                "kicad-cli pcb drc must succeed on generated PCB. stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
