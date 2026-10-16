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
    let mut obstacles = field_obstacles(board, layout, &placements);
    let mut power_names = Vec::new();
    for component in &board.components {
        if let Some(placement) = placements.get(&component.id) {
            let overrides = variant_dnp.get(component.refdes.as_str());
            for s in build_symbol_instance(component, placement, project, board, &mut obstacles) {
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

    // The fan-out legs come after the power flags, so their stubs are
    // registered first or a power name could land on one.
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
    let reconciled_pins = crate::pin_reconcile::reconcile_all(board);
    for leg in reconciled_pins.iter().flat_map(|r| &r.power_legs) {
        if on_sheet(leg.component) && rail_flag(leg.net).is_some() {
            if let Some(((x, y), (flag_x, flag_y))) = fanned_leg_stub(board, leg, &placements) {
                obstacles.push(FieldObstacle {
                    owner: None,
                    rect: power_flag_rect(flag_x, flag_y),
                });
                obstacles.push(FieldObstacle {
                    owner: None,
                    rect: wire_rect((x, y), (flag_x, flag_y)),
                });
            }
        }
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
        ) {
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
        let color = net_colors.get(&label.net).copied();
        if let Some(sexps) = build_net_label(board, label, &placements, project, color) {
            children.extend(sexps);
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

    // Titled outline boxes: one `(rectangle ...)` per declared `group`
    // (§21.1), framing the caption and its parts. Same graphic
    // status as captions — no electrical meaning, never trips ERC.
    // Shape grammar mirrors the embedded symbol library
    // (`(rectangle … (stroke …) (fill …))`), which is what
    // `kicad-cli sch erc` accepts at top level — a bare
    // `(rect … (fill none))` fails to load.
    //
    // Phase B2 styling: a dashed stroke in the group's deterministic
    // hue ([`synth_layout::group_color`]) plus a translucent fill of
    // the same hue, so each region reads as its own block (reference
    // mechanism 2). The grammar was verified against `kicad-cli 10`:
    // `(stroke (type dash) (color r g b 1))` and
    // `(fill (type color) (color r g b 0.08))` both load.
    for group_box in &layout.group_boxes {
        let uuid = derive_entity_uuid(project, "group_box", &group_box.group);
        let [r, g, b] = group_box.color;
        children.push(Sexp::list(
            "rectangle",
            vec![
                Sexp::list(
                    "start",
                    vec![num(group_box.min_mm.0), num(group_box.min_mm.1)],
                ),
                Sexp::list(
                    "end",
                    vec![num(group_box.max_mm.0), num(group_box.max_mm.1)],
                ),
                Sexp::list(
                    "stroke",
                    vec![
                        Sexp::list("width", vec![num(0.254)]),
                        Sexp::list("type", vec![Sexp::atom("dash")]),
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
                Sexp::list(
                    "fill",
                    vec![
                        Sexp::list("type", vec![Sexp::atom("color")]),
                        Sexp::list(
                            "color",
                            vec![
                                Sexp::atom(r.to_string()),
                                Sexp::atom(g.to_string()),
                                Sexp::atom(b.to_string()),
                                num(0.08),
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
            if !on_sheet(leg.component) {
                continue;
            }
            if let Some(rail) = rail_flag(leg.net) {
                if let Some(sexps) = build_fanned_power_symbol(
                    board,
                    leg,
                    rail,
                    &placements,
                    project,
                    &mut *power_refs,
                    &mut obstacles,
                    &mut power_names,
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

/// Where a fanned power leg's stub runs: from the pin terminal to the point
/// its power symbol sits at.
fn fanned_leg_stub(
    board: &Board,
    leg: &crate::pin_reconcile::PowerLeg,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> Option<((f64, f64), (f64, f64))> {
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

    // Offset the symbol one grid unit outward from the pin so it
    // sits clear of the body.
    let stub_len = POWER_FLAG_STUB_LEN;
    let flag_x = x + dx * stub_len;
    let flag_y = y + dy * stub_len;
    Some(((x, y), (flag_x, flag_y)))
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
        (flag_x, flag_y),
        obstacles,
        power_names,
    );
    let sym_sexp = power_symbol_sexp(lib_id_out, flag_x, flag_y, sym_uuid, properties);

    Some(vec![wire_sexp, sym_sexp])
}

/// Build the `(symbol ...)` S-expression for a power flag carrying the given
/// Reference and Value properties.
fn power_symbol_sexp(
    lib_id: String,
    flag_x: f64,
    flag_y: f64,
    sym_uuid: Uuid,
    properties: Vec<Sexp>,
) -> Sexp {
    let mut children = vec![
        str_pair("lib_id", lib_id),
        Sexp::list("at", vec![num(flag_x), num(flag_y), num(0.0)]),
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

/// How far (mm) a label on a vertical pin continues out along the pin
/// before turning sideways — one grid step, enough to lift the label text
/// off the part's end and away from its side-placed fields.
const VERTICAL_LABEL_RISE: f64 = 2.54;

/// Emit a KiCad `(global_label)` entry attached to a stub extending
/// from the pin's coordinate.
fn build_net_label(
    board: &Board,
    label: &synth_layout::NetLabel,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    project: &Uuid,
    color: Option<[u8; 3]>,
) -> Option<Vec<Sexp>> {
    let component = board.component(label.component)?;
    let (x, y, dx, dy) = pin_terminal_xy(board, label.component, label.pin, placements)?;
    let stub_len = 5.08;
    // A label on a vertical pin (a resistor or LED turned upright) used to
    // turn sideways right at the pin end, which drops its text beside the
    // part's end — in the same column as the part's side-placed
    // Reference/Value fields. Carry the wire one grid step further out
    // along the pin first, so the label clears the part's extent.
    let elbow_y = if dx.abs() < 0.1 && dy.abs() > 0.1 {
        y + dy.signum() * VERTICAL_LABEL_RISE
    } else {
        y
    };
    let (stub_x, angle) = if dx >= -0.1 {
        (x + stub_len, 0.0)
    } else {
        (x - stub_len, 180.0)
    };

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
    let mut out = Vec::with_capacity(3);
    if (elbow_y - y).abs() > f64::EPSILON {
        let rise_uuid = derive_entity_uuid(project, "net_label_rise", &key);
        out.push(wire((x, y), (x, elbow_y), rise_uuid));
    }
    out.push(wire((x, elbow_y), (stub_x, elbow_y), wire_uuid));

    // The label text carries the same hue as its wire, so a name and
    // the line it names read as one object (reference sheet
    // mechanism 4).
    let mut font = vec![Sexp::list("size", vec![num(1.27), num(1.27)])];
    if let Some(rgb) = color {
        font.push(rgb_color(rgb));
    }
    let label_sexp = Sexp::list(
        "label",
        vec![
            Sexp::str(&label.label),
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
        ],
    );
    out.push(label_sexp);

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
    if !stub_is_blocked(board, flag, placements, layout, (x, y), natural) {
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
    let mut stub_points = vec![(x, y)];
    if let Some(corner) = stub_corner {
        stub_points.push(corner);
    }
    stub_points.push((flag_x, flag_y));

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

    obstacles.extend(stub_points.windows(2).map(|seg| FieldObstacle {
        owner: None,
        rect: wire_rect(seg[0], seg[1]),
    }));
    let properties = power_symbol_properties(
        &refdes_pwr,
        &flag.label,
        flag.kind,
        (flag_x, flag_y),
        obstacles,
        power_names,
    );
    let sym_sexp = power_symbol_sexp(lib_id, flag_x, flag_y, sym_uuid, properties);

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
/// How far past its usual spot a power name may move to clear what is in the
/// way, nearest first, in mm.
const POWER_NAME_RINGS_MM: [f64; 3] = [0.0, 2.54, 5.08];
/// How far the tip of the arrow or triangle is from the pin, in mm.
const POWER_TIP_MM: f64 = 2.54;

/// A power symbol's position and the name it prints there.
type PowerName = ((f64, f64), String);

/// The Reference and Value properties of a power symbol at `at`. The
/// Reference (`#PWR12`) stays hidden. The Value is the net name, drawn past
/// the arrow tip or triangle, or at the level of the tip to either side when
/// that spot is taken. The name is added to `obstacles` for what is placed
/// afterwards.
/// Stacked pins give several symbols at one point; the first draws the name and
/// the rest keep theirs hidden, so the same text is not printed over itself.
fn power_symbol_properties(
    refdes: &str,
    name: &str,
    kind: synth_layout::PowerFlagKind,
    at: (f64, f64),
    obstacles: &mut Vec<FieldObstacle>,
    power_names: &mut Vec<PowerName>,
) -> Vec<Sexp> {
    // A rail arrow points up the sheet, a ground triangle down.
    let away = match kind {
        synth_layout::PowerFlagKind::Vcc => -1.0,
        synth_layout::PowerFlagKind::Gnd => 1.0,
    };
    let candidates: Vec<(f64, f64, Option<&'static str>)> = POWER_NAME_RINGS_MM
        .iter()
        .flat_map(|&ring| {
            [
                (0.0, away * (POWER_NAME_OFFSET_MM + ring), None),
                (POWER_NAME_SIDE_MM + ring, away * POWER_TIP_MM, Some("left")),
                (
                    -(POWER_NAME_SIDE_MM + ring),
                    away * POWER_TIP_MM,
                    Some("right"),
                ),
            ]
        })
        .collect();
    let repeats = power_names.iter().any(|(p, n)| *p == at && n == name);
    let chars = name.chars().count();
    let rect_at = |(dx, dy, justify): (f64, f64, Option<&'static str>)| {
        text_rect_at((at.0 + dx, at.1 + dy), justify, chars)
    };
    let others = obstacles.iter().map(|o| &o.rect);
    let (dx, dy, justify) = field_clear::pick_clear(&candidates, &rect_at, &others);
    let pos = (at.0 + dx, at.1 + dy);
    if !repeats {
        power_names.push((at, name.to_string()));
        obstacles.push(FieldObstacle {
            owner: None,
            rect: text_rect_at(pos, justify, chars),
        });
    }
    let value_effects = if repeats {
        hidden_field_effects()
    } else {
        field_effects(justify)
    };
    let property = |field: &str, text: &str, pos: (f64, f64), effects: Sexp| {
        Sexp::list(
            "property",
            vec![
                Sexp::str(field),
                Sexp::str(text),
                Sexp::list("at", vec![num(pos.0), num(pos.1), num(0.0)]),
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
            synth_layout::kicad_lib_loader::unit_offset_mm(unit).1
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

/// The four sides of a symbol body a field can sit on.
const FIELD_SIDES: [FieldSide; 4] = [
    FieldSide::Top,
    FieldSide::Bottom,
    FieldSide::Right,
    FieldSide::Left,
];
/// How far past its usual spot a field may move to clear what is in the way,
/// nearest first, in mm. The 5.08 is a ring spacing that happens to equal the
/// net label stub, not a reference to it.
const FIELD_RINGS_MM: [f64; 3] = [0.0, 5.08, 10.16];

/// Height of a Reference or Value string in mm, from KiCad's plot of 1.27 mm text.
const FIELD_TEXT_HEIGHT_MM: f64 = 1.4;
/// Width of one Reference or Value character in mm, from the same plot.
const FIELD_CHAR_WIDTH_MM: f64 = 1.2;
/// Half the thickness given to a wire when it is an obstacle, in mm.
const WIRE_HALF_WIDTH_MM: f64 = 0.15;
/// Half the height of a pin line and its number, in mm.
const PIN_TEXT_HALF_MM: f64 = 1.3;
/// Half the side of the square a power flag symbol covers, in mm.
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

/// Where a field may go, best first: the sides at each distance in
/// [`FIELD_RINGS_MM`], nearest ring first. `first` and `second` are the sides
/// the field would take by preference, then the other two follow.
fn field_candidates(first: FieldSide, second: FieldSide) -> Vec<(FieldSide, f64)> {
    let rest = FIELD_SIDES.iter().filter(|&&s| s != first && s != second);
    let order: Vec<FieldSide> = [first, second].into_iter().chain(rest.copied()).collect();
    FIELD_RINGS_MM
        .iter()
        .flat_map(|&ring| order.iter().map(move |&side| (side, ring)))
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

/// The square a power flag symbol at (`x`, `y`) covers.
fn power_flag_rect(x: f64, y: f64) -> field_clear::Rect {
    field_clear::Rect {
        x0: x - POWER_FLAG_HALF_MM,
        y0: y - POWER_FLAG_HALF_MM,
        x1: x + POWER_FLAG_HALF_MM,
        y1: y + POWER_FLAG_HALF_MM,
    }
}

/// Every wire, body, pin, net label, power flag and caption on the sheet as an
/// obstacle for the Reference and Value text of the symbols.
fn field_obstacles(
    board: &Board,
    layout: &synth_layout::Layout,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> Vec<FieldObstacle> {
    let fixed = |rect| FieldObstacle { owner: None, rect };
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
            rect: field_clear::Rect {
                x0: x - w / 2.0,
                y0: y - h / 2.0,
                x1: x + w / 2.0,
                y1: y + h / 2.0,
            },
        });
    }
    // A net label is its text and the stub that joins it to the pin; a
    // cross-sheet label is drawn the same way.
    let labels = layout
        .net_labels
        .iter()
        .map(|l| (&l.label, l.component, l.pin))
        .chain(
            layout
                .hierarchical_labels
                .iter()
                .map(|l| (&l.label, l.component, l.pin)),
        );
    for (text, component, pin) in labels {
        let Some((x0, y0, x1, y1)) =
            synth_layout::route::net_label_rect(board, text, component, pin, placements)
        else {
            continue;
        };
        let Some((pin_x, ..)) = pin_terminal_xy(board, component, pin, placements) else {
            continue;
        };
        out.push(fixed(field_clear::Rect {
            x0: x0.min(pin_x),
            y0,
            x1: x1.max(pin_x),
            y1,
        }));
    }
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
    for flag in &layout.power_flags {
        if let Some((x, y, dx, dy)) = pin_terminal_xy(board, flag.component, flag.pin, placements) {
            let (cx, cy) = (x + dx * POWER_FLAG_STUB_LEN, y + dy * POWER_FLAG_STUB_LEN);
            out.push(fixed(power_flag_rect(cx, cy)));
            out.push(fixed(wire_rect((x, y), (cx, cy))));
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
    let mut place_field = |text: &str, candidates: &[(FieldSide, f64)]| {
        let spot = |(side, ring): (FieldSide, f64)| {
            let (pos, justify) = field_anchor(side, pins_on(counts, side), (x, y), body_edges);
            (push_out(pos, side, ring), justify)
        };
        let rect_at = |candidate| {
            let (pos, justify) = spot(candidate);
            text_rect_at(pos, justify, text.chars().count())
        };
        let others = obstacles
            .iter()
            .filter(|o| o.owner != Some(component.id))
            .map(|o| &o.rect);
        let (pos, justify) = spot(field_clear::pick_clear(candidates, &rect_at, &others));
        let rect = text_rect_at(pos, justify, text.chars().count());
        obstacles.push(FieldObstacle { owner: None, rect });
        (pos, written_justify(justify, angle, field_angle))
    };
    let (ref_pos, ref_justify) =
        place_field(&component.refdes, &field_candidates(ref_pref, value_pref));
    let (value_pos, value_justify) =
        place_field(display_value, &field_candidates(value_pref, ref_pref));

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
    for (name, value) in &component.properties {
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
        assert!(text.contains("(type dash)"), "dashed stroke");
        assert!(text.contains("(type color)"), "colour fill");
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
    fn group_box_exports_rectangle() {
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
        assert!(text.contains("(rectangle"), "group outline box must render");
        assert!(text.contains("\"Input\""), "group caption must render");
        // Phase B2: dashed stroke in the group hue plus a translucent
        // fill of the same hue.
        assert!(text.contains("(type dash)"), "box stroke must be dashed");
        assert!(
            text.contains("(fill") && text.contains("(type color)"),
            "box must carry a colour fill"
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

    #[test]
    fn reference_and_value_text_stays_off_wires() {
        let mut hits = Vec::new();
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
            hits.extend(field_text_on_wires(name));
        }
        assert!(hits.is_empty(), "text with a wire through it: {hits:?}");
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

    /// One power symbol's Reference and Value as written: the symbol's `at`,
    /// then for each property its name, text, `at`, justify and whether it is
    /// hidden.
    type PowerField = (String, String, (f64, f64), Option<String>, bool);
    type PowerSymbol = ((f64, f64), Vec<PowerField>);

    fn power_symbols(sexp: &Sexp) -> Vec<PowerSymbol> {
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
                let fields = props
                    .iter()
                    .filter(|p| matches!(p, Sexp::List { head, .. } if head == "property"))
                    .map(|p| {
                        let Sexp::List { children: v, .. } = p else {
                            unreachable!("filtered to lists")
                        };
                        let effects = sub_node(p, "effects");
                        let (x, y, _) = at_of(p);
                        (
                            str_of(&v[0]),
                            str_of(&v[1]),
                            (x, y),
                            sub(effects, "justify").and_then(|j| j.first().map(str_of)),
                            sub(effects, "hide").is_some_and(|h| str_of(&h[0]) == "yes"),
                        )
                    })
                    .collect();
                let (x, y, _) = at_of(s);
                ((x, y), fields)
            })
            .collect()
    }

    fn power_value(fields: &[PowerField]) -> &PowerField {
        fields.iter().find(|f| f.0 == "Value").expect("a Value")
    }

    #[test]
    fn a_rail_symbol_shows_its_name_above_the_arrow() {
        let (_board, sexp, _text) = build_reference("ref_ldo_sensor");
        let rails: Vec<_> = power_symbols(&sexp)
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
        let grounds: Vec<_> = power_symbols(&sexp)
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

    #[test]
    fn a_power_symbol_keeps_its_reference_hidden() {
        let (_board, sexp, _text) = build_reference("ref_ldo_sensor");
        for (_, fields) in power_symbols(&sexp) {
            let reference = fields.iter().find(|f| f.0 == "Reference").unwrap();
            assert!(reference.4, "{} is shown", reference.1);
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
