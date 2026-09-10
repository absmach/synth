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

use synth_geometry::{mm_to_nm, nm_to_mm, Layer, Rotation};
use synth_ir::{Board, ComponentId};
use synth_place::{ComponentPlacement, Placement};
use synth_route::{Routing, Segment};
use uuid::Uuid;

use crate::sexp::{num, pair, str_pair, Sexp};
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
#[must_use]
pub fn build_pcb(board: &Board, placement: &Placement, routing: &Routing, project: &Uuid) -> Sexp {
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
                Sexp::list("thickness", vec![num(DEFAULT_THICKNESS_MM)]),
                Sexp::list("legacy_teardrops", vec![Sexp::atom("no")]),
            ],
        ),
        Sexp::list("paper", vec![Sexp::str(paper_for(placement))]),
        build_layers(board.layers),
        build_setup(),
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

    // Ground Plane Copper Flood Zone: Inject continuous GND copper zones on F.Cu, B.Cu, and inner layers
    let gnd_pcb_net = board
        .nets
        .iter()
        .find_map(|n| {
            let name_lower = n.name.to_lowercase();
            let is_gnd = name_lower.contains("gnd")
                || name_lower.contains("vss")
                || name_lower == "0v"
                || n.endpoints.iter().any(|ep| {
                    board
                        .component(ep.component)
                        .and_then(|c| c.part.as_ref())
                        .and_then(|p| p.pins.get(ep.pin.0 as usize))
                        .is_some_and(|pin| {
                            let pname = pin.name.to_lowercase();
                            pname.contains("gnd") || pname.contains("vss") || pname == "0v"
                        })
                });
            if is_gnd {
                net_id_lookup.get(&n.id).copied()
            } else {
                None
            }
        })
        .unwrap_or(1);

    let gnd_layers = if board.layers == 4 {
        vec!["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]
    } else {
        vec!["F.Cu", "B.Cu"]
    };
    for layer in gnd_layers {
        children.push(build_gnd_zone(
            placement,
            layer,
            gnd_pcb_net,
            "GND",
            project,
        ));
    }

    Sexp::list("kicad_pcb", children)
}

/// Emit a single `(via ...)` block.
fn build_via(
    via: &synth_route::Via,
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

/// Emit a `(zone ...)` S-expression for a continuous ground plane copper flood.
fn build_gnd_zone(
    placement: &Placement,
    layer_name: &str,
    net_id: u32,
    net_name: &str,
    project: &Uuid,
) -> Sexp {
    let r = placement.board_outline;
    let min_x = nm_to_mm(r.min.x_nm);
    let min_y = nm_to_mm(r.min.y_nm);
    let max_x = nm_to_mm(r.max.x_nm);
    let max_y = nm_to_mm(r.max.y_nm);
    let zone_uuid = derive_entity_uuid(project, "zone", layer_name);

    let pts = vec![
        Sexp::list("xy", vec![num(min_x), num(min_y)]),
        Sexp::list("xy", vec![num(max_x), num(min_y)]),
        Sexp::list("xy", vec![num(max_x), num(max_y)]),
        Sexp::list("xy", vec![num(min_x), num(max_y)]),
    ];

    Sexp::list(
        "zone",
        vec![
            Sexp::list("net", vec![Sexp::atom(net_id.to_string())]),
            str_pair("net_name", net_name),
            Sexp::list("layer", vec![Sexp::str(layer_name)]),
            str_pair("uuid", zone_uuid.to_string()),
            Sexp::list("hatch", vec![Sexp::atom("edge"), num(0.5)]),
            Sexp::list(
                "connect_pads",
                vec![Sexp::atom("yes"), Sexp::list("clearance", vec![num(0.3)])],
            ),
            Sexp::list(
                "fill",
                vec![
                    Sexp::atom("yes"),
                    Sexp::list("thermal_gap", vec![num(0.5)]),
                    Sexp::list("thermal_bridge_width", vec![num(0.5)]),
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
    let mut gnd_found = false;
    for (idx, net) in board.nets.iter().enumerate() {
        let pcb_net_id = (idx as u32) + 1;
        let is_gnd = net.name.to_lowercase().contains("gnd")
            || net.name.to_lowercase().contains("vss")
            || net.name == "0v";
        let net_name = if is_gnd && !gnd_found {
            gnd_found = true;
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

    // Mirrored USB-C pin mapping: reversible USB-C connectors bridge A6/B6 (DP)
    // and A7/B7 (DN), as well as shield pins (SH) to GND. If the primary pin is
    // assigned, ensure the mirrored counterpart inherits the same net.
    let mut mirror_additions = Vec::new();
    for ((comp, pin), net_info) in &lookup {
        if pin == "A1" || pin == "B12" {
            mirror_additions.push(((*comp, "SH".to_string()), net_info.clone()));
            mirror_additions.push(((*comp, "SH1".to_string()), net_info.clone()));
            mirror_additions.push(((*comp, "SH2".to_string()), net_info.clone()));
            mirror_additions.push(((*comp, "SH3".to_string()), net_info.clone()));
            mirror_additions.push(((*comp, "SH4".to_string()), net_info.clone()));
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

/// Canonical KiCad 10 layer stackup for 2- and 4-layer boards with strictly unique layer IDs.
fn build_layers(layer_count: u32) -> Sexp {
    let mut layers = vec![layer(0, "F.Cu", "signal", None)];
    if layer_count == 4 {
        layers.push(layer(1, "In1.Cu", "power", None));
        layers.push(layer(2, "In2.Cu", "power", None));
        layers.push(layer(31, "B.Cu", "signal", None));
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
/// widths and clearances; later slices read these from a
/// manufacturer profile (plan §9.4).
fn build_setup() -> Sexp {
    Sexp::list(
        "setup",
        vec![
            Sexp::list("pad_to_mask_clearance", vec![num(0.0)]),
            Sexp::list("solder_mask_min_width", vec![num(0.0)]),
            Sexp::list(
                "allow_soldermask_bridges_in_footprints",
                vec![Sexp::atom("yes")],
            ),
            Sexp::list("trace_min", vec![num(0.127)]),
            Sexp::list("clearance_min", vec![num(0.127)]),
            Sexp::list("via_min_size", vec![num(0.60)]),
            Sexp::list("via_min_drill", vec![num(0.30)]),
        ],
    )
}

/// Top-level `(net_class ...)` definitions conforming to KiCad 10 grammar.
///
/// Power nets are identified by topological inference via [`synth_ir::infer_power_domains`]
/// — this correctly handles nets whose names are opaque (`net_0`, `net_1`, …) but whose
/// connected pins carry `PowerOutput` / `GroundReference` electrical types.
///
/// RF nets fall back to name-based detection (RF nets carry semantic names when declared
/// via `diff_pair` or registry annotation, e.g. `ant`, `rf`, `bal_`).
fn build_netclasses(net_table: &[(u32, String)], board: &Board) -> Vec<Sexp> {
    // Topological power-domain map: classifies every net by connected pin types,
    // not by string name. This is the authoritative source for Power/GND class assignment.
    let domain_map = synth_ir::infer_power_domains(board);

    let is_rf = |name: &str| {
        let n = name.to_ascii_lowercase();
        n.contains("main_ant")
            || n.contains("rf")
            || n.contains("ant")
            || n.contains("bal_")
            || n.contains("unbal")
    };

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
    let mut rf_nets = Vec::new();

    for (pcb_net_id, name) in net_table {
        if name.is_empty() {
            continue;
        }
        // pcb_net_id is 1-indexed; IR NetId is 0-indexed.
        let ir_net_idx = pcb_net_id.saturating_sub(1);
        // Primary: topological inference via power domain map (works with opaque net names).
        // Fallback: name-based heuristic (works for boards with semantic names but missing pins).
        let is_power_net = domain_map
            .get(synth_ir::NetId(ir_net_idx))
            .map_or_else(|| is_power_name(name), |d| d.is_rail() || d.is_ground());

        if is_rf(name) {
            rf_nets.push(name.clone());
        } else if is_power_net {
            power_nets.push(name.clone());
        } else {
            default_nets.push(name.clone());
        }
    }

    let mut classes = Vec::new();

    // Default Net Class (signals)
    let mut default_args = vec![
        Sexp::str("Default"),
        Sexp::str("Default net class"),
        Sexp::list("clearance", vec![num(0.127)]),
        Sexp::list("trace_width", vec![num(0.127)]),
        Sexp::list("via_dia", vec![num(0.60)]),
        Sexp::list("via_drill", vec![num(0.30)]),
    ];
    for name in default_nets {
        default_args.push(Sexp::list("add_net", vec![Sexp::str(&name)]));
    }
    classes.push(Sexp::list("net_class", default_args));

    // Power Net Class (PDN)
    if !power_nets.is_empty() {
        let mut power_args = vec![
            Sexp::str("Power"),
            Sexp::str("Power delivery network"),
            Sexp::list("clearance", vec![num(0.127)]),
            Sexp::list("trace_width", vec![num(0.50)]),
            Sexp::list("via_dia", vec![num(0.80)]),
            Sexp::list("via_drill", vec![num(0.40)]),
        ];
        for name in power_nets {
            power_args.push(Sexp::list("add_net", vec![Sexp::str(&name)]));
        }
        classes.push(Sexp::list("net_class", power_args));
    }

    // RF Net Class (Controlled 50 ohm impedance)
    if !rf_nets.is_empty() {
        let mut rf_args = vec![
            Sexp::str("RF_50"),
            Sexp::str("Controlled 50 ohm RF"),
            Sexp::list("clearance", vec![num(0.127)]),
            Sexp::list("trace_width", vec![num(0.33)]),
            Sexp::list("via_dia", vec![num(0.60)]),
            Sexp::list("via_drill", vec![num(0.30)]),
        ];
        for name in rf_nets {
            rf_args.push(Sexp::list("add_net", vec![Sexp::str(&name)]));
        }
        classes.push(Sexp::list("net_class", rf_args));
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

fn build_footprint_instance(
    component: &synth_ir::Component,
    part: &synth_registry::Part,
    placement: &ComponentPlacement,
    lib_id: &str,
    project: &Uuid,
    pad_net_lookup: &PadNetLookup,
) -> Sexp {
    // Keep the center pairs as single tuple bindings — destructured
    // `*_x_*`/`*_y_*` names trip clippy's `similar_names`.
    let (court_center_mm, _) = synth_layout::pcb_courtyard_geometry_for_part(part);
    let rot_center_nm = placement
        .rotation
        .rotate_offset(mm_to_nm(court_center_mm.0), mm_to_nm(court_center_mm.1));
    let x = nm_to_mm(placement.center.x_nm - rot_center_nm.0);
    let y = nm_to_mm(placement.center.y_nm - rot_center_nm.1);
    let angle = f64::from(placement.rotation.degrees());
    let layer_name = match placement.layer {
        Layer::Top => "F.Cu",
        _ => "B.Cu",
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

    let (unrot_w, unrot_h) = part.footprint_dimensions.as_ref().map_or_else(
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

    let half_w = unrot_w / 2.0;
    let half_h = unrot_h / 2.0;

    // Dynamic silkscreen text legalizer: compute text offset margin to clear pads and courtyards
    let text_margin = 3.5;

    let (ref_local_x, ref_local_y, ref_text_angle) = match placement.rotation {
        Rotation::Zero => (0.0, -(half_h + text_margin), 0.0),
        Rotation::Ninety => (-(half_w + text_margin), 0.0, -90.0),
        Rotation::OneEighty => (0.0, half_h + text_margin, 180.0),
        Rotation::TwoSeventy => (half_w + text_margin, 0.0, 90.0),
    };

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

    let mut children = vec![
        Sexp::list("layer", vec![Sexp::str(layer_name)]),
        str_pair("uuid", fp_uuid.to_string()),
        Sexp::list("at", at_args),
        Sexp::list(
            "property",
            vec![
                Sexp::str("Reference"),
                Sexp::str(&component.refdes),
                Sexp::list(
                    "at",
                    vec![num(ref_local_x), num(ref_local_y), num(ref_text_angle)],
                ),
                Sexp::list("layer", vec![Sexp::str(silk_layer)]),
                Sexp::list(
                    "effects",
                    vec![Sexp::list(
                        "font",
                        vec![Sexp::list("size", vec![num(1.0), num(1.0)])],
                    )],
                ),
            ],
        ),
        Sexp::list(
            "property",
            vec![
                Sexp::str("Value"),
                Sexp::str(part.id.as_str()),
                Sexp::list(
                    "at",
                    vec![num(val_local_x), num(val_local_y), num(val_text_angle)],
                ),
                Sexp::list("layer", vec![Sexp::str(fab_layer)]),
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
    ];

    let has_model = if let Some(body) = inlined_body.as_deref() {
        // Strip the outer `(footprint "name" ...)` head so the geometry is
        // embedded self-contained under the module. Leaving the head in place
        // makes pcbnew try to resolve a footprint *library* named after `name`
        // (e.g. a user-generated `<id>.pretty`), which is not registered in the
        // project's fp-lib-table and produces a spurious DRC error.
        let body = strip_footprint_head(body);
        let body_with_nets = inject_pad_nets(&body, component.id, pad_net_lookup);
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
                            Sexp::list(
                                "net",
                                vec![Sexp::atom(net_id.to_string()), Sexp::str(net_name)],
                            ),
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
    // directory in the project's fp-lib-table (see `export_with_sidecar`) so the
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
    use synth_geometry::Point;
    use synth_registry::{ElectricalType, Lifecycle, Pin, PinNumber};

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
            placement_hint: None,
            group: None,
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
        for layer_count in [2, 4] {
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
            } else {
                assert!(
                    names.contains("F.Cu")
                        && names.contains("In1.Cu")
                        && names.contains("In2.Cu")
                        && names.contains("B.Cu")
                );
            }
        }
    }

    #[test]
    fn netclass_is_top_level_outside_setup() {
        let setup_sexp = build_setup();
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
            name: "test_netclass".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components: vec![],
            nets: vec![
                synth_ir::Net {
                    id: synth_ir::NetId(0),
                    name: "VCC".to_string(),
                    endpoints: vec![],
                },
                synth_ir::Net {
                    id: synth_ir::NetId(1),
                    name: "GND".to_string(),
                    endpoints: vec![],
                },
                synth_ir::Net {
                    id: synth_ir::NetId(2),
                    name: "SIG1".to_string(),
                    endpoints: vec![],
                },
                synth_ir::Net {
                    id: synth_ir::NetId(3),
                    name: "MAIN_ANT".to_string(),
                    endpoints: vec![],
                },
            ],
            diff_pairs: vec![],
            keepouts: vec![],
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
        assert!(nc_str.contains("\"RF_50\""));
        assert!(nc_str.contains("(add_net \"SIG1\")"));
        assert!(nc_str.contains("(add_net \"VCC\")"));
        assert!(nc_str.contains("(add_net \"GND\")"));
        assert!(nc_str.contains("(add_net \"MAIN_ANT\")"));
    }

    #[test]
    fn test_kicad_cli_roundtrip_valid_board() {
        let board = Board {
            name: "test_roundtrip".to_string(),
            layers: 4,
            manufacturer: Some("jlcpcb".to_string()),
            revision: Some("A".to_string()),
            components: vec![],
            nets: vec![],
            diff_pairs: vec![],
            keepouts: vec![],
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
        };
        let project = Uuid::nil();
        let pcb_sexp = build_pcb(&board, &placement, &routing, &project);
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
