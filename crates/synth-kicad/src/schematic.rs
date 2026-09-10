// SPDX-License-Identifier: Apache-2.0

//! `.kicad_sch` generator.
//!
//! Placement *and* wire routing come from [`synth_layout::layout`] so
//! the browser preview and the KiCad export agree on every component's
//! position and every signal's route. `layout.wires` carries one
//! connected orthogonal polyline per root-to-endpoint route (produced
//! by `synth_layout::route::route_board`); this module only turns
//! those into KiCad `(wire ...)` s-expression nodes, junction dots,
//! power symbols and net labels. Coordinates are millimetres.

// KiCad coordinates are millimetres in `f64`. usize/u32 → f64 casts
// at the export boundary are intentional and well-bounded by pin
// counts and column indices.
#![allow(clippy::cast_precision_loss, clippy::cast_lossless)]

use std::collections::{HashMap, HashSet};

use synth_ir::{Board, Component, ComponentId, NetId};
/// Dedup key for a single emitted wire segment: `(net, endpoint_a, endpoint_b)`
/// in quantized (x100) integer coordinates.
type EmittedSegment = (NetId, (i64, i64), (i64, i64));

/// Content-addressed UUID for one emitted wire segment.
///
/// Keyed by the net name plus the quantized segment endpoints instead
/// of positional route indices. Reordering functionally identical
/// input therefore reshuffles emission order but not identity —
/// every other entity kind is already content-keyed via
/// [`derive_entity_uuid`], and wires dominate file size, so this is
/// what keeps diffs stable. Canonicalizes `a`/`b` order internally
/// (rather than trusting the caller to) so the same physical segment
/// hashes identically regardless of which endpoint the caller treats
/// as the start.
fn wire_segment_uuid(project: &Uuid, net_name: &str, a: (i64, i64), b: (i64, i64)) -> Uuid {
    let (a, b) = if a.0 < b.0 || (a.0 == b.0 && a.1 < b.1) {
        (a, b)
    } else {
        (b, a)
    };
    let key = format!("wire:{net_name}:({},{})-({},{})", a.0, a.1, b.0, b.1);
    derive_entity_uuid(project, "wire", &key)
}

/// Deterministic, collision-free allocator for hidden power-symbol
/// reference designators (`#PWR####`, `#FLG####`).
///
/// The previous scheme projected the symbol UUID into a 9000-slot
/// space (`uuid % 9000 + 1000`); past ~100 instances birthday
/// collisions became likely and KiCad flags duplicate references.
/// Starting from that same projection but scanning forward on
/// conflict keeps output deterministic (emission order is
/// deterministic) while making duplicates structurally impossible.
#[derive(Default)]
struct PowerRefAllocator {
    taken: HashSet<String>,
}

impl PowerRefAllocator {
    fn allocate(&mut self, prefix: &str, sym_uuid: &Uuid) -> String {
        let mut n = ((sym_uuid.as_u128() % 9000) + 1000) as u32;
        loop {
            let candidate = format!("{prefix}{n:04}");
            if self.taken.insert(candidate.clone()) {
                return candidate;
            }
            n = if n >= 9999 { 1000 } else { n + 1 };
        }
    }
}
use synth_layout::{ComponentPlacement, PinSide, Rotation};
use uuid::Uuid;

use crate::sexp::{num, pair, str_pair, Sexp};
use crate::symbol_lib::{self, LIBRARY_NICKNAME};
use crate::uuid_v5::derive_entity_uuid;

pub use synth_layout::route::pin_terminal_xy;
use synth_layout::route::{
    classify_ic_pin, is_two_pin_symbol_kind, natural_rotation_offset, snap_grid_127,
    BODY_HALF_WIDTH, BODY_PIN_PADDING, MIN_BODY_HEIGHT, PIN_LENGTH, PIN_PITCH, TWOPIN_HALF_W,
};

/// Build the entire `.kicad_sch` s-expression for `board`.
/// `project` is the per-project UUID namespace.
#[allow(clippy::too_many_lines)]
pub fn build_schematic(board: &Board, project: &Uuid) -> Sexp {
    build_schematic_from_layout(board, project, &synth_layout::layout(board))
}

/// Build the schematic honouring an optional sidecar override file
/// (`<design>.synth.layout.toml`). The sidecar moves components
/// between placement and routing (see
/// [`synth_layout::layout_with_sidecar`]), so the exported wires and
/// labels always match the manually tuned positions.
pub fn build_schematic_with_sidecar(
    board: &Board,
    project: &Uuid,
    sidecar: Option<&std::path::Path>,
) -> Sexp {
    let layout = synth_layout::layout_with_sidecar(board, sidecar);
    build_schematic_from_layout(board, project, &layout)
}

pub(crate) fn build_schematic_from_layout(
    board: &Board,
    project: &Uuid,
    layout: &synth_layout::Layout,
) -> Sexp {
    let placements: HashMap<ComponentId, &ComponentPlacement> =
        layout.components.iter().map(|p| (p.id, p)).collect();
    // Single source of truth for which nets need a `power:PWR_FLAG`
    // driver — consumed by both the lib_symbols embedding decision
    // (below) and the instance emission loop (bottom of this
    // function). Keeping these on one computation guarantees the
    // embedded definition set exactly matches the emitted instances.
    let power_drivers = crate::pin_reconcile::undriven_power_nets(board, &placements);
    let paper = match layout.sheet_size {
        synth_layout::SheetSize::A4 => "A4",
        synth_layout::SheetSize::A3 => "A3",
        // KiCad doesn't carry a "Custom" enum value in the standard
        // paper sizes; A2 is the safe upper bound for V1 designs.
        synth_layout::SheetSize::A2 | synth_layout::SheetSize::Custom { .. } => "A2",
    };

    // Fabrication target from the `manufacturer "…"` board statement.
    // Professional drawings document who builds the board; KiCad
    // renders this inside the title block as note 3. Omitted entirely
    // when no fab is declared (no empty comment clutter). The
    // design-authority "company" field is deliberately left blank —
    // the fab is not the design company.
    let fab_comment = board.manufacturer.as_deref().map(|fab| {
        Sexp::list(
            "comment",
            vec![Sexp::atom("3"), Sexp::str(format!("Fab: {fab}"))],
        )
    });

    let mut children = vec![
        pair("version", Sexp::atom("20260306")),
        str_pair("generator", "synth-eda"),
        str_pair("generator_version", "10.0"),
        str_pair(
            "uuid",
            derive_entity_uuid(project, "sheet", "root").to_string(),
        ),
        Sexp::list("paper", vec![Sexp::str(paper)]),
        // Title block from board metadata. Deliberately NO date:
        // embedding today's date would break the byte-identical
        // re-export guarantee that motivates UUIDv5 everywhere else.
        // Field-position semantics note (verified against KiCad 10.0.5
        // via `kicad-cli sch export svg`, 2026-08): instance property
        // `at` coordinates are ABSOLUTE sheet coordinates regardless
        // of the instance rotation — no inverse transform needed when
        // writing Reference/Value positions below.
        Sexp::list(
            "title_block",
            vec![
                Sexp::list("title", vec![Sexp::str(&board.name)]),
                Sexp::list("date", vec![Sexp::str("")]),
                // Revision from the `revision "…"` board statement, or
                // blank when unset (Sierra Circuits "Schematic Design
                // Rules": the title block should display the
                // Revision).
                Sexp::list(
                    "rev",
                    vec![Sexp::str(board.revision.as_deref().unwrap_or(""))],
                ),
                Sexp::list("company", vec![Sexp::str("")]),
                // Design notes (ProtoExpress "Schematic Design Rules":
                // "Provide all the required notes related to the
                // schematic"). KiCad renders `comment` entries inside
                // its own title block, so they can never collide with
                // placed symbols. Deterministic content only — no
                // timestamps (byte-identical re-export guarantee).
                Sexp::list(
                    "comment",
                    vec![Sexp::atom("1"), Sexp::str("Generated by synth-eda")],
                ),
                Sexp::list(
                    "comment",
                    vec![
                        Sexp::atom("2"),
                        Sexp::str(format!(
                            "{} components / {} nets",
                            board.components.len(),
                            board.nets.len()
                        )),
                    ],
                ),
            ]
            .into_iter()
            .chain(fab_comment)
            .collect::<Vec<_>>(),
        ),
        embed_library(board, layout, &power_drivers),
    ];

    // Symbol instances.
    for component in &board.components {
        if let Some(placement) = placements.get(&component.id) {
            if let Some(s) = build_symbol_instance(component, placement, project) {
                children.push(s);
            }
        }
    }

    // Wires arrive pre-routed from `synth_layout::layout`. Each
    // `WirePath` is one connected orthogonal polyline (a
    // root-to-endpoint route); KiCad wires are single 2-point
    // segments, so split each polyline into its consecutive segments
    // and emit one `(wire ...)` per segment.
    let mut emitted: HashSet<EmittedSegment> = HashSet::new();
    let quant = |v: f64| (v * 100.0).round() as i64;
    for wire in &layout.wires {
        let net_name = board
            .net(wire.net)
            .map_or_else(|| format!("NET_{}", wire.net.0), |n| n.name.clone());
        for pair in wire.points.windows(2) {
            let (p1, p2) = (pair[0], pair[1]);
            let a = (quant(p1.0), quant(p1.1));
            let b = (quant(p2.0), quant(p2.1));
            if a == b {
                continue;
            }
            let seg = if a.0 < b.0 || (a.0 == b.0 && a.1 < b.1) {
                (a, b)
            } else {
                (b, a)
            };
            if !emitted.insert((wire.net, seg.0, seg.1)) {
                continue; // duplicate segment
            }

            let wire_uuid = wire_segment_uuid(project, &net_name, seg.0, seg.1);
            children.push(Sexp::list(
                "wire",
                vec![
                    Sexp::list(
                        "pts",
                        vec![
                            Sexp::list("xy", vec![num(p1.0), num(p1.1)]),
                            Sexp::list("xy", vec![num(p2.0), num(p2.1)]),
                        ],
                    ),
                    Sexp::list(
                        "stroke",
                        vec![
                            Sexp::list("width", vec![num(0.0)]),
                            Sexp::list("type", vec![Sexp::atom("default")]),
                        ],
                    ),
                    str_pair("uuid", wire_uuid.to_string()),
                ],
            ));
        }
    }

    // Junction dots (the ≥3-way meeting filtering already happened
    // inside `synth_layout::route::route_board`).
    for (jx, jy) in &layout.junctions {
        let qx = quant(*jx);
        let qy = quant(*jy);
        let junc_key = format!("junction_{qx}_{qy}");
        let junc_uuid = derive_entity_uuid(project, "junction", &junc_key);
        children.push(Sexp::list(
            "junction",
            vec![
                Sexp::list("at", vec![num(*jx), num(*jy)]),
                Sexp::list("diameter", vec![num(0.0)]),
                Sexp::list("color", vec![num(0.0), num(0.0), num(0.0), num(0.0)]),
                str_pair("uuid", junc_uuid.to_string()),
            ],
        ));
    }

    // KiCad power symbols at every power-flag pin (wire stub + symbol).
    let mut power_refs = PowerRefAllocator::default();
    for flag in &layout.power_flags {
        if let Some(mut sexps) =
            build_power_symbol(board, flag, &placements, project, &mut power_refs)
        {
            children.append(&mut sexps);
        }
    }

    // KiCad per-pin net labels at every labeled-net endpoint. Each
    // endpoint gets its own short stub + local `(label)` with the
    // same name; same-named labels on the sheet are electrically
    // connected (the same primitive the reference designs use for
    // SCL/SDA/D+/D-). No long wires, no top rails. Unroutable nets
    // were merged into `layout.net_labels` by `synth_layout::layout`,
    // so this loop covers the routing-fallback labels too.
    for label in &layout.net_labels {
        if let Some(sexps) = build_net_label(board, label, &placements, project) {
            children.extend(sexps);
        }
    }

    // Sub-circuit captions: one text run per declared `group`, drawn
    // above the parts it names. These are pure annotation — KiCad
    // treats `(text)` as a graphic with no electrical meaning, so a
    // caption can never join a net or trip ERC.
    for (index, annotation) in layout.annotations.iter().enumerate() {
        let (x, y) = annotation.at_mm;
        let uuid = derive_entity_uuid(
            project,
            "annotation",
            &format!("{index}_{}", annotation.text),
        );
        children.push(Sexp::list(
            "text",
            vec![
                Sexp::str(&annotation.text),
                Sexp::list("at", vec![num(x), num(y), num(0.0)]),
                Sexp::list(
                    "effects",
                    vec![
                        Sexp::list(
                            "font",
                            vec![Sexp::list(
                                "size",
                                vec![num(annotation.size_mm), num(annotation.size_mm)],
                            )],
                        ),
                        Sexp::list("justify", vec![Sexp::atom("left"), Sexp::atom("bottom")]),
                    ],
                ),
                str_pair("uuid", uuid.to_string()),
            ],
        ));
    }

    // Physical-pin reconciliation: the registry declares *logical*
    // pins, but the referenced KiCad symbol carries every physical
    // pin. Fan the rails out to the undeclared power legs (VDD/VDDA/
    // VSS/VSSA ...) so KiCad ERC sees a fully-powered part, and emit
    // `(no_connect ...)` markers on every other unreached pin so the
    // intentionally-unused GPIOs/stubs don't trip `pin_not_connected`.
    // See `crate::pin_reconcile` for the reasoning.
    // The fan-out leg must share the rail's *actual* net label (e.g.
    // "OUT", "VBUS", "+3V3") rather than a hard-coded "+3V3", or KiCad
    // would place it on a distinct global net with no driver.
    let rail_label = |net_id: NetId| -> Option<&str> {
        layout
            .power_flags
            .iter()
            .find(|f| f.net == net_id)
            .map(|f| f.label.as_str())
    };
    for reconciled in crate::pin_reconcile::reconcile_all(board) {
        for leg in &reconciled.power_legs {
            if let Some(label) = rail_label(leg.net) {
                if let Some(sexps) = build_fanned_power_symbol(
                    board,
                    leg,
                    label,
                    &placements,
                    project,
                    &mut power_refs,
                ) {
                    children.extend(sexps);
                }
            }
        }
        for pin in &reconciled.no_connects {
            if let Some(sexp) = build_no_connect(board, pin, &placements, project) {
                children.push(sexp);
            }
        }
    }

    // KiCad ERC requires every power net to have a `power_out`
    // driver. Rails fed from a connector/passive header—where the
    // "driver" is the external supply, not an on-sheet power_out
    // pin—would otherwise trip `power_pin_not_driven`. Emit a
    // `power:PWR_FLAG` (electrical type `power_out`) on every such
    // net so ERC treats the incoming rail as driven, exactly as the
    // KiCad demos place `#FLG` symbols at board power entry points.
    for driver in &power_drivers {
        if let Some(sexp) = build_power_flag_driver(board, driver, project, &mut power_refs) {
            children.push(sexp);
        }
    }

    Sexp::list("kicad_sch", children)
}

/// Emit a `power:PWR_FLAG` symbol on a net that needs an external
/// power-out driver. Placed exactly at the position of the rail's own
/// power-flag symbol (one stub-length out from the anchor pin's
/// terminal). The rail's power-flag stub wire already runs from the
/// pin terminal to that point, so the PWR_FLAG's power_out pin joins
/// the rail without a redundant duplicate wire.
fn build_power_flag_driver(
    board: &Board,
    driver: &crate::pin_reconcile::PowerDriver,
    project: &Uuid,
    power_refs: &mut PowerRefAllocator,
) -> Option<Sexp> {
    let component = board.component(driver.anchor_component)?;
    let part = component.part.as_ref()?;
    let lib_id = part.kicad_symbol.as_deref()?;

    let natural_offset = natural_rotation_offset(part);
    // The anchor pin for an undriven power net is a declared power-input
    // pin; those are placed un-rotated in the layout's canonical
    // orientation, so logical rotation is zero.
    let total_deg = natural_offset.rem_euclid(360.0);

    let (x, y, dx, dy) = crate::pin_reconcile::physical_terminal(
        lib_id,
        &driver.anchor_pin_number,
        driver.anchor_center,
        total_deg,
    )?;

    let stub_len = 2.54;
    let flag_x = x + dx * stub_len;
    let flag_y = y + dy * stub_len;

    let key = format!("pwr_flag_net_{}", driver.net.0);
    let sym_uuid = derive_entity_uuid(project, "power_flag", &key);
    let refdes = power_refs.allocate("#FLG", &sym_uuid);

    Some(Sexp::list(
        "symbol",
        vec![
            str_pair("lib_id", "power:PWR_FLAG"),
            Sexp::list("at", vec![num(flag_x), num(flag_y), num(0.0)]),
            pair("unit", Sexp::atom("1")),
            pair("in_bom", Sexp::atom("no")),
            pair("on_board", Sexp::atom("no")),
            str_pair("uuid", sym_uuid.to_string()),
            Sexp::list(
                "property",
                vec![
                    Sexp::str("Reference"),
                    Sexp::str(&refdes),
                    Sexp::list("at", vec![num(flag_x), num(flag_y), num(0.0)]),
                    Sexp::list(
                        "effects",
                        vec![
                            Sexp::list(
                                "font",
                                vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                            ),
                            Sexp::list("hide", vec![Sexp::atom("yes")]),
                        ],
                    ),
                ],
            ),
            Sexp::list(
                "property",
                vec![
                    Sexp::str("Value"),
                    Sexp::str("PWR_FLAG"),
                    Sexp::list("at", vec![num(flag_x), num(flag_y), num(0.0)]),
                    Sexp::list(
                        "effects",
                        vec![
                            Sexp::list(
                                "font",
                                vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                            ),
                            Sexp::list("hide", vec![Sexp::atom("yes")]),
                        ],
                    ),
                ],
            ),
        ],
    ))
}

/// Emit a power symbol + stub for a physical power leg that the
/// netlist doesn't reach but belongs on an existing rail net (fan-out
/// of the declared VDD/VSS legs). Mirrors [`build_power_symbol`] but
/// drives the geometry from the symbol's physical pin positions.
fn build_fanned_power_symbol(
    board: &Board,
    leg: &crate::pin_reconcile::PowerLeg,
    rail_label: &str,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    project: &Uuid,
    power_refs: &mut PowerRefAllocator,
) -> Option<Vec<Sexp>> {
    let component = board.component(leg.component)?;
    let part = component.part.as_ref()?;
    let lib_id = part.kicad_symbol.as_deref()?;
    let placement = placements.get(&component.id)?;

    let natural_offset = natural_rotation_offset(part);
    let logical_deg = match placement.rotation {
        Rotation::Zero => 0.0,
        Rotation::Ninety => 90.0,
        Rotation::OneEighty => 180.0,
        Rotation::TwoSeventy => 270.0,
    };
    let total_deg = (logical_deg + natural_offset).rem_euclid(360.0);

    let (x, y, dx, dy) = crate::pin_reconcile::physical_terminal(
        lib_id,
        &leg.pin_number,
        (
            snap_grid_127(placement.center_mm.0),
            snap_grid_127(placement.center_mm.1),
        ),
        total_deg,
    )?;

    let label = match leg.family {
        // Both families fly the *net's own* power-flag label: since
        // `classify_power_flags` prefers declared net names, a
        // separate `AGND`/`DGND` rail keeps its identity here, and
        // positive legs join their actual rail (`VBUS`, `+3V3`, ...)
        // rather than a spurious driver-less global net.
        crate::pin_reconcile::RailFamily::Ground | crate::pin_reconcile::RailFamily::Positive => {
            rail_label
        }
    };

    // Offset the symbol one grid unit outward from the pin so it
    // sits clear of the body.
    let stub_len = 2.54;
    let flag_x = x + dx * stub_len;
    let flag_y = y + dy * stub_len;

    let lib_id_out = power_symbol_lib_id(label);
    let key = format!("power_fanout_{}_{}", component.refdes, leg.pin_number);
    let sym_uuid = derive_entity_uuid(project, "power_symbol", &key);
    let wire_uuid = derive_entity_uuid(project, "power_wire", &key);
    let refdes_pwr = power_refs.allocate("#PWR", &sym_uuid);

    let wire_sexp = Sexp::list(
        "wire",
        vec![
            Sexp::list(
                "pts",
                vec![
                    Sexp::list("xy", vec![num(x), num(y)]),
                    Sexp::list("xy", vec![num(flag_x), num(flag_y)]),
                ],
            ),
            Sexp::list(
                "stroke",
                vec![
                    Sexp::list("width", vec![num(0.0)]),
                    Sexp::list("type", vec![Sexp::atom("default")]),
                ],
            ),
            str_pair("uuid", wire_uuid.to_string()),
        ],
    );

    let sym_sexp = fanned_symbol_sexp(lib_id_out, flag_x, flag_y, sym_uuid, &refdes_pwr, label);

    Some(vec![wire_sexp, sym_sexp])
}

/// Build the `(symbol ...)` S-expression for a fanned power flag,
/// carrying hidden Reference/Value properties at the given position.
fn fanned_symbol_sexp(
    lib_id: String,
    flag_x: f64,
    flag_y: f64,
    sym_uuid: Uuid,
    refdes: &str,
    label: &str,
) -> Sexp {
    let property_at = Sexp::list("at", vec![num(flag_x), num(flag_y), num(0.0)]);
    let effects = Sexp::list(
        "effects",
        vec![
            Sexp::list("font", vec![Sexp::list("size", vec![num(1.27), num(1.27)])]),
            Sexp::list("hide", vec![Sexp::atom("yes")]),
        ],
    );
    Sexp::list(
        "symbol",
        vec![
            str_pair("lib_id", lib_id),
            Sexp::list("at", vec![num(flag_x), num(flag_y), num(0.0)]),
            pair("unit", Sexp::atom("1")),
            pair("in_bom", Sexp::atom("no")),
            pair("on_board", Sexp::atom("no")),
            str_pair("uuid", sym_uuid.to_string()),
            Sexp::list(
                "property",
                vec![
                    Sexp::str("Reference"),
                    Sexp::str(refdes),
                    property_at.clone(),
                    effects.clone(),
                ],
            ),
            Sexp::list(
                "property",
                vec![Sexp::str("Value"), Sexp::str(label), property_at, effects],
            ),
        ],
    )
}

/// Emit a `(no_connect ...)` marker at a physical pin's terminal so
/// KiCad ERC treats the pin as intentionally unconnected.
fn build_no_connect(
    board: &Board,
    pin: &crate::pin_reconcile::NoConnectPin,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    project: &Uuid,
) -> Option<Sexp> {
    let component = board.component(pin.component)?;
    let part = component.part.as_ref()?;
    let placement = placements.get(&component.id)?;

    let center = (
        snap_grid_127(placement.center_mm.0),
        snap_grid_127(placement.center_mm.1),
    );
    let natural_offset = natural_rotation_offset(part);
    let logical_deg = match placement.rotation {
        Rotation::Zero => 0.0,
        Rotation::Ninety => 90.0,
        Rotation::OneEighty => 180.0,
        Rotation::TwoSeventy => 270.0,
    };
    let total_deg = (logical_deg + natural_offset).rem_euclid(360.0);

    // Prefer the referenced symbol's physical pin positions; fall back
    // to the synthesized-rectangle terminal for parts without a
    // `kicad_symbol` mapping (BG95, etc.).
    let (x, y) = if let Some(lib_id) = part.kicad_symbol.as_deref() {
        crate::pin_reconcile::physical_terminal(lib_id, &pin.pin_number, center, total_deg)
            .map(|(x, y, _, _)| (x, y))?
    } else {
        let pin_idx = part
            .pins
            .iter()
            .position(|p| p.number.0 == pin.pin_number)
            .map(|i| i as u32)?;
        let (x, y, _dx, _dy) =
            pin_terminal_xy(board, component.id, synth_ir::PinId(pin_idx), placements)?;
        (snap_grid_127(x), snap_grid_127(y))
    };

    let key = format!("no_connect_{}_{}", component.refdes, pin.pin_number);
    let node_uuid = derive_entity_uuid(project, "no_connect", &key);
    Some(crate::pin_reconcile::no_connect_sexp(
        x,
        y,
        &node_uuid.to_string(),
    ))
}

/// Emit a KiCad `(global_label)` entry attached to a stub extending
/// from the pin's coordinate.
fn build_net_label(
    board: &Board,
    label: &synth_layout::NetLabel,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    project: &Uuid,
) -> Option<Vec<Sexp>> {
    let component = board.component(label.component)?;
    let (x, y, dx, _dy) = pin_terminal_xy(board, label.component, label.pin, placements)?;
    let stub_len = 5.08;
    let (stub_x, angle) = if dx >= -0.1 {
        (x + stub_len, 0.0)
    } else {
        (x - stub_len, 180.0)
    };

    let key = format!("net_label_{}_{}", component.refdes, label.pin.0);
    let wire_uuid = derive_entity_uuid(project, "net_label_wire", &key);
    let label_uuid = derive_entity_uuid(project, "net_label", &key);

    let wire_sexp = Sexp::list(
        "wire",
        vec![
            Sexp::list(
                "pts",
                vec![
                    Sexp::list("xy", vec![num(x), num(y)]),
                    Sexp::list("xy", vec![num(stub_x), num(y)]),
                ],
            ),
            Sexp::list(
                "stroke",
                vec![
                    Sexp::list("width", vec![num(0.0)]),
                    Sexp::list("type", vec![Sexp::atom("default")]),
                ],
            ),
            str_pair("uuid", wire_uuid.to_string()),
        ],
    );

    let label_sexp = Sexp::list(
        "label",
        vec![
            Sexp::str(&label.label),
            Sexp::list("at", vec![num(stub_x), num(y), num(angle)]),
            Sexp::list(
                "effects",
                vec![Sexp::list(
                    "font",
                    vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                )],
            ),
            str_pair("uuid", label_uuid.to_string()),
        ],
    );

    Some(vec![wire_sexp, label_sexp])
}

/// Resolve a power-flag label to a KiCad lib_id, preferring the
/// bundled `power:` library when available.
fn power_symbol_lib_id(label: &str) -> String {
    let power_lib_id = format!("power:{label}");
    if synth_layout::kicad_lib_loader::load_symbol(&power_lib_id).is_some() {
        return power_lib_id;
    }
    format!("{LIBRARY_NICKNAME}:{label}")
}

/// Emit a KiCad `(symbol)` entry for a power-flag symbol attached
/// just outside the pin terminal with a short connecting wire.
/// Returns `[wire, symbol]` so the stub is explicit and the flag
/// never overlaps the component body.
fn build_power_symbol(
    board: &Board,
    flag: &synth_layout::PowerFlag,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    project: &Uuid,
    power_refs: &mut PowerRefAllocator,
) -> Option<Vec<Sexp>> {
    let component = board.component(flag.component)?;
    // Pre-emission validation: verify net match against source netlist declaration
    if let Some(net) = board.net(flag.net) {
        assert!(
            net.endpoints
                .iter()
                .any(|ep| ep.component == flag.component && ep.pin == flag.pin),
            "power flag net mismatch: component {:?} pin {:?} not found in net {:?}",
            flag.component,
            flag.pin,
            flag.net
        );
    }
    let (x, y, dx, dy) = pin_terminal_xy(board, flag.component, flag.pin, placements)?;

    // Offset the symbol one grid unit outward from the pin so it
    // sits clear of the body.
    let stub_len = 2.54;
    let flag_x = x + dx * stub_len;
    let flag_y = y + dy * stub_len;

    let lib_id = power_symbol_lib_id(&flag.label);
    let key = format!("power_{}_{}", component.refdes, flag.pin.0);
    let sym_uuid = derive_entity_uuid(project, "power_symbol", &key);
    let wire_uuid = derive_entity_uuid(project, "power_wire", &key);
    let refdes_pwr = power_refs.allocate("#PWR", &sym_uuid);

    let wire_sexp = Sexp::list(
        "wire",
        vec![
            Sexp::list(
                "pts",
                vec![
                    Sexp::list("xy", vec![num(x), num(y)]),
                    Sexp::list("xy", vec![num(flag_x), num(flag_y)]),
                ],
            ),
            Sexp::list(
                "stroke",
                vec![
                    Sexp::list("width", vec![num(0.0)]),
                    Sexp::list("type", vec![Sexp::atom("default")]),
                ],
            ),
            str_pair("uuid", wire_uuid.to_string()),
        ],
    );

    let sym_sexp = Sexp::list(
        "symbol",
        vec![
            str_pair("lib_id", lib_id),
            Sexp::list("at", vec![num(flag_x), num(flag_y), num(0.0)]),
            pair("unit", Sexp::atom("1")),
            pair("in_bom", Sexp::atom("no")),
            pair("on_board", Sexp::atom("no")),
            str_pair("uuid", sym_uuid.to_string()),
            Sexp::list(
                "property",
                vec![
                    Sexp::str("Reference"),
                    Sexp::str(&refdes_pwr),
                    Sexp::list("at", vec![num(flag_x), num(flag_y), num(0.0)]),
                    Sexp::list(
                        "effects",
                        vec![
                            Sexp::list(
                                "font",
                                vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                            ),
                            Sexp::list("hide", vec![Sexp::atom("yes")]),
                        ],
                    ),
                ],
            ),
            Sexp::list(
                "property",
                vec![
                    Sexp::str("Value"),
                    Sexp::str(&flag.label),
                    Sexp::list("at", vec![num(flag_x), num(flag_y), num(0.0)]),
                    Sexp::list(
                        "effects",
                        vec![
                            Sexp::list(
                                "font",
                                vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                            ),
                            Sexp::list("hide", vec![Sexp::atom("yes")]),
                        ],
                    ),
                ],
            ),
        ],
    );

    Some(vec![wire_sexp, sym_sexp])
}

fn embed_library(
    board: &Board,
    layout: &synth_layout::Layout,
    power_drivers: &[crate::pin_reconcile::PowerDriver],
) -> Sexp {
    // KiCad supports an inline lib_symbols block as well as an
    // external .kicad_sym library. We emit the symbols inline so the
    // schematic is openable without any library lookup path setup.
    let lib = symbol_lib::build_library(board, layout);
    let Sexp::List {
        children: lib_children,
        ..
    } = lib
    else {
        unreachable!("symbol_lib::build_library always returns a list")
    };
    // The first three children of the library are (version ...),
    // (generator ...), (generator_version ...); drop those — only
    // the symbol nodes belong in lib_symbols. Both synthesized
    // (List head "symbol") and stock-loaded (Raw) variants survive
    // the filter.
    let inner: Vec<Sexp> = lib_children
        .into_iter()
        .filter(|c| match c {
            Sexp::List { head, .. } => head == "symbol",
            Sexp::Raw(_) => true,
            _ => false,
        })
        .collect();

    // Ensure `power:PWR_FLAG` is embedded whenever an undriven power
    // net gets a driver flag. The driver set comes from
    // `undriven_power_nets` — the same computation that emits the
    // instances — so the embed decision can never drift from the
    // emit decision (e.g. `GroundReference`-only nets, which the old
    // inline predicate missed). The stock value is loaded from
    // KiCad's bundled library so it resolves even when the global
    // sym-lib-table doesn't map `power`; without KiCad installed we
    // synthesize the definition under the referenced name.
    let mut inner = inner;
    if !power_drivers.is_empty() {
        match crate::symbol_lib::load_stock_symbol("power:PWR_FLAG") {
            Some(raw) => inner.push(Sexp::Raw(raw)),
            None => inner.push(crate::symbol_lib::build_pwr_flag_fallback()),
        }
    }

    Sexp::list("lib_symbols", inner)
}

#[allow(clippy::too_many_lines)]
/// Cardinal side of a symbol body where a Reference/Value field may
/// anchor. Mirrors KiCad's `AUTOPLACER::SIDE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldSide {
    Top,
    Bottom,
    Left,
    Right,
}

/// Count how many pins land on each side of `part` once drawn at
/// `rotation`, so [`choose_field_sides`] can steer labels away from
/// the densest side — the same signal KiCad's field autoplacer
/// (`AUTOPLACER::pinsOnSide`) uses.
///
/// Only 2-pin parts and diodes ever leave `Rotation::Zero` (see
/// `rotate_two_pin_with_power_flags`, `rotate_led_chains`,
/// `rotate_usb_esd_diodes`); every other part keeps
/// `classify_ic_pin`'s canonical Left/Top/Bottom/Right answer as its
/// on-sheet side. 2-pin parts use the pin0-left/pin1-right
/// convention (`Rotation::Zero` doc on the `Rotation` enum) and swap
/// onto Top/Bottom when logically vertical.
fn side_pin_counts(part: &synth_registry::Part, rotation: Rotation) -> [(FieldSide, usize); 4] {
    if part.pins.len() == 2 && is_two_pin_symbol_kind(&part.kind) {
        if matches!(rotation, Rotation::Ninety | Rotation::TwoSeventy) {
            [
                (FieldSide::Top, 1),
                (FieldSide::Bottom, 1),
                (FieldSide::Left, 0),
                (FieldSide::Right, 0),
            ]
        } else {
            [
                (FieldSide::Top, 0),
                (FieldSide::Bottom, 0),
                (FieldSide::Left, 1),
                (FieldSide::Right, 1),
            ]
        }
    } else {
        let sides: Vec<PinSide> = part.pins.iter().map(classify_ic_pin).collect();
        let count = |s: PinSide| sides.iter().filter(|&&x| x == s).count();
        [
            (FieldSide::Top, count(PinSide::Top)),
            (FieldSide::Bottom, count(PinSide::Bottom)),
            (FieldSide::Left, count(PinSide::Left)),
            (FieldSide::Right, count(PinSide::Right)),
        ]
    }
}

/// Pick which axis — Top/Bottom or Left/Right — carries the
/// Reference and Value fields, choosing whichever axis has fewer
/// pins in total. Adapted from KiCad's `chooseSideForFields`
/// heuristic (`AUTOPLACER::pinsOnSide`), but simplified to keep
/// Reference and Value as a fixed pair on one axis rather than
/// independently picking a side per field: two ICs with the same
/// pinout should always show Reference/Value in the same relative
/// spot, not have them swap because one has one more GND pin than
/// VCC pin. Within the chosen axis the pairing is fixed — Reference
/// leads (Top, or Right when the axis flips), Value trails — which
/// reproduces the long-standing Reference-above / Value-below
/// convention whenever Top/Bottom are equally or more free than
/// Left/Right (the common case for horizontally-drawn parts and
/// ICs), and only swaps the whole pair onto Left/Right when that
/// axis is genuinely less crowded (e.g. a decoupling cap rotated so
/// VCC/GND pins now sit Top/Bottom).
fn choose_field_sides(counts: [(FieldSide, usize); 4]) -> (FieldSide, FieldSide) {
    let get = |target: FieldSide| {
        counts
            .iter()
            .find(|&&(side, _)| side == target)
            .map_or(0, |&(_, n)| n)
    };
    let top_bottom_pins = get(FieldSide::Top) + get(FieldSide::Bottom);
    let left_right_pins = get(FieldSide::Left) + get(FieldSide::Right);
    if top_bottom_pins <= left_right_pins {
        (FieldSide::Top, FieldSide::Bottom)
    } else {
        (FieldSide::Right, FieldSide::Left)
    }
}

/// Gap between the body edge and a field anchor when that side has
/// no pins to clear — one pin-pitch grid step, close to the body
/// without crowding the outline.
const FIELD_GAP_TIGHT: f64 = 2.54;
/// Gap used when the chosen side does carry pins: clears a pin
/// stub's full length (`PIN_LENGTH`) plus a small margin so the
/// label doesn't sit on top of the stub's wire.
const FIELD_GAP_CLEAR: f64 = PIN_LENGTH + 1.27;

/// Body edges in sheet coordinates, `(left, right, top, bottom)`.
type BodyEdges = (f64, f64, f64, f64);

/// Anchor position and horizontal justification for a field placed
/// on `side` of a body centred at `center`, with edges `body`.
/// `pins_on_side` selects how much clearance the anchor needs — tight
/// against the body when nothing else occupies that side, wider when
/// a pin stub is there to clear. Top/Bottom fields stay
/// centre-justified (KiCad's default) exactly like before; Left/Right
/// fields justify away from the body so the text grows outward
/// instead of straddling the anchor back over the symbol.
fn field_anchor(
    side: FieldSide,
    pins_on_side: usize,
    center: (f64, f64),
    body: BodyEdges,
) -> ((f64, f64), Option<&'static str>) {
    let (x, y) = center;
    let (body_left, body_right, body_top, body_bottom) = body;
    let gap = if pins_on_side == 0 {
        FIELD_GAP_TIGHT
    } else {
        FIELD_GAP_CLEAR
    };
    match side {
        FieldSide::Top => ((x, body_top - gap), None),
        FieldSide::Bottom => ((x, body_bottom + gap), None),
        FieldSide::Right => ((body_right + gap, y), Some("left")),
        FieldSide::Left => ((body_left - gap, y), Some("right")),
    }
}

/// Look up the pin count [`side_pin_counts`] recorded for `side`.
fn pins_on(counts: [(FieldSide, usize); 4], side: FieldSide) -> usize {
    counts
        .iter()
        .find(|&&(s, _)| s == side)
        .map_or(0, |&(_, n)| n)
}

/// `(font (size ..)) [(justify ..)]` effects block for a field.
fn field_effects(justify: Option<&'static str>) -> Sexp {
    let mut children = vec![Sexp::list(
        "font",
        vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
    )];
    if let Some(j) = justify {
        children.push(Sexp::list("justify", vec![Sexp::atom(j)]));
    }
    Sexp::list("effects", children)
}

/// Effects for hidden instance properties (Footprint, Datasheet,
/// MPN, LCSC): standard 50 mil text, `(hide yes)` per KiCad
/// convention — metadata survives into the file without cluttering
/// the drawing.
fn hidden_field_effects() -> Sexp {
    Sexp::list(
        "effects",
        vec![
            Sexp::list("font", vec![Sexp::list("size", vec![num(1.27), num(1.27)])]),
            Sexp::list("hide", vec![Sexp::atom("yes")]),
        ],
    )
}

#[allow(clippy::too_many_lines)]
fn build_symbol_instance(
    component: &Component,
    placement: &ComponentPlacement,
    project: &Uuid,
) -> Option<Sexp> {
    let part = component.part.as_ref()?;
    let (x, y) = (
        snap_grid_127(placement.center_mm.0),
        snap_grid_127(placement.center_mm.1),
    );
    let comp_uuid = derive_entity_uuid(project, "symbol", &component.refdes);

    let part_id = part.id.as_str();
    // Prefer the registry's `kicad_symbol` reference (resolves to
    // KiCad's bundled global library — e.g. `Device:R`). Fall back
    // to our synthesized `synth:<id>` rectangle when the registry
    // entry hasn't been mapped to a stock symbol yet.
    let lib_id = part
        .kicad_symbol
        .clone()
        .unwrap_or_else(|| format!("{LIBRARY_NICKNAME}:{part_id}"));

    let pin_count = part.pins.len();
    let is_two_pin = pin_count == 2 && is_two_pin_symbol_kind(&part.kind);
    let (body_w, body_h) = if is_two_pin {
        let (w, h) = (TWOPIN_HALF_W * 2.0, 4.0);
        // Logically-vertical 2-pin parts (decoupling caps rotated so
        // VCC lands up, ESD diodes, LED current-limit resistors) are
        // drawn tall-and-thin, not wide-and-short — swap the extents
        // so `field_anchor` below measures the true on-sheet body.
        if matches!(placement.rotation, Rotation::Ninety | Rotation::TwoSeventy) {
            (h, w)
        } else {
            (w, h)
        }
    } else {
        let sides: Vec<PinSide> = part.pins.iter().map(classify_ic_pin).collect();
        let top_n = sides.iter().filter(|s| **s == PinSide::Top).count();
        let bottom_n = sides.iter().filter(|s| **s == PinSide::Bottom).count();
        let left_n = sides.iter().filter(|s| **s == PinSide::Left).count();
        let right_n = sides.iter().filter(|s| **s == PinSide::Right).count();

        let horiz_max = top_n.max(bottom_n).max(2);
        let vert_max = left_n.max(right_n).max(2);
        let w =
            ((horiz_max as f64) * PIN_PITCH + 2.0 * BODY_PIN_PADDING).max(BODY_HALF_WIDTH * 2.0);
        let h = ((vert_max as f64) * PIN_PITCH + 2.0 * BODY_PIN_PADDING).max(MIN_BODY_HEIGHT);
        (w, h)
    };

    let body_top = y - body_h / 2.0;
    let body_bottom = y + body_h / 2.0;
    let body_left = x - body_w / 2.0;
    let body_right = x + body_w / 2.0;
    let body_edges: BodyEdges = (body_left, body_right, body_top, body_bottom);
    let counts = side_pin_counts(part, placement.rotation);
    let (ref_side, value_side) = choose_field_sides(counts);
    let (ref_pos, ref_justify) =
        field_anchor(ref_side, pins_on(counts, ref_side), (x, y), body_edges);
    let (value_pos, value_justify) =
        field_anchor(value_side, pins_on(counts, value_side), (x, y), body_edges);
    let display_value_raw = component
        .value
        .as_deref()
        .or(part.mpn.as_deref())
        .unwrap_or(part_id);
    let display_value = if display_value_raw.starts_with("c_generic_") {
        "C"
    } else if display_value_raw.starts_with("r_generic_") {
        "R"
    } else if display_value_raw.starts_with("led_") {
        "LED"
    } else if display_value_raw.starts_with("esd_") {
        "ESD"
    } else {
        display_value_raw
    };

    let natural_offset_deg = natural_rotation_offset(part);
    let logical_deg = match placement.rotation {
        Rotation::Zero => 0.0,
        Rotation::Ninety => 90.0,
        Rotation::OneEighty => 180.0,
        Rotation::TwoSeventy => 270.0,
    };
    let angle = ((logical_deg + natural_offset_deg) as i32).rem_euclid(360) as f64;

    Some(Sexp::list(
        "symbol",
        vec![
            str_pair("lib_id", lib_id),
            Sexp::list("at", vec![num(x), num(y), num(angle)]),
            pair("unit", Sexp::atom("1")),
            pair("in_bom", Sexp::atom("yes")),
            pair("on_board", Sexp::atom("yes")),
            str_pair("uuid", comp_uuid.to_string()),
            Sexp::list(
                "property",
                vec![
                    Sexp::str("Reference"),
                    Sexp::str(&component.refdes),
                    Sexp::list("at", vec![num(ref_pos.0), num(ref_pos.1), num(0.0)]),
                    field_effects(ref_justify),
                ],
            ),
            Sexp::list(
                "property",
                vec![
                    Sexp::str("Value"),
                    Sexp::str(display_value),
                    Sexp::list("at", vec![num(value_pos.0), num(value_pos.1), num(0.0)]),
                    field_effects(value_justify),
                ],
            ),
            Sexp::list(
                "property",
                vec![
                    Sexp::str("Footprint"),
                    Sexp::str(part.kicad_footprint.as_deref().unwrap_or("")),
                    Sexp::list("at", vec![num(x), num(y), num(0.0)]),
                    hidden_field_effects(),
                ],
            ),
            Sexp::list(
                "property",
                vec![
                    Sexp::str("Datasheet"),
                    // Populated from the registry provenance so the
                    // datasheet link survives into the placed instance
                    // (the library symbol already carries it; the
                    // instance must not blank it back to ""). Hidden,
                    // as KiCad convention dictates. StackExchange
                    // #28251: "Annotate liberally — put the datasheet
                    // reference on the schematic."
                    Sexp::str(
                        part.provenance
                            .as_ref()
                            .and_then(|p| p.datasheet_url.as_deref())
                            .unwrap_or(""),
                    ),
                    Sexp::list("at", vec![num(x), num(y), num(0.0)]),
                    hidden_field_effects(),
                ],
            ),
            // Sourcing fields (hidden, KiCad convention). KiCad's BOM
            // tooling and the JLCPCB/DigiKey plugin ecosystem read
            // `MPN`/`LCSC` from the *schematic instance*, not from a
            // sidecar CSV — emitting them here makes the exported
            // project self-sufficient for sourcing workflows (the
            // bom.csv carries the same data for direct ordering).
            Sexp::list(
                "property",
                vec![
                    Sexp::str("MPN"),
                    Sexp::str(part.mpn.as_deref().unwrap_or("")),
                    Sexp::list("at", vec![num(x), num(y), num(0.0)]),
                    hidden_field_effects(),
                ],
            ),
            Sexp::list(
                "property",
                vec![
                    Sexp::str("LCSC"),
                    Sexp::str(part.lcsc_pn.as_deref().unwrap_or("")),
                    Sexp::list("at", vec![num(x), num(y), num(0.0)]),
                    hidden_field_effects(),
                ],
            ),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use synth_ir::lower;
    use synth_registry::load_dir;

    fn workspace_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .unwrap()
    }

    #[test]
    fn schematic_for_single_mcu_is_deterministic() {
        let registry = load_dir(&workspace_root().join("registry").join("parts")).unwrap();
        let src = std::fs::read_to_string(
            workspace_root()
                .join("fixtures")
                .join("ir")
                .join("single_mcu.synth"),
        )
        .unwrap();
        let parsed = synth_parser::parse(&src, "single_mcu.synth");
        let ast = parsed.ast.unwrap();
        let board = lower(&ast, &registry, "single_mcu.synth").board.unwrap();
        let project = crate::uuid_v5::project_namespace(&board.name);
        let a = build_schematic(&board, &project).to_string_pretty();
        let b = build_schematic(&board, &project).to_string_pretty();
        assert_eq!(a, b, "schematic generation must be deterministic");
    }

    #[test]
    fn wire_segment_uuid_is_order_and_direction_stable() {
        let project = crate::uuid_v5::project_namespace("t");
        let a = (7112, 6610);
        let b = (6731, 3810);
        let forward = wire_segment_uuid(&project, "SDA", a, b);
        let reversed = wire_segment_uuid(&project, "SDA", b, a);
        assert_eq!(
            forward, reversed,
            "canonical ordering must absorb direction"
        );
        let other_net = wire_segment_uuid(&project, "SCL", a, b);
        assert_ne!(forward, other_net, "net identity must key the uuid");
    }

    #[test]
    fn power_ref_allocator_never_repeats_a_designator() {
        let mut alloc = PowerRefAllocator::default();
        // 8500 distinct seeds projecting into the 9000-slot
        // `#PWR1000..#PWR9999` space: by pigeonhole hundreds of
        // collisions must occur, and the allocator must absorb every
        // one of them. (The space itself saturates at exactly 9000 —
        // iterating past capacity cannot terminate, so the loop stays
        // strictly below it.)
        let mut seen = std::collections::HashSet::new();
        for i in 0..8500_u128 {
            let uuid = derive_entity_uuid(
                &crate::uuid_v5::project_namespace("refs"),
                "probe",
                &i.to_string(),
            );
            let r = alloc.allocate("#PWR", &uuid);
            assert!(seen.insert(r.clone()), "duplicate designator {r}");
        }
        assert_eq!(seen.len(), 8500);
    }

    #[test]
    fn title_block_carries_board_name_without_date() {
        let (_board, sexp, _text) = build_reference("sensor_logger");
        let text = sexp.to_string_pretty();
        // Collapse every whitespace run (newlines + indentation) to a
        // single space so the pretty-printed, multi-line s-expression
        // can be matched against a flat single-line pattern below.
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains("(title_block (title \"sensor_logger\")"),
            "exported schematic must carry the board name in its title block"
        );
        assert!(
            flat.contains("(date \"\")"),
            "date must be present but empty — a real date would break \
             byte-identical re-export"
        );
    }

    #[test]
    fn title_block_rev_flows_from_board_revision() {
        let (mut board, _sexp, _text) = build_reference("sensor_logger");
        board.revision = Some("C".to_string());
        let sexp = build_schematic(&board, &crate::uuid_v5::project_namespace(&board.name));
        let text = sexp.to_string_pretty();
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains("(rev \"C\")"),
            "title block must carry the board revision when declared"
        );
    }

    #[test]
    fn title_block_carries_fab_target_comment() {
        let (board, _sexp, text) = build_reference("sensor_logger");
        let fab = board
            .manufacturer
            .clone()
            .expect("sensor_logger fixture declares a fab");
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains(&format!("(comment 3 \"Fab: {fab}\")")),
            "title block must document the fabrication target"
        );

        // No fab declared → no comment-3 clutter.
        let mut board = board;
        board.manufacturer = None;
        let sexp = build_schematic(&board, &crate::uuid_v5::project_namespace(&board.name));
        let flat: String = sexp
            .to_string_pretty()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !flat.contains("(comment 3"),
            "unset manufacturer must not emit an empty fab comment"
        );
    }

    #[test]
    fn instances_carry_hidden_sourcing_fields() {
        // sensor_logger resolves STM32F103C8T6 / AMS1117-3.3 from the
        // seed registry; their MPN/LCSC values must survive onto the
        // *placed instances* so KiCad BOM + JLCPCB/DigiKey plugin
        // workflows read a sourcing-complete project without the
        // sidecar bom.csv.
        let (board, _sexp, text) = build_reference("sensor_logger");
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains("(property \"MPN\" \"STM32F103C8T6\""),
            "U1 instance must carry the registry MPN"
        );
        assert!(
            flat.contains("(property \"LCSC\" \"C8734\""),
            "U1 instance must carry the registry LCSC number"
        );
        // Every component gets both fields (empty string when the
        // registry entry has no sourcing data) — nothing missing for
        // downstream BOM tooling, nothing hidden from the diff.
        let mpn_count = flat.matches("(property \"MPN\"").count();
        let lcsc_count = flat.matches("(property \"LCSC\"").count();
        assert_eq!(
            mpn_count,
            board.components.len(),
            "every component must carry an MPN field"
        );
        assert_eq!(
            lcsc_count,
            board.components.len(),
            "every component must carry an LCSC field"
        );
        // Hidden per KiCad convention: sourcing metadata must never
        // clutter the drawing. Each new property's effects carry
        // `(hide yes)` — verify by checking a known-valued one.
        let known = flat
            .split("(property")
            .find(|chunk| chunk.starts_with(" \"MPN\" \"STM32F103C8T6\""))
            .expect("U1 MPN property present");
        assert!(
            known.contains("(hide yes)"),
            "sourcing fields must be hidden on the sheet"
        );
    }

    // ------------------------------------------------------------------
    // Field autoplacement (Reference/Value label sides)
    // ------------------------------------------------------------------

    fn capacitor_part() -> synth_registry::Part {
        let registry = load_dir(&workspace_root().join("registry").join("parts")).unwrap();
        let src = std::fs::read_to_string(
            workspace_root()
                .join("fixtures")
                .join("kicad-reference")
                .join("sensor_logger.synth"),
        )
        .unwrap();
        let parsed = synth_parser::parse(&src, "sensor_logger.synth");
        let ast = parsed.ast.unwrap();
        let board = lower(&ast, &registry, "sensor_logger.synth").board.unwrap();
        board
            .components
            .iter()
            .find(|c| c.refdes == "C1")
            .and_then(|c| c.part.clone())
            .expect("sensor_logger fixture has a C1 capacitor with a resolved part")
    }

    #[test]
    fn horizontal_two_pin_part_keeps_fields_on_top_bottom() {
        let part = capacitor_part();
        let counts = side_pin_counts(&part, Rotation::Zero);
        assert_eq!(
            choose_field_sides(counts),
            (FieldSide::Top, FieldSide::Bottom),
            "an unrotated 2-pin part has its pins on Left/Right, so fields \
             should keep the long-standing Reference-above/Value-below spot"
        );
    }

    #[test]
    fn vertical_two_pin_part_moves_fields_off_the_pin_axis() {
        // Decoupling caps get rotated onto Rotation::Ninety/TwoSeventy so
        // VCC lands up and GND lands down (rotate_two_pin_with_power_flags).
        // Their pins now occupy Top/Bottom, so fields must move to
        // Left/Right or the Reference/Value text would sit on top of the
        // pin stub and its power-flag wire.
        let part = capacitor_part();
        for rotation in [Rotation::Ninety, Rotation::TwoSeventy] {
            let counts = side_pin_counts(&part, rotation);
            assert_eq!(
                choose_field_sides(counts),
                (FieldSide::Right, FieldSide::Left),
                "rotation {rotation:?} puts pins on Top/Bottom; fields must \
                 move to the pin-free Left/Right axis"
            );
        }
    }

    #[test]
    fn field_anchor_tightens_when_its_side_has_no_pins() {
        let body: BodyEdges = (5.0, 15.0, 5.0, 15.0);
        let (tight, _) = field_anchor(FieldSide::Top, 0, (10.0, 10.0), body);
        let (clear, _) = field_anchor(FieldSide::Top, 1, (10.0, 10.0), body);
        assert!(
            (5.0 - tight.1) < (5.0 - clear.1),
            "a pin-free side should anchor closer to the body edge (y=5.0) \
             than a side with a pin stub to clear: tight={tight:?} clear={clear:?}"
        );
    }

    #[test]
    fn left_right_fields_justify_away_from_the_body() {
        let body: BodyEdges = (5.0, 15.0, 5.0, 15.0);
        let (_, right_justify) = field_anchor(FieldSide::Right, 0, (10.0, 10.0), body);
        let (_, left_justify) = field_anchor(FieldSide::Left, 0, (10.0, 10.0), body);
        // Anchored at the body's right edge, "left"-justified text grows
        // further right (away from the body); mirrored on the left.
        assert_eq!(right_justify, Some("left"));
        assert_eq!(left_justify, Some("right"));
    }

    // ------------------------------------------------------------------
    // Quality assertions against the reference designs
    // ------------------------------------------------------------------

    fn build_reference(board_name: &str) -> (Board, Sexp, String) {
        let registry = load_dir(&workspace_root().join("registry").join("parts")).unwrap();
        let name = format!("{board_name}.synth");
        let src = std::fs::read_to_string(
            workspace_root()
                .join("fixtures")
                .join("kicad-reference")
                .join(&name),
        )
        .unwrap();
        let parsed = synth_parser::parse(&src, &name);
        let ast = parsed.ast.unwrap();
        let board = lower(&ast, &registry, &name).board.unwrap();
        let project = crate::uuid_v5::project_namespace(&board.name);
        let sexp = build_schematic(&board, &project);
        let text = sexp.to_string_pretty();
        (board, sexp, text)
    }

    fn coords_with_prefix(text: &str, prefix: &str) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        let mut pos = 0;
        while let Some(idx) = text[pos..].find(prefix) {
            let start = pos + idx + prefix.len();
            let rest = &text[start..];
            let mut tokens = rest.split_whitespace();
            if let (Some(a), Some(b)) = (tokens.next(), tokens.next()) {
                // Trim trailing ')' — pretty-printed coordinates end
                // with `)` (e.g. `45.72)`), which would fail f64 parse.
                if let (Ok(x), Ok(y)) = (
                    a.trim_end_matches(')').parse::<f64>(),
                    b.trim_end_matches(')').parse::<f64>(),
                ) {
                    out.push((x, y));
                }
            }
            pos = start;
        }
        out
    }

    fn is_grid_aligned(v: f64) -> bool {
        let rem = v.rem_euclid(1.27);
        rem < 1e-3 || (1.27 - rem).abs() < 1e-3
    }

    fn wire_spans(text: &str) -> Vec<f64> {
        let mut spans = Vec::new();
        for block in text.split("(wire").skip(1) {
            let coords = coords_with_prefix(block, "(xy ");
            if coords.len() >= 2 {
                let dx = (coords[0].0 - coords[1].0).abs();
                let dy = (coords[0].1 - coords[1].1).abs();
                spans.push(dx.max(dy));
            }
        }
        spans
    }

    fn has_duplicate_wire_segments(text: &str) -> bool {
        let mut seen = std::collections::HashSet::new();
        for block in text.split("(wire").skip(1) {
            let coords = coords_with_prefix(block, "(xy ");
            if coords.len() >= 2 {
                let a = coords[0];
                let b = coords[1];
                let seg = if a.0 < b.0 || (a.0 == b.0 && a.1 < b.1) {
                    (a, b)
                } else {
                    (b, a)
                };
                let key = format!(
                    "{:.2},{:.2}|{:.2},{:.2}",
                    seg.0 .0, seg.0 .1, seg.1 .0, seg.1 .1
                );
                if !seen.insert(key) {
                    return true;
                }
            }
        }
        false
    }

    fn parse_num_atom(sexp: &Sexp) -> Option<f64> {
        match sexp {
            Sexp::Atom(a) => a.parse().ok(),
            _ => None,
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn ref_ldo_sensor_grid_aligned() {
        let (_board, sexp, _text) = build_reference("ref_ldo_sensor");
        let mut coords = Vec::new();
        if let Sexp::List { head, children } = &sexp {
            assert_eq!(head, "kicad_sch");
            for child in children {
                if let Sexp::List {
                    head: h,
                    children: c,
                } = child
                {
                    match h.as_str() {
                        "symbol" => {
                            // Only check power symbols (in_bom no, on_board no).
                            let mut is_power = false;
                            for gc in c {
                                if let Sexp::List {
                                    head: gh,
                                    children: ggc,
                                } = gc
                                {
                                    if (gh == "in_bom" || gh == "on_board") && ggc.len() == 1 {
                                        if let Sexp::Atom(a) = &ggc[0] {
                                            if a == "no" {
                                                is_power = true;
                                            }
                                        }
                                    }
                                }
                            }
                            if is_power {
                                for gc in c {
                                    if let Sexp::List {
                                        head: gh,
                                        children: ggc,
                                    } = gc
                                    {
                                        if gh == "at" {
                                            if let (Some(x), Some(y)) = (ggc.first(), ggc.get(1)) {
                                                if let (Some(xv), Some(yv)) =
                                                    (parse_num_atom(x), parse_num_atom(y))
                                                {
                                                    if !is_grid_aligned(xv) || !is_grid_aligned(yv)
                                                    {
                                                        println!(
                                                            "UNALIGNED POWER_SYMBOL: xv={xv}, yv={yv}"
                                                        );
                                                    }
                                                    coords.push((xv, yv));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        "global_label" => {
                            for gc in c {
                                if let Sexp::List {
                                    head: gh,
                                    children: ggc,
                                } = gc
                                {
                                    if gh == "at" {
                                        if let (Some(x), Some(y)) = (ggc.first(), ggc.get(1)) {
                                            if let (Some(xv), Some(yv)) =
                                                (parse_num_atom(x), parse_num_atom(y))
                                            {
                                                if !is_grid_aligned(xv) || !is_grid_aligned(yv) {
                                                    println!(
                                                        "UNALIGNED GLOBAL_LABEL: xv={xv}, yv={yv}"
                                                    );
                                                }
                                                coords.push((xv, yv));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        "wire" => {
                            for gc in c {
                                if let Sexp::List {
                                    head: gh,
                                    children: ggc,
                                } = gc
                                {
                                    if gh == "pts" {
                                        for ggg in ggc {
                                            if let Sexp::List {
                                                head: ggh,
                                                children: gggc,
                                            } = ggg
                                            {
                                                if ggh == "xy" {
                                                    if let (Some(x), Some(y)) =
                                                        (gggc.first(), gggc.get(1))
                                                    {
                                                        if let (Some(xv), Some(yv)) =
                                                            (parse_num_atom(x), parse_num_atom(y))
                                                        {
                                                            if !is_grid_aligned(xv)
                                                                || !is_grid_aligned(yv)
                                                            {
                                                                println!(
                                                                    "UNALIGNED WIRE: xv={xv}, yv={yv}"
                                                                );
                                                            }
                                                            coords.push((xv, yv));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(!coords.is_empty(), "expected some coordinates in schematic");
        for (x, y) in coords {
            if !is_grid_aligned(x) || !is_grid_aligned(y) {
                println!("UNALIGNED COORD: x={x}, y={y}");
            }
            assert!(is_grid_aligned(x), "x={x} is not aligned to grid");
            assert!(is_grid_aligned(y), "y={y} is not aligned to grid");
        }
    }

    #[test]
    fn ref_ldo_sensor_no_long_wires() {
        let (_board, _sexp, text) = build_reference("ref_ldo_sensor");
        let spans = wire_spans(&text);
        for span in spans {
            assert!(
                span <= 10.0,
                "found wire segment spanning {span} mm — only short stubs expected"
            );
        }
    }

    #[test]
    fn ref_ldo_sensor_no_duplicate_wires() {
        let (_board, _sexp, text) = build_reference("ref_ldo_sensor");
        assert!(
            !has_duplicate_wire_segments(&text),
            "found duplicate wire segments"
        );
    }

    // ------------------------------------------------------------------
    // sensor_logger: power-rail naming, net labels, and wire quality
    // ------------------------------------------------------------------

    #[test]
    fn sensor_logger_uses_named_power_rails() {
        let (_board, _sexp, text) = build_reference("sensor_logger");
        // The regulator output rail must be +3V3 (power:+3V3 arrow when
        // KiCad stock libraries are installed, or synth:+3V3 fallback),
        // not a generic synth:VOUT rectangle.
        assert!(
            text.contains("power:+3V3") || text.contains("synth:+3V3"),
            "expected power:+3V3 or synth:+3V3 symbols for the regulator output rail"
        );
        assert!(
            !text.contains("synth:VOUT"),
            "synth:VOUT rectangle should not appear when +3V3 is available"
        );
    }

    #[test]
    fn sensor_logger_no_body_crossing_wires() {
        let (board, _sexp, text) = build_reference("sensor_logger");
        let spans = wire_spans(&text);
        // No wire segment longer than 20 mm — the sensor_logger is
        // a compact single-row design; long wires indicate a route
        // that crossed component bodies (the pre-fix NRST disaster
        // spanned 74 mm; stock KiCad symbol is ~15 mm, fallback is 17.8 mm).
        for span in &spans {
            assert!(
                *span <= 20.0,
                "wire segment spans {span} mm — suspected body crossing"
            );
        }
        // Sanity: the schematic must contain at least some signal
        // wires (the NRST reset network is a local wire, not just
        // labels).
        assert!(
            !spans.is_empty(),
            "expected at least some wires in the schematic"
        );
        let _ = board;
    }

    #[test]
    fn sensor_logger_i2c_nets_use_labels() {
        let (_board, _sexp, text) = build_reference("sensor_logger");
        // SDA and SCL are multi-drop (MCU + sensor + pull-up) and
        // must render as per-endpoint net labels, not long wires —
        // matching the hand-drawn reference design.
        assert!(text.contains(r#""SDA""#), "SDA net must use local labels");
        assert!(text.contains(r#""SCL""#), "SCL net must use local labels");
    }
}
