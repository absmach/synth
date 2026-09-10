// SPDX-License-Identifier: Apache-2.0

//! In-house schematic aesthetic ERC rule engine (`E-SYNTH-SCHEM-001..010`).
//!
//! Pure, deterministic rules over a [`synth_layout::Layout`] (plus the
//! [`synth_ir::Board`] where connectivity context is needed). This is
//! the design-time complement to `crate::erc_validate`: that module
//! shells out to `kicad-cli` for KiCad's own ERC, while this module
//! never leaves the process and is fully testable with synthetic
//! layouts.
//!
//! The rules are advisory aesthetic checks per §7.7.7 of the
//! implementation plan, reported at [`Severity::Warning`] (they must
//! never block compilation). Implemented here, mapped onto the
//! `E-SYNTH-SCHEM-001..010` code range:
//!
//! * **E-SYNTH-SCHEM-001** — Inverted power symbol (GND pointing up or
//!   VCC pointing down).
//! * **E-SYNTH-SCHEM-002** — Excessive wire-crossing density
//!   (> [`SchemErcConfig::max_crossings`] crossings on a single sheet).
//! * **E-SYNTH-SCHEM-003** — Decoupling capacitor too far
//!   (> [`SchemErcConfig::decoupling_max_mm`]) from its target IC.
//! * **E-SYNTH-SCHEM-004** — Long explicit net (span exceeding
//!   [`SchemErcConfig::long_net_max_mm`]) drawn as a wire without
//!   net-label truncation.
//!
//! The remaining `E-SYNTH-SCHEM-005..010` codes cover the connection
//! and page conventions from Sierra Circuits' "Schematic Design
//! Rules" knowledge base (protoexpress.com/kb/schematic-design-rules/):
//!
//! * **E-SYNTH-SCHEM-005** — Junction fan-out: more than
//!   [`SchemErcConfig::max_junction_degree`] wire lines meeting at a
//!   node ("It is always a good practice to have only 3 lines
//!   connected to a node").
//! * **E-SYNTH-SCHEM-006** — Cross-net junction: a node dot placed at
//!   a point a *different* net's wire touches ("Lines that intersect
//!   with each other are not connected unless there is a node present
//!   at the point of intersection" — so a node must never sit on a
//!   foreign net's geometry, or KiCad reads it as a designed-in short).
//! * **E-SYNTH-SCHEM-007** — Page overflow: content outside the
//!   selected sheet size ("Select the `page` size based on the size of
//!   your circuit design").
//!
//! `E-SYNTH-SCHEM-008..010` cover the naming conventions from the
//! canonical "Rules and guidelines for drawing good schematics"
//! thread (electronics.stackexchange.com/questions/28251):
//!
//! * **E-SYNTH-SCHEM-008** — Net label not UPPERCASE ("Use all caps
//!   for net names and pin names").
//! * **E-SYNTH-SCHEM-009** — Net label longer than
//!   [`SchemErcConfig::max_net_label_len`] chars ("Keep names
//!   reasonably short").
//! * **E-SYNTH-SCHEM-010** — Ambiguous power rail name: a rail flying
//!   the generic `VCC`/`VDD`/`VPP` symbol instead of an explicit
//!   voltage ("DO NOT USE VDD or VCC as they are ambiguous. Make a
//!   new symbol for explicitly declaring what the voltage is").

use std::collections::{HashMap, HashSet};

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, EntityRef, Severity};
use synth_ir::{Board, ComponentId, NetId, PinId};
use synth_layout::{Layout, PinSide, PowerFlagKind, Rotation, WirePath};

/// Default wire-crossing budget on a single sheet before
/// `E-SYNTH-SCHEM-002` fires. Plan §7.7.7: "> 5 crossings".
const DEFAULT_MAX_CROSSINGS: usize = 5;
/// Default schematic distance (mm) a decoupling capacitor may sit from
/// its target IC before `E-SYNTH-SCHEM-003` fires. Plan §7.7.7:
/// "> 15 mm".
const DEFAULT_DECOUPLING_MAX_MM: f64 = 15.0;
/// Default per-net span (mm) before `E-SYNTH-SCHEM-004` fires. Plan
/// §7.7.7: "> 100 mm".
const DEFAULT_LONG_NET_MAX_MM: f64 = 100.0;
/// Default number of wire lines a node may carry before
/// `E-SYNTH-SCHEM-005` fires. Sierra Circuits "Schematic Design
/// Rules": "It is always a good practice to have only 3 lines
/// connected to a node".
const DEFAULT_MAX_JUNCTION_DEGREE: usize = 3;
/// Default rendered-net-name budget (chars) before `E-SYNTH-SCHEM-009`
/// fires. StackExchange #28251: "Keep names reasonably short — no
/// names is no information, but lots of long names are clutter."
const DEFAULT_MAX_NET_LABEL_LEN: usize = 16;
/// Power-rail labels too generic to be useful, flagged by
/// `E-SYNTH-SCHEM-010`. StackExchange #28251: "DO NOT USE VDD or VCC
/// as they are ambiguous. Make a new symbol for explicitly declaring
/// what the voltage is." These are exactly the strings the rail
/// classifier falls back to when no declared name or voltage token
/// exists — renaming the net (or using a regulator whose part carries
/// a voltage) makes the warning go away.
const AMBIGUOUS_RAIL_LABELS: [&str; 3] = ["VCC", "VDD", "VPP"];

/// Thresholds that tune the aesthetic rules. All fields have
/// [`Default`] values matching §7.7.7 so [`check`] needs no
/// configuration for the common case.
#[derive(Debug, Clone, Copy)]
pub struct SchemErcConfig {
    /// `E-SYNTH-SCHEM-002`: a sheet with *more* than this many
    /// different-net wire crossings is flagged.
    pub max_crossings: usize,
    /// `E-SYNTH-SCHEM-003`: a decoupling capacitor further than this
    /// (mm, schematic distance) from its target IC is flagged.
    pub decoupling_max_mm: f64,
    /// `E-SYNTH-SCHEM-004`: an explicitly drawn net whose span exceeds
    /// this (mm) without net-label truncation is flagged.
    pub long_net_max_mm: f64,
    /// `E-SYNTH-SCHEM-005`: a node with *more* than this many wire
    /// lines meeting at it is flagged.
    pub max_junction_degree: usize,
    /// `E-SYNTH-SCHEM-009`: a rendered net name longer than this many
    /// characters is flagged.
    pub max_net_label_len: usize,
}

impl Default for SchemErcConfig {
    fn default() -> Self {
        Self {
            max_crossings: DEFAULT_MAX_CROSSINGS,
            decoupling_max_mm: DEFAULT_DECOUPLING_MAX_MM,
            long_net_max_mm: DEFAULT_LONG_NET_MAX_MM,
            max_junction_degree: DEFAULT_MAX_JUNCTION_DEGREE,
            max_net_label_len: DEFAULT_MAX_NET_LABEL_LEN,
        }
    }
}

/// Run every aesthetic ERC rule over `layout`/`board` and return the
/// violations, using the §7.7.7 default thresholds.
///
/// Order is deterministic and rule-stable: 001 → 002 → … → 010.
pub fn check(layout: &Layout, board: &Board) -> Vec<Diagnostic> {
    check_with_config(layout, board, SchemErcConfig::default())
}

/// Run every aesthetic ERC rule over `layout`/`board` with explicit
/// thresholds. See [`check`].
pub fn check_with_config(
    layout: &Layout,
    board: &Board,
    config: SchemErcConfig,
) -> Vec<Diagnostic> {
    let mut violations = Vec::new();
    violations.extend(check_inverted_power(layout));
    violations.extend(check_wire_crossings(layout, config.max_crossings));
    violations.extend(check_decoupling_distance(
        board,
        layout,
        config.decoupling_max_mm,
    ));
    violations.extend(check_long_nets(layout, config.long_net_max_mm));
    violations.extend(check_junction_fanout(layout, config.max_junction_degree));
    violations.extend(check_cross_net_junctions(layout));
    violations.extend(check_page_overflow(layout));
    violations.extend(check_net_label_case(layout));
    violations.extend(check_net_label_length(layout, config.max_net_label_len));
    violations.extend(check_ambiguous_power_rails(layout));
    violations
}

// ----- E-SYNTH-SCHEM-001: inverted power symbol ------------------------------

/// Resolve which side of the body a 2-pin part's pin lands on at a
/// given rotation.
///
/// Mirrors the mapping documented on `Rotation` and enforced by
/// `synth_layout`'s `rotate_two_pin_with_power_flags`: at `Zero` pin 0
/// is left and pin 1 right; `Ninety` (CW) moves pin 0 to bottom and
/// pin 1 to top; `TwoSeventy` (CCW) moves pin 0 to top and pin 1 to
/// bottom; `OneEighty` swaps left/right.
///
/// Returns `None` for parts with more than two pins, whose sides are
/// determined by `classify_ic_pin` rather than rotation. The inversion
/// rule only applies to the rotation-driven orientation of 2-pin parts
/// (decoupling caps, pull-ups, ESD diodes), so that is the only case
/// we resolve here.
fn two_pin_flag_side(rotation: Rotation, pin: PinId) -> Option<PinSide> {
    let is_p0 = match pin.0 {
        0 => true,
        1 => false,
        _ => return None,
    };
    Some(match rotation {
        Rotation::Zero => {
            if is_p0 {
                PinSide::Left
            } else {
                PinSide::Right
            }
        }
        Rotation::Ninety => {
            if is_p0 {
                PinSide::Bottom
            } else {
                PinSide::Top
            }
        }
        Rotation::OneEighty => {
            if is_p0 {
                PinSide::Right
            } else {
                PinSide::Left
            }
        }
        Rotation::TwoSeventy => {
            if is_p0 {
                PinSide::Top
            } else {
                PinSide::Bottom
            }
        }
    })
}

/// `E-SYNTH-SCHEM-001`: a power-flag symbol pointing against its
/// conventional direction — a GND flag whose pin faces up, or a VCC
/// flag whose pin faces down.
///
/// The symbol grows outward from the pin terminal in the pin's facing
/// direction, so "VCC pointing up" means the VCC pin sits on the Top
/// of the body and "GND pointing down" means the GND pin sits on the
/// Bottom. A pin on the opposing axis is the inverted-symbol case the
/// rule guards against. Horizontal (Left/Right) flags are neither up
/// nor down and are left alone.
fn check_inverted_power(layout: &Layout) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for flag in &layout.power_flags {
        let Some(placement) = layout.placement(flag.component) else {
            continue;
        };
        let Some(side) = two_pin_flag_side(placement.rotation, flag.pin) else {
            continue;
        };
        let inverted = match flag.kind {
            PowerFlagKind::Vcc => side == PinSide::Bottom,
            PowerFlagKind::Gnd => side == PinSide::Top,
        };
        if !inverted {
            continue;
        }
        let direction = match side {
            PinSide::Bottom => "down",
            PinSide::Top => "up",
            PinSide::Left | PinSide::Right => "sideways",
        };
        let expected = match flag.kind {
            PowerFlagKind::Vcc => "up",
            PowerFlagKind::Gnd => "down",
        };
        out.push(
            DiagnosticBuilder::new(
                "E-SYNTH-SCHEM-001",
                Severity::Warning,
                "inverted power symbol",
            )
            .message(format!(
                "{label} power flag on component {component} pin {pin} points {direction}; \
                 a {label} symbol should point {expected}",
                label = flag.label,
                component = flag.component.0,
                pin = flag.pin.0,
            ))
            .entity(EntityRef::Pin {
                component: flag.component.0.to_string(),
                pin: flag.pin.0.to_string(),
            })
            .expected(format!(
                "{label} power symbol pointing {expected}",
                label = flag.label
            ))
            .found(format!("points {direction}"))
            .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-001")
            .build(),
        );
    }
    out
}

// ----- E-SYNTH-SCHEM-002: excessive wire-crossing density --------------------

/// Coordinates are grid-snapped millimetres which can carry ~1e-13 mm
/// of float noise on a conceptually-exact value. This tolerance is far
/// below the 2.54 mm grid pitch, so it cannot misclassify two distinct
/// grid lines as one (mirrors `synth_layout::score::COORD_EPSILON_MM`).
const COORD_EPSILON_MM: f64 = 1e-6;

/// Whether two axis-aligned segments properly cross — intersect at a
/// point interior to both, not merely touch at a shared endpoint or
/// run collinear. `WirePath` segments are always orthogonal.
///
/// Re-implemented here (rather than reusing
/// `synth_layout::score::segments_cross`, which is `pub(crate)` to that
/// crate) so the rule engine stays self-contained.
fn segments_cross(a1: (f64, f64), a2: (f64, f64), b1: (f64, f64), b2: (f64, f64)) -> bool {
    let a_horizontal = (a1.1 - a2.1).abs() < COORD_EPSILON_MM;
    let b_horizontal = (b1.1 - b2.1).abs() < COORD_EPSILON_MM;
    match (a_horizontal, b_horizontal) {
        (true, false) => crosses_h_v(a1, a2, b1, b2),
        (false, true) => crosses_h_v(b1, b2, a1, a2),
        _ => false,
    }
}

/// `h1`-`h2` is horizontal, `v1`-`v2` vertical. True if they cross at
/// a point interior to both segments.
fn crosses_h_v(h1: (f64, f64), h2: (f64, f64), v1: (f64, f64), v2: (f64, f64)) -> bool {
    let y_h = h1.1;
    let (x_h_min, x_h_max) = (h1.0.min(h2.0), h1.0.max(h2.0));
    let x_v = v1.0;
    let (y_v_min, y_v_max) = (v1.1.min(v2.1), v1.1.max(v2.1));
    x_v > x_h_min && x_v < x_h_max && y_h > y_v_min && y_h < y_v_max
}

/// Count distinct interior segment-segment crossings between wires on
/// different nets (shared endpoints / same-net junctions don't count).
fn count_crossings(layout: &Layout) -> usize {
    type Seg = (NetId, (f64, f64), (f64, f64));
    let mut segments: Vec<Seg> = Vec::new();
    for wire in &layout.wires {
        for pair in wire.points.windows(2) {
            segments.push((wire.net, pair[0], pair[1]));
        }
    }
    let mut count = 0usize;
    for i in 0..segments.len() {
        for j in (i + 1)..segments.len() {
            let (net_a, a1, a2) = segments[i];
            let (net_b, b1, b2) = segments[j];
            if net_a == net_b {
                continue;
            }
            if segments_cross(a1, a2, b1, b2) {
                count += 1;
            }
        }
    }
    count
}

/// `E-SYNTH-SCHEM-002`: more than `max_crossings` different-net wire
/// crossings on the single sheet (plan §7.7.7: "> 5 crossings").
fn check_wire_crossings(layout: &Layout, max_crossings: usize) -> Vec<Diagnostic> {
    let crossings = count_crossings(layout);
    if crossings <= max_crossings {
        return Vec::new();
    }
    vec![DiagnosticBuilder::new(
        "E-SYNTH-SCHEM-002",
        Severity::Warning,
        "excessive wire-crossing density",
    )
    .message(format!(
        "sheet has {crossings} different-net wire crossings, exceeding the budget of \
             {max_crossings}"
    ))
    .expected(format!("at most {max_crossings} crossings"))
    .found(format!("{crossings} crossings"))
    .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-002")
    .build()]
}

// ----- E-SYNTH-SCHEM-003: decoupling capacitor separation --------------------

/// `E-SYNTH-SCHEM-003`: a decoupling capacitor connected to one of an
/// IC's `required_decoupling` power nets sits further than
/// `max_mm` (schematic distance) from the IC.
///
/// Schematic distance is measured from the capacitor's placement
/// centre to the IC's placement centre — a deterministic proxy for the
/// "distance from the target IC power pin" the plan names, computed
/// without pulling in KiCad symbol-pin geometry. Cap-to-IC pairs are
/// deduplicated so a cap shared across multiple required nets only
/// yields one diagnostic.
fn check_decoupling_distance(board: &Board, layout: &Layout, max_mm: f64) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut seen: HashSet<(ComponentId, ComponentId)> = HashSet::new();

    for ic in &board.components {
        let Some(part) = ic.part.as_ref() else {
            continue;
        };
        if part.required_decoupling.is_empty() {
            continue;
        }
        let Some(ic_center) = layout.placement(ic.id).map(|p| p.center_mm) else {
            continue;
        };

        for req in &part.required_decoupling {
            for net in &board.nets {
                if net.name != req.net || !net.endpoints.iter().any(|e| e.component == ic.id) {
                    continue;
                }
                for ep in &net.endpoints {
                    if ep.component == ic.id {
                        continue;
                    }
                    let Some(cap) = board.component(ep.component) else {
                        continue;
                    };
                    if cap.part.as_ref().is_none_or(|p| p.kind != "capacitor") {
                        continue;
                    }
                    if !seen.insert((ic.id, ep.component)) {
                        continue;
                    }
                    let Some(cap_center) = layout.placement(ep.component).map(|p| p.center_mm)
                    else {
                        continue;
                    };
                    let dist = ((ic_center.0 - cap_center.0).powi(2)
                        + (ic_center.1 - cap_center.1).powi(2))
                    .sqrt();
                    if dist <= max_mm {
                        continue;
                    }
                    out.push(
                        DiagnosticBuilder::new(
                            "E-SYNTH-SCHEM-003",
                            Severity::Warning,
                            "decoupling capacitor separation",
                        )
                        .message(format!(
                            "decoupling capacitor {cap} is {dist:.1} mm from its target IC {ic} \
                             (net \"{net_name}\"), exceeding the {max_mm} mm limit",
                            cap = cap.refdes,
                            ic = ic.refdes,
                            net_name = req.net,
                        ))
                        .entity(EntityRef::Component {
                            id: ic.refdes.clone(),
                        })
                        .expected(format!("decoupling capacitor within {max_mm} mm of the IC"))
                        .found(format!("{dist:.1} mm separation"))
                        .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-003")
                        .build(),
                    );
                }
            }
        }
    }
    out
}

// ----- E-SYNTH-SCHEM-004: long explicit net ----------------------------------

/// `E-SYNTH-SCHEM-004`: a net that is *explicitly drawn* as a wire
/// (rather than truncated to net labels) spans more than `max_mm`.
///
/// A net's span is the larger of its wire bounding-box width and
/// height. Nets truncated to labels are excluded: they carry no wires
/// (the `labeled_net_ids` guard is a belt-and-braces filter) and are
/// exactly the "net-label truncation" the rule wants to reward.
///
/// Nets are aggregated across all their `WirePath`s (max span wins) and
/// emitted in ascending net-id order for deterministic output.
fn check_long_nets(layout: &Layout, max_mm: f64) -> Vec<Diagnostic> {
    let labeled: HashSet<NetId> = layout.labeled_net_ids();
    let mut spans: HashMap<NetId, f64> = HashMap::new();

    for wire in &layout.wires {
        if labeled.contains(&wire.net) {
            continue;
        }
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        for &(x, y) in &wire.points {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        let span = (max_x - min_x).max(max_y - min_y);
        let entry = spans.entry(wire.net).or_insert(0.0);
        *entry = entry.max(span);
    }

    let mut offenders: Vec<(NetId, f64)> = spans
        .into_iter()
        .filter(|&(_, span)| span > max_mm)
        .collect();
    offenders.sort_by_key(|(net, _)| net.0);

    let mut out = Vec::with_capacity(offenders.len());
    for (net, span) in offenders {
        out.push(
            DiagnosticBuilder::new(
                "E-SYNTH-SCHEM-004",
                Severity::Warning,
                "long explicit net without net-label truncation",
            )
            .message(format!(
                "net {net} spans {span:.1} mm as a drawn wire, exceeding the {max_mm} mm limit; \
                 truncate it to net labels instead",
                net = net.0,
            ))
            .entity(EntityRef::Net {
                name: format!("net_{}", net.0),
            })
            .expected(format!(
                "net span at most {max_mm} mm (or a net-label break)"
            ))
            .found(format!("{span:.1} mm"))
            .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-004")
            .build(),
        );
    }
    out
}

// ----- E-SYNTH-SCHEM-005: junction fan-out -----------------------------------

/// Count how many same-net wire *lines* meet at quantized point `p`
/// across all wires — the same per-net vertex-counting fold
/// (`synth_layout::route::junctions_from_wires`) uses to decide where
/// dots belong, so a dot emitted by the router is expected to carry
/// exactly the degree recorded here. Foreign nets passing through the
/// point don't inflate the degree (they are rule 006's problem, not
/// this rule's).
fn junction_degree(wires: &[WirePath], p: (i64, i64)) -> usize {
    let mut per_net: HashMap<NetId, usize> = HashMap::new();
    for wire in wires {
        for pair in wire.points.windows(2) {
            for &(x, y) in [&pair[0], &pair[1]] {
                let q = ((x * 100.0).round() as i64, (y * 100.0).round() as i64);
                if q == p {
                    *per_net.entry(wire.net).or_insert(0) += 1;
                }
            }
        }
    }
    per_net.values().copied().max().unwrap_or(0)
}

/// `E-SYNTH-SCHEM-005`: a node carrying more than `max_degree` wire
/// lines (Sierra Circuits: "It is always a good practice to have only
/// 3 lines connected to a node"). A 4-way node is unreadable — split
/// it into two staggered T junctions or truncate the net to labels.
fn check_junction_fanout(layout: &Layout, max_degree: usize) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for &(jx, jy) in &layout.junctions {
        let p = ((jx * 100.0).round() as i64, (jy * 100.0).round() as i64);
        let degree = junction_degree(&layout.wires, p);
        if degree <= max_degree {
            continue;
        }
        out.push(
            DiagnosticBuilder::new(
                "E-SYNTH-SCHEM-005",
                Severity::Warning,
                "excessive junction fan-out",
            )
            .message(format!(
                "node at ({jx}, {jy}) mm carries {degree} wire lines, exceeding the \
                     {max_degree}-line limit; split the node or truncate the net to labels"
            ))
            .expected(format!("at most {max_degree} lines per node"))
            .found(format!("{degree} lines"))
            .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-005")
            .build(),
        );
    }
    out
}

// ----- E-SYNTH-SCHEM-006: cross-net junction ---------------------------------

/// Whether quantized point `p` lies on wire `w`'s geometry — as a
/// vertex or anywhere along one of its orthogonal segments.
fn point_touches_wire(p: (i64, i64), wire: &WirePath) -> bool {
    for pair in wire.points.windows(2) {
        let a = (
            (pair[0].0 * 100.0).round() as i64,
            (pair[0].1 * 100.0).round() as i64,
        );
        let b = (
            (pair[1].0 * 100.0).round() as i64,
            (pair[1].1 * 100.0).round() as i64,
        );
        if p == a || p == b {
            return true;
        }
        // Orthogonal segments only: a point lies on the segment when
        // it shares the constant axis and falls within the span of
        // the varying axis.
        if a.0 == b.0 && p.0 == a.0 && p.1 > a.1.min(b.1) && p.1 < a.1.max(b.1) {
            return true;
        }
        if a.1 == b.1 && p.1 == a.1 && p.0 > a.0.min(b.0) && p.0 < a.0.max(b.0) {
            return true;
        }
    }
    false
}

/// `E-SYNTH-SCHEM-006`: a junction dot placed where a *different*
/// net's wire touches the same point.
///
/// Sierra Circuits: "Lines that intersect with each other are not
/// connected unless there is a node present at the point of
/// intersection" — inverse corollary: where a node *is* present,
/// everything touching it is connected. The canonical router scopes
/// junction detection per net so foreign wires crossing a same-net
/// junction get no dot; if a dot ever lands on foreign geometry
/// anyway, KiCad reads it as a designed-in short between two
/// unrelated nets.
fn check_cross_net_junctions(layout: &Layout) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for &(jx, jy) in &layout.junctions {
        let p = ((jx * 100.0).round() as i64, (jy * 100.0).round() as i64);
        // The net that owns the dot: whichever net has a vertex here
        // (the router only emits dots at same-net ≥3-way meetings).
        let owner = layout
            .wires
            .iter()
            .find(|w| {
                w.points
                    .iter()
                    .any(|&(x, y)| ((x * 100.0).round() as i64, (y * 100.0).round() as i64) == p)
            })
            .map(|w| w.net);
        let Some(owner) = owner else {
            continue; // dot with no wire at all — junk geometry, not a cross-net short
        };
        let foreign = layout
            .wires
            .iter()
            .any(|w| w.net != owner && point_touches_wire(p, w));
        if !foreign {
            continue;
        }
        out.push(
            DiagnosticBuilder::new(
                "E-SYNTH-SCHEM-006",
                Severity::Warning,
                "junction dot on a different net's wire",
            )
            .message(format!(
                "junction at ({jx}, {jy}) mm sits on a wire of a different net — \
                     the dot merges them into a short; move one net's route off the node"
            ))
            .expected("junction dots only on their own net's geometry")
            .found("dot touches a foreign net's wire")
            .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-006")
            .build(),
        );
    }
    out
}

// ----- E-SYNTH-SCHEM-007: page overflow --------------------------------------

/// Slack (mm) tolerated past the sheet edge before flagging — far
/// below any grid pitch, only absorbing float noise.
const PAGE_OVERFLOW_EPSILON_MM: f64 = 0.01;

/// `E-SYNTH-SCHEM-007`: content placed outside the selected sheet.
///
/// Sierra Circuits: "Select the [page] size based on the size of your
/// circuit design." The placer escalates A4 → A3 → A2 from the content
/// bounding box; past A2 it stops growing (multi-sheet is §21.2
/// future work) and silently overflows. This rule makes that overflow
/// visible: any component placement or wire point beyond the sheet's
/// landscape dimensions is flagged.
fn check_page_overflow(layout: &Layout) -> Vec<Diagnostic> {
    let (w, h) = layout.sheet_size.dims_mm();
    let max_x = w + PAGE_OVERFLOW_EPSILON_MM;
    let max_y = h + PAGE_OVERFLOW_EPSILON_MM;

    let mut worst: Option<(f64, f64)> = None;
    for placement in &layout.components {
        let (x, y) = placement.center_mm;
        if (x > max_x || y > max_y) && worst.is_none_or(|(wx, wy)| x + y > wx + wy) {
            worst = Some((x, y));
        }
    }
    for wire in &layout.wires {
        for &(x, y) in &wire.points {
            if (x > max_x || y > max_y) && worst.is_none_or(|(wx, wy)| x + y > wx + wy) {
                worst = Some((x, y));
            }
        }
    }
    let Some((x, y)) = worst else {
        return Vec::new();
    };
    vec![DiagnosticBuilder::new(
        "E-SYNTH-SCHEM-007",
        Severity::Warning,
        "content overflows the selected sheet size",
    )
    .message(format!(
        "content reaches ({x:.1}, {y:.1}) mm but the sheet is only {w:.0} × {h:.0} mm; \
                 shrink the layout or move to a larger sheet",
    ))
    .expected(format!("all content within {w:.0} × {h:.0} mm"))
    .found(format!("content at ({x:.1}, {y:.1}) mm"))
    .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-007")
    .build()]
}

// ----- E-SYNTH-SCHEM-008: net name case --------------------------------------

/// Every rendered net name on the sheet — signal-net labels plus
/// power-rail flag labels — deduplicated and sorted so output is
/// deterministic.
fn rendered_net_names(layout: &Layout) -> Vec<&str> {
    let mut names: Vec<&str> = layout
        .net_labels
        .iter()
        .map(|l| l.label.as_str())
        .chain(layout.power_flags.iter().map(|f| f.label.as_str()))
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// `E-SYNTH-SCHEM-008`: a rendered net name containing a lowercase
/// letter (StackExchange #28251: "Use all caps for net names and pin
/// names"). KiCad net labels are case-sensitive, so `sda` and `SDA`
/// are different nets — mixed-case declared names are a shorted-net
/// trap as well as a readability wart.
fn check_net_label_case(layout: &Layout) -> Vec<Diagnostic> {
    rendered_net_names(layout)
        .into_iter()
        .filter(|name| name.chars().any(char::is_lowercase))
        .map(|name| {
            DiagnosticBuilder::new(
                "E-SYNTH-SCHEM-008",
                Severity::Warning,
                "net name not uppercase",
            )
            .message(format!(
                "net name \"{name}\" is not uppercase; rename it (e.g. to \"{upper}\") — \
                     same-named labels connect, and KiCad matches them case-sensitively",
                upper = name.to_ascii_uppercase(),
            ))
            .entity(EntityRef::Net {
                name: name.to_string(),
            })
            .expected("UPPERCASE net name")
            .found(name.to_string())
            .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-008")
            .build()
        })
        .collect()
}

// ----- E-SYNTH-SCHEM-009: net name length ------------------------------------

/// `E-SYNTH-SCHEM-009`: a rendered net name longer than `max_len`
/// characters (StackExchange #28251: "Keep names reasonably short —
/// lots of long names are clutter, which then decreases clarity").
fn check_net_label_length(layout: &Layout, max_len: usize) -> Vec<Diagnostic> {
    rendered_net_names(layout)
        .into_iter()
        .filter(|name| name.chars().count() > max_len)
        .map(|name| {
            let len = name.chars().count();
            DiagnosticBuilder::new("E-SYNTH-SCHEM-009", Severity::Warning, "net name too long")
                .message(format!(
                    "net name \"{name}\" is {len} characters, exceeding the {max_len}-character \
                     budget; shorten it to the information a reader needs (e.g. \"CLK\", \"3V3\")"
                ))
                .entity(EntityRef::Net {
                    name: name.to_string(),
                })
                .expected(format!("at most {max_len} characters"))
                .found(format!("{len} characters"))
                .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-009")
                .build()
        })
        .collect()
}

// ----- E-SYNTH-SCHEM-010: ambiguous power rail -------------------------------

/// `E-SYNTH-SCHEM-010`: a power rail flying the generic `VCC`/`VDD`/
/// `VPP` symbol (StackExchange #28251: "DO NOT USE VDD or VCC as they
/// are ambiguous. Make a new symbol for explicitly declaring what the
/// voltage is"). One diagnostic per offending rail label, deduped —
/// every endpoint of the same rail shares the label.
fn check_ambiguous_power_rails(layout: &Layout) -> Vec<Diagnostic> {
    rendered_net_names(layout)
        .into_iter()
        .filter(|name| AMBIGUOUS_RAIL_LABELS.contains(name))
        .map(|name| {
            DiagnosticBuilder::new(
                "E-SYNTH-SCHEM-010",
                Severity::Warning,
                "ambiguous power rail name",
            )
            .message(format!(
                "power rail uses the ambiguous name \"{name}\"; declare the explicit \
                     voltage instead (e.g. \"+3V3\", \"+5V\") so the rail symbol states \
                     what it drives"
            ))
            .entity(EntityRef::Net {
                name: name.to_string(),
            })
            .expected("explicit voltage rail name (e.g. +3V3)")
            .found(name.to_string())
            .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-010")
            .build()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_diagnostics::Span;
    use synth_ir::{Component, Net, NetEndpoint};
    use synth_layout::{ComponentPlacement, NetLabel, PowerFlag, SheetSize, WirePath};

    fn layout(
        components: Vec<ComponentPlacement>,
        wires: Vec<WirePath>,
        power_flags: Vec<PowerFlag>,
        net_labels: Vec<NetLabel>,
    ) -> Layout {
        Layout {
            components,
            wires,
            junctions: Vec::new(),
            power_flags,
            net_labels,
            annotations: Vec::new(),
            sheet_size: SheetSize::A4,
        }
    }

    fn flag(component: ComponentId, pin: PinId, kind: PowerFlagKind, label: &str) -> PowerFlag {
        PowerFlag {
            net: NetId(0),
            component,
            pin,
            kind,
            label: label.to_string(),
        }
    }

    fn wire(net: NetId, points: Vec<(f64, f64)>) -> WirePath {
        WirePath {
            net,
            points,
            junctions: Vec::new(),
        }
    }

    fn placement(id: ComponentId, x: f64, y: f64, rotation: Rotation) -> ComponentPlacement {
        ComponentPlacement {
            id,
            center_mm: (x, y),
            rotation,
        }
    }

    // ----- E-SYNTH-SCHEM-001 ----------------------------------------------

    #[test]
    fn gnd_flag_pointing_up_is_inverted() {
        // GND on pin 1, rotated Ninety puts pin 1 on Top → GND points up.
        let layout = layout(
            vec![placement(ComponentId(0), 10.0, 10.0, Rotation::Ninety)],
            Vec::new(),
            vec![flag(ComponentId(0), PinId(1), PowerFlagKind::Gnd, "GND")],
            Vec::new(),
        );
        let violations = check_inverted_power(&layout);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-001");
        assert_eq!(violations[0].severity, Severity::Warning);
    }

    #[test]
    fn vcc_flag_pointing_down_is_inverted() {
        // VCC on pin 0, rotated Ninety puts pin 0 on Bottom → VCC points down.
        let layout = layout(
            vec![placement(ComponentId(0), 10.0, 10.0, Rotation::Ninety)],
            Vec::new(),
            vec![flag(ComponentId(0), PinId(0), PowerFlagKind::Vcc, "VCC")],
            Vec::new(),
        );
        let violations = check_inverted_power(&layout);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-001");
    }

    #[test]
    fn correct_orientation_is_silent() {
        // GND on pin 0, TwoSeventy puts pin 0 on Top? No — TwoSeventy
        // puts pin 0 on Top, so a GND there would point up. Use the
        // conventional cases: GND down (pin on Bottom) and VCC up (pin
        // on Top).
        let layout = layout(
            vec![placement(ComponentId(0), 10.0, 10.0, Rotation::TwoSeventy)],
            Vec::new(),
            vec![
                flag(ComponentId(0), PinId(1), PowerFlagKind::Gnd, "GND"),
                flag(ComponentId(0), PinId(0), PowerFlagKind::Vcc, "VCC"),
            ],
            Vec::new(),
        );
        // TwoSeventy: pin 0 → Top (VCC up ✓), pin 1 → Bottom (GND down ✓).
        assert!(check_inverted_power(&layout).is_empty());
    }

    // ----- E-SYNTH-SCHEM-002 ----------------------------------------------

    #[test]
    fn clean_plus_crossing_counts_one_but_stays_silent() {
        let layout = layout(
            Vec::new(),
            vec![
                wire(NetId(0), vec![(0.0, 5.0), (10.0, 5.0)]),
                wire(NetId(1), vec![(5.0, 0.0), (5.0, 10.0)]),
            ],
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(count_crossings(&layout), 1);
        assert!(check_wire_crossings(&layout, 5).is_empty());
    }

    #[test]
    fn six_crossings_are_flagged() {
        // Six distinct vertical wires, each crossing one horizontal wire
        // that they are not on the same net as: build 6 vertical
        // segments all crossing one horizontal segment. To get 6
        // different-net crossings, pair each vertical (its own net)
        // against the horizontal (its own net) — that yields 6.
        let mut wires = vec![wire(NetId(0), vec![(0.0, 5.0), (100.0, 5.0)])];
        for i in 1u32..=6 {
            let x = f64::from(i) * 5.0;
            wires.push(wire(NetId(i), vec![(x, 0.0), (x, 10.0)]));
        }
        let layout = layout(Vec::new(), wires, Vec::new(), Vec::new());
        assert_eq!(count_crossings(&layout), 6);
        let violations = check_wire_crossings(&layout, 5);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-002");
    }

    #[test]
    fn shared_endpoint_is_a_junction_not_a_crossing() {
        let layout = layout(
            Vec::new(),
            vec![
                wire(NetId(0), vec![(0.0, 0.0), (5.0, 0.0)]),
                wire(NetId(1), vec![(5.0, 0.0), (5.0, 10.0)]),
            ],
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(count_crossings(&layout), 0);
        assert!(check_wire_crossings(&layout, 5).is_empty());
    }

    // ----- E-SYNTH-SCHEM-003 ----------------------------------------------

    fn part_with_decoupling() -> synth_registry::Part {
        synth_registry::Part {
            id: synth_registry::PartId("ic".to_string()),
            kind: "mcu".to_string(),
            description: None,
            version: 0,
            lifecycle: synth_registry::Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: None,
            lcsc_pn: None,
            pins: Vec::new(),
            required_decoupling: vec![synth_registry::RequiredDecoupling {
                net: "VCC".to_string(),
                value: "100n".to_string(),
                count: 1,
                max_distance_mm: None,
            }],
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
            provenance: None,
        }
    }

    fn cap_part() -> synth_registry::Part {
        let mut part = part_with_decoupling();
        part.id = synth_registry::PartId("cap".to_string());
        part.kind = "capacitor".to_string();
        part.required_decoupling = Vec::new();
        part
    }

    #[test]
    fn decoupling_cap_far_from_ic_is_flagged() {
        // IC at (10, 10); a VCC net connecting IC pin 0 and cap C1 at
        // (100, 10) — 90 mm apart, beyond the 15 mm default.
        let board = Board {
            name: "b".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components: vec![
                Component {
                    id: ComponentId(0),
                    refdes: "U1".to_string(),
                    kind: "mcu".to_string(),
                    part: Some(part_with_decoupling()),
                    value: None,
                    placement_hint: None,
                    group: None,
                    source_span: Span::new(0, 0),
                },
                Component {
                    id: ComponentId(1),
                    refdes: "C1".to_string(),
                    kind: "capacitor".to_string(),
                    part: Some(cap_part()),
                    value: None,
                    placement_hint: None,
                    group: None,
                    source_span: Span::new(0, 0),
                },
            ],
            nets: vec![Net {
                id: NetId(0),
                name: "VCC".to_string(),
                endpoints: vec![
                    NetEndpoint {
                        component: ComponentId(0),
                        pin: PinId(0),
                        source_span: Span::new(0, 0),
                    },
                    NetEndpoint {
                        component: ComponentId(1),
                        pin: PinId(0),
                        source_span: Span::new(0, 0),
                    },
                ],
            }],
            diff_pairs: Vec::new(),
            keepouts: Vec::new(),
            source_span: Span::new(0, 0),
        };
        let layout = layout(
            vec![
                placement(ComponentId(0), 10.0, 10.0, Rotation::Zero),
                placement(ComponentId(1), 100.0, 10.0, Rotation::Zero),
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let violations = check_decoupling_distance(&board, &layout, 15.0);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-003");
        assert!(violations[0].message.as_deref().unwrap().contains("C1"));
    }

    #[test]
    fn decoupling_cap_near_ic_is_silent() {
        let board = Board {
            name: "b".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components: vec![
                Component {
                    id: ComponentId(0),
                    refdes: "U1".to_string(),
                    kind: "mcu".to_string(),
                    part: Some(part_with_decoupling()),
                    value: None,
                    placement_hint: None,
                    group: None,
                    source_span: Span::new(0, 0),
                },
                Component {
                    id: ComponentId(1),
                    refdes: "C1".to_string(),
                    kind: "capacitor".to_string(),
                    part: Some(cap_part()),
                    value: None,
                    placement_hint: None,
                    group: None,
                    source_span: Span::new(0, 0),
                },
            ],
            nets: vec![Net {
                id: NetId(0),
                name: "VCC".to_string(),
                endpoints: vec![
                    NetEndpoint {
                        component: ComponentId(0),
                        pin: PinId(0),
                        source_span: Span::new(0, 0),
                    },
                    NetEndpoint {
                        component: ComponentId(1),
                        pin: PinId(0),
                        source_span: Span::new(0, 0),
                    },
                ],
            }],
            diff_pairs: Vec::new(),
            keepouts: Vec::new(),
            source_span: Span::new(0, 0),
        };
        let layout = layout(
            vec![
                placement(ComponentId(0), 10.0, 10.0, Rotation::Zero),
                placement(ComponentId(1), 15.0, 10.0, Rotation::Zero),
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert!(check_decoupling_distance(&board, &layout, 15.0).is_empty());
    }

    // ----- E-SYNTH-SCHEM-004 ----------------------------------------------

    #[test]
    fn long_explicit_net_is_flagged() {
        let layout = layout(
            Vec::new(),
            vec![wire(NetId(0), vec![(0.0, 0.0), (150.0, 0.0)])],
            Vec::new(),
            Vec::new(),
        );
        let violations = check_long_nets(&layout, 100.0);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-004");
    }

    #[test]
    fn short_explicit_net_is_silent() {
        let layout = layout(
            Vec::new(),
            vec![wire(NetId(0), vec![(0.0, 0.0), (50.0, 0.0)])],
            Vec::new(),
            Vec::new(),
        );
        assert!(check_long_nets(&layout, 100.0).is_empty());
    }

    #[test]
    fn truncated_to_label_net_is_not_flagged() {
        // A 150 mm net that was truncated to a net label carries no
        // wires, so it must not fire. Belt-and-braces: also include a
        // labeled-net id in `net_labels`.
        let net_label = NetLabel {
            net: NetId(0),
            component: ComponentId(0),
            pin: PinId(0),
            label: "LONG_SIG".to_string(),
        };
        let layout = layout(Vec::new(), Vec::new(), Vec::new(), vec![net_label]);
        assert!(check_long_nets(&layout, 100.0).is_empty());
    }

    // ----- E-SYNTH-SCHEM-005 ----------------------------------------------

    #[test]
    fn three_line_t_junction_is_silent() {
        // Horizontal run with a vertex at (5, 5) (2 segment-ends) plus
        // a stub ending there (1) — exactly 3 same-net lines.
        let layout = layout(
            Vec::new(),
            vec![
                wire(NetId(0), vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]),
                wire(NetId(0), vec![(5.0, 0.0), (5.0, 5.0)]),
            ],
            Vec::new(),
            Vec::new(),
        );
        let mut l = layout;
        l.junctions = vec![(5.0, 5.0)];
        assert_eq!(
            junction_degree(&l.wires, ((5.0 * 100.0) as i64, (5.0 * 100.0) as i64)),
            3
        );
        assert!(check_junction_fanout(&l, 3).is_empty());
    }

    #[test]
    fn four_line_node_is_flagged() {
        // A plus-cross of two same-net polylines: (5,5) is a vertex of
        // both → 4 segment-ends on one net.
        let layout = layout(
            Vec::new(),
            vec![
                wire(NetId(0), vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]),
                wire(NetId(0), vec![(5.0, 0.0), (5.0, 5.0), (5.0, 10.0)]),
            ],
            Vec::new(),
            Vec::new(),
        );
        let mut l = layout;
        l.junctions = vec![(5.0, 5.0)];
        let violations = check_junction_fanout(&l, 3);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-005");
    }

    #[test]
    fn foreign_net_passing_through_does_not_inflate_degree() {
        // Same T junction as the silent case, but a *different* net's
        // wire has a vertex at the same point. Degree stays 3 (per-net
        // fold); the foreign touch is 006's job.
        let layout = layout(
            Vec::new(),
            vec![
                wire(NetId(0), vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]),
                wire(NetId(0), vec![(5.0, 0.0), (5.0, 5.0)]),
                wire(NetId(1), vec![(5.0, 5.0), (5.0, 10.0)]),
            ],
            Vec::new(),
            Vec::new(),
        );
        let mut l = layout;
        l.junctions = vec![(5.0, 5.0)];
        assert!(check_junction_fanout(&l, 3).is_empty());
    }

    // ----- E-SYNTH-SCHEM-006 ----------------------------------------------

    #[test]
    fn junction_dot_on_foreign_wire_is_flagged() {
        // Net 0 owns a T junction at (5, 5); net 1's vertical wire
        // passes straight through that point.
        let layout = layout(
            Vec::new(),
            vec![
                wire(NetId(0), vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]),
                wire(NetId(0), vec![(5.0, 0.0), (5.0, 5.0)]),
                wire(NetId(1), vec![(5.0, 0.0), (5.0, 10.0)]),
            ],
            Vec::new(),
            Vec::new(),
        );
        let mut l = layout;
        l.junctions = vec![(5.0, 5.0)];
        let violations = check_cross_net_junctions(&l);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-006");
    }

    #[test]
    fn junction_dot_on_own_net_only_is_silent() {
        // Same 3-line T junction but the other net's wire stops short
        // of the node — a legal perpendicular crossing elsewhere.
        let layout = layout(
            Vec::new(),
            vec![
                wire(NetId(0), vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]),
                wire(NetId(0), vec![(5.0, 0.0), (5.0, 5.0)]),
                wire(NetId(1), vec![(20.0, 0.0), (20.0, 10.0)]),
            ],
            Vec::new(),
            Vec::new(),
        );
        let mut l = layout;
        l.junctions = vec![(5.0, 5.0)];
        assert!(check_cross_net_junctions(&l).is_empty());
    }

    // ----- E-SYNTH-SCHEM-007 ----------------------------------------------

    #[test]
    fn placement_beyond_sheet_edge_is_flagged() {
        // A4 landscape is 297 × 210 mm; a placement at x=320 overflows.
        let layout = layout(
            vec![placement(ComponentId(0), 320.0, 50.0, Rotation::Zero)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let violations = check_page_overflow(&layout);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-007");
    }

    #[test]
    fn wire_beyond_sheet_edge_is_flagged() {
        let layout = layout(
            Vec::new(),
            vec![wire(NetId(0), vec![(50.0, 100.0), (50.0, 250.0)])],
            Vec::new(),
            Vec::new(),
        );
        let violations = check_page_overflow(&layout);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-007");
    }

    #[test]
    fn content_within_sheet_is_silent() {
        let layout = layout(
            vec![
                placement(ComponentId(0), 10.0, 10.0, Rotation::Zero),
                placement(ComponentId(1), 280.0, 200.0, Rotation::Zero),
            ],
            vec![wire(NetId(0), vec![(10.0, 10.0), (280.0, 10.0)])],
            Vec::new(),
            Vec::new(),
        );
        assert!(check_page_overflow(&layout).is_empty());
    }

    #[test]
    fn larger_sheet_fits_the_same_content() {
        // The overflow case from placement_beyond_sheet_edge_is_flagged,
        // re-selected onto A3 (420 × 297 mm) — the fix the rule wants.
        let mut layout = layout(
            vec![placement(ComponentId(0), 320.0, 50.0, Rotation::Zero)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        layout.sheet_size = SheetSize::A3;
        assert!(check_page_overflow(&layout).is_empty());
    }

    // ----- E-SYNTH-SCHEM-008 ----------------------------------------------

    fn net_label(net: NetId, component: ComponentId, label: &str) -> NetLabel {
        NetLabel {
            net,
            component,
            pin: PinId(0),
            label: label.to_string(),
        }
    }

    #[test]
    fn lowercase_net_label_is_flagged() {
        let layout = layout(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![net_label(NetId(0), ComponentId(0), "i2c_data")],
        );
        let violations = check_net_label_case(&layout);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-008");
        assert!(violations[0]
            .message
            .as_deref()
            .unwrap()
            .contains("I2C_DATA"));
    }

    #[test]
    fn uppercase_net_labels_are_silent() {
        let layout = layout(
            Vec::new(),
            Vec::new(),
            vec![flag(ComponentId(0), PinId(0), PowerFlagKind::Vcc, "+3V3")],
            vec![net_label(NetId(0), ComponentId(0), "D+")],
        );
        assert!(check_net_label_case(&layout).is_empty());
    }

    // ----- E-SYNTH-SCHEM-009 ----------------------------------------------

    #[test]
    fn overly_long_net_label_is_flagged() {
        let layout = layout(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![net_label(NetId(0), ComponentId(0), "8MHZ_CLOCK_TO_MY_PIC")],
        );
        let violations = check_net_label_length(&layout, 16);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-009");
    }

    #[test]
    fn short_net_label_is_silent() {
        let layout = layout(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![net_label(NetId(0), ComponentId(0), "SCL")],
        );
        assert!(check_net_label_length(&layout, 16).is_empty());
    }

    // ----- E-SYNTH-SCHEM-010 ----------------------------------------------

    #[test]
    fn ambiguous_vcc_rail_is_flagged() {
        let layout = layout(
            Vec::new(),
            Vec::new(),
            vec![
                flag(ComponentId(0), PinId(0), PowerFlagKind::Vcc, "VCC"),
                flag(ComponentId(1), PinId(0), PowerFlagKind::Vcc, "VCC"),
            ],
            Vec::new(),
        );
        let violations = check_ambiguous_power_rails(&layout);
        // Two flags, one rail label — deduped to one diagnostic.
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-010");
    }

    #[test]
    fn explicit_voltage_rail_is_silent() {
        let layout = layout(
            Vec::new(),
            Vec::new(),
            vec![
                flag(ComponentId(0), PinId(0), PowerFlagKind::Vcc, "+3V3"),
                flag(ComponentId(1), PinId(0), PowerFlagKind::Gnd, "GND"),
            ],
            Vec::new(),
        );
        assert!(check_ambiguous_power_rails(&layout).is_empty());
    }

    // ----- aggregate entry point ------------------------------------------

    #[test]
    fn aggregate_check_returns_warnings_in_rule_order() {
        let board = Board {
            name: "b".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components: vec![
                Component {
                    id: ComponentId(0),
                    refdes: "U1".to_string(),
                    kind: "mcu".to_string(),
                    part: Some(part_with_decoupling()),
                    value: None,
                    placement_hint: None,
                    group: None,
                    source_span: Span::new(0, 0),
                },
                Component {
                    id: ComponentId(1),
                    refdes: "C1".to_string(),
                    kind: "capacitor".to_string(),
                    part: Some(cap_part()),
                    value: None,
                    placement_hint: None,
                    group: None,
                    source_span: Span::new(0, 0),
                },
                Component {
                    id: ComponentId(2),
                    refdes: "U2".to_string(),
                    kind: "mcu".to_string(),
                    part: Some(cap_part()),
                    value: None,
                    placement_hint: None,
                    group: None,
                    source_span: Span::new(0, 0),
                },
            ],
            nets: vec![Net {
                id: NetId(0),
                name: "VCC".to_string(),
                endpoints: vec![
                    NetEndpoint {
                        component: ComponentId(0),
                        pin: PinId(0),
                        source_span: Span::new(0, 0),
                    },
                    NetEndpoint {
                        component: ComponentId(1),
                        pin: PinId(0),
                        source_span: Span::new(0, 0),
                    },
                ],
            }],
            diff_pairs: Vec::new(),
            keepouts: Vec::new(),
            source_span: Span::new(0, 0),
        };
        let mut layout = layout(
            vec![
                placement(ComponentId(0), 10.0, 10.0, Rotation::Ninety),
                // Cap 90 mm away → 003.
                placement(ComponentId(1), 100.0, 10.0, Rotation::Zero),
                // Beyond the A4 sheet edge (297 mm) → 007.
                placement(ComponentId(2), 350.0, 50.0, Rotation::Zero),
            ],
            // 6 crossings (6 verticals × 1 horizontal) + a 150 mm net.
            vec![
                wire(NetId(0), vec![(0.0, 5.0), (150.0, 5.0)]), // long → 004
                wire(NetId(1), vec![(5.0, 0.0), (5.0, 10.0)]),
                wire(NetId(2), vec![(10.0, 0.0), (10.0, 10.0)]),
                wire(NetId(3), vec![(15.0, 0.0), (15.0, 10.0)]),
                wire(NetId(4), vec![(20.0, 0.0), (20.0, 10.0)]),
                wire(NetId(5), vec![(25.0, 0.0), (25.0, 10.0)]),
                wire(NetId(6), vec![(30.0, 0.0), (30.0, 10.0)]),
                // Same-net 4-way node at (5, 5) → 005: the 3-point
                // polyline contributes 2 segment-ends, the stub and
                // the tail one each.
                wire(NetId(1), vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]),
                wire(NetId(1), vec![(5.0, 10.0), (5.0, 5.0)]),
                wire(NetId(1), vec![(5.0, 5.0), (5.0, 15.0)]),
                // A different net's wire passing straight through the
                // same node → 006.
                wire(NetId(7), vec![(5.0, 0.0), (5.0, 10.0)]),
            ],
            // GND on pin 0 at Ninety points down (fine, no 001). The
            // ambiguous VCC rail label → 010.
            vec![
                flag(ComponentId(0), PinId(0), PowerFlagKind::Gnd, "GND"),
                flag(ComponentId(2), PinId(0), PowerFlagKind::Vcc, "VCC"),
            ],
            // Lowercase + overly long labels → 008 + 009. The label
            // rides its own net id so the 150 mm NetId(0) wire above
            // stays unlabeled and keeps firing 004.
            vec![
                net_label(NetId(8), ComponentId(0), "i2c_data_bus_extension"),
                net_label(NetId(9), ComponentId(1), "SCL"),
            ],
        );
        layout.junctions = vec![(5.0, 5.0)];
        let violations = check(&layout, &board);
        let codes: Vec<&str> = violations.iter().map(|d| d.code.as_str()).collect();
        // Expect 002 (6 crossings), 003 (90 mm cap), 004 (150 mm net),
        // 005 (4-line node), 006 (foreign wire through the node),
        // 007 (placement past the sheet edge), 008 (lowercase name),
        // 009 (long name), 010 (ambiguous VCC rail).
        assert_eq!(
            codes,
            vec![
                "E-SYNTH-SCHEM-002",
                "E-SYNTH-SCHEM-003",
                "E-SYNTH-SCHEM-004",
                "E-SYNTH-SCHEM-005",
                "E-SYNTH-SCHEM-006",
                "E-SYNTH-SCHEM-007",
                "E-SYNTH-SCHEM-008",
                "E-SYNTH-SCHEM-009",
                "E-SYNTH-SCHEM-010",
            ]
        );
    }
}
