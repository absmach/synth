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
pub(crate) struct PowerRefAllocator {
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

use crate::field_clear;
use crate::pin_reconcile::fanned_leg_stub;
use crate::sexp::{num, pair, str_pair, Sexp};
use crate::symbol_lib::{self, LIBRARY_NICKNAME};
use crate::uuid_v5::derive_entity_uuid;

pub use synth_layout::route::pin_terminal_xy;
use synth_layout::route::{
    classify_ic_pin, is_two_pin_symbol_kind, natural_rotation_offset, snap_grid_127,
    synthesized_body, PIN_LENGTH, TWOPIN_HALF_W,
};

/// Build the entire `.kicad_sch` s-expression for `board`.
/// `project` is the per-project UUID namespace.
#[allow(clippy::too_many_lines)]
/// A wire `(stroke …)`, tinted with the net's class hue when it has
/// one (schematic-quality plan Phase B1).
///
/// `net_settings` in the `.kicad_pro` carries the same hue per class,
/// but KiCad matches those by net *name*, which it derives from the
/// drawing at load time — an unlabelled local net is auto-named
/// `Net-(U2-BOOT0)` and no pre-written assignment can reach it. The
/// explicit stroke reaches every net and is what the SVG plot shows.
fn wire_stroke(color: Option<[u8; 3]>) -> Sexp {
    let mut parts = vec![
        Sexp::list("width", vec![num(0.0)]),
        Sexp::list("type", vec![Sexp::atom("default")]),
    ];
    if let Some(rgb) = color {
        parts.push(rgb_color(rgb));
    }
    Sexp::list("stroke", parts)
}

/// `(color r g b a)` — KiCad wants the alpha as a float.
fn rgb_color(rgb: [u8; 3]) -> Sexp {
    Sexp::list(
        "color",
        vec![
            num(f64::from(rgb[0])),
            num(f64::from(rgb[1])),
            num(f64::from(rgb[2])),
            num(1.0),
        ],
    )
}

pub fn build_schematic(board: &Board, project: &Uuid) -> Sexp {
    build_schematic_from_layout(board, project, &synth_layout::layout(board))
}

/// Build the schematic honouring an optional schematic sidecar override file
/// (`<design>.schematic.layout.toml`, sheet millimetres). The sidecar moves components
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
    let root_uuid = derive_entity_uuid(project, "sheet", "root");
    let mut power_refs = PowerRefAllocator::default();
    build_sheet_schematic(
        board,
        project,
        layout,
        &SheetRender {
            name: None,
            uuid: root_uuid,
            project_name: &board.name,
            symbol_path: None,
            members: None,
            power_drivers: None,
            root_furniture: None,
        },
        &mut power_refs,
    )
}

/// Which sheet is being rendered, and the cross-sheet furniture it
/// needs. Single-sheet export renders the root with no furniture and
/// behaves exactly as before; the §P26 multi-sheet path renders one
/// sub-sheet per entry plus a root carrying sheet instances.
#[derive(Debug)]
pub struct SheetRender<'a> {
    /// Sub-sheet name, or `None` for the root sheet.
    pub name: Option<&'a str>,
    /// File uuid: the root uuid for the root sheet, the sheet uuid
    /// for sub-sheets (matched by the root's sheet instances and the
    /// `.kicad_pro` sheet list).
    pub uuid: Uuid,
    /// Project stem for `(instances (project …))` blocks.
    pub project_name: &'a str,
    /// Sheet instance path for symbols on this sheet
    /// (`/root-uuid/sheet-uuid`), or `None` on the root sheet (flat
    /// symbols carry no instances block, as before).
    pub symbol_path: Option<String>,
    /// Component membership for reconcile filtering, or `None` for
    /// the whole board (single-sheet path).
    pub members: Option<&'a HashSet<ComponentId>>,
    /// Precomputed power-flag drivers, or `None` to derive from this
    /// sheet's placements (single-sheet path). Multi-sheet callers
    /// precompute per sheet and dedup by net so one flag drives the
    /// merged hierarchical net project-wide.
    pub power_drivers: Option<&'a [crate::pin_reconcile::PowerDriver]>,
    /// Sheet instances plus inter-pin wires; `Some` only on the root
    /// sheet of a multi-sheet export.
    pub root_furniture: Option<&'a RootFurniture>,
}

/// Sheet instances, their pin stubs, and the same-named local
/// labels joining those pins to the root's cross-sheet endpoints.
/// Placed by the multi-sheet exporter below the root content.
#[derive(Debug, Clone, PartialEq)]
pub struct RootFurniture {
    pub instances: Vec<PlacedSheetInstance>,
    pub wires: Vec<RootWire>,
    pub junctions: Vec<(f64, f64)>,
    /// One `(net name, stub end)` per sheet pin: the local label that
    /// joins that pin to the root's same-named cross-sheet net.
    pub pin_labels: Vec<(String, (f64, f64))>,
}

/// One sub-sheet instance box on the root sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSheetInstance {
    pub name: String,
    pub file: String,
    pub uuid: Uuid,
    pub at_mm: (f64, f64),
    pub size_mm: (f64, f64),
    /// `(net name, position, uuid)` pins along the box bottom edge.
    pub pins: Vec<(String, (f64, f64), Uuid)>,
}

/// One 2-point root wire segment joining same-net sheet pins.
#[derive(Debug, Clone, PartialEq)]
pub struct RootWire {
    pub net: String,
    pub a_mm: (f64, f64),
    pub b_mm: (f64, f64),
}

#[allow(clippy::too_many_lines)]
pub(crate) fn build_sheet_schematic(
    board: &Board,
    project: &Uuid,
    layout: &synth_layout::Layout,
    render: &SheetRender<'_>,
    power_refs: &mut PowerRefAllocator,
) -> Sexp {
    let placements: HashMap<ComponentId, &ComponentPlacement> =
        layout.components.iter().map(|p| (p.id, p)).collect();
    // Single source of truth for which nets need a `power:PWR_FLAG`
    // driver — consumed by both the lib_symbols embedding decision
    // (below) and the instance emission loop (bottom of this
    // function). Keeping these on one computation guarantees the
    // embedded definition set exactly matches the emitted instances.
    let computed_drivers;
    let power_drivers: &[crate::pin_reconcile::PowerDriver] =
        if let Some(drivers) = render.power_drivers {
            drivers
        } else {
            computed_drivers = crate::pin_reconcile::undriven_power_nets(board, &placements);
            &computed_drivers
        };
    // Standard sizes are written by name. KiCad's own paper list runs A0-A5,
    // so A5 needs no special handling (an earlier comment here claimed
    // otherwise); only a measured page falls outside the named set, and that
    // is written as `(paper "User" W H)`, which KiCad honours exactly.
    let paper = match layout.sheet_size {
        synth_layout::SheetSize::A5 => Sexp::list("paper", vec![Sexp::str("A5")]),
        synth_layout::SheetSize::A4 => Sexp::list("paper", vec![Sexp::str("A4")]),
        synth_layout::SheetSize::A3 => Sexp::list("paper", vec![Sexp::str("A3")]),
        synth_layout::SheetSize::A2 => Sexp::list("paper", vec![Sexp::str("A2")]),
        synth_layout::SheetSize::A1 => Sexp::list("paper", vec![Sexp::str("A1")]),
        synth_layout::SheetSize::A0 => Sexp::list("paper", vec![Sexp::str("A0")]),
        synth_layout::SheetSize::Custom {
            width_mm,
            height_mm,
        } => Sexp::list(
            "paper",
            vec![Sexp::str("User"), num(width_mm), num(height_mm)],
        ),
    };

    // Fabrication target from the `manufacturer "…"` board statement.
    // Professional drawings document who builds the board; KiCad
    // renders this inside the title block as note 3. Omitted entirely
    // when no fab is declared (no empty comment clutter). The
    // design-authority "company" field below comes from the
    // `company "…"` board statement — the fab is not the design
    // company.
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
        str_pair("uuid", render.uuid.to_string()),
        paper,
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
                Sexp::list(
                    "title",
                    vec![Sexp::str(match render.name {
                        Some(sheet) => format!("{} — {sheet}", board.name),
                        None => board.name.clone(),
                    })],
                ),
                Sexp::list("date", vec![Sexp::str("")]),
                // Revision from the `revision "…"` board statement, or
                // blank when unset; the title block displays the
                // revision by convention.
                Sexp::list(
                    "rev",
                    vec![Sexp::str(board.revision.as_deref().unwrap_or(""))],
                ),
                Sexp::list(
                    "company",
                    vec![Sexp::str(board.company.as_deref().unwrap_or(""))],
                ),
                // Design notes: a schematic should carry the notes a
                // reader needs. KiCad renders `comment` entries inside
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
        embed_library(board, layout, power_drivers),
    ];

    // Declared buses (`bus "I2C0" (sda, scl)`) export as KiCad bus
    // aliases on every sheet that carries one of their members. A
    // member is carried by a per-pin label named `<bus>.<member>`;
    // KiCad resolves that label to the bus member by name, so no bus
    // graphic is drawn — the exporter is label-driven for nets too
    // (see `layout.net_labels`). The alias teaches KiCad's bus tools
    // the member set without adding a floating bus wire that ERC
    // would flag.
    {
        let mut bus_labels: Vec<&str> = board
            .nets
            .iter()
            .map(|n| n.name.as_str())
            .chain(layout.net_labels.iter().map(|l| l.label.as_str()))
            .chain(layout.hierarchical_labels.iter().map(|l| l.label.as_str()))
            .collect();
        bus_labels.sort_unstable();
        bus_labels.dedup();
        for bus in &board.buses {
            let used = bus_labels.iter().any(|label| {
                label
                    .strip_prefix(bus.name.as_str())
                    .and_then(|rest| rest.strip_prefix('.'))
                    .is_some_and(|member| bus.members.iter().any(|m| m == member))
            });
            if !used {
                continue;
            }
            children.push(Sexp::list(
                "bus_alias",
                vec![
                    Sexp::str(bus.name.clone()),
                    Sexp::list(
                        "members",
                        bus.members.iter().map(|m| Sexp::str(m.clone())).collect(),
                    ),
                ],
            ));
        }
    }

    // Symbol instances. On sub-sheets every symbol carries its
    // `(instances (project … (path …)))` block so KiCad maps the
    // reference to the sheet path; root symbols stay bare — *unless* a
    // design variant overrides the symbol, which also lives in that
    // block. A design with no variants keeps byte-identical output.
    let variant_dnp = variant_dnp_by_refdes(board);
    let rail_flag = |net_id: NetId| -> Option<&synth_layout::PowerFlag> {
        layout.power_flags.iter().find(|f| f.net == net_id)
    };
    // Multi-sheet: reconcile runs board-wide, but each sheet emits only
    // its own members' legs.
    let on_sheet = |component: ComponentId| {
        render
            .members
            .is_none_or(|members| members.contains(&component))
    };
    let leg_rail = |leg: &crate::pin_reconcile::PowerLeg| -> Option<&synth_layout::PowerFlag> {
        on_sheet(leg.component)
            .then(|| rail_flag(leg.net))
            .flatten()
    };
    let reconciled_pins = crate::pin_reconcile::reconcile_all(board);
    // The fan-out legs are drawn after the symbols, so their stubs are known
    // up front or a Reference, a Value or a power name could land on one.
    let mut obstacles = field_obstacles(board, layout, &placements);
    obstacles.extend(fanned_leg_obstacles(
        board,
        &placements,
        &reconciled_pins,
        leg_rail,
    ));
    let bounds = field_bounds(board, layout);
    let mut power_names = Vec::new();
    for component in &board.components {
        if let Some(placement) = placements.get(&component.id) {
            let overrides = variant_dnp.get(component.refdes.as_str());
            for s in build_symbol_instance(
                component,
                placement,
                project,
                board,
                &mut obstacles,
                bounds.get(&component.id).copied(),
            ) {
                let path = render
                    .symbol_path
                    .clone()
                    .or_else(|| overrides.map(|_| format!("/{}", render.uuid)));
                let Some(path) = path else {
                    children.push(s);
                    continue;
                };
                let mut path_children = vec![
                    Sexp::str(&path),
                    Sexp::list("reference", vec![Sexp::str(&component.refdes)]),
                    Sexp::list("unit", vec![Sexp::atom(symbol_unit_number(&s).to_string())]),
                ];
                if let Some(names) = overrides {
                    for name in names {
                        path_children.push(Sexp::list(
                            "variant",
                            vec![
                                Sexp::list("name", vec![Sexp::str(*name)]),
                                Sexp::list("dnp", vec![Sexp::atom("yes")]),
                            ],
                        ));
                    }
                }
                let instances = Sexp::list(
                    "instances",
                    vec![Sexp::list(
                        "project",
                        vec![
                            Sexp::str(render.project_name),
                            Sexp::list("path", path_children),
                        ],
                    )],
                );
                children.push(match s {
                    Sexp::List {
                        head,
                        children: mut sc,
                    } => {
                        sc.push(instances);
                        Sexp::List { head, children: sc }
                    }
                    other => other,
                });
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
    let net_colors = synth_layout::netclass::net_colors(board);
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
                    wire_stroke(net_colors.get(&wire.net).copied()),
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
    for flag in &layout.power_flags {
        if let Some(mut sexps) = build_power_symbol(
            board,
            flag,
            &placements,
            layout,
            project,
            &mut *power_refs,
            &mut obstacles,
            &mut power_names,
            bounds.get(&flag.component).copied(),
        ) {
            children.append(&mut sexps);
        }
    }

    // KiCad per-pin net labels at every labeled-net endpoint. Each
    // endpoint gets its own short stub + `(global_label)` flag with the
    // same name; same-named labels are electrically connected (the same
    // primitive the reference designs use for SCL/SDA/D+/D-). With several
    // sheets the label is a local `(label)`, so it cannot reach other
    // sheets. No long wires, no top rails. Unroutable nets were merged
    // into `layout.net_labels` by `synth_layout::layout`, so this loop
    // covers the routing-fallback labels too.
    let single_sheet = render.symbol_path.is_none();
    for label in &layout.net_labels {
        let color = net_colors.get(&label.net).copied();
        if let Some(sexps) =
            build_net_label(board, label, &placements, project, color, single_sheet)
        {
            children.extend(sexps);
        }
    }

    // A net the author named and the router drew only as wires has no flag to
    // carry its name; a local label on its longest segment keeps it, so KiCad
    // and the PCB use the same name instead of `Net-(...)`.
    let flagged = layout.labeled_net_ids();
    let power = layout.power_net_ids();
    let all_stubs: Vec<(synth_ir::NetId, Vec<(f64, f64)>)> = layout
        .net_labels
        .iter()
        .filter_map(|l| {
            synth_layout::route::net_label_plan(board, l.component, l.pin, &placements, l.spot)
                .map(|plan| (l.net, plan.wire))
        })
        .collect();
    for net in board
        .nets
        .iter()
        .filter(|n| !n.auto_named && !flagged.contains(&n.id) && !power.contains(&n.id))
    {
        let longest = layout
            .wires
            .iter()
            .filter(|w| w.net == net.id)
            .flat_map(|w| w.points.windows(2))
            .max_by(|a, b| segment_len(a).total_cmp(&segment_len(b)));
        if let Some(seg) = longest {
            let stubs: Vec<&[(f64, f64)]> = all_stubs
                .iter()
                .filter(|(label_net, _)| *label_net != net.id)
                .flat_map(|(_, wire)| wire.windows(2))
                .collect();
            let mut others: Vec<&[(f64, f64)]> = layout
                .wires
                .iter()
                .filter(|w| w.net != net.id)
                .flat_map(|w| w.points.windows(2))
                .collect();
            others.extend(stubs);
            let mut points: Vec<(f64, f64)> = layout.junctions.clone();
            points.extend(layout.wires.iter().flat_map(|w| w.points.iter().copied()));
            let at = wire_name_label_at(seg, &others, &points);
            let uuid = derive_entity_uuid(project, "wire_name_label", &net.name);
            children.push(build_local_label(&net.name, at, uuid));
        }
    }

    // Cross-sheet stubs (§P26): one stub per in-sheet endpoint of a
    // net that continues elsewhere. Empty on single-sheet layouts.
    // On a sub-sheet the stub carries a hierarchical label, which the
    // parent sheet instance's matching pin joins. On the *root* sheet
    // the same net is instead carried by a local label: root endpoints
    // (and the labelled stubs on each sheet pin, below) connect by
    // name within the root sheet, which is what links a root
    // endpoint to the sheet pins. Power nets never appear here (their
    // symbols are global).
    //
    // Emitting nothing on root was a bug: a cross-sheet signal net
    // with a root endpoint left that pin and the sheet pins dangling
    // (`pin_not_connected`).
    for label in &layout.hierarchical_labels {
        let doc = match render.name {
            Some(sheet) => build_hierarchical_label(board, label, &placements, project, sheet),
            None => build_root_cross_label(board, label, &placements, project),
        };
        if let Some(sexps) = doc {
            children.extend(sexps);
        }
    }

    // Sub-circuit captions: one text run per declared `group`, drawn
    // inside the group's box at the top-left. These are pure
    // annotation — KiCad treats `(text)` as a graphic with no
    // electrical meaning, so a caption can never join a net or trip
    // ERC. Captions render bold (Phase B4 typography hierarchy: bold
    // 2.0 mm caption, 1.27 mm refdes/value, 1.0 mm note/legend line);
    // every other kind renders plain. Per-text color is not part of
    // KiCad's grammar — `kicad-cli` rejects `(color …)` inside text
    // effects — so hue lives on the box and on net classes only.
    for (index, annotation) in layout.annotations.iter().enumerate() {
        let (x, y) = annotation.at_mm;
        let uuid = derive_entity_uuid(
            project,
            "annotation",
            &format!("{index}_{}", annotation.text),
        );
        let mut font = vec![Sexp::list(
            "size",
            vec![num(annotation.size_mm), num(annotation.size_mm)],
        )];
        if annotation.kind == synth_layout::TextKind::Caption {
            font.push(Sexp::atom("bold"));
        }
        children.push(Sexp::list(
            "text",
            vec![
                Sexp::str(&annotation.text),
                Sexp::list("at", vec![num(x), num(y), num(0.0)]),
                Sexp::list(
                    "effects",
                    vec![
                        Sexp::list("font", font),
                        Sexp::list("justify", vec![Sexp::atom("left"), Sexp::atom("bottom")]),
                    ],
                ),
                str_pair("uuid", uuid.to_string()),
            ],
        ));
    }

    // The outline of the region boxes (§21.1), framing each caption and its
    // parts. Same graphic status as captions — no electrical meaning, never
    // trips ERC. The boxes tile the page, so together they read as a single
    // outline cut into sections: one colour, no tint, and each boundary
    // drawn once as a `(polyline ...)` rather than once per box.
    let [r, g, b] = REGION_LINE_RGB;
    for (n, (from, to)) in region_outline(&layout.group_boxes, layout.sheet_size.dims_mm())
        .into_iter()
        .enumerate()
    {
        let uuid = derive_entity_uuid(project, "region_line", &n.to_string());
        children.push(Sexp::list(
            "polyline",
            vec![
                Sexp::list(
                    "pts",
                    vec![
                        Sexp::list("xy", vec![num(from.0), num(from.1)]),
                        Sexp::list("xy", vec![num(to.0), num(to.1)]),
                    ],
                ),
                Sexp::list(
                    "stroke",
                    vec![
                        Sexp::list("width", vec![num(0.254)]),
                        Sexp::list("type", vec![Sexp::atom("solid")]),
                        Sexp::list(
                            "color",
                            vec![
                                Sexp::atom(r.to_string()),
                                Sexp::atom(g.to_string()),
                                Sexp::atom(b.to_string()),
                                num(1.0),
                            ],
                        ),
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
    for reconciled in &reconciled_pins {
        for leg in &reconciled.power_legs {
            if let Some(rail) = leg_rail(leg) {
                if let Some(sexps) = build_fanned_power_symbol(
                    board,
                    leg,
                    rail,
                    &placements,
                    project,
                    &mut *power_refs,
                    &mut obstacles,
                    &mut power_names,
                    bounds.get(&leg.component).copied(),
                ) {
                    children.extend(sexps);
                }
            }
        }
        for pin in &reconciled.no_connects {
            if render
                .members
                .is_some_and(|members| !members.contains(&pin.component))
            {
                continue;
            }
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
    for driver in power_drivers {
        if let Some(sexp) = build_power_flag_driver(board, driver, project, &mut *power_refs) {
            children.push(sexp);
        }
    }

    // Root furniture (§P26 multi-sheet only): sub-sheet instance
    // boxes with one pin per cross-sheet net, a stub + same-named
    // local label on each pin (so it joins the root endpoints labelled
    // above), and their junction dots.
    if let Some(furniture) = render.root_furniture {
        for (index, instance) in furniture.instances.iter().enumerate() {
            children.push(build_sheet_instance(
                instance,
                render.project_name,
                &render.uuid,
                (index + 2) as u32,
            ));
        }
        for wire in &furniture.wires {
            children.push(build_root_wire(wire, project));
        }
        for (net, at) in &furniture.pin_labels {
            let key = format!("pin_label_{net}_{}_{}", at.0, at.1);
            children.push(build_local_label(
                net,
                *at,
                derive_entity_uuid(project, "pin_label", &key),
            ));
        }
        for (jx, jy) in &furniture.junctions {
            let key = format!("root_junction_{jx}_{jy}");
            children.push(Sexp::list(
                "junction",
                vec![
                    Sexp::list("at", vec![num(*jx), num(*jy)]),
                    Sexp::list("diameter", vec![num(0.0)]),
                    Sexp::list("color", vec![num(0.0), num(0.0), num(0.0), num(0.0)]),
                    str_pair(
                        "uuid",
                        derive_entity_uuid(project, "junction", &key).to_string(),
                    ),
                ],
            ));
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
    let (x, y, dx, dy) = if let Some(terminal) = driver.anchor_terminal {
        terminal
    } else {
        let lib_id = part.kicad_symbol.as_deref()?;
        let natural_offset = natural_rotation_offset(part);
        // The anchor pin for an undriven power net is a declared power-input
        // pin; those are placed un-rotated in the layout's canonical
        // orientation, so logical rotation is zero.
        let total_deg = natural_offset.rem_euclid(360.0);
        crate::pin_reconcile::physical_terminal(
            lib_id,
            &driver.anchor_pin_number,
            driver.anchor_center,
            total_deg,
        )?
    };

    let stub_len = POWER_FLAG_STUB_LEN;
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

/// The flag square and stub of every fanned power leg on the sheet, as
/// obstacles for text. `leg_rail` gives the rail flag a leg joins, and `None`
/// for a leg the emitter does not draw on this sheet, so the two agree.
fn fanned_leg_obstacles<'a>(
    board: &Board,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    reconciled: &[crate::pin_reconcile::ReconciledPins],
    leg_rail: impl Fn(&crate::pin_reconcile::PowerLeg) -> Option<&'a synth_layout::PowerFlag>,
) -> Vec<FieldObstacle> {
    let mut out = Vec::new();
    for leg in reconciled.iter().flat_map(|r| &r.power_legs) {
        let Some(rail) = leg_rail(leg) else {
            continue;
        };
        if let Some(((x, y), (flag_x, flag_y))) = fanned_leg_stub(board, leg, placements) {
            out.push(FieldObstacle::fixed(power_flag_rect(
                flag_x,
                flag_y,
                upright(rail.kind),
            )));
            out.push(FieldObstacle::fixed(wire_rect((x, y), (flag_x, flag_y))));
        }
    }
    out
}

/// Emit a power symbol + stub for a physical power leg that the
/// netlist doesn't reach but belongs on an existing rail net (fan-out
/// of the declared VDD/VSS legs). Mirrors [`build_power_symbol`] but
/// drives the geometry from the symbol's physical pin positions.
#[allow(clippy::too_many_arguments)]
fn build_fanned_power_symbol(
    board: &Board,
    leg: &crate::pin_reconcile::PowerLeg,
    rail: &synth_layout::PowerFlag,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    project: &Uuid,
    power_refs: &mut PowerRefAllocator,
    obstacles: &mut Vec<FieldObstacle>,
    power_names: &mut Vec<PowerName>,
    bounds: Option<field_clear::Rect>,
) -> Option<Vec<Sexp>> {
    let rail_label = rail.label.as_str();
    let component = board.component(leg.component)?;
    let ((x, y), (flag_x, flag_y)) = fanned_leg_stub(board, leg, placements)?;

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

    let properties = power_symbol_properties(
        &refdes_pwr,
        label,
        rail.kind,
        upright(rail.kind),
        (flag_x, flag_y),
        obstacles,
        power_names,
        bounds,
    );
    let sym_sexp = power_symbol_sexp(lib_id_out, flag_x, flag_y, 0.0, sym_uuid, properties);

    Some(vec![wire_sexp, sym_sexp])
}

/// Build the `(symbol ...)` S-expression for a power flag carrying the given
/// Reference and Value properties.
fn power_symbol_sexp(
    lib_id: String,
    flag_x: f64,
    flag_y: f64,
    angle: f64,
    sym_uuid: Uuid,
    properties: Vec<Sexp>,
) -> Sexp {
    let mut children = vec![
        str_pair("lib_id", lib_id),
        Sexp::list("at", vec![num(flag_x), num(flag_y), num(angle)]),
        pair("unit", Sexp::atom("1")),
        pair("in_bom", Sexp::atom("no")),
        pair("on_board", Sexp::atom("no")),
        str_pair("uuid", sym_uuid.to_string()),
    ];
    children.extend(properties);
    Sexp::list("symbol", children)
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
    let (x, y) = crate::pin_reconcile::no_connect_position(board, pin, placements)?;

    let key = format!("no_connect_{}_{}", component.refdes, pin.pin_number);
    let node_uuid = derive_entity_uuid(project, "no_connect", &key);
    Some(crate::pin_reconcile::no_connect_sexp(
        x,
        y,
        &node_uuid.to_string(),
    ))
}

/// Text height (mm) of a net label flag. KiCad draws the box twice the text
/// height plus a 0.15 mm line (about 2.7 mm at 1.27, 2.45 mm at 1.15, measured
/// in a kicad-cli render), so 1.15 fits the 2.54 mm pin pitch and flags on
/// neighbouring pins stay apart.
const FLAG_FONT_MM: f64 = 1.15;

/// Emit a KiCad net label attached to a stub extending from the pin's
/// coordinate. With `global` it is a `(global_label)` whose output shape
/// draws a boxed flag with its point away from the stub. A global label
/// joins same-named nets on every sheet, so a design with several sheets
/// passes `false` and gets a plain local `(label)` instead. Where the stub
/// runs is `synth_layout::route::net_label_plan`.
fn build_net_label(
    board: &Board,
    label: &synth_layout::NetLabel,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    project: &Uuid,
    color: Option<[u8; 3]>,
    global: bool,
) -> Option<Vec<Sexp>> {
    let component = board.component(label.component)?;
    let plan = synth_layout::route::net_label_plan(
        board,
        label.component,
        label.pin,
        placements,
        label.spot,
    )?;
    let (stub_x, elbow_y) = plan.anchor;
    let angle = if plan.faces_right { 0.0 } else { 180.0 };

    let key = format!("net_label_{}_{}", component.refdes, label.pin.0);
    let wire_uuid = derive_entity_uuid(project, "net_label_wire", &key);
    let label_uuid = derive_entity_uuid(project, "net_label", &key);

    let wire = |from: (f64, f64), to: (f64, f64), uuid: Uuid| {
        Sexp::list(
            "wire",
            vec![
                Sexp::list(
                    "pts",
                    vec![
                        Sexp::list("xy", vec![num(from.0), num(from.1)]),
                        Sexp::list("xy", vec![num(to.0), num(to.1)]),
                    ],
                ),
                wire_stroke(color),
                str_pair("uuid", uuid.to_string()),
            ],
        )
    };
    let rise_uuid = derive_entity_uuid(project, "net_label_rise", &key);
    let mut out = Vec::with_capacity(plan.wire.len());
    for seg in plan.wire.windows(2) {
        let uuid = if seg[0].0 == seg[1].0 {
            rise_uuid
        } else {
            wire_uuid
        };
        out.push(wire(seg[0], seg[1], uuid));
    }

    // The label text carries the same hue as its wire, so a name and
    // the line it names read as one object (reference sheet
    // mechanism 4).
    let size = if global { FLAG_FONT_MM } else { 1.27 };
    let mut font = vec![Sexp::list("size", vec![num(size), num(size)])];
    if let Some(rgb) = color {
        font.push(rgb_color(rgb));
    }
    let mut label_parts = vec![Sexp::str(&label.label)];
    if global {
        label_parts.push(Sexp::list("shape", vec![Sexp::atom("output")]));
    }
    label_parts.extend([
        Sexp::list("at", vec![num(stub_x), num(elbow_y), num(angle)]),
        Sexp::list(
            "effects",
            vec![
                Sexp::list("font", font),
                // KiCad centres a label's text on its anchor unless the
                // file says which way it runs, which drops half of it on
                // the stub and the pin. Run it away from the pin.
                Sexp::list(
                    "justify",
                    vec![Sexp::atom(if angle == 180.0 { "right" } else { "left" })],
                ),
            ],
        ),
        str_pair("uuid", label_uuid.to_string()),
    ]);
    out.push(Sexp::list(
        if global { "global_label" } else { "label" },
        label_parts,
    ));

    Some(out)
}

/// Cross-sheet stub for one in-sheet endpoint of a net that continues
/// on another sheet (§P26): a short stub wire plus a hierarchical
/// label carrying the shared net name. Positioned exactly like a
/// local net label (pin terminal plus stub); the parent sheet
/// instance carries the matching pin, and the root sheet wires
/// same-named pins together.
fn build_hierarchical_label(
    board: &Board,
    label: &synth_layout::HierarchicalLabel,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    project: &Uuid,
    sheet: &str,
) -> Option<Vec<Sexp>> {
    let component = board.component(label.component)?;
    let (x, y, dx, _dy) = pin_terminal_xy(board, label.component, label.pin, placements)?;
    let stub_len = 5.08;
    let (stub_x, angle) = if dx >= -0.1 {
        (x + stub_len, 0.0)
    } else {
        (x - stub_len, 180.0)
    };

    // Sheet-scoped uuid keys: the same endpoint renders once per
    // sheet, and each rendering needs its own identity.
    let key = format!("hier_label_{sheet}_{}_{}", component.refdes, label.pin.0);
    let wire_uuid = derive_entity_uuid(project, "hier_label_wire", &key);
    let label_uuid = derive_entity_uuid(project, "hier_label", &key);

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
        "hierarchical_label",
        vec![
            Sexp::str(&label.label),
            Sexp::list("shape", vec![Sexp::atom("bidirectional")]),
            Sexp::list("at", vec![num(stub_x), num(y), num(angle)]),
            Sexp::list(
                "effects",
                vec![
                    Sexp::list("font", vec![Sexp::list("size", vec![num(1.27), num(1.27)])]),
                    Sexp::list(
                        "justify",
                        vec![Sexp::atom(if angle == 0.0 { "left" } else { "right" })],
                    ),
                ],
            ),
            str_pair("uuid", label_uuid.to_string()),
        ],
    );

    Some(vec![wire_sexp, label_sexp])
}

/// One sub-sheet instance box on the root sheet: the box, its
/// sheetname/sheetfile properties, one pin per cross-sheet net, and
/// the sheet instance path block. Pins are bidirectional — every
/// cross-sheet signal net enters and leaves through matching
/// hierarchical labels. Shape follows dev-docs KiCad schematic
/// format (sheet section: stroke + fill are mandatory, pins carry a
/// type/effects/angle, instances map the project path to a page).
fn build_sheet_instance(
    instance: &PlacedSheetInstance,
    project_name: &str,
    root_uuid: &Uuid,
    page: u32,
) -> Sexp {
    let (x, y) = instance.at_mm;
    let (w, h) = instance.size_mm;
    let mut children = vec![
        Sexp::list("at", vec![num(x), num(y)]),
        Sexp::list("size", vec![num(w), num(h)]),
        Sexp::list(
            "stroke",
            vec![
                Sexp::list("width", vec![num(0.254)]),
                Sexp::list("type", vec![Sexp::atom("default")]),
            ],
        ),
        Sexp::list("fill", vec![Sexp::list("type", vec![Sexp::atom("none")])]),
        str_pair("uuid", instance.uuid.to_string()),
        Sexp::list(
            "property",
            vec![
                Sexp::str("Sheetname"),
                Sexp::str(&instance.name),
                Sexp::list("at", vec![num(x), num(y - 2.54), num(0.0)]),
                field_effects(Some("left")),
            ],
        ),
        Sexp::list(
            "property",
            vec![
                Sexp::str("Sheetfile"),
                Sexp::str(&instance.file),
                Sexp::list("at", vec![num(x), num(y - 2.54), num(0.0)]),
                hidden_field_effects(),
            ],
        ),
    ];
    for (net, (px, py), pin_uuid) in &instance.pins {
        children.push(Sexp::list(
            "pin",
            vec![
                Sexp::str(net),
                Sexp::atom("bidirectional"),
                Sexp::list("at", vec![num(*px), num(*py), num(270.0)]),
                Sexp::list(
                    "effects",
                    vec![Sexp::list(
                        "font",
                        vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                    )],
                ),
                Sexp::list("uuid", vec![Sexp::str(pin_uuid.to_string())]),
            ],
        ));
    }
    children.push(Sexp::list(
        "instances",
        vec![Sexp::list(
            "project",
            vec![
                Sexp::str(project_name),
                Sexp::list(
                    "path",
                    vec![
                        Sexp::str(format!("/{root_uuid}/{}", instance.uuid)),
                        Sexp::list("page", vec![Sexp::str(page.to_string())]),
                    ],
                ),
            ],
        )],
    ));
    Sexp::list("sheet", children)
}

fn segment_len(seg: &[(f64, f64)]) -> f64 {
    (seg[1].0 - seg[0].0).hypot(seg[1].1 - seg[0].1)
}

/// Where a net's name label sits on its wire segment `seg`: the midpoint, or
/// the nearest point to it that is clear of every wire in `others`. KiCad joins
/// a label to every wire under it, so a midpoint where another net crosses
/// would merge the two nets.
fn wire_name_label_at(
    seg: &[(f64, f64)],
    others: &[&[(f64, f64)]],
    points: &[(f64, f64)],
) -> (f64, f64) {
    const STEP_MM: f64 = 1.27;
    const CLEAR_MM: f64 = 0.5;
    let at = |t: f64| {
        (
            seg[0].0 + (seg[1].0 - seg[0].0) * t,
            seg[0].1 + (seg[1].1 - seg[0].1) * t,
        )
    };
    let clear = |p: (f64, f64)| {
        others
            .iter()
            .all(|other| point_to_segment_mm(p, other[0], other[1]) > CLEAR_MM)
            && points
                .iter()
                .all(|q| (p.0 - q.0).hypot(p.1 - q.1) > CLEAR_MM)
    };
    let len = segment_len(seg);
    let steps = if len > 0.0 {
        (len / STEP_MM) as i32 / 2
    } else {
        0
    };
    for step in 0..=steps {
        let shift = f64::from(step) * STEP_MM / len.max(f64::EPSILON);
        for t in [0.5 - shift, 0.5 + shift] {
            if (0.0..=1.0).contains(&t) && clear(at(t)) {
                return at(t);
            }
        }
    }
    at(0.5)
}

/// Distance in mm from `p` to the segment `a`-`b`.
fn point_to_segment_mm(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len_sq).clamp(0.0, 1.0)
    };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

/// A local label at `at_mm`, text anchored left. Same-named local
/// labels on one sheet are electrically connected, which is how the
/// root joins its cross-sheet endpoints to the labelled sheet-pin
/// stubs (and to each other).
fn build_local_label(net: &str, at_mm: (f64, f64), uuid: Uuid) -> Sexp {
    Sexp::list(
        "label",
        vec![
            Sexp::str(net),
            Sexp::list("at", vec![num(at_mm.0), num(at_mm.1), num(0.0)]),
            Sexp::list(
                "effects",
                vec![
                    Sexp::list("font", vec![Sexp::list("size", vec![num(1.27), num(1.27)])]),
                    Sexp::list("justify", vec![Sexp::atom("left")]),
                ],
            ),
            str_pair("uuid", uuid.to_string()),
        ],
    )
}

/// Root-side cross-sheet stub: a short stub wire plus a local label
/// carrying the shared net name. Root has no parent sheet pin to
/// attach a hierarchical label to, so it uses the local-label form
/// (same-named local labels are one net on a sheet).
fn build_root_cross_label(
    board: &Board,
    label: &synth_layout::HierarchicalLabel,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    project: &Uuid,
) -> Option<Vec<Sexp>> {
    let component = board.component(label.component)?;
    let (x, y, dx, _dy) = pin_terminal_xy(board, label.component, label.pin, placements)?;
    let stub_len = 5.08;
    let (stub_x, _angle) = if dx >= -0.1 {
        (x + stub_len, 0.0)
    } else {
        (x - stub_len, 180.0)
    };
    let key = format!("root_cross_{}_{}", component.refdes, label.pin.0);
    let wire_uuid = derive_entity_uuid(project, "root_cross_wire", &key);
    let label_uuid = derive_entity_uuid(project, "root_cross_label", &key);
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
    Some(vec![
        wire_sexp,
        build_local_label(&label.label, (stub_x, y), label_uuid),
    ])
}

/// One root wire segment joining same-net sheet pins.
fn build_root_wire(wire: &RootWire, project: &Uuid) -> Sexp {
    let key = format!(
        "root_wire_{}_{}_{}_{}",
        wire.a_mm.0, wire.a_mm.1, wire.b_mm.0, wire.b_mm.1
    );
    Sexp::list(
        "wire",
        vec![
            Sexp::list(
                "pts",
                vec![
                    Sexp::list("xy", vec![num(wire.a_mm.0), num(wire.a_mm.1)]),
                    Sexp::list("xy", vec![num(wire.b_mm.0), num(wire.b_mm.1)]),
                ],
            ),
            Sexp::list(
                "stroke",
                vec![
                    Sexp::list("width", vec![num(0.0)]),
                    Sexp::list("type", vec![Sexp::atom("default")]),
                ],
            ),
            str_pair(
                "uuid",
                derive_entity_uuid(project, "wire", &key).to_string(),
            ),
        ],
    )
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

/// One grid unit out from a pin terminal is where its power-flag
/// symbol sits.
const POWER_FLAG_STUB_LEN: f64 = 2.54;

/// The one colour every region box is stroked in.
const REGION_LINE_RGB: [u8; 3] = [49, 66, 150];

/// How far apart two box edges can be and still be one boundary: more than the
/// gap the layout leaves between neighbours.
const REGION_JOIN_MM: f64 = 0.3;

/// The lines that outline `boxes` on a sheet `sheet` mm across, each boundary
/// once. A box draws its right and bottom edges; it draws its left and top
/// edges only where no neighbour's edge already runs along them. An edge that
/// lies on the sheet frame, or on the top line of the title block, is left to
/// them.
fn region_outline(
    boxes: &[synth_layout::GroupBox],
    sheet: (f64, f64),
) -> Vec<((f64, f64), (f64, f64))> {
    let near = |a: f64, b: f64| (a - b).abs() < REGION_JOIN_MM;
    let frame = synth_layout::SHEET_BORDER_MM;
    let (block_x, block_y) = synth_layout::title_block_origin(sheet);
    let mut lines = Vec::new();
    for (i, b) in boxes.iter().enumerate() {
        let ((x0, y0), (x1, y1)) = (b.min_mm, b.max_mm);
        let others = || {
            boxes
                .iter()
                .enumerate()
                .filter(move |&(j, _)| j != i)
                .map(|(_, o)| o)
        };
        if !near(x1, sheet.0 - frame) {
            lines.push(((x1, y0), (x1, y1)));
        }
        if !near(y1, sheet.1 - frame) {
            // Over the title block's top line the block draws the rest.
            let end = if near(y1, block_y) {
                x1.min(block_x)
            } else {
                x1
            };
            if end - x0 > REGION_JOIN_MM {
                lines.push(((x0, y1), (end, y1)));
            }
        }
        if !near(x0, frame) {
            let met = others()
                .filter(|o| near(o.max_mm.0, x0))
                .map(|o| (o.min_mm.1, o.max_mm.1))
                .collect();
            lines.extend(
                uncovered((y0, y1), met)
                    .into_iter()
                    .map(|(from, to)| ((x0, from), (x0, to))),
            );
        }
        if !near(y0, frame) {
            let met = others()
                .filter(|o| near(o.max_mm.1, y0))
                .map(|o| (o.min_mm.0, o.max_mm.0))
                .collect();
            lines.extend(
                uncovered((x0, x1), met)
                    .into_iter()
                    .map(|(from, to)| ((from, y0), (to, y0))),
            );
        }
    }
    lines
}

/// The parts of `span` that none of the `met` spans cover.
fn uncovered(span: (f64, f64), mut met: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    met.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut at = span.0;
    for (from, to) in met {
        if from > at + REGION_JOIN_MM {
            out.push((at, from.min(span.1)));
        }
        at = at.max(to);
    }
    if at < span.1 - REGION_JOIN_MM {
        out.push((at, span.1));
    }
    out
}

/// Grid tolerance for "these two points are the same point" when
/// deciding whether a flag lands on a foreign pin.
const PIN_COINCIDENT_EPS: f64 = 0.05;

/// Whether the flag's connecting stub would collide with something
/// that is not part of its own net.
///
/// Two things count. A *foreign pin terminal*: drawing a symbol on a
/// pin joins that net, which is how a `+3V3` flag once welded an
/// `I2C_SDA` pin to the rail and took all eight of its pins with it.
/// And a *foreign wire*: a stub that runs along a signal wire stays
/// electrically separate (KiCad joins wires at endpoints, not where
/// they merely overlap) but reads as a short to anyone checking the
/// sheet, and lands on top of the other net's copper in any tool that
/// treats an overlap as a connection.
fn stub_is_blocked(
    board: &Board,
    flag: &synth_layout::PowerFlag,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    layout: &synth_layout::Layout,
    from: (f64, f64),
    to: (f64, f64),
) -> bool {
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        for idx in 0..part.pins.len() {
            let pin_id = synth_ir::PinId(idx as u32);
            // A pin on the flag's own net is a legal landing spot.
            if board.net(flag.net).is_some_and(|net| {
                net.endpoints
                    .iter()
                    .any(|ep| ep.component == component.id && ep.pin == pin_id)
            }) {
                continue;
            }
            if let Some((px, py, _, _)) = pin_terminal_xy(board, component.id, pin_id, placements) {
                if point_on_segment((px, py), from, to) {
                    return true;
                }
            }
        }
    }
    for wire in &layout.wires {
        if wire.net == flag.net {
            continue;
        }
        for pair in wire.points.windows(2) {
            if segments_touch(from, to, pair[0], pair[1]) {
                return true;
            }
        }
    }
    false
}

/// Whether `p` lies on the axis-aligned segment `a`..`b`, allowing a
/// grid of slack so a coincident-in-theory touch is still caught.
fn point_on_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> bool {
    const SLACK: f64 = PIN_COINCIDENT_EPS;
    let (lo_x, hi_x) = (a.0.min(b.0) - SLACK, a.0.max(b.0) + SLACK);
    let (lo_y, hi_y) = (a.1.min(b.1) - SLACK, a.1.max(b.1) + SLACK);
    p.0 >= lo_x && p.0 <= hi_x && p.1 >= lo_y && p.1 <= hi_y
}

/// Whether two axis-aligned segments touch or overlap.
fn segments_touch(a0: (f64, f64), a1: (f64, f64), b0: (f64, f64), b1: (f64, f64)) -> bool {
    point_on_segment(a0, b0, b1)
        || point_on_segment(a1, b0, b1)
        || point_on_segment(b0, a0, a1)
        || point_on_segment(b1, a0, a1)
}

/// A pin terminal plus the unit vector pointing outward along it, as
/// returned by [`pin_terminal_xy`]. Bundled so callers pass one
/// geometry value around rather than four loose coordinates.
#[derive(Clone, Copy)]
struct PinAnchor {
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
}

/// Pick a power-flag position whose stub cannot short a neighbour.
///
/// The natural spot is one stub length out from the anchor pin's
/// terminal. When two 2-pin parts sit close together and are rotated
/// so their pins point at each other, that stub can land exactly on
/// the *other* part's pin terminal.
///
/// Returns the flag position and, when the route needs a bend, the
/// corner it validated. Handing the corner back rather than letting
/// the emitter re-derive it keeps the drawn stub identical to the
/// route that was checked — the two drifting apart is how a clear
/// candidate turns into a stub that runs back through the obstacle.
///
/// Candidate order matters. Stepping *perpendicular* comes first: the
/// leg leaves the pin sideways, so it never sweeps along the axis
/// through whatever is sitting alongside. Reaching further *along* the
/// axis is the fallback, and it is the direction that produced the
/// 2.54 mm of collinear overlap with a neighbouring signal wire.
fn clear_power_flag_position(
    board: &Board,
    flag: &synth_layout::PowerFlag,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    layout: &synth_layout::Layout,
    anchor: PinAnchor,
) -> (f64, f64, Option<(f64, f64)>) {
    let (x, y, dx, dy) = (anchor.x, anchor.y, anchor.dx, anchor.dy);
    let stub_len = POWER_FLAG_STUB_LEN;
    let natural = (x + dx * stub_len, y + dy * stub_len);
    let natural_is_clear = !stub_is_blocked(board, flag, placements, layout, (x, y), natural);
    // A ground on a top edge pin would hang its triangle back over its own
    // stub, so run the stub up and across and let the triangle drop beside
    // the pin's column.
    if natural_is_clear && flag.kind == synth_layout::PowerFlagKind::Gnd && dx == 0.0 && dy < 0.0 {
        for step in [stub_len, -stub_len] {
            let candidate = (natural.0 + step, natural.1);
            let hangs = power_flag_rect(candidate.0, candidate.1, upright(flag.kind));
            if !stub_is_blocked(board, flag, placements, layout, natural, candidate)
                && !body_is_blocked(board, layout, placements, flag, &hangs)
            {
                return (candidate.0, candidate.1, Some(natural));
            }
        }
    }
    if natural_is_clear {
        return (natural.0, natural.1, None);
    }
    // Perpendicular from the pin, then along the axis. Bending the
    // other way — out to the natural spot first, then across — routes
    // the first leg straight through the obstacle, so the
    // perpendicular route would never be viable.
    let (perp_x, perp_y) = (-dy, dx);
    for step in [stub_len, 2.0 * stub_len, -stub_len, -2.0 * stub_len] {
        let candidate = (natural.0 + perp_x * step, natural.1 + perp_y * step);
        let corner = (x + perp_x * step, y + perp_y * step);
        if !stub_is_blocked(board, flag, placements, layout, (x, y), corner)
            && !stub_is_blocked(board, flag, placements, layout, corner, candidate)
        {
            return (candidate.0, candidate.1, Some(corner));
        }
    }
    // Fall back to reaching further out along the stub axis, straight.
    for reach in [2.0 * stub_len, 3.0 * stub_len, -stub_len, -2.0 * stub_len] {
        let candidate = (x + dx * reach, y + dy * reach);
        if !stub_is_blocked(board, flag, placements, layout, (x, y), candidate) {
            return (candidate.0, candidate.1, None);
        }
    }
    // Every route out is occupied, so drop the stub entirely and sit the
    // symbol on its own anchor pin. That point is by definition on the
    // flag's own net, so it cannot short anything — which makes it the
    // one position that is always safe. The symbol overlaps its own pin
    // stub, which is an aesthetic cost, not an electrical one.
    (x, y, None)
}

/// The points a power flag's stub runs through: the pin terminal, the corner
/// when the stub had to bend, and the flag.
fn power_stub_points(
    pin: (f64, f64),
    corner: Option<(f64, f64)>,
    flag: (f64, f64),
) -> Vec<(f64, f64)> {
    std::iter::once(pin)
        .chain(corner)
        .chain(std::iter::once(flag))
        .collect()
}

/// Emit a KiCad `(symbol)` entry for a power-flag symbol attached
/// just outside the pin terminal with a short connecting wire.
/// Returns `[wire, symbol]` so the stub is explicit and the flag
/// never overlaps the component body.
#[allow(clippy::too_many_arguments)]
fn build_power_symbol(
    board: &Board,
    flag: &synth_layout::PowerFlag,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    layout: &synth_layout::Layout,
    project: &Uuid,
    power_refs: &mut PowerRefAllocator,
    obstacles: &mut Vec<FieldObstacle>,
    power_names: &mut Vec<PowerName>,
    bounds: Option<field_clear::Rect>,
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
    let (flag_x, flag_y, stub_corner) =
        clear_power_flag_position(board, flag, placements, layout, PinAnchor { x, y, dx, dy });

    let lib_id = power_symbol_lib_id(&flag.label);
    let key = format!("power_{}_{}", component.refdes, flag.pin.0);
    let sym_uuid = derive_entity_uuid(project, "power_symbol", &key);
    let wire_uuid = derive_entity_uuid(project, "power_wire", &key);
    let refdes_pwr = power_refs.allocate("#PWR", &sym_uuid);

    // The connecting stub. Normally one straight segment from the pin
    // terminal to the flag; when the flag had to step off the stub
    // axis, bend at the pin so the run matches the route
    // `clear_power_flag_position` validated.
    let stub_points = power_stub_points((x, y), stub_corner, (flag_x, flag_y));

    let mut stub_wires = Vec::new();
    for (segment, pair) in stub_points.windows(2).enumerate() {
        let seg_uuid = if segment == 0 {
            wire_uuid
        } else {
            derive_entity_uuid(project, "power_wire_bend", &format!("{key}_{segment}"))
        };
        stub_wires.push(Sexp::list(
            "wire",
            vec![
                Sexp::list(
                    "pts",
                    vec![
                        Sexp::list("xy", vec![num(pair[0].0), num(pair[0].1)]),
                        Sexp::list("xy", vec![num(pair[1].0), num(pair[1].1)]),
                    ],
                ),
                Sexp::list(
                    "stroke",
                    vec![
                        Sexp::list("width", vec![num(0.0)]),
                        Sexp::list("type", vec![Sexp::atom("default")]),
                    ],
                ),
                str_pair("uuid", seg_uuid.to_string()),
            ],
        ));
    }

    let body = power_body(board, layout, placements, flag, (flag_x, flag_y), dx);
    let properties = power_symbol_properties(
        &refdes_pwr,
        &flag.label,
        flag.kind,
        body,
        (flag_x, flag_y),
        obstacles,
        power_names,
        bounds,
    );
    let angle = power_symbol_angle(flag.kind, body);
    let sym_sexp = power_symbol_sexp(lib_id, flag_x, flag_y, angle, sym_uuid, properties);

    let mut out = stub_wires;
    out.push(sym_sexp);
    Some(out)
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

/// How far a power symbol's name sits from its pin in mm: 1.27 mm past the
/// tip of the arrow or triangle, where KiCad's own power symbols put it.
const POWER_NAME_OFFSET_MM: f64 = 3.81;
/// How far from the symbol's axis a name that moved to a side starts, in mm.
const POWER_NAME_SIDE_MM: f64 = 1.9;
/// How far apart the distances are that a power name tries, nearest first, in
/// mm, when its usual spot is taken. See [`ring_distances`].
const POWER_NAME_RING_STEP_MM: f64 = 2.54;
/// How far the tip of the arrow or triangle is from the pin, in mm.
const POWER_TIP_MM: f64 = 2.54;
/// How far clear of other things a power name has to be, in mm. Under half
/// the 1.27 mm between the name and its own symbol's drawing, so the symbol
/// itself does not count as in the way.
const POWER_NAME_CLEARANCE_MM: f64 = 0.4;

/// A power symbol's position and the name it prints there.
type PowerName = ((f64, f64), String);

/// The Reference and Value properties of a power symbol at `at`. The
/// Reference (`#PWR12`) stays hidden. The Value is the net name, drawn past
/// the arrow tip or triangle, or at the level of the tip to either side when
/// that spot is taken. The name is added to `obstacles` for what is placed
/// afterwards.
/// Stacked pins give several symbols at one point; the first draws the name and
/// the rest keep theirs hidden, so the same text is not printed over itself.
#[allow(clippy::too_many_arguments)]
fn power_symbol_properties(
    refdes: &str,
    name: &str,
    kind: synth_layout::PowerFlagKind,
    body: Body,
    at: (f64, f64),
    obstacles: &mut Vec<FieldObstacle>,
    power_names: &mut Vec<PowerName>,
    bounds: Option<field_clear::Rect>,
) -> Vec<Sexp> {
    // A rail arrow points up the sheet, a ground triangle down.
    let away = upright(kind).1;
    let candidates: Vec<(f64, f64, Option<&'static str>)> =
        ring_distances(POWER_NAME_RING_STEP_MM, obstacle_reach(obstacles, at))
            .flat_map(|ring| {
                if body.0 == 0.0 {
                    vec![
                        (0.0, away * (POWER_NAME_OFFSET_MM + ring), None),
                        (POWER_NAME_SIDE_MM + ring, away * POWER_TIP_MM, Some("left")),
                        (
                            -(POWER_NAME_SIDE_MM + ring),
                            away * POWER_TIP_MM,
                            Some("right"),
                        ),
                    ]
                } else {
                    // A symbol turned along its stub keeps its name on its own
                    // row past the tip, or just above or below that row when
                    // the row is taken.
                    let justify = Some(if body.0 > 0.0 { "left" } else { "right" });
                    let x = body.0 * POWER_NAME_OFFSET_MM;
                    vec![
                        (body.0 * (POWER_NAME_OFFSET_MM + ring), 0.0, justify),
                        (x, -(POWER_NAME_SIDE_MM + ring), justify),
                        (x, POWER_NAME_SIDE_MM + ring, justify),
                    ]
                }
            })
            .collect();
    let angle = power_symbol_angle(kind, body);
    // The field is counter-rotated like a turned part's, so the name stays
    // horizontal.
    let field_angle = if angle == 0.0 { 0.0 } else { 90.0 };
    let repeats = power_names.iter().any(|(p, n)| *p == at && n == name);
    let chars = name.chars().count();
    let rect_at = |(dx, dy, justify): (f64, f64, Option<&'static str>)| {
        text_rect_at((at.0 + dx, at.1 + dy), justify, chars)
    };
    let outside: Vec<field_clear::Rect> = bounds
        .into_iter()
        .flat_map(field_clear::outside_strips)
        .collect();
    // A name next to a pin, wire or text it only just misses reads as part of
    // it, so a spot is clear when the margin around it is.
    let others: Vec<field_clear::Rect> = obstacles
        .iter()
        .map(|o| o.rect.grown(POWER_NAME_CLEARANCE_MM))
        .chain(outside)
        .collect();
    let (dx, dy, justify) = field_clear::pick_clear(&candidates, &rect_at, &others.iter());
    let pos = (at.0 + dx, at.1 + dy);
    if !repeats {
        power_names.push((at, name.to_string()));
        obstacles.push(FieldObstacle::fixed(text_rect_at(pos, justify, chars)));
    }
    let value_effects = if repeats {
        hidden_field_effects()
    } else {
        field_effects(written_justify(justify, angle, field_angle))
    };
    let property = |field: &str, text: &str, pos: (f64, f64), effects: Sexp| {
        Sexp::list(
            "property",
            vec![
                Sexp::str(field),
                Sexp::str(text),
                Sexp::list("at", vec![num(pos.0), num(pos.1), num(field_angle)]),
                effects,
            ],
        )
    };
    vec![
        property("Reference", refdes, at, hidden_field_effects()),
        property("Value", name, pos, value_effects),
    ]
}

/// Which variants mark each refdes do-not-populate, keyed by refdes and
/// listing the variant names in declaration order. Empty when the board
/// declares no variants.
fn variant_dnp_by_refdes(board: &Board) -> std::collections::BTreeMap<&str, Vec<&str>> {
    let mut out: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    for variant in &board.variants {
        for refdes in &variant.dnp {
            out.entry(refdes.as_str()).or_default().push(&variant.name);
        }
    }
    out
}

/// The `(unit N)` a placed symbol was emitted with, defaulting to 1.
fn symbol_unit_number(sym: &Sexp) -> u32 {
    if let Sexp::List { children, .. } = sym {
        for child in children {
            if let Sexp::List { head, children } = child {
                if head == "unit" {
                    if let Some(Sexp::Atom(a)) = children.first() {
                        return a.parse().unwrap_or(1);
                    }
                }
            }
        }
    }
    1
}

/// One placed symbol per *unit*. A single-unit part yields one symbol
/// (byte-identical to the pre-multi-unit output); a part whose stock
/// symbol declares several units (a dual op-amp, a quad gate) yields
/// one placed symbol per unit, stacked downward, each with its own
/// `(unit N)` — otherwise KiCad reports the unplaced units and their
/// pins never reach the netlist.
fn build_symbol_instance(
    component: &Component,
    placement: &ComponentPlacement,
    project: &Uuid,
    board: &Board,
    obstacles: &mut Vec<FieldObstacle>,
    bounds: Option<field_clear::Rect>,
) -> Vec<Sexp> {
    let Some(part) = component.part.as_ref() else {
        return Vec::new();
    };
    let units = part
        .kicad_symbol
        .as_deref()
        .and_then(synth_layout::kicad_lib_loader::symbol_units);
    let count = units.as_ref().map_or(1, |(_, count)| *count);
    let mut out = Vec::with_capacity(count as usize);
    for unit in 1..=count {
        let dy = if count > 1 {
            part.kicad_symbol.as_deref().map_or(0.0, |lib_id| {
                synth_layout::kicad_lib_loader::unit_offset_mm(lib_id, unit).1
            })
        } else {
            0.0
        };
        let uuid_key = if count > 1 {
            format!("{}_u{unit}", component.refdes)
        } else {
            component.refdes.clone()
        };
        out.push(build_symbol_unit(
            component,
            placement,
            project,
            board,
            &mut *obstacles,
            bounds,
            unit,
            dy,
            &uuid_key,
            units.as_ref().map(|(map, _)| map),
        ));
    }
    out
}

/// Something a symbol's Reference or Value text must stay off: a wire, another
/// symbol's body, a caption or text already placed. `owner` is the symbol a
/// body belongs to, so a symbol's own body is not an obstacle to its own text.
struct FieldObstacle {
    owner: Option<ComponentId>,
    rect: field_clear::Rect,
}

impl FieldObstacle {
    /// Something every text keeps off.
    fn fixed(rect: field_clear::Rect) -> Self {
        Self { owner: None, rect }
    }
}

/// The four sides of a symbol body a field can sit on.
const FIELD_SIDES: [FieldSide; 4] = [
    FieldSide::Top,
    FieldSide::Bottom,
    FieldSide::Right,
    FieldSide::Left,
];
/// How far apart the distances are that a field tries, nearest first, in mm,
/// when its usual spot is taken. The 5.08 is a ring spacing that happens to
/// equal the net label stub, not a reference to it. See [`ring_distances`].
const FIELD_RING_STEP_MM: f64 = 5.08;
/// How far above and below the body's centre the Reference and Value sit when
/// they stack beside a body, in mm.
const FIELD_STACK_MM: f64 = 1.27;

/// Height of a Reference or Value string in mm, from KiCad's plot of 1.27 mm text.
const FIELD_TEXT_HEIGHT_MM: f64 = 1.4;
/// Width of one Reference or Value character in mm, from the same plot.
const FIELD_CHAR_WIDTH_MM: f64 = 1.2;
/// Half the thickness given to a wire when it is an obstacle, in mm.
const WIRE_HALF_WIDTH_MM: f64 = 0.15;
/// Half the height of a pin line and its number, in mm.
const PIN_TEXT_HALF_MM: f64 = 1.3;
/// Half the width a power flag symbol covers, in mm.
const POWER_FLAG_HALF_MM: f64 = 1.4;
/// Width of one caption or note character, as a share of its font size.
const NOTE_CHAR_WIDTH: f64 = 0.95;

/// The rectangle `chars` characters of Reference or Value text cover when
/// anchored at `pos` with the justification `field_anchor` returned.
fn text_rect_at(pos: (f64, f64), justify: Option<&'static str>, chars: usize) -> field_clear::Rect {
    let width = chars as f64 * FIELD_CHAR_WIDTH_MM;
    let (x0, x1) = match justify {
        Some("left") => (pos.0, pos.0 + width),
        Some("right") => (pos.0 - width, pos.0),
        _ => (pos.0 - width / 2.0, pos.0 + width / 2.0),
    };
    field_clear::Rect {
        x0,
        y0: pos.1 - FIELD_TEXT_HEIGHT_MM / 2.0,
        x1,
        y1: pos.1 + FIELD_TEXT_HEIGHT_MM / 2.0,
    }
}

/// How far from `at` the farthest edge of any of `obstacles` lies, in mm.
fn obstacle_reach(obstacles: &[FieldObstacle], at: (f64, f64)) -> f64 {
    obstacles
        .iter()
        .flat_map(|o| {
            [
                (o.rect.x0 - at.0).abs(),
                (o.rect.x1 - at.0).abs(),
                (o.rect.y0 - at.1).abs(),
                (o.rect.y1 - at.1).abs(),
            ]
        })
        .fold(0.0, f64::max)
}

/// The distances, `step` mm apart from 0, a text may be pushed out by. They run
/// past the farthest obstacle, `reach` mm from the symbol, so a spot clear of
/// every obstacle known at that moment always exists. Nearest first, so text
/// only goes far when it is walled in. `reach` is not negative and `step` is
/// positive, so the count cannot be negative.
#[allow(clippy::cast_sign_loss)]
fn ring_distances(step: f64, reach: f64) -> impl Iterator<Item = f64> {
    let count = ((reach + FIELD_TEXT_HEIGHT_MM) / step).ceil() as usize;
    (0..=count).map(move |i| i as f64 * step)
}

/// Where a field may go, best first: the sides at each distance out to past
/// everything within `reach` mm, nearest ring first. `first` and `second` are
/// the sides the field would take by preference, then the other two follow.
fn field_candidates(first: FieldSide, second: FieldSide, reach: f64) -> Vec<(FieldSide, f64)> {
    let rest = FIELD_SIDES.iter().filter(|&&s| s != first && s != second);
    let order: Vec<FieldSide> = [first, second].into_iter().chain(rest.copied()).collect();
    ring_distances(FIELD_RING_STEP_MM, reach)
        .flat_map(|ring| order.iter().map(move |&side| (side, ring)))
        .collect()
}

/// `anchor` moved `ring` mm further out from the body on `side`.
fn push_out(anchor: (f64, f64), side: FieldSide, ring: f64) -> (f64, f64) {
    match side {
        FieldSide::Top => (anchor.0, anchor.1 - ring),
        FieldSide::Bottom => (anchor.0, anchor.1 + ring),
        FieldSide::Right => (anchor.0 + ring, anchor.1),
        FieldSide::Left => (anchor.0 - ring, anchor.1),
    }
}

/// The rectangle a wire segment covers.
fn wire_rect(a: (f64, f64), b: (f64, f64)) -> field_clear::Rect {
    field_clear::Rect {
        x0: a.0.min(b.0) - WIRE_HALF_WIDTH_MM,
        y0: a.1.min(b.1) - WIRE_HALF_WIDTH_MM,
        x1: a.0.max(b.0) + WIRE_HALF_WIDTH_MM,
        y1: a.1.max(b.1) + WIRE_HALF_WIDTH_MM,
    }
}

/// The way a power symbol's body points from its point, as a unit step on the
/// sheet (y grows downward).
type Body = (f64, f64);

/// How KiCad draws a power symbol unturned: a ground triangle points down, a
/// rail arrow up.
fn upright(kind: synth_layout::PowerFlagKind) -> Body {
    match kind {
        synth_layout::PowerFlagKind::Gnd => (0.0, 1.0),
        synth_layout::PowerFlagKind::Vcc => (0.0, -1.0),
    }
}

/// The angle to write for a symbol of `kind` so its body points along `body`.
/// KiCad turns counter-clockwise on the sheet, so a triangle that points down
/// ends up pointing right at 90 and left at 270, and an arrow that points up
/// the other way round.
fn power_symbol_angle(kind: synth_layout::PowerFlagKind, body: Body) -> f64 {
    if body.0 == 0.0 {
        return 0.0;
    }
    if (kind == synth_layout::PowerFlagKind::Gnd) == (body.0 > 0.0) {
        90.0
    } else {
        270.0
    }
}

/// What a power symbol at (`x`, `y`) draws: from its point to the tip of the
/// triangle or arrow, as wide as the symbol across the body.
fn power_flag_rect(x: f64, y: f64, body: Body) -> field_clear::Rect {
    let (tip_x, tip_y) = (x + body.0 * POWER_TIP_MM, y + body.1 * POWER_TIP_MM);
    let (half_x, half_y) = (
        POWER_FLAG_HALF_MM * body.1.abs(),
        POWER_FLAG_HALF_MM * body.0.abs(),
    );
    field_clear::Rect {
        x0: x.min(tip_x) - half_x,
        y0: y.min(tip_y) - half_y,
        x1: x.max(tip_x) + half_x,
        y1: y.max(tip_y) + half_y,
    }
}

/// How the symbol of `flag`, drawn at `at` on a stub that leaves its pin along
/// `dx`, points.
///
/// Unturned, a ground triangle drops one pin pitch below its stub, so beside a
/// column of pins it ends on the next pin's row: across that pin's wire, number
/// and flag, and its name finds no free spot nearer than the pin after that.
/// Turned to point along the stub it stays in its own pin's row. It is only
/// turned when the unturned body would land on a wire of another net or on a
/// net label (a power net never has one), so a sheet where nothing is in the
/// way keeps the usual symbol.
fn power_body(
    board: &Board,
    layout: &synth_layout::Layout,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    flag: &synth_layout::PowerFlag,
    at: (f64, f64),
    dx: f64,
) -> Body {
    let unturned = upright(flag.kind);
    if dx == 0.0 {
        return unturned;
    }
    let rect = power_flag_rect(at.0, at.1, unturned);
    if body_is_blocked(board, layout, placements, flag, &rect) {
        (dx.signum(), 0.0)
    } else {
        unturned
    }
}

/// Whether a symbol body drawn as `rect` would land on a wire or a label of
/// another net, or on the stub of another rail's flag. On a header the arrow
/// of one pin ends on the stub of the pin above it.
fn body_is_blocked(
    board: &Board,
    layout: &synth_layout::Layout,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    flag: &synth_layout::PowerFlag,
    rect: &field_clear::Rect,
) -> bool {
    let on_a_wire = layout
        .wires
        .iter()
        .filter(|wire| wire.net != flag.net)
        .flat_map(|wire| wire.points.windows(2))
        .any(|seg| rect.overlaps(&wire_rect(seg[0], seg[1])));
    let on_a_flag = label_rects(board, layout, placements)
        .iter()
        .any(|label| rect.overlaps(label));
    let on_a_stub = layout
        .power_flags
        .iter()
        .filter(|other| other.net != flag.net)
        .filter_map(|other| pin_terminal_xy(board, other.component, other.pin, placements))
        .any(|(x, y, sx, sy)| {
            let end = (x + sx * POWER_FLAG_STUB_LEN, y + sy * POWER_FLAG_STUB_LEN);
            rect.overlaps(&wire_rect((x, y), end))
        });
    on_a_wire || on_a_flag || on_a_stub
}

/// The box of the group each component belongs to, so its text can stay
/// inside it and out of the title band at its top.
fn field_bounds(
    board: &Board,
    layout: &synth_layout::Layout,
) -> HashMap<ComponentId, field_clear::Rect> {
    board
        .components
        .iter()
        .filter_map(|c| {
            let group = synth_layout::effective_group(board, c.id)?;
            let b = layout.group_boxes.iter().find(|b| b.group == group)?;
            let rect = field_clear::Rect {
                x0: b.min_mm.0,
                y0: b.min_mm.1 + synth_layout::GROUP_TITLE_BAND_MM,
                x1: b.max_mm.0,
                y1: b.max_mm.1,
            };
            Some((c.id, rect))
        })
        .collect()
}

/// What each net label covers: its text and the stub that joins it to the pin.
/// A cross-sheet label is drawn the same way.
fn label_rects(
    board: &Board,
    layout: &synth_layout::Layout,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> Vec<field_clear::Rect> {
    let labels = layout
        .net_labels
        .iter()
        .map(|l| (&l.label, l.component, l.pin, l.spot))
        .chain(layout.hierarchical_labels.iter().map(|l| {
            (
                &l.label,
                l.component,
                l.pin,
                synth_layout::LabelSpot::default(),
            )
        }));
    let mut out = Vec::new();
    for (text, component, pin, spot) in labels {
        let Some((x0, y0, x1, y1)) =
            synth_layout::route::net_label_rect(board, text, component, pin, placements, spot)
        else {
            continue;
        };
        let Some((pin_x, ..)) = pin_terminal_xy(board, component, pin, placements) else {
            continue;
        };
        out.push(field_clear::Rect {
            x0: x0.min(pin_x),
            y0,
            x1: x1.max(pin_x),
            y1,
        });
    }
    out
}

/// Every wire, body, pin, net label, power flag and caption on the sheet as an
/// obstacle for the Reference and Value text of the symbols.
fn field_obstacles(
    board: &Board,
    layout: &synth_layout::Layout,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> Vec<FieldObstacle> {
    let fixed = FieldObstacle::fixed;
    let mut out = Vec::new();
    for wire in &layout.wires {
        for seg in wire.points.windows(2) {
            out.push(fixed(wire_rect(seg[0], seg[1])));
        }
    }
    // Walk the board, not the placement map: `pick_clear` sums overlap areas
    // in obstacle order, and a hash map would shuffle that between runs.
    for component in &board.components {
        let (Some(part), Some(placement)) =
            (component.part.as_ref(), placements.get(&component.id))
        else {
            continue;
        };
        let (w, h) = symbol_body_size(part, placement.rotation);
        let (x, y) = (
            snap_grid_127(placement.center_mm.0),
            snap_grid_127(placement.center_mm.1),
        );
        out.push(FieldObstacle {
            owner: Some(component.id),
            ..FieldObstacle::fixed(field_clear::Rect {
                x0: x - w / 2.0,
                y0: y - h / 2.0,
                x1: x + w / 2.0,
                y1: y + h / 2.0,
            })
        });
    }
    out.extend(
        label_rects(board, layout, placements)
            .into_iter()
            .map(fixed),
    );
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        // A multi-pin symbol draws each pin's name inside the body, along the
        // pin; a passive draws none.
        let names = !(part.pins.len() == 2 && is_two_pin_symbol_kind(&part.kind));
        for (index, part_pin) in part.pins.iter().enumerate() {
            let pin = synth_ir::PinId(index as u32);
            let Some((x, y, dx, dy)) = pin_terminal_xy(board, component.id, pin, placements) else {
                continue;
            };
            // The pin line, its number and its name, from the terminal back
            // toward the body. The number is centred on the line, so one
            // longer than the line runs past the terminal.
            let name = if names {
                part_pin.name.chars().count() as f64 * FIELD_CHAR_WIDTH_MM
            } else {
                0.0
            };
            let over = ((part_pin.number.0.chars().count() as f64 * FIELD_CHAR_WIDTH_MM
                - PIN_LENGTH)
                / 2.0)
                .max(0.0);
            let (front, back) = (over, -(PIN_LENGTH + name));
            out.push(fixed(if dx.abs() >= dy.abs() {
                let (a, b) = (x + dx.signum() * front, x + dx.signum() * back);
                field_clear::Rect {
                    x0: a.min(b),
                    y0: y - PIN_TEXT_HALF_MM,
                    x1: a.max(b),
                    y1: y + PIN_TEXT_HALF_MM,
                }
            } else {
                let (a, b) = (y + dy.signum() * front, y + dy.signum() * back);
                field_clear::Rect {
                    x0: x - PIN_TEXT_HALF_MM,
                    y0: a.min(b),
                    x1: x + PIN_TEXT_HALF_MM,
                    y1: a.max(b),
                }
            }));
        }
    }
    // Each flag where it is drawn, which is off the pin's own stub when a
    // neighbour is in the way, together with its stub and any bend.
    for flag in &layout.power_flags {
        if let Some((x, y, dx, dy)) = pin_terminal_xy(board, flag.component, flag.pin, placements) {
            // The spot the symbol is drawn at, so the turn is decided where
            // `build_power_symbol` decides it.
            let (cx, cy, corner) = clear_power_flag_position(
                board,
                flag,
                placements,
                layout,
                PinAnchor { x, y, dx, dy },
            );
            let body = power_body(board, layout, placements, flag, (cx, cy), dx);
            out.push(fixed(power_flag_rect(cx, cy, body)));
            out.extend(
                power_stub_points((x, y), corner, (cx, cy))
                    .windows(2)
                    .map(|seg| fixed(wire_rect(seg[0], seg[1]))),
            );
        }
    }
    // A note is anchored left and bottom, so it rises from `at_mm`.
    for note in &layout.annotations {
        let width = note.text.chars().count() as f64 * note.size_mm * NOTE_CHAR_WIDTH;
        out.push(fixed(field_clear::Rect {
            x0: note.at_mm.0,
            y0: note.at_mm.1 - note.size_mm,
            x1: note.at_mm.0 + width,
            y1: note.at_mm.1 + 0.3 * note.size_mm,
        }));
    }
    out
}

/// The justification to write for a field so KiCad draws it as `wanted`.
/// KiCad draws a string upside down (symbol angle plus field angle of 180) the
/// right way up and swaps left and right justification on the way, so the
/// value has to be mirrored for those symbols.
fn written_justify(
    wanted: Option<&'static str>,
    symbol_angle: f64,
    field_angle: f64,
) -> Option<&'static str> {
    // Both angles are whole degrees by construction, so the compare is exact.
    if (symbol_angle + field_angle).rem_euclid(360.0) != 180.0 {
        return wanted;
    }
    match wanted {
        Some("left") => Some("right"),
        Some("right") => Some("left"),
        other => other,
    }
}

/// The on-sheet width and height of a symbol's body, in mm, as the exporter
/// measures it for text placement and the layouter places against.
fn symbol_body_size(part: &synth_registry::Part, rotation: Rotation) -> (f64, f64) {
    let pin_count = part.pins.len();
    let is_two_pin = pin_count == 2 && is_two_pin_symbol_kind(&part.kind);
    if is_two_pin {
        let (w, h) = (TWOPIN_HALF_W * 2.0, 4.0);
        // Logically-vertical 2-pin parts (decoupling caps rotated so
        // VCC lands up, ESD diodes, LED current-limit resistors) are
        // drawn tall-and-thin, not wide-and-short, so swap the extents
        // so `field_anchor` measures the true on-sheet body.
        if matches!(rotation, Rotation::Ninety | Rotation::TwoSeventy) {
            (h, w)
        } else {
            (w, h)
        }
    } else if part.kicad_symbol.is_some() {
        // A part mapped to a stock KiCad symbol is drawn with *that*
        // symbol's geometry, which the pin-count synthesis below does
        // not predict: a USB-C receptacle is ~50 mm tall where the
        // synthesis guessed ~30, so the Value field landed 8 mm inside
        // the body, on top of the pin names. `body_size_for_part` is
        // the same measurement the layouter places against, so fields
        // and placement now agree on where the body ends.
        let (w, h) = synth_layout::body_size_for_part(part);
        if matches!(rotation, Rotation::Ninety | Rotation::TwoSeventy) {
            (h, w)
        } else {
            (w, h)
        }
    } else {
        let sides: Vec<PinSide> = part.pins.iter().map(classify_ic_pin).collect();
        let body = synthesized_body(&sides);
        (body.width, body.height)
    }
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn build_symbol_unit(
    component: &Component,
    placement: &ComponentPlacement,
    project: &Uuid,
    board: &Board,
    obstacles: &mut Vec<FieldObstacle>,
    bounds: Option<field_clear::Rect>,
    unit: u32,
    dy: f64,
    uuid_key: &str,
    units: Option<&std::collections::BTreeMap<String, u32>>,
) -> Sexp {
    let part = component
        .part
        .as_ref()
        .expect("caller checked the part resolves");
    let (x, y) = (
        snap_grid_127(placement.center_mm.0),
        snap_grid_127(placement.center_mm.1) + dy,
    );
    let comp_uuid = derive_entity_uuid(project, "symbol", uuid_key);

    let part_id = part.id.as_str();
    // Prefer the registry's `kicad_symbol` reference (resolves to
    // KiCad's bundled global library — e.g. `Device:R`). Fall back
    // to our synthesized `synth:<id>` rectangle when the registry
    // entry hasn't been mapped to a stock symbol yet.
    let lib_id = part
        .kicad_symbol
        .clone()
        .unwrap_or_else(|| format!("{LIBRARY_NICKNAME}:{part_id}"));

    let (body_w, body_h) = symbol_body_size(part, placement.rotation);

    let body_top = y - body_h / 2.0;
    let body_bottom = y + body_h / 2.0;
    let body_left = x - body_w / 2.0;
    let body_right = x + body_w / 2.0;
    let body_edges: BodyEdges = (body_left, body_right, body_top, body_bottom);
    let counts = side_pin_counts(part, placement.rotation);
    let display_value_raw = component
        .value
        .as_deref()
        .or(part.mpn.as_deref())
        // Schematic-quality plan Phase A1: `value` → part `mpn` →
        // `(no value)` sentinel, never the registry part id. An
        // unorderable value must look unorderable (`E-SYNTH-VALUE-001`
        // fires for the generic case).
        .unwrap_or("(no value)");
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
    // KiCad reads an instance field's angle relative to the symbol's, so a
    // field written at 0 on a symbol turned 90/270 draws vertical text that
    // runs along the body (an LED's value into its emission arrows).
    // Counter-rotating keeps Reference/Value horizontal on every part.
    let field_angle = if angle == 90.0 || angle == 270.0 {
        90.0
    } else {
        0.0
    };

    // The Reference is pushed onto `obstacles` once placed, so the Value
    // cannot take its spot. The symbol's own body is skipped, since its text
    // sits against it.
    let (ref_pref, value_pref) = choose_field_sides(counts);
    let outside: Vec<field_clear::Rect> = bounds
        .into_iter()
        .flat_map(field_clear::outside_strips)
        .collect();
    // Beside a body the Reference and Value stack, Reference above, so the pair
    // is centred on the body; they do not each take a side.
    let mut place_field = |text: &str, first: FieldSide, second: FieldSide, stack: f64| {
        let candidates = field_candidates(first, second, obstacle_reach(obstacles, (x, y)));
        let spot = |(side, ring): (FieldSide, f64)| {
            let (pos, justify) = field_anchor(side, pins_on(counts, side), (x, y), body_edges);
            let pos = push_out(pos, side, ring);
            if matches!(side, FieldSide::Left | FieldSide::Right) {
                ((pos.0, pos.1 + stack), justify)
            } else {
                (pos, justify)
            }
        };
        let rect_at = |candidate| {
            let (pos, justify) = spot(candidate);
            text_rect_at(pos, justify, text.chars().count())
        };
        let others = obstacles
            .iter()
            .filter(|o| o.owner != Some(component.id))
            .map(|o| &o.rect)
            .chain(outside.iter());
        let chosen = field_clear::pick_clear(&candidates, &rect_at, &others);
        let (pos, justify) = spot(chosen);
        let rect = text_rect_at(pos, justify, text.chars().count());
        obstacles.push(FieldObstacle::fixed(rect));
        (pos, written_justify(justify, angle, field_angle), chosen.0)
    };
    let (ref_pos, ref_justify, ref_side) =
        place_field(&component.refdes, ref_pref, value_pref, -FIELD_STACK_MM);
    // The Value follows the Reference to the side of a body.
    let (value_first, value_second) = match ref_side {
        FieldSide::Right => (FieldSide::Right, FieldSide::Left),
        FieldSide::Left => (FieldSide::Left, FieldSide::Right),
        FieldSide::Top | FieldSide::Bottom => (value_pref, ref_pref),
    };
    let (value_pos, value_justify, _) =
        place_field(display_value, value_first, value_second, FIELD_STACK_MM);

    let mut fields = vec![
        str_pair("lib_id", lib_id),
        Sexp::list("at", vec![num(x), num(y), num(angle)]),
        pair("unit", Sexp::atom(unit.to_string())),
        pair("in_bom", Sexp::atom("yes")),
        pair("on_board", Sexp::atom("yes")),
        str_pair("uuid", comp_uuid.to_string()),
    ];
    // Do-not-populate: KiCad renders the symbol crossed out and
    // excludes it from BOM/PnP tooling. Emitted only when set so
    // populated parts keep byte-identical output.
    if component.dnp {
        fields.push(Sexp::list("dnp", vec![Sexp::atom("yes")]));
    }
    fields.push(Sexp::list(
        "property",
        vec![
            Sexp::str("Reference"),
            Sexp::str(&component.refdes),
            Sexp::list("at", vec![num(ref_pos.0), num(ref_pos.1), num(field_angle)]),
            field_effects(ref_justify),
        ],
    ));
    fields.push(Sexp::list(
        "property",
        vec![
            Sexp::str("Value"),
            Sexp::str(display_value),
            Sexp::list(
                "at",
                vec![num(value_pos.0), num(value_pos.1), num(field_angle)],
            ),
            field_effects(value_justify),
        ],
    ));
    fields.push(Sexp::list(
        "property",
        vec![
            Sexp::str("Footprint"),
            Sexp::str(part.kicad_footprint.as_deref().unwrap_or("")),
            Sexp::list("at", vec![num(x), num(y), num(0.0)]),
            hidden_field_effects(),
        ],
    ));
    fields.push(Sexp::list(
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
    ));
    // Sourcing fields (hidden, KiCad convention). KiCad's BOM
    // tooling and the JLCPCB/DigiKey plugin ecosystem read
    // `MPN`/`LCSC` from the *schematic instance*, not from a
    // sidecar CSV — emitting them here makes the exported
    // project self-sufficient for sourcing workflows (the
    // bom.csv carries the same data for direct ordering).
    fields.push(Sexp::list(
        "property",
        vec![
            Sexp::str("MPN"),
            Sexp::str(part.mpn.as_deref().unwrap_or("")),
            Sexp::list("at", vec![num(x), num(y), num(0.0)]),
            hidden_field_effects(),
        ],
    ));
    fields.push(Sexp::list(
        "property",
        vec![
            Sexp::str("LCSC"),
            Sexp::str(part.lcsc_pn.as_deref().unwrap_or("")),
            Sexp::list("at", vec![num(x), num(y), num(0.0)]),
            hidden_field_effects(),
        ],
    ));

    // Structured component data (tolerance, voltage/power rating,
    // dielectric) as hidden symbol fields, so KiCad BOM tooling and the
    // derating checks read them from the schematic. Same convention as
    // the MPN/LCSC fields above.
    // `lcsc_pn`/`mpn` are instance overrides already folded into the part
    // above, so they are not stamped a second time.
    for (name, value) in &component.properties {
        if matches!(name.as_str(), "lcsc_pn" | "mpn") {
            continue;
        }
        fields.push(Sexp::list(
            "property",
            vec![
                Sexp::str(name),
                Sexp::str(value),
                Sexp::list("at", vec![num(x), num(y), num(0.0)]),
                hidden_field_effects(),
            ],
        ));
    }

    // Pin functions: select the design's function name on each pin that
    // carries one, so the schematic reads `I2C1_SCL` rather than `PB6`.
    // The name is a subset of what `build_library` declared on the
    // symbol (both derive from `alternates::pin_function_alternates`),
    // so KiCad always finds the alternate.
    for (number, name) in crate::alternates::pin_function_alternates(board, component) {
        // Only this unit's pins are drawn on this symbol.
        if let Some(map) = units {
            if map.get(&number).copied().unwrap_or(1) != unit {
                continue;
            }
        }
        let pin_uuid = derive_entity_uuid(
            project,
            "symbol_pin",
            &format!("{}_{number}", component.refdes),
        );
        fields.push(Sexp::list(
            "pin",
            vec![
                Sexp::str(&number),
                Sexp::list("alternate", vec![Sexp::str(&name)]),
                str_pair("uuid", pin_uuid.to_string()),
            ],
        ));
    }

    Sexp::list("symbol", fields)
}

#[cfg(test)]
mod tests {
    /// A name label on the midpoint of a wire joins any other net's wire that
    /// crosses there, because KiCad connects a label to every wire under it.
    #[test]
    fn a_wire_name_label_keeps_off_a_wire_of_another_net() {
        let bus = [(242.57, 31.75), (242.57, 67.31)];
        let crossing = [(245.11, 49.53), (240.03, 49.53)];
        let others: [&[(f64, f64)]; 1] = [&crossing];
        let (x, y) = wire_name_label_at(&bus, &others, &[]);
        assert!((x - 242.57).abs() < 1e-9, "the label stays on the wire");
        assert!((31.75..=67.31).contains(&y), "the label stays on the wire");
        assert!(
            (y - 49.53).abs() > 0.5,
            "the label at y {y} touches the crossing wire"
        );
    }

    /// A branch of the *same* net meets the wire at a junction. KiCad joins a
    /// label to every wire under it, so the name must move off the junction.
    #[test]
    fn a_wire_name_label_keeps_off_a_junction_of_the_same_net() {
        let wire = [(0.0, 0.0), (25.4, 0.0)];
        let junction = [(12.7, 0.0)];
        let (x, y) = wire_name_label_at(&wire, &[], &junction);
        assert!((y - 0.0).abs() < 1e-9, "the label stays on the wire");
        assert!(
            (x - 12.7).abs() > 0.5,
            "the label at x {x} sits on the junction"
        );
    }

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

    fn lower_inline(src: &str) -> synth_ir::Board {
        let registry = load_dir(&workspace_root().join("registry").join("parts")).unwrap();
        let parsed = synth_parser::parse(src, "inline.synth");
        assert!(
            !parsed.has_errors(),
            "parse diagnostics: {:?}",
            parsed.diagnostics
        );
        let lowered = lower(&parsed.ast.unwrap(), &registry, "inline.synth");
        assert!(
            !lowered.has_errors(),
            "lower diagnostics: {:?}",
            lowered.diagnostics
        );
        lowered.board.unwrap()
    }

    /// Lower without insisting on a clean ERC (the alternates test uses
    /// a deliberately incomplete board).
    fn lower_lenient(src: &str) -> synth_ir::Board {
        let registry = load_dir(&workspace_root().join("registry").join("parts")).unwrap();
        let parsed = synth_parser::parse(src, "inline.synth");
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        lower(&parsed.ast.unwrap(), &registry, "inline.synth")
            .board
            .unwrap()
    }

    /// A power-flag stub must not short a neighbouring part, and must
    /// not run along a neighbouring wire.
    ///
    /// Two 2-pin parts placed close together and rotated so their pins
    /// face each other can be only one stub apart, so the flag's
    /// one-grid offset lands exactly on the neighbour's signal pin.
    /// KiCad treats a symbol sitting on a pin as joining that net, so
    /// the two merge silently and every pin of the signal net is
    /// dragged onto the rail — which is how a whole `I2C_SDA` bus once
    /// disappeared into `+3V3`. The stub must also stay clear of wires
    /// belonging to other nets: an overlap is electrically separate,
    /// but it reads as a short to anyone checking the sheet.
    #[test]
    fn power_flag_steps_clear_of_a_neighbouring_signal_pin() {
        use std::collections::HashMap;

        use synth_layout::{ComponentPlacement, PowerFlag, PowerFlagKind, Rotation};

        let board = lower_lenient(
            r#"board "b" {
                component R1: resistor "r_generic_0603" value "4.7k"
                component R2: resistor "r_generic_0603" value "4.7k"
                connect R1.p1 -> R2.p1 as "SIG"
                connect R1.p2 -> "GND"
                connect R2.p2 -> "GND"
            }"#,
        );

        // The rail R2 belongs to besides SIG, so its flag is the one at
        // risk. Resolved by net membership rather than a hard-coded pin
        // index, which depends on the registry's pin ordering.
        let r2_id = board.components[1].id;
        let rail = board
            .nets
            .iter()
            .find(|n| n.name != "SIG" && n.endpoints.iter().any(|e| e.component == r2_id))
            .expect("R2 sits on a rail as well as SIG");
        let rail_pin = rail
            .endpoints
            .iter()
            .find(|e| e.component == r2_id)
            .expect("R2 is an endpoint of the rail")
            .pin;
        let flag = PowerFlag {
            net: rail.id,
            component: r2_id,
            pin: rail_pin,
            kind: PowerFlagKind::Gnd,
            label: "GND".to_string(),
        };

        // R2 sits at a fixed spot; R1 is then positioned so its SIG pin
        // lands exactly on the naive flag position. Deriving R1's
        // placement rather than hard-coding it keeps the fixture valid
        // if the symbol's pin geometry ever changes.
        let r2 = ComponentPlacement {
            id: r2_id,
            center_mm: (127.0, 63.5),
            rotation: Rotation::TwoSeventy,
        };
        let probe = ComponentPlacement {
            id: board.components[0].id,
            center_mm: (0.0, 0.0),
            rotation: Rotation::TwoSeventy,
        };
        let probe_map: HashMap<_, &ComponentPlacement> =
            [(probe.id, &probe), (r2.id, &r2)].into_iter().collect();

        let (px, py, dx, dy) =
            pin_terminal_xy(&board, flag.component, flag.pin, &probe_map).unwrap();
        let naive = (px + dx * POWER_FLAG_STUB_LEN, py + dy * POWER_FLAG_STUB_LEN);

        // R1's SIG pin, read at the origin, gives the offset to cancel.
        let sig = board
            .nets
            .iter()
            .find(|n| n.name == "SIG")
            .expect("SIG is declared");
        let sig_pin = sig
            .endpoints
            .iter()
            .find(|e| e.component == probe.id)
            .expect("R1 is an endpoint of SIG")
            .pin;
        let (ox, oy, _, _) = pin_terminal_xy(&board, probe.id, sig_pin, &probe_map).unwrap();
        let r1 = ComponentPlacement {
            id: probe.id,
            center_mm: (naive.0 - ox, naive.1 - oy),
            rotation: Rotation::TwoSeventy,
        };
        let placements: HashMap<_, &ComponentPlacement> =
            [(r1.id, &r1), (r2.id, &r2)].into_iter().collect();

        // Only the hand-built placements matter here, so drop whatever
        // wires the router produced.
        let mut layout = synth_layout::layout(&board);
        layout.wires.clear();

        // The naive one-stub offset now sits on R1's signal pin. This
        // assertion matters: without it the test would pass vacuously
        // if the geometry ever stopped reproducing the collision.
        assert!(
            stub_is_blocked(&board, &flag, &placements, &layout, (px, py), naive),
            "fixture no longer reproduces the collision"
        );

        let (cx, cy, corner) = clear_power_flag_position(
            &board,
            &flag,
            &placements,
            &layout,
            PinAnchor {
                x: px,
                y: py,
                dx,
                dy,
            },
        );
        let cleared = (cx, cy);
        assert_ne!(cleared, naive, "flag must move off the foreign pin");
        // Check the legs the guard actually validated. Measuring the
        // single segment pin->flag would be wrong for a bent route: its
        // bounding box covers a rectangle the wire never enters, so it
        // reports hits that do not exist.
        let leg_clear = match corner {
            Some(corner) => {
                !stub_is_blocked(&board, &flag, &placements, &layout, (px, py), corner)
                    && !stub_is_blocked(&board, &flag, &placements, &layout, corner, cleared)
            }
            None => !stub_is_blocked(&board, &flag, &placements, &layout, (px, py), cleared),
        };
        assert!(leg_clear, "flag stub must not touch a foreign pin");

        // Text is kept off the flag where it really is drawn, not where it
        // would sit with no neighbour in the way.
        let kind = flag.kind;
        layout.power_flags = vec![flag];
        let obstacles = field_obstacles(&board, &layout, &placements);
        assert!(
            obstacles
                .iter()
                .any(|o| o.rect == power_flag_rect(cx, cy, upright(kind))),
            "no obstacle on the moved flag at {cleared:?}"
        );
    }

    /// A flag steps *perpendicular* off the stub axis rather than
    /// reaching further *along* it.
    ///
    /// This is the shape the I2C pull-ups hit: R5 and R6 sit one stub
    /// apart with their pins facing each other, so R6's rail flag
    /// cannot sit on its natural spot (R5's signal pin is there) and
    /// every further reach along that axis runs along R5's signal
    /// wire. Reaching along is electrically separate — KiCad joins
    /// wires at endpoints, not where they merely overlap — but it left
    /// 2.54 mm of collinear overlap that reads as a short to anyone
    /// checking the sheet, and lands on top of the other net's copper
    /// in any tool that treats an overlap as a connection.
    #[test]
    fn power_flag_steps_off_axis_rather_than_along_a_foreign_wire() {
        use std::collections::HashMap;

        use synth_layout::{ComponentPlacement, PowerFlag, PowerFlagKind, Rotation, WirePath};

        let board = lower_lenient(
            r#"board "b" {
                component R1: resistor "r_generic_0603" value "4.7k"
                component R2: resistor "r_generic_0603" value "4.7k"
                connect R1.p1 -> R2.p1 as "SIG"
                connect R1.p2 -> "GND"
                connect R2.p2 -> "GND"
            }"#,
        );
        let r2_id = board.components[1].id;
        let rail = board
            .nets
            .iter()
            .find(|n| n.name != "SIG" && n.endpoints.iter().any(|e| e.component == r2_id))
            .expect("R2 sits on a rail as well as SIG");
        let rail_pin = rail
            .endpoints
            .iter()
            .find(|e| e.component == r2_id)
            .expect("R2 is an endpoint of the rail")
            .pin;
        let flag = PowerFlag {
            net: rail.id,
            component: r2_id,
            pin: rail_pin,
            kind: PowerFlagKind::Gnd,
            label: "GND".to_string(),
        };
        let sig_net = board
            .nets
            .iter()
            .find(|n| n.name == "SIG")
            .expect("SIG is declared")
            .id;

        // R1's signal pin is placed exactly on the naive flag spot, the
        // way the two pull-ups sit one stub apart in the real design.
        let r2 = ComponentPlacement {
            id: r2_id,
            center_mm: (127.0, 63.5),
            rotation: Rotation::TwoSeventy,
        };
        let probe = ComponentPlacement {
            id: board.components[0].id,
            center_mm: (0.0, 0.0),
            rotation: Rotation::TwoSeventy,
        };
        let probe_map: HashMap<_, &ComponentPlacement> =
            [(probe.id, &probe), (r2.id, &r2)].into_iter().collect();
        let (px, py, dx, dy) =
            pin_terminal_xy(&board, flag.component, flag.pin, &probe_map).unwrap();
        let stub = POWER_FLAG_STUB_LEN;
        let natural = (px + dx * stub, py + dy * stub);
        let sig_pin = board
            .nets
            .iter()
            .find(|n| n.name == "SIG")
            .unwrap()
            .endpoints
            .iter()
            .find(|e| e.component == probe.id)
            .expect("R1 is an endpoint of SIG")
            .pin;
        let (ox, oy, _, _) = pin_terminal_xy(&board, probe.id, sig_pin, &probe_map).unwrap();
        let r1 = ComponentPlacement {
            id: probe.id,
            center_mm: (natural.0 - ox, natural.1 - oy),
            rotation: Rotation::TwoSeventy,
        };
        let placements: HashMap<_, &ComponentPlacement> =
            [(r1.id, &r1), (r2.id, &r2)].into_iter().collect();

        // A SIG wire along the stub axis, covering the reaches beyond
        // the natural spot.
        let mut layout = synth_layout::layout(&board);
        layout.wires = vec![WirePath {
            net: sig_net,
            points: vec![
                (px + dx * stub * 2.0, py + dy * stub * 2.0),
                (px + dx * stub * 4.0, py + dy * stub * 4.0),
            ],
            junctions: vec![],
        }];

        // The natural spot is taken by R1's pin, and every reach along
        // the axis crosses the wire…
        assert!(
            stub_is_blocked(&board, &flag, &placements, &layout, (px, py), natural),
            "natural spot should be blocked by the neighbour pin"
        );
        assert!(
            stub_is_blocked(
                &board,
                &flag,
                &placements,
                &layout,
                (px, py),
                (px + dx * stub * 2.0, py + dy * stub * 2.0)
            ),
            "along-axis reach should be blocked by the foreign wire"
        );

        // …so the guard must step perpendicular.
        let (cx, cy, corner) = clear_power_flag_position(
            &board,
            &flag,
            &placements,
            &layout,
            PinAnchor {
                x: px,
                y: py,
                dx,
                dy,
            },
        );
        let cleared = (cx, cy);
        // The route must bend off the axis rather than run straight down
        // it, and must not be one of the along-axis reaches.
        let corner = corner.expect("a blocked natural spot must bend off the axis");
        let along_reach = |m: f64| (px + dx * m, py + dy * m);
        assert!(
            ![1.0, 2.0, 3.0, -1.0, -2.0]
                .iter()
                .any(|m| cleared == along_reach(*m * stub)),
            "flag must not sit on an along-axis reach, got {cleared:?}"
        );
        assert!(
            !stub_is_blocked(&board, &flag, &placements, &layout, (px, py), corner)
                && !stub_is_blocked(&board, &flag, &placements, &layout, corner, cleared),
            "flag stub must not touch the foreign wire"
        );
    }

    #[test]
    fn pin_function_alternates_are_declared_and_selected() {
        let board = lower_lenient(
            r#"board "b" {
                component U1: mcu "rp2350"
                component U2: sensor "bmp280_pressure"
                connect U1.gp1 -> "I2C1_SCL"
                connect U2.scl -> "I2C1_SCL"
                connect U1.gp0 -> "I2C1_SDA"
                connect U2.sda -> "I2C1_SDA"
            }"#,
        );
        let project = crate::uuid_v5::project_namespace(&board.name);
        let text = build_schematic(&board, &project).to_string_pretty();
        // The library symbol declares the design's function names as
        // alternates (injected into the embedded stock symbol)…
        assert!(
            text.contains("(alternate \"I2C1_SCL\" "),
            "library must declare the SCL alternate"
        );
        assert!(
            text.contains("(alternate \"I2C1_SDA\" "),
            "library must declare the SDA alternate"
        );
        // …and each placed instance selects it, so the pin reads the
        // function rather than the package name.
        assert!(
            text.contains("(alternate \"I2C1_SCL\")"),
            "instance must select the SCL alternate"
        );
        assert!(
            text.contains("(alternate \"I2C1_SDA\")"),
            "instance must select the SDA alternate"
        );
    }

    #[test]
    fn multi_unit_part_exports_one_symbol_per_unit() {
        // The LM358 is a 3-unit symbol (two amplifiers + a power unit).
        // Emitting a single `(unit 1)` would leave units 2 and 3
        // unplaced — KiCad reports `missing_unit` and their pins never
        // reach the netlist.
        let board = lower_lenient(
            r#"board "b" {
                component U1: opamp "lm358_dual"
                component R1: resistor "r_generic_0603"
                component R2: resistor "r_generic_0603"
                component C1: capacitor "c_generic_0603"
                component C2: capacitor "c_generic_0603"
                connect U1.out_a -> U1.in_a_neg
                connect U1.in_a_pos -> R1.p1
                connect R1.p2 -> U1.vcc_pos
                connect U1.out_b -> U1.in_b_neg
                connect U1.in_b_pos -> R2.p1
                connect R2.p2 -> U1.vcc_pos
                connect U1.vcc_pos -> "VCC"
                connect C1.p1 -> "VCC"
                connect C2.p1 -> "VCC"
                connect U1.vcc_neg -> "GND"
                connect C1.p2 -> "GND"
                connect C2.p2 -> "GND"
            }"#,
        );
        let project = crate::uuid_v5::project_namespace(&board.name);
        let text = build_schematic(&board, &project).to_string_pretty();
        assert!(text.contains("(unit 1)"));
        assert!(
            text.contains("(unit 2)"),
            "unit 2 must be placed, not left missing — if the bundled KiCad \
             symbol library is absent (no /usr/share/kicad/symbols and no \
             KICAD_SYMBOL_DIR), the LM358 resolves to a single-unit symbol \
             and this assertion cannot hold. Install `kicad-libraries`."
        );
        assert!(
            text.contains("(unit 3)"),
            "the power unit must be placed too"
        );
    }

    #[test]
    fn variant_and_structured_values_are_exported() {
        let board = lower_lenient(
            r#"board "b" {
                component U1: mcu "rp2350"
                component U2: sensor "bmp280_pressure"
                component C1: capacitor "c_generic_0603" value "100nF" dielectric "X7R" voltage "25V"
                connect U1.gp0 -> U2.sda
                connect U1.gp1 -> U2.scl
                variant "lite" description "no sensor" { dnp U2 }
            }"#,
        );
        let project = crate::uuid_v5::project_namespace(&board.name);
        let text = build_schematic(&board, &project).to_string_pretty();
        // The variant override lives in the symbol's instances block.
        assert!(text.contains("(name \"lite\")"), "variant name");
        assert!(text.contains("(dnp yes)"), "variant dnp override");
        // Structured values are hidden symbol fields.
        assert!(text.contains("\"Dielectric\""), "dielectric field");
        assert!(text.contains("\"Voltage\""), "voltage field");
    }

    #[test]
    fn design_without_variants_has_no_variant_blocks() {
        let board = lower_lenient(
            r#"board "b" {
                component R1: resistor "r_generic_0603"
                component R2: resistor "r_generic_0603"
                connect R1.p1 -> R2.p1
            }"#,
        );
        let project = crate::uuid_v5::project_namespace(&board.name);
        let text = build_schematic(&board, &project).to_string_pretty();
        assert!(!text.contains("(variant"), "no variant block expected");
    }

    #[test]
    fn dnp_component_exports_dnp_flag_only() {
        let board = lower_inline(
            r#"board "b" {
                component R1: resistor "r_generic_0603"
                component R2: resistor "r_generic_0603" dnp
                connect R1.p1 -> R2.p1
                connect R1.p2 -> R2.p2
            }"#,
        );
        let project = crate::uuid_v5::project_namespace(&board.name);
        let text = build_schematic(&board, &project).to_string_pretty();
        assert_eq!(
            text.matches("(dnp yes)").count(),
            1,
            "exactly the DNP part carries the flag"
        );
    }

    /// Schematic-quality plan B2: the styled group rectangle (dashed
    /// stroke, `(color …)`, `(fill (type color) …)`) and the bold
    /// caption must both load in `kicad-cli` — the shape grammar is
    /// sensitive (a bare `(fill none)` fails). Skips when KiCad is not
    /// installed.
    #[test]
    fn styled_group_box_and_bold_caption_load_in_kicad() {
        let board = lower_inline(
            r#"board "b" {
                component R1: resistor "r_generic_0603" value "10k"
                group "Input" {
                    component C1: capacitor "c_generic_0603" value "100nF"
                    notes "Input notes" {
                        "Keep leads short."
                    }
                }
                connect R1.p1 -> C1.p1
                connect R1.p2 -> C1.p2
            }"#,
        );
        let project = crate::uuid_v5::project_namespace(&board.name);
        let text = build_schematic(&board, &project).to_string_pretty();
        // Sanity: the styled forms are present before we ask KiCad.
        assert!(text.contains("(type solid)"), "solid stroke");
        assert!(text.contains("(polyline"), "outline line");
        assert!(text.contains("bold"), "bold caption");
        assert!(text.contains("Keep leads short."), "in-box note line");

        let dir = std::env::temp_dir().join(format!("synth-b2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("b.kicad_sch");
        std::fs::write(&path, &text).unwrap();
        let erc = crate::run_kicad_erc(&path);
        match erc.evidence.reason {
            Some(synth_diagnostics::UnknownReason::NotInstalled) => {
                eprintln!("kicad-cli not installed; skipping styled-box load test");
            }
            Some(_) => panic!(
                "styled group box must load in kicad-cli: {}",
                erc.evidence.summary_line()
            ),
            None => {}
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn group_box_exports_outline() {
        let board = lower_inline(
            r#"board "b" {
                component R1: resistor "r_generic_0603"
                group "Input" {
                    component C1: capacitor "c_generic_0603"
                }
                connect R1.p1 -> C1.p1
                connect R1.p2 -> C1.p2
            }"#,
        );
        let project = crate::uuid_v5::project_namespace(&board.name);
        let text = build_schematic(&board, &project).to_string_pretty();
        assert!(text.contains("(polyline"), "group outline must render");
        assert!(text.contains("\"Input\""), "group caption must render");
        // Thin solid stroke in the one region colour.
        assert!(
            text.contains("(type solid)"),
            "outline stroke must be solid"
        );
        // Phase B4: the caption renders bold.
        assert!(
            text.contains("bold"),
            "group caption must render bold: {text}"
        );
    }

    #[test]
    fn notes_block_exports_text() {
        let board = lower_inline(
            r#"board "b" {
                component R1: resistor "r_generic_0603"
                component R2: resistor "r_generic_0603"
                connect R1.p1 -> R2.p1
                connect R1.p2 -> R2.p2
                notes "Build" {
                    "Assemble at JLCPCB."
                }
            }"#,
        );
        let project = crate::uuid_v5::project_namespace(&board.name);
        let text = build_schematic(&board, &project).to_string_pretty();
        assert!(text.contains("\"Build\""), "notes title must render");
        assert!(
            text.contains("\"Assemble at JLCPCB.\""),
            "notes line must render"
        );
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
    fn title_block_company_flows_from_board_company() {
        let (mut board, _sexp, _text) = build_reference("sensor_logger");
        board.company = Some("Absmach".to_string());
        let sexp = build_schematic(&board, &crate::uuid_v5::project_namespace(&board.name));
        let text = sexp.to_string_pretty();
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains("(company \"Absmach\")"),
            "title block must carry the design-authority company when declared"
        );
    }

    #[test]
    fn title_block_company_blank_when_undeclared() {
        let (board, _sexp, text) = build_reference("sensor_logger");
        assert!(
            board.company.is_none(),
            "sensor_logger fixture declares no company"
        );
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains("(company \"\")"),
            "title block company must be blank (not omitted) when undeclared"
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

    #[test]
    fn justify_is_mirrored_only_where_kicad_draws_the_text_upside_down() {
        // Measured on the plot of absmach/synth#103: a symbol at 180 degrees
        // and a symbol at 90 with the field turned to 90 draw "left" text
        // right-justified; 0 and 270 (field 90) do not.
        for (symbol, field, flipped) in [
            (0.0, 0.0, false),
            (90.0, 90.0, true),
            (180.0, 0.0, true),
            (270.0, 90.0, false),
        ] {
            assert_eq!(
                written_justify(Some("left"), symbol, field),
                Some(if flipped { "right" } else { "left" }),
                "symbol {symbol} field {field}"
            );
            assert_eq!(
                written_justify(Some("right"), symbol, field),
                Some(if flipped { "left" } else { "right" }),
                "symbol {symbol} field {field}"
            );
            assert_eq!(written_justify(None, symbol, field), None);
        }
    }

    /// Where each Reference and Value is drawn for `board` laid out as `layout`.
    fn field_boxes_for(
        board: &Board,
        layout: &synth_layout::Layout,
    ) -> Vec<(String, f64, f64, f64, f64)> {
        let project = crate::uuid_v5::project_namespace(&board.name);
        drawn_field_boxes(&build_schematic_from_layout(board, &project, layout))
    }

    fn boxes_overlap(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
        a.0 < b.2 && a.2 > b.0 && a.1 < b.3 && a.3 > b.1
    }

    fn box_of(boxes: &[(String, f64, f64, f64, f64)], text: &str) -> (f64, f64, f64, f64) {
        let (_, x0, y0, x1, y1) = boxes
            .iter()
            .find(|b| b.0 == text)
            .expect("the text is drawn");
        (*x0, *y0, *x1, *y1)
    }

    #[test]
    fn a_field_moves_off_a_wire_that_crosses_its_usual_spot() {
        let (board, ..) = build_reference("ref_ldo_sensor");
        let mut layout = synth_layout::layout(&board);
        let refdes = board.components[0].refdes.as_str();
        let usual = box_of(&field_boxes_for(&board, &layout), refdes);
        let x = f64::midpoint(usual.0, usual.2);
        layout.wires.push(synth_layout::WirePath {
            net: board.nets[0].id,
            points: vec![(x, usual.1 - 3.0), (x, usual.3 + 3.0)],
            junctions: Vec::new(),
        });
        let moved = box_of(&field_boxes_for(&board, &layout), refdes);
        let wire = (x - 0.15, usual.1 - 3.0, x + 0.15, usual.3 + 3.0);
        assert!(
            !boxes_overlap(moved, wire),
            "{refdes} at {moved:?} on the wire"
        );
        assert_ne!(moved, usual, "{refdes} did not move");
    }

    #[test]
    fn a_field_moves_inside_its_group_box_when_its_usual_spot_is_outside() {
        let (mut board, ..) = build_reference("ref_ldo_sensor");
        let mut layout = synth_layout::layout(&board);
        let refdes = board.components[0].refdes.clone();
        let usual = box_of(&field_boxes_for(&board, &layout), &refdes);
        let (cx, cy) = layout.components[0].center_mm;
        // A box that takes in the whole sheet except the side of the body
        // where the field usually sits.
        let far = 1000.0;
        let bounds = if usual.0 > cx {
            (usual.2 + 0.1, -far, far, far)
        } else if usual.2 < cx {
            (-far, -far, usual.0 - 0.1, far)
        } else if usual.1 > cy {
            (-far, usual.3 + 0.1, far, far)
        } else {
            (-far, -far, far, usual.1 - 0.1)
        };
        board.components[0].group = Some("G".to_string());
        layout.group_boxes = vec![synth_layout::GroupBox {
            group: "G".to_string(),
            min_mm: (bounds.0, bounds.1),
            max_mm: (bounds.2, bounds.3),
            color: [0, 0, 0],
            caption_inside: true,
        }];
        let moved = box_of(&field_boxes_for(&board, &layout), &refdes);
        assert!(
            moved.0 >= bounds.0
                && moved.1 >= bounds.1
                && moved.2 <= bounds.2
                && moved.3 <= bounds.3,
            "{refdes} at {moved:?} is outside {bounds:?}"
        );
    }

    #[test]
    fn a_field_moves_off_a_net_label_that_covers_its_usual_spot() {
        const LABEL: &str = "A_VERY_LONG_NET_LABEL_TO_REACH_ACROSS_THE_SHEET";
        let (board, ..) = build_reference("ref_ldo_sensor");
        let layout = synth_layout::layout(&board);
        let placements: HashMap<ComponentId, &ComponentPlacement> =
            layout.components.iter().map(|p| (p.id, p)).collect();
        let baseline = field_boxes_for(&board, &layout);
        // A label on some pin that lands on a field in the baseline.
        let mut hit = None;
        'search: for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            for index in 0..part.pins.len() {
                let pin = synth_ir::PinId(index as u32);
                let Some(rect) = synth_layout::route::net_label_rect(
                    &board,
                    LABEL,
                    component.id,
                    pin,
                    &placements,
                    synth_layout::LabelSpot::default(),
                ) else {
                    continue;
                };
                if let Some(field) = baseline
                    .iter()
                    .position(|b| boxes_overlap((b.1, b.2, b.3, b.4), rect))
                {
                    hit = Some((component.id, pin, rect, field));
                    break 'search;
                }
            }
        }
        let (component, pin, rect, field) = hit.expect("a label that lands on a field");
        let mut layout = layout;
        layout.net_labels.push(synth_layout::NetLabel {
            net: board.nets[0].id,
            component,
            pin,
            label: LABEL.to_string(),
            spot: synth_layout::LabelSpot::default(),
        });
        let after = field_boxes_for(&board, &layout);
        let (text, x0, y0, x1, y1) = &after[field];
        assert!(
            !boxes_overlap((*x0, *y0, *x1, *y1), rect),
            "{text} still on the label {rect:?}"
        );
    }

    #[test]
    fn a_wire_is_an_obstacle_where_it_runs() {
        let (board, _sexp, _text) = build_reference("ref_ldo_sensor");
        let mut layout = synth_layout::layout(&board);
        layout.wires.push(synth_layout::WirePath {
            net: board.nets[0].id,
            points: vec![(900.0, 500.0), (920.0, 500.0)],
            junctions: Vec::new(),
        });
        let placements: HashMap<ComponentId, &ComponentPlacement> =
            layout.components.iter().map(|p| (p.id, p)).collect();
        let obstacles = field_obstacles(&board, &layout, &placements);
        assert!(
            obstacles.iter().any(|o| {
                o.rect.x0 <= 910.0 && 910.0 <= o.rect.x1 && o.rect.y0 <= 500.0 && 500.0 <= o.rect.y1
            }),
            "no obstacle on the wire"
        );
    }

    #[test]
    fn body_obstacles_follow_the_board_order_not_the_placement_map() {
        // `pick_clear` sums overlaps in obstacle order, so a hash-map order
        // here would let near-equal costs flip between runs.
        let (board, ..) = build_reference("ref_ldo_sensor");
        let layout = synth_layout::layout(&board);
        let placements: HashMap<ComponentId, &ComponentPlacement> =
            layout.components.iter().map(|p| (p.id, p)).collect();
        let owners: Vec<ComponentId> = field_obstacles(&board, &layout, &placements)
            .iter()
            .filter_map(|o| o.owner)
            .collect();
        let expected: Vec<ComponentId> = board
            .components
            .iter()
            .filter(|c| c.part.is_some() && placements.contains_key(&c.id))
            .map(|c| c.id)
            .collect();
        assert!(expected.len() > 4, "too few bodies to tell an order");
        assert_eq!(owners, expected);
    }

    #[test]
    fn field_text_keeps_clear_of_a_cross_sheet_label_and_its_stub() {
        let (board, _sexp, _text) = build_reference("ref_ldo_sensor");
        let mut layout = synth_layout::layout(&board);
        layout.wires.clear();
        layout.net_labels.clear();
        layout.power_flags.clear();
        let component = board.components[0].id;
        let pin = synth_ir::PinId(0);
        layout
            .hierarchical_labels
            .push(synth_layout::HierarchicalLabel {
                net: board.nets[0].id,
                component,
                pin,
                label: "CROSS_SHEET_NET".to_string(),
            });
        let placements: HashMap<ComponentId, &ComponentPlacement> =
            layout.components.iter().map(|p| (p.id, p)).collect();
        let (x, y, dx, _) = pin_terminal_xy(&board, component, pin, &placements).unwrap();
        // Out along the pin: on the stub, then in the middle of the text.
        let out = if dx >= -0.1 { 1.0 } else { -1.0 };
        let obstacles = field_obstacles(&board, &layout, &placements);
        for reach in [2.5, 11.0] {
            let px = x + out * reach;
            assert!(
                obstacles.iter().any(|o| {
                    o.owner.is_none()
                        && o.rect.x0 <= px
                        && px <= o.rect.x1
                        && o.rect.y0 <= y
                        && y <= o.rect.y1
                }),
                "no obstacle {reach} mm out from the pin"
            );
        }
    }

    /// `build_net_label` for pin 0 of every placed part of `ref_ldo_sensor`,
    /// as the entries it emits.
    fn net_label_entries(global: bool) -> Vec<Sexp> {
        let (board, _sexp, _text) = build_reference("ref_ldo_sensor");
        let layout = synth_layout::layout(&board);
        let placements: HashMap<ComponentId, &ComponentPlacement> =
            layout.components.iter().map(|p| (p.id, p)).collect();
        let project = crate::uuid_v5::project_namespace(&board.name);
        board
            .components
            .iter()
            .filter_map(|c| {
                let label = synth_layout::NetLabel {
                    net: board.nets[0].id,
                    component: c.id,
                    pin: synth_ir::PinId(0),
                    label: "SIG".to_string(),
                    spot: synth_layout::LabelSpot::default(),
                };
                build_net_label(&board, &label, &placements, &project, None, global)
            })
            .filter_map(|mut parts| parts.pop())
            .collect()
    }

    #[test]
    fn a_net_label_is_a_global_label_flag() {
        let entries = net_label_entries(true);
        assert!(entries.len() > 4, "too few labels to tell");
        for entry in &entries {
            let Sexp::List { head, .. } = entry else {
                panic!("not a list")
            };
            assert_eq!(head, "global_label", "a signal label is a flag");
        }
    }

    #[test]
    fn a_net_label_stays_local_when_the_design_has_several_sheets() {
        let entries = net_label_entries(false);
        assert!(entries.len() > 4, "too few labels to tell");
        for entry in &entries {
            let Sexp::List { head, .. } = entry else {
                panic!("not a list")
            };
            assert_eq!(
                head, "label",
                "a global label would join same-named nets across sheets"
            );
            assert!(sub(entry, "shape").is_none(), "a local label has no shape");
        }
    }

    #[test]
    fn a_net_label_flag_points_away_from_its_stub() {
        let entries = net_label_entries(true);
        let mut sides = [false; 2];
        for entry in &entries {
            let shape = sub(entry, "shape").expect("a global label has a shape");
            assert!(matches!(&shape[0], Sexp::Atom(a) if a == "output"));
            let at = sub(entry, "at").expect("at");
            let angle = parse_num_atom(&at[2]).expect("angle");
            let effects = sub(entry, "effects").expect("effects");
            let justify = effects
                .iter()
                .find_map(|c| match c {
                    Sexp::List { head, children } if head == "justify" => Some(children),
                    _ => None,
                })
                .expect("justify");
            let want = if angle == 180.0 { "right" } else { "left" };
            assert!(matches!(&justify[0], Sexp::Atom(a) if a == want));
            sides[usize::from(angle == 180.0)] = true;
        }
        assert_eq!(sides, [true, true], "both stub sides must occur");
    }

    #[test]
    fn only_net_labels_become_flags_and_power_symbols_stay() {
        let (board, _sexp, text) = build_reference("ref_ldo_sensor");
        let labelled = synth_layout::layout(&board).net_labels.len();
        assert!(labelled > 0, "the board must carry net labels");
        assert_eq!(text.matches("\n\t(global_label\n").count(), labelled);
        assert!(
            text.contains("(lib_id \"power:") || text.contains("(lib_id \"synth:"),
            "power symbols must still be drawn"
        );
    }

    #[test]
    fn a_cross_sheet_label_stays_a_hierarchical_label() {
        let (board, _sexp, _text) = build_reference("ref_ldo_sensor");
        let layout = synth_layout::layout(&board);
        let placements: HashMap<ComponentId, &ComponentPlacement> =
            layout.components.iter().map(|p| (p.id, p)).collect();
        let project = crate::uuid_v5::project_namespace(&board.name);
        let label = synth_layout::HierarchicalLabel {
            net: board.nets[0].id,
            component: board.components[0].id,
            pin: synth_ir::PinId(0),
            label: "SIG".to_string(),
        };
        let parts =
            build_hierarchical_label(&board, &label, &placements, &project, "A").expect("label");
        let text: String = parts.iter().map(Sexp::to_string_pretty).collect();
        assert!(text.contains("hierarchical_label"));
        assert!(!text.contains("global_label"));
    }

    /// The first child list of `sexp` with head `head`, as its children.
    fn sub<'a>(sexp: &'a Sexp, head: &str) -> Option<&'a [Sexp]> {
        let Sexp::List { children, .. } = sexp else {
            return None;
        };
        children.iter().find_map(|c| match c {
            Sexp::List { head: h, children } if h == head => Some(children.as_slice()),
            _ => None,
        })
    }

    fn sub_node<'a>(sexp: &'a Sexp, head: &str) -> &'a Sexp {
        let Sexp::List { children, .. } = sexp else {
            panic!("not a list");
        };
        children
            .iter()
            .find(|c| matches!(c, Sexp::List { head: h, .. } if h == head))
            .expect("a child with that head")
    }

    fn str_of(sexp: &Sexp) -> String {
        match sexp {
            Sexp::Atom(a) | Sexp::Str(a) => a.clone(),
            _ => String::new(),
        }
    }

    fn at_of(sexp: &Sexp) -> (f64, f64, f64) {
        let at = sub(sexp, "at").expect("an at child");
        let n = |i: usize| at.get(i).map_or(0.0, |a| str_of(a).parse().unwrap());
        (n(0), n(1), n(2))
    }

    /// The text of every placed symbol's Reference and Value as KiCad draws
    /// it: `(text, x0, y0, x1, y1)`. A string drawn upside down (symbol angle
    /// plus field angle of 180) swaps its left and right justification, which
    /// is checked on the plot of absmach/synth#103.
    fn drawn_field_boxes(sexp: &Sexp) -> Vec<(String, f64, f64, f64, f64)> {
        let Sexp::List { children, .. } = sexp else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for symbol in children {
            let Sexp::List {
                head,
                children: props,
            } = symbol
            else {
                continue;
            };
            // Power symbols and flags have no body and sit on their own stub.
            let on_bom = sub(symbol, "in_bom").is_some_and(|v| str_of(&v[0]) == "yes");
            if head != "symbol" || !on_bom {
                continue;
            }
            let symbol_angle = at_of(symbol).2;
            for prop in props {
                let Sexp::List { head, children: p } = prop else {
                    continue;
                };
                if head != "property" || !matches!(str_of(&p[0]).as_str(), "Reference" | "Value") {
                    continue;
                }
                let text = str_of(&p[1]);
                let (x, y, field_angle) = at_of(prop);
                let mut justify =
                    sub(sub_node(prop, "effects"), "justify").and_then(|j| j.first().map(str_of));
                // Written out again here, not taken from `written_justify`, so
                // the tests that use this box are an independent check on it.
                if (symbol_angle + field_angle).rem_euclid(360.0) == 180.0 {
                    justify = match justify.as_deref() {
                        Some("left") => Some("right".to_string()),
                        Some("right") => Some("left".to_string()),
                        other => other.map(str::to_string),
                    };
                }
                let w = text.chars().count() as f64 * FIELD_CHAR_WIDTH_MM;
                let (x0, x1) = match justify.as_deref() {
                    Some("left") => (x, x + w),
                    Some("right") => (x - w, x),
                    _ => (x - w / 2.0, x + w / 2.0),
                };
                out.push((
                    text,
                    x0,
                    y - FIELD_TEXT_HEIGHT_MM / 2.0,
                    x1,
                    y + FIELD_TEXT_HEIGHT_MM / 2.0,
                ));
            }
        }
        out
    }

    /// Every wire segment of a built schematic.
    fn wire_segments(sexp: &Sexp) -> Vec<((f64, f64), (f64, f64))> {
        let Sexp::List { children, .. } = sexp else {
            return Vec::new();
        };
        children
            .iter()
            .filter(|c| matches!(c, Sexp::List { head, .. } if head == "wire"))
            .filter_map(|w| {
                let pts = sub(w, "pts")?;
                let xy = |i: usize| {
                    let Sexp::List { children: p, .. } = pts.get(i)? else {
                        return None;
                    };
                    Some((str_of(&p[0]).parse().ok()?, str_of(&p[1]).parse().ok()?))
                };
                Some((xy(0)?, xy(1)?))
            })
            .collect()
    }

    /// `(fixture, text)` of every Reference or Value with a wire through it.
    fn field_text_on_wires(board_name: &str) -> Vec<(String, String)> {
        let (_board, sexp, _text) = build_reference(board_name);
        let wires = wire_segments(&sexp);
        drawn_field_boxes(&sexp)
            .into_iter()
            .filter(|(_, x0, y0, x1, y1)| {
                wires.iter().any(|((ax, ay), (bx, by))| {
                    ax.min(*bx) < *x1 - 0.05
                        && ax.max(*bx) > *x0 + 0.05
                        && ay.min(*by) < *y1 - 0.05
                        && ay.max(*by) > *y0 + 0.05
                })
            })
            .map(|(text, ..)| (board_name.to_string(), text))
            .collect()
    }

    const REFERENCE_BOARDS: [&str; 10] = [
        "ref_battery_charger",
        "ref_crystal_mcu",
        "ref_ldo_sensor",
        "ref_led_driver",
        "ref_secure_element",
        "ref_spi_flash",
        "ref_uart_bridge",
        "ref_usb_cdc",
        "secure_tracker",
        "sensor_logger",
    ];

    #[test]
    fn reference_and_value_text_stays_off_wires() {
        let mut hits = Vec::new();
        for name in REFERENCE_BOARDS {
            hits.extend(field_text_on_wires(name));
        }
        assert!(hits.is_empty(), "text with a wire through it: {hits:?}");
    }

    #[test]
    fn every_fanned_power_leg_is_an_obstacle_for_text() {
        let mut legs_seen = 0;
        for name in ["ref_crystal_mcu", "ref_usb_cdc", "ref_secure_element"] {
            let (board, ..) = build_reference(name);
            let layout = synth_layout::layout(&board);
            let placements: HashMap<ComponentId, &ComponentPlacement> =
                layout.components.iter().map(|p| (p.id, p)).collect();
            let reconciled = crate::pin_reconcile::reconcile_all(&board);
            let rail_of = |leg: &crate::pin_reconcile::PowerLeg| {
                layout.power_flags.iter().find(|f| f.net == leg.net)
            };
            let obstacles = fanned_leg_obstacles(&board, &placements, &reconciled, rail_of);
            for leg in reconciled.iter().flat_map(|r| &r.power_legs) {
                let (Some(rail), Some((pin, flag))) =
                    (rail_of(leg), fanned_leg_stub(&board, leg, &placements))
                else {
                    continue;
                };
                legs_seen += 1;
                for want in [
                    power_flag_rect(flag.0, flag.1, upright(rail.kind)),
                    wire_rect(pin, flag),
                ] {
                    assert!(
                        obstacles.iter().any(|o| o.rect == want),
                        "{name}: no obstacle on the leg at {flag:?}"
                    );
                }
            }
        }
        assert!(legs_seen > 0, "no fixture has a fanned power leg");
    }

    #[test]
    fn a_fanned_leg_the_emitter_skips_is_not_an_obstacle() {
        let (board, ..) = build_reference("ref_crystal_mcu");
        let layout = synth_layout::layout(&board);
        let placements: HashMap<ComponentId, &ComponentPlacement> =
            layout.components.iter().map(|p| (p.id, p)).collect();
        let reconciled = crate::pin_reconcile::reconcile_all(&board);
        let skipped = |_: &crate::pin_reconcile::PowerLeg| None;
        assert!(fanned_leg_obstacles(&board, &placements, &reconciled, skipped).is_empty());
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
        // No wire may pass through the inside of an IC or connector body (the
        // pre-fix NRST wire ran 74 mm straight through one). A part turned on
        // its side is left out: its symbol size is that of the part unturned.
        let layout = synth_layout::layout(&board);
        for placement in &layout.components {
            let Some(part) = board.component(placement.id).and_then(|c| c.part.as_ref()) else {
                continue;
            };
            let (bw, bh) = synth_layout::body_size_for_part(part);
            let upright = matches!(
                placement.rotation,
                synth_layout::Rotation::Zero | synth_layout::Rotation::OneEighty
            );
            if bw < 1.0 || bh < 1.0 || !upright {
                continue;
            }
            let (cx, cy) = placement.center_mm;
            let (hw, hh) = (bw / 2.0 - 0.5, bh / 2.0 - 0.5);
            for wire in &layout.wires {
                for s in wire.points.windows(2) {
                    let (lo_x, hi_x) = (s[0].0.min(s[1].0), s[0].0.max(s[1].0));
                    let (lo_y, hi_y) = (s[0].1.min(s[1].1), s[0].1.max(s[1].1));
                    assert!(
                        hi_x <= cx - hw || lo_x >= cx + hw || hi_y <= cy - hh || lo_y >= cy + hh,
                        "wire {s:?} runs through the body of {}",
                        board
                            .component(placement.id)
                            .map_or("?", |c| c.refdes.as_str())
                    );
                }
            }
        }
        // Sanity: the schematic must contain at least some signal
        // wires (the NRST reset network is a local wire, not just
        // labels).
        assert!(
            !spans.is_empty(),
            "expected at least some wires in the schematic"
        );
    }

    /// One power symbol's Reference and Value as written: the symbol's `at`,
    /// then for each property its name, text, `at`, justify and whether it is
    /// hidden.
    type PowerField = (String, String, (f64, f64), Option<String>, bool);
    type PowerSymbol = ((f64, f64), Vec<PowerField>);

    fn power_symbols(sexp: &Sexp) -> Vec<PowerSymbol> {
        power_symbols_at_angle(sexp)
            .into_iter()
            .map(|(_, symbol)| symbol)
            .collect()
    }

    /// The symbols that stand upright, whose triangle hangs below and whose
    /// arrow rises above their point.
    fn upright_power_symbols(sexp: &Sexp) -> Vec<PowerSymbol> {
        power_symbols_at_angle(sexp)
            .into_iter()
            .filter(|(angle, _)| *angle == 0.0)
            .map(|(_, symbol)| symbol)
            .collect()
    }

    /// Each power symbol with the angle it is written at. A name is given as
    /// KiCad draws it: it swaps left and right justification on a string it
    /// turns the right way up, which is the case for a symbol at 90 with its
    /// fields at 90.
    fn power_symbols_at_angle(sexp: &Sexp) -> Vec<(f64, PowerSymbol)> {
        let Sexp::List { children, .. } = sexp else {
            return Vec::new();
        };
        children
            .iter()
            .filter(|c| matches!(c, Sexp::List { head, .. } if head == "symbol"))
            .filter(|s| {
                sub(s, "lib_id").is_some_and(|v| {
                    let id = str_of(&v[0]);
                    id.starts_with("power:") || id.starts_with("synth:")
                }) && sub(s, "in_bom").is_some_and(|v| str_of(&v[0]) == "no")
            })
            .map(|s| {
                let Sexp::List {
                    children: props, ..
                } = s
                else {
                    unreachable!("filtered to lists")
                };
                let (x, y, angle) = at_of(s);
                let fields = props
                    .iter()
                    .filter(|p| matches!(p, Sexp::List { head, .. } if head == "property"))
                    .map(|p| {
                        let Sexp::List { children: v, .. } = p else {
                            unreachable!("filtered to lists")
                        };
                        let effects = sub_node(p, "effects");
                        let (fx, fy, field_angle) = at_of(p);
                        let justify = sub(effects, "justify").and_then(|j| j.first().map(str_of));
                        let flip = (angle + field_angle).rem_euclid(360.0) == 180.0;
                        let justify = match justify.as_deref() {
                            Some("left") if flip => Some("right".to_string()),
                            Some("right") if flip => Some("left".to_string()),
                            _ => justify,
                        };
                        (
                            str_of(&v[0]),
                            str_of(&v[1]),
                            (fx, fy),
                            justify,
                            sub(effects, "hide").is_some_and(|h| str_of(&h[0]) == "yes"),
                        )
                    })
                    .collect();
                (angle, ((x, y), fields))
            })
            .collect()
    }

    fn power_value(fields: &[PowerField]) -> &PowerField {
        fields.iter().find(|f| f.0 == "Value").expect("a Value")
    }

    #[test]
    fn a_rail_symbol_shows_its_name_above_the_arrow() {
        let (_board, sexp, _text) = build_reference("ref_ldo_sensor");
        let rails: Vec<_> = upright_power_symbols(&sexp)
            .into_iter()
            .filter(|(_, f)| !matches!(power_value(f).1.as_str(), "GND" | "PWR_FLAG"))
            .collect();
        assert!(!rails.is_empty(), "the fixture has a supply rail");
        for ((_, y), fields) in &rails {
            let value = power_value(fields);
            assert!(!value.4, "{} is hidden", value.1);
            assert!(value.2 .1 < *y, "{} is not above its arrow", value.1);
        }
    }

    #[test]
    fn a_ground_symbol_shows_its_name_below_the_triangle() {
        let (_board, sexp, _text) = build_reference("ref_ldo_sensor");
        let grounds: Vec<_> = upright_power_symbols(&sexp)
            .into_iter()
            .filter(|(_, f)| power_value(f).1 == "GND")
            .collect();
        assert!(!grounds.is_empty(), "the fixture has a ground");
        for ((_, y), fields) in &grounds {
            let value = power_value(fields);
            assert!(!value.4, "GND is hidden");
            assert!(value.2 .1 > *y, "GND is not below its triangle");
        }
    }

    fn overlaps(a: &field_clear::Rect, b: &field_clear::Rect) -> bool {
        a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
    }

    /// Half the side of the blocks the tests wall a symbol in, in mm: close
    /// by, past a page, and far past any search with a fixed reach.
    const WALL_HALF_MM: [f64; 3] = [12.0, 40.0, 400.0];

    /// A block `half` mm either side of the origin.
    fn wall(half: f64) -> field_clear::Rect {
        field_clear::Rect {
            x0: -half,
            y0: -half,
            x1: half,
            y1: half,
        }
    }

    fn walled(half: f64) -> Vec<FieldObstacle> {
        vec![FieldObstacle {
            owner: None,
            rect: wall(half),
        }]
    }

    #[test]
    fn a_walled_in_power_name_moves_out_until_it_is_clear() {
        for half in WALL_HALF_MM {
            for kind in [
                synth_layout::PowerFlagKind::Gnd,
                synth_layout::PowerFlagKind::Vcc,
            ] {
                let mut obstacles = walled(half);
                power_symbol_properties(
                    "#PWR1",
                    "GND",
                    kind,
                    upright(kind),
                    (0.0, 0.0),
                    &mut obstacles,
                    &mut Vec::new(),
                    None,
                );
                let placed = &obstacles.last().expect("the name is an obstacle now").rect;
                assert!(
                    !overlaps(placed, &wall(half)),
                    "{kind:?} name on a {half} mm wall: {placed:?}"
                );
            }
        }
    }

    // Four characters anchored at the origin stand in for a field's text, run
    // outward from the anchor the way `field_anchor` justifies it.
    fn four_char_rect((side, ring): (FieldSide, f64)) -> field_clear::Rect {
        let justify = match side {
            FieldSide::Right => Some("left"),
            FieldSide::Left => Some("right"),
            FieldSide::Top | FieldSide::Bottom => None,
        };
        text_rect_at(push_out((0.0, 0.0), side, ring), justify, 4)
    }

    #[test]
    fn a_walled_in_field_moves_out_until_it_is_clear() {
        for half in WALL_HALF_MM {
            let obstacles = walled(half);
            let reach = obstacle_reach(&obstacles, (0.0, 0.0));
            let candidates = field_candidates(FieldSide::Right, FieldSide::Left, reach);
            let others = obstacles.iter().map(|o| &o.rect);
            let picked = field_clear::pick_clear(&candidates, &four_char_rect, &others);
            assert!(
                !overlaps(&four_char_rect(picked), &wall(half)),
                "{picked:?} on a {half} mm wall"
            );
        }
    }

    #[test]
    fn with_nothing_in_the_way_a_field_keeps_its_nearest_spot() {
        let candidates = field_candidates(FieldSide::Right, FieldSide::Left, 0.0);
        assert_eq!(candidates[0], (FieldSide::Right, 0.0));
    }

    /// The ground symbol of C1 on a board of a regulator and C1, with its
    /// name as written: the symbol's position, the name's position and
    /// justify. A horizontal wire runs `gap` mm under the place a name below
    /// the triangle would end.
    fn ground_name_over_wire(gap: f64) -> ((f64, f64), (f64, f64), Option<String>) {
        let board = lower_lenient(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C1: capacitor "c_generic_0603"
                connect U1.vin -> C1.p1
                connect U1.gnd -> C1.p2
            }"#,
        );
        let project = crate::uuid_v5::project_namespace(&board.name);
        let mut layout = synth_layout::layout(&board);
        layout.wires.clear();
        layout.net_labels.clear();
        let index = |refdes: &str| board.components.iter().position(|c| c.refdes == refdes);
        layout.components[index("U1").unwrap()].center_mm = (300.0, 100.0);
        layout.components[index("C1").unwrap()].center_mm = (100.0, 100.0);
        let ground = |layout: &synth_layout::Layout| {
            let sexp = build_schematic_from_layout(&board, &project, layout);
            let (at, fields) = power_symbols(&sexp)
                .into_iter()
                .filter(|(_, f)| power_value(f).1 == "GND")
                .min_by(|(a, _), (b, _)| (a.0 - 100.0).abs().total_cmp(&(b.0 - 100.0).abs()))
                .expect("C1 has a ground symbol");
            let value = power_value(&fields).clone();
            (at, value.2, value.3)
        };
        let (at, ..) = ground(&layout);
        // The wire is as thick as an obstacle wire, so its top edge is `gap`
        // under the name.
        let y = at.1 + POWER_NAME_OFFSET_MM + FIELD_TEXT_HEIGHT_MM / 2.0 + gap + WIRE_HALF_WIDTH_MM;
        layout.wires.push(synth_layout::WirePath {
            net: board.nets[0].id,
            points: vec![(at.0 - 5.0, y), (at.0 + 5.0, y)],
            junctions: Vec::new(),
        });
        ground(&layout)
    }

    #[test]
    fn a_ground_name_moves_beside_the_triangle_when_a_wire_is_close_below() {
        // The wire does not touch the name below the triangle, but is only
        // 0.2 mm from it.
        let (at, name_at, justify) = ground_name_over_wire(0.2);
        assert_eq!(
            justify.as_deref(),
            Some("left"),
            "{name_at:?} is not at the side"
        );
        assert!(
            name_at.0 > at.0,
            "{name_at:?} is not right of the triangle {at:?}"
        );
        assert!(
            (name_at.1 - (at.1 + POWER_TIP_MM)).abs() < 1e-6,
            "{name_at:?} is not at the level of the tip of the triangle at {at:?}"
        );
    }

    #[test]
    fn a_ground_name_stays_below_the_triangle_when_a_wire_is_further_below() {
        let (at, name_at, justify) = ground_name_over_wire(0.6);
        assert_eq!(justify, None, "{name_at:?} left the spot below");
        assert!((name_at.0 - at.0).abs() < 1e-6 && name_at.1 > at.1 + POWER_TIP_MM);
    }

    #[test]
    fn power_names_keep_clear_of_wires_and_other_text() {
        for name in [
            "ref_ldo_sensor",
            "ref_usb_cdc",
            "ref_crystal_mcu",
            "ref_battery_charger",
            "sensor_logger",
        ] {
            let (board, sexp, _) = build_reference(name);
            let names: Vec<(String, (f64, f64, f64, f64))> = power_symbols(&sexp)
                .into_iter()
                .filter_map(|(_, fields)| {
                    let (_, text, pos, justify, hidden) = power_value(&fields).clone();
                    let w = text.chars().count() as f64 * FIELD_CHAR_WIDTH_MM;
                    let (x0, x1) = match justify.as_deref() {
                        Some("left") => (pos.0, pos.0 + w),
                        Some("right") => (pos.0 - w, pos.0),
                        _ => (pos.0 - w / 2.0, pos.0 + w / 2.0),
                    };
                    let half = FIELD_TEXT_HEIGHT_MM / 2.0;
                    (!hidden).then_some((text, (x0, pos.1 - half, x1, pos.1 + half)))
                })
                .collect();
            assert!(!names.is_empty(), "{name} shows no power name");
            let wires = wire_segments(&sexp);
            let fields = field_boxes_for(&board, &synth_layout::layout(&board));
            for (i, (text, rect)) in names.iter().enumerate() {
                for (a, b) in &wires {
                    let wire = (
                        a.0.min(b.0) - WIRE_HALF_WIDTH_MM,
                        a.1.min(b.1) - WIRE_HALF_WIDTH_MM,
                        a.0.max(b.0) + WIRE_HALF_WIDTH_MM,
                        a.1.max(b.1) + WIRE_HALF_WIDTH_MM,
                    );
                    assert!(
                        !boxes_overlap(*rect, wire),
                        "{name}: {text} {rect:?} on a wire {wire:?}"
                    );
                }
                for (field, x0, y0, x1, y1) in &fields {
                    assert!(
                        !boxes_overlap(*rect, (*x0, *y0, *x1, *y1)),
                        "{name}: {text} on {field}"
                    );
                }
                for (other, other_rect) in &names[i + 1..] {
                    assert!(
                        !boxes_overlap(*rect, *other_rect),
                        "{name}: {text} {rect:?} on {other} {other_rect:?}"
                    );
                }
            }
        }
    }

    /// Where each power name must be drawn, from the board and its layout and
    /// not from the schematic: one per rail flag and one per fanned power
    /// leg, at the point the layout puts its symbol. Symbols at one point share
    /// a name, so they count once.
    fn expected_power_names(
        board: &Board,
        layout: &synth_layout::Layout,
    ) -> Vec<(String, (f64, f64))> {
        let placements: HashMap<ComponentId, &ComponentPlacement> =
            layout.components.iter().map(|p| (p.id, p)).collect();
        let rail_of = |net| layout.power_flags.iter().find(|f| f.net == net);
        let mut terminals = Vec::new();
        for flag in &layout.power_flags {
            if let Some((x, y, dx, dy)) =
                pin_terminal_xy(board, flag.component, flag.pin, &placements)
            {
                let anchor = PinAnchor { x, y, dx, dy };
                let (symbol_x, symbol_y, _) =
                    clear_power_flag_position(board, flag, &placements, layout, anchor);
                terminals.push((flag.label.clone(), (symbol_x, symbol_y)));
            }
        }
        for leg in crate::pin_reconcile::reconcile_all(board)
            .iter()
            .flat_map(|r| &r.power_legs)
        {
            if let (Some(rail), Some((_, symbol))) =
                (rail_of(leg.net), fanned_leg_stub(board, leg, &placements))
            {
                terminals.push((rail.label.clone(), symbol));
            }
        }
        terminals.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap()));
        terminals.dedup_by(|a, b| a.0 == b.0 && (a.1 .0 - b.1 .0).hypot(a.1 .1 - b.1 .1) < 0.01);
        terminals
    }

    /// How far a drawn power symbol may sit from its pin, in mm: a stub, plus a
    /// bend when the symbol steps off the stub axis.
    const POWER_SYMBOL_REACH_MM: f64 = 5.0;
    /// How far a power name may sit from its symbol, in mm: the usual spot
    /// past the arrow tip, plus the rings it moves out by when that is taken.
    const POWER_NAME_REACH_MM: f64 = 9.0;

    /// What is wrong with the power names `sexp` shows, given the names the
    /// board needs. Each expected name must be matched by one shown name from
    /// a symbol near its point, with the name beside that symbol. An expected
    /// name nothing matches (missing or hidden) and a shown name no expected
    /// name matches (drawn twice, or off its pin) are both reported.
    fn power_name_problems(expected: &[(String, (f64, f64))], sexp: &Sexp) -> Vec<String> {
        let mut shown = Vec::new();
        for (at, fields) in power_symbols(sexp) {
            let Some((_, text, pos, _, hidden)) = fields.iter().find(|f| f.0 == "Value") else {
                continue;
            };
            if !hidden && text != "PWR_FLAG" {
                shown.push((text.clone(), at, *pos));
            }
        }
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for (s, (text, at, _)) in shown.iter().enumerate() {
            for (e, (label, point)) in expected.iter().enumerate() {
                let d = (at.0 - point.0).hypot(at.1 - point.1);
                if label == text && d <= POWER_SYMBOL_REACH_MM {
                    pairs.push((d, s, e));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut shown_used = vec![false; shown.len()];
        let mut expected_used = vec![false; expected.len()];
        for (_, s, e) in pairs {
            if !shown_used[s] && !expected_used[e] {
                shown_used[s] = true;
                expected_used[e] = true;
            }
        }
        let mut problems = Vec::new();
        for ((text, at, pos), used) in shown.iter().zip(shown_used) {
            let away = (pos.0 - at.0).hypot(pos.1 - at.1);
            if !used {
                problems.push(format!("{text} shown at {at:?} belongs to no pin"));
            } else if away > POWER_NAME_REACH_MM {
                problems.push(format!("{text} at {pos:?} is {away:.1} mm from its symbol"));
            }
        }
        for ((label, point), used) in expected.iter().zip(expected_used) {
            if !used {
                problems.push(format!("{label} at {point:?} is shown 0 times"));
            }
        }
        problems
    }

    const POWER_NAME_REFERENCES: [&str; 5] = [
        "ref_ldo_sensor",
        "ref_usb_cdc",
        "ref_crystal_mcu",
        "ref_battery_charger",
        "sensor_logger",
    ];

    #[test]
    fn every_power_name_the_board_needs_is_shown_once() {
        for name in POWER_NAME_REFERENCES {
            let (board, sexp, _) = build_reference(name);
            let expected = expected_power_names(&board, &synth_layout::layout(&board));
            assert!(!expected.is_empty(), "{name} needs no power name");
            let problems = power_name_problems(&expected, &sexp);
            assert!(problems.is_empty(), "{name}: {problems:#?}");
        }
    }

    /// Takes the first shown power name out of `sexp`, hidden or with its Value
    /// field removed, and says which name it was.
    fn drop_first_power_name(sexp: &mut Sexp, hide: bool) -> String {
        let Sexp::List { children, .. } = sexp else {
            panic!("a schematic is a list");
        };
        for symbol in children.iter_mut() {
            let is_power = sub(symbol, "lib_id").is_some_and(|v| {
                let id = str_of(&v[0]);
                id.starts_with("power:") || id.starts_with("synth:")
            }) && sub(symbol, "in_bom").is_some_and(|v| str_of(&v[0]) == "no");
            let Sexp::List {
                children: props, ..
            } = symbol
            else {
                continue;
            };
            let shown = props.iter().position(|p| {
                matches!(p, Sexp::List { head, children } if head == "property"
                    && str_of(&children[0]) == "Value"
                    && str_of(&children[1]) != "PWR_FLAG")
                    && sub(sub_node(p, "effects"), "hide").is_none()
            });
            let Some(i) = shown.filter(|_| is_power) else {
                continue;
            };
            let Sexp::List { children: v, .. } = &mut props[i] else {
                unreachable!("matched a property list")
            };
            let text = str_of(&v[1]);
            if hide {
                *v.last_mut().unwrap() = hidden_field_effects();
            } else {
                props.remove(i);
            }
            return text;
        }
        panic!("no shown power name to drop");
    }

    #[test]
    fn a_power_name_that_is_hidden_or_removed_is_reported() {
        for hide in [true, false] {
            let (board, mut sexp, _) = build_reference("sensor_logger");
            let expected = expected_power_names(&board, &synth_layout::layout(&board));
            let dropped = drop_first_power_name(&mut sexp, hide);
            let problems = power_name_problems(&expected, &sexp);
            assert!(
                problems
                    .iter()
                    .any(|p| p.starts_with(&dropped) && p.ends_with("is shown 0 times")),
                "{dropped} was {} but the check says {problems:?}",
                if hide { "hidden" } else { "removed" }
            );
        }
    }

    #[test]
    fn a_vertical_resistor_shows_reference_above_value_beside_its_body() {
        let board = lower_lenient(
            r#"board "b" {
                component R1: resistor "r_generic_0603" value "5.1k"
                connect R1.p1 -> "VBUS"
                connect R1.p2 -> "GND"
            }"#,
        );
        let project = crate::uuid_v5::project_namespace(&board.name);
        let mut layout = synth_layout::layout(&board);
        layout.components[0].rotation = Rotation::TwoSeventy;
        let sexp = build_schematic_from_layout(&board, &project, &layout);
        let Sexp::List { children, .. } = &sexp else {
            panic!("a schematic is a list");
        };
        let body = children
            .iter()
            .find(|c| {
                matches!(c, Sexp::List { head, .. } if head == "symbol")
                    && sub(c, "in_bom").is_some_and(|v| str_of(&v[0]) == "yes")
            })
            .expect("the resistor symbol");
        let (cx, cy, angle) = at_of(body);
        assert!(
            angle.rem_euclid(180.0) == 0.0,
            "the resistor is not vertical: angle {angle}"
        );
        let boxes = drawn_field_boxes(&sexp);
        let (reference, value) = (box_of(&boxes, "R1"), box_of(&boxes, "5.1k"));
        assert!(
            reference.0 > cx,
            "R1 {reference:?} is not right of the body at x {cx}"
        );
        assert!(
            (reference.0 - value.0).abs() < 1e-6,
            "R1 {reference:?} and 5.1k {value:?} do not share a left edge"
        );
        assert!(
            reference.3 <= value.1,
            "R1 {reference:?} is not above 5.1k {value:?}"
        );
        let middle = f64::midpoint(reference.1, value.3);
        assert!(
            (middle - cy).abs() < 1e-6,
            "the pair is centred at y {middle}, the body at y {cy}"
        );
    }

    #[test]
    fn a_reference_and_value_in_one_row_leave_a_gap_so_they_do_not_read_as_one_string() {
        for name in [
            "ref_battery_charger",
            "ref_crystal_mcu",
            "ref_ldo_sensor",
            "ref_led_driver",
            "ref_secure_element",
            "ref_spi_flash",
            "ref_uart_bridge",
            "ref_usb_cdc",
            "secure_tracker",
            "sensor_logger",
        ] {
            let (_board, sexp, _text) = build_reference(name);
            // Each symbol writes its Reference, then its Value.
            for pair in drawn_field_boxes(&sexp).chunks(2) {
                let (a, b) = (&pair[0], &pair[1]);
                let same_row = a.4.min(b.4) - a.2.max(b.2) > 0.0;
                let gap = (b.1 - a.3).max(a.1 - b.3);
                assert!(
                    !(same_row && (0.0..1.0).contains(&gap)),
                    "{name}: {} and {} are {gap} mm apart in one row",
                    a.0,
                    b.0
                );
            }
        }
    }

    #[test]
    fn field_text_keeps_clear_of_the_drawing_of_power_symbols() {
        for name in [
            "ref_battery_charger",
            "ref_crystal_mcu",
            "ref_ldo_sensor",
            "ref_led_driver",
            "ref_secure_element",
            "ref_spi_flash",
            "ref_uart_bridge",
            "ref_usb_cdc",
            "secure_tracker",
            "sensor_logger",
        ] {
            let (_board, sexp, _text) = build_reference(name);
            // A ground triangle hangs below its point, a rail arrow rises
            // above; a symbol turned to 90 points right, at 270 left.
            let drawings: Vec<(f64, f64, f64, f64)> = power_symbols_at_angle(&sexp)
                .into_iter()
                .filter_map(|(angle, ((x, y), fields))| {
                    let value = power_value(&fields);
                    let drawing = if angle == 0.0 {
                        let (y0, y1) = if value.2 .1 > y {
                            (y, y + POWER_TIP_MM)
                        } else {
                            (y - POWER_TIP_MM, y)
                        };
                        (x - 1.27, y0, x + 1.27, y1)
                    } else if value.2 .0 > x {
                        (x, y - 1.27, x + POWER_TIP_MM, y + 1.27)
                    } else {
                        (x - POWER_TIP_MM, y - 1.27, x, y + 1.27)
                    };
                    (!value.4).then_some(drawing)
                })
                .collect();
            for (field, x0, y0, x1, y1) in drawn_field_boxes(&sexp) {
                for drawing in &drawings {
                    assert!(
                        !boxes_overlap((x0, y0, x1, y1), *drawing),
                        "{name}: {field} {:?} on a power symbol {drawing:?}",
                        (x0, y0, x1, y1)
                    );
                }
            }
        }
    }

    #[test]
    fn a_power_name_moves_inside_its_group_box_when_its_usual_spot_is_outside() {
        let (mut board, ..) = build_reference("ref_ldo_sensor");
        let mut layout = synth_layout::layout(&board);
        let project = crate::uuid_v5::project_namespace(&board.name);
        let shown = |sexp: &Sexp| -> Vec<((f64, f64), (f64, f64))> {
            power_symbols(sexp)
                .into_iter()
                .filter_map(|(at, fields)| {
                    let value = power_value(&fields);
                    (!value.4).then_some((at, value.2))
                })
                .collect()
        };
        let before = shown(&build_schematic_from_layout(&board, &project, &layout));
        let (at, name) = before[0];
        // A box that takes in the whole sheet except the band 3.3 mm past the
        // symbol where its name usually sits. Text keeps out of the title band
        // at the top of a box, so a box that ends above the symbol starts that
        // band's height higher.
        let far = 1000.0;
        let band = synth_layout::GROUP_TITLE_BAND_MM;
        let bounds = if name.1 > at.1 {
            (-far, -far, far, at.1 + 3.3)
        } else {
            (-far, at.1 - 3.3 - band, far, far)
        };
        for component in &mut board.components {
            component.group = Some("G".to_string());
        }
        layout.group_boxes = vec![synth_layout::GroupBox {
            group: "G".to_string(),
            min_mm: (bounds.0, bounds.1),
            max_mm: (bounds.2, bounds.3),
            color: [0, 0, 0],
            caption_inside: true,
        }];
        let after = shown(&build_schematic_from_layout(&board, &project, &layout));
        let (_, moved) = after
            .into_iter()
            .find(|(a, _)| *a == at)
            .expect("the symbol");
        assert_ne!(moved, name, "the name did not move");
        assert!(
            moved.1 > bounds.1 + band && moved.1 < bounds.3,
            "name at y {} is outside {bounds:?}",
            moved.1
        );
    }

    #[test]
    fn a_power_name_stays_out_of_the_title_band_of_its_group_box() {
        let (mut board, ..) = build_reference("ref_ldo_sensor");
        let mut layout = synth_layout::layout(&board);
        let project = crate::uuid_v5::project_namespace(&board.name);
        let shown = |sexp: &Sexp| -> Vec<((f64, f64), (f64, f64))> {
            power_symbols(sexp)
                .into_iter()
                .filter_map(|(at, fields)| {
                    let value = power_value(&fields);
                    (!value.4).then_some((at, value.2))
                })
                .collect()
        };
        let before = shown(&build_schematic_from_layout(&board, &project, &layout));
        // A rail symbol: its name sits above it.
        let (at, name) = before
            .into_iter()
            .find(|(at, name)| name.1 < at.1)
            .expect("a rail symbol");
        // The title band ends 3.3 mm above the symbol, so the usual spot of
        // the name is in the band and a spot beside the symbol is not.
        let band_end = at.1 - 3.3;
        let top = band_end - synth_layout::GROUP_TITLE_BAND_MM;
        for component in &mut board.components {
            component.group = Some("G".to_string());
        }
        layout.group_boxes = vec![synth_layout::GroupBox {
            group: "G".to_string(),
            min_mm: (-1000.0, top),
            max_mm: (1000.0, 1000.0),
            color: [0, 0, 0],
            caption_inside: true,
        }];
        let after = shown(&build_schematic_from_layout(&board, &project, &layout));
        let (_, moved) = after
            .into_iter()
            .find(|(a, _)| *a == at)
            .expect("the symbol");
        assert_ne!(moved, name, "the name stayed in the band");
        assert!(
            moved.1 - FIELD_TEXT_HEIGHT_MM / 2.0 >= band_end - 1e-6,
            "name at y {} is in the band that ends at {band_end}",
            moved.1
        );
    }

    #[test]
    fn a_power_symbol_keeps_its_reference_hidden() {
        let (_board, sexp, _text) = build_reference("ref_ldo_sensor");
        for (_, fields) in power_symbols(&sexp) {
            let reference = fields.iter().find(|f| f.0 == "Reference").unwrap();
            assert!(reference.4, "{} is shown", reference.1);
        }
    }

    /// A net the author named and the schematic draws only as wires has no
    /// flag to carry the name, so KiCad would call it `Net-(...)` and the PCB
    /// would disagree. It carries a local label instead.
    #[test]
    fn a_named_net_drawn_only_as_wires_carries_its_name() {
        let (mut board, _sexp, _text) = build_reference("sensor_logger");
        let layout = synth_layout::layout(&board);
        let flagged = layout.labeled_net_ids();
        let power = layout.power_net_ids();
        let wired: Vec<NetId> = board
            .nets
            .iter()
            .filter(|n| {
                !flagged.contains(&n.id)
                    && !power.contains(&n.id)
                    && layout.wires.iter().any(|w| w.net == n.id)
            })
            .map(|n| n.id)
            .collect();
        assert!(!wired.is_empty(), "no wired signal net to name");
        for net in board.nets.iter_mut().filter(|n| wired.contains(&n.id)) {
            net.name = format!("WIRED_{}", net.id.0);
            net.auto_named = false;
        }
        let project = crate::uuid_v5::project_namespace(&board.name);
        let text = build_schematic(&board, &project).to_string_pretty();
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        for id in wired {
            assert!(
                flat.contains(&format!("(label \"WIRED_{}\"", id.0)),
                "wired net {} has no name label",
                id.0
            );
        }
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
