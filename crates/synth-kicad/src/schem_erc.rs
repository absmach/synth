// SPDX-License-Identifier: Apache-2.0

//! In-house schematic aesthetic ERC rule engine (`E-SYNTH-SCHEM-001..012`).
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
//! never block compilation), plus the schematic-quality plan's
//! legibility rules (011–012, and the `E-SYNTH-VALUE-001` error owned
//! by `synth-validate`). Implemented here, mapped onto the
//! `E-SYNTH-SCHEM-001..012` code range:
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
//!
//! The schematic-quality plan's legibility rules continue the range:
//!
//! * **E-SYNTH-SCHEM-011** — Overlapping text runs: two free-text
//!   runs (captions, note lines, legend lines) still overlapping
//!   after the layout `resolve_text_overlaps` pass.
//! * **E-SYNTH-SCHEM-012** — Sheet fill ratio below threshold: content
//!   covers less than [`SchemErcConfig::min_sheet_fill_ratio`] of the
//!   chosen sheet (info).
//! * **E-SYNTH-SCHEM-013** — Group regions overlap, or a component
//!   from one group falls inside another group's box: the region
//!   placement (Phase C1) failed to keep groups contiguous.
//! * **E-SYNTH-SCHEM-014** — Auto-named net (`net_N`) rendered on the
//!   sheet as a label, suggesting a name from its endpoint pin.
//! * **E-SYNTH-SCHEM-015** — A declared `group` carries no `notes`
//!   block (info): the reference sheet's regions each explain intent.

use std::collections::{HashMap, HashSet};

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, EntityRef, Severity};
use synth_ir::{Board, ComponentId, NetId, PinId};
use synth_layout::{Layout, PinSide, PowerFlagKind, Rotation, WirePath};

/// Default wire-crossing budget on a single sheet before
/// `E-SYNTH-SCHEM-002` fires. Plan §7.7.7: "> 5 crossings".
const DEFAULT_MAX_CROSSINGS: usize = 5;
/// Default empty space (mm) between a decoupling capacitor's symbol
/// body and its target IC's before `E-SYNTH-SCHEM-003` fires.
///
/// Plan §7.7.7 says "> 15 mm", but measured centre to centre — which
/// is incoherent across part sizes: 15 mm centres allowed a ~5 mm gap
/// beside a small AMS1117 and was unsatisfiable beside an LQFP-48,
/// whose half-diagonal alone exceeds it. The metric is now the gap
/// between bodies, so the budget is restated for it: 25 mm is about
/// ten grid steps, the distance at which a reader stops seeing the
/// cap as belonging to the IC. The two numbers are not comparable.
const DEFAULT_DECOUPLING_MAX_MM: f64 = 25.0;
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
/// Default sheet-fill ratio (content bbox area over sheet area)
/// below which `E-SYNTH-SCHEM-012` fires. Plan §A5: 45 %.
const DEFAULT_MIN_SHEET_FILL_RATIO: f64 = 0.45;
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
    /// `E-SYNTH-SCHEM-012`: content covering less than this fraction
    /// of the sheet area is flagged (info).
    pub min_sheet_fill_ratio: f64,
}

impl Default for SchemErcConfig {
    fn default() -> Self {
        Self {
            max_crossings: DEFAULT_MAX_CROSSINGS,
            decoupling_max_mm: DEFAULT_DECOUPLING_MAX_MM,
            long_net_max_mm: DEFAULT_LONG_NET_MAX_MM,
            max_junction_degree: DEFAULT_MAX_JUNCTION_DEGREE,
            max_net_label_len: DEFAULT_MAX_NET_LABEL_LEN,
            min_sheet_fill_ratio: DEFAULT_MIN_SHEET_FILL_RATIO,
        }
    }
}

/// Fill in each diagnostic's source location from the entity it names.
///
/// The aesthetic rules run over a *layout*, which has no source spans
/// — so every finding printed as `(?)` and a sheet with seventeen
/// identical decoupling warnings gave the reader no way to tell which
/// capacitor each meant. Every rule already attaches the component or
/// net it is about; this resolves that back to the declaration's span
/// so the CLI can print `file:start-end` like any other diagnostic.
///
/// Diagnostics naming no locatable entity (page overflow, wire-crossing
/// density — properties of the sheet, not of one part) are left
/// without a location, which is honest.
pub fn attach_locations(diagnostics: &mut [Diagnostic], board: &Board, file: &str) {
    for diagnostic in diagnostics {
        if diagnostic.location.is_some() {
            continue;
        }
        let span = diagnostic
            .entities
            .iter()
            .chain(diagnostic.peer_entities.iter())
            .find_map(|entity| match entity {
                EntityRef::Component { id } => board
                    .components
                    .iter()
                    .find(|c| c.refdes == *id)
                    .map(|c| c.source_span),
                EntityRef::Pin { component, .. } => board
                    .components
                    .iter()
                    .find(|c| c.refdes == *component)
                    .map(|c| c.source_span),
                EntityRef::Net { name } => board
                    .nets
                    .iter()
                    .find(|n| n.name == *name)
                    .and_then(|n| n.endpoints.first())
                    .and_then(|ep| board.component(ep.component))
                    .map(|c| c.source_span),
                _ => None,
            });
        if let Some(span) = span {
            diagnostic.location = Some(synth_diagnostics::Location::from_span(
                file.to_string(),
                span,
            ));
        }
    }
}

/// Run every aesthetic ERC rule over `layout`/`board` and return the
/// violations, using the §7.7.7 default thresholds.
///
/// Order is deterministic and rule-stable: 001 → 002 → … → 012.
pub fn check(layout: &Layout, board: &Board) -> Vec<Diagnostic> {
    check_with_config(layout, board, SchemErcConfig::default())
}

/// Run every aesthetic ERC rule over every sheet of a (§P26 split)
/// board and return the concatenated violations, sheet by sheet in
/// export order (root first).
///
/// Single-sheet input behaves exactly like [`check`]. On multi-sheet
/// input each diagnostic's title gains a `[sheet]` prefix so findings
/// attribute to the page that carries them — the underlying rule
/// thresholds and ordering are unchanged per sheet. Running the
/// single-sheet [`check`] on a split board's global layout instead
/// would false-positive page overflow and cross-sheet wire crossings
/// that no longer exist once sheets separate.
pub fn check_sheets(
    board: &Board,
    sheets: &[synth_layout::sheets::SheetLayout],
) -> Vec<Diagnostic> {
    if sheets.len() == 1 {
        return check(&sheets[0].layout, board);
    }
    let mut out = Vec::new();
    for sheet in sheets {
        let name = sheet.name.as_deref().unwrap_or("root");
        for mut diagnostic in check(&sheet.layout, board) {
            diagnostic.title = format!("[{name}] {}", diagnostic.title);
            out.push(diagnostic);
        }
    }
    out
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
    violations.extend(check_ambiguous_power_rails(layout, board));
    violations.extend(check_text_overlaps(layout));
    violations.extend(check_sheet_fill(board, layout, config.min_sheet_fill_ratio));
    violations.extend(check_auto_named_nets(board, layout));
    violations.extend(check_group_regions(board, layout));
    violations.extend(check_group_notes(board));
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

/// Empty space (mm) between two placed components' symbol bodies,
/// zero when they overlap. `None` when either is unplaced or has no
/// resolved part.
fn body_gap_mm(
    board: &Board,
    layout: &Layout,
    a: synth_ir::ComponentId,
    b: synth_ir::ComponentId,
) -> Option<f64> {
    let half = |id: synth_ir::ComponentId| -> Option<((f64, f64), (f64, f64))> {
        let place = layout.placement(id)?;
        let part = board.component(id)?.part.as_ref()?;
        let (w, h) = synth_layout::body_size_for_part(part);
        // A rotated symbol presents its other axis to the gap.
        let (w, h) = match place.rotation {
            synth_layout::Rotation::Zero | synth_layout::Rotation::OneEighty => (w, h),
            synth_layout::Rotation::Ninety | synth_layout::Rotation::TwoSeventy => (h, w),
        };
        Some((place.center_mm, (w / 2.0, h / 2.0)))
    };
    let ((ax, ay), (ahw, ahh)) = half(a)?;
    let ((bx, by), (bhw, bhh)) = half(b)?;
    let dx = ((ax - bx).abs() - (ahw + bhw)).max(0.0);
    let dy = ((ay - by).abs() - (ahh + bhh)).max(0.0);
    Some(dx.hypot(dy))
}

/// Whether `ic` is the closest decoupling-capable part to `cap` among
/// everything sharing `rail`.
///
/// Ownership mirrors how the layouter assigns an orphan rail cap to a
/// cluster (`patterns::ic_block::attach_orphan_rail_caps`), so the
/// rule judges the same pairing the placer built.
fn is_nearest_ic_on_rail(
    board: &Board,
    layout: &Layout,
    rail: &synth_ir::Net,
    cap: ComponentId,
    ic: ComponentId,
) -> bool {
    let Some(own) = body_gap_mm(board, layout, ic, cap) else {
        return true;
    };
    for ep in &rail.endpoints {
        if ep.component == cap || ep.component == ic {
            continue;
        }
        let Some(other) = board.component(ep.component) else {
            continue;
        };
        let Some(part) = other.part.as_ref() else {
            continue;
        };
        // Only parts that declare decoupling compete for ownership;
        // another cap or a pull-up on the rail is not a candidate.
        if part.required_decoupling.is_empty() {
            continue;
        }
        if body_gap_mm(board, layout, ep.component, cap).is_some_and(|d| d < own) {
            return false;
        }
    }
    true
}

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
///
/// Schematic-quality plan Phase A2: the rail net is resolved through
/// the IC's power *pin*, not by net-name equality. Merged rails keep
/// one global net whose name rarely equals the manifest key, and
/// power-flag-mediated nets are nets all the same — matching by name
/// silently passed every shared-rail decoupling cap.
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
            // The rail net carrying the IC's power pin. Resolved
            // through the pin (not by net-name equality): merged rails
            // keep one global net whose name rarely equals the manifest
            // key, and power-flag-mediated nets are nets all the same.
            // When the part declares no such pin (synthetic boards),
            // fall back to the legacy name match so the rule still fires.
            // u32 cast is bounded: the index comes from the part's own pin list.
            let rail_nets: Vec<&synth_ir::Net> =
                match part.pins.iter().position(|p| p.name == req.net) {
                    Some(pin_idx) => board
                        .nets_containing(ic.id, PinId(pin_idx as u32))
                        .map(|(_, net)| net)
                        .next()
                        .into_iter()
                        .collect(),
                    None => board
                        .nets
                        .iter()
                        .filter(|net| {
                            net.name == req.net
                                && net.endpoints.iter().any(|e| e.component == ic.id)
                        })
                        .collect(),
                };
            for net in rail_nets {
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
                    // A shared rail reaches every IC on the board, so
                    // a naive sweep reported each cap against all of
                    // them — one misplaced cap became N findings, and
                    // no placement could satisfy them all at once. A
                    // cap decouples exactly one part: the one it sits
                    // nearest. Only that pair is judged; for the rest
                    // this cap is simply not their decoupling.
                    if !is_nearest_ic_on_rail(board, layout, net, ep.component, ic.id) {
                        continue;
                    }
                    let Some(cap_center) = layout.placement(ep.component).map(|p| p.center_mm)
                    else {
                        continue;
                    };
                    // Gap between the two symbol bodies, not centre to
                    // centre: a stock LQFP-48 symbol is ~25 x 55 mm, so
                    // its half-diagonal alone exceeds the budget and a
                    // centre measure could never be satisfied however
                    // tightly the cap is placed. The budget is empty
                    // space between the parts, which is what "too far"
                    // means to a reader.
                    let dist =
                        body_gap_mm(board, layout, ic.id, ep.component).unwrap_or_else(|| {
                            ((ic_center.0 - cap_center.0).powi(2)
                                + (ic_center.1 - cap_center.1).powi(2))
                            .sqrt()
                        });
                    // Epsilon: both sides are 2.54 mm-grid sums, so a
                    // gap that lands exactly on the budget must pass
                    // rather than fail on the last float bit.
                    if dist <= max_mm + 1e-6 {
                        continue;
                    }
                    out.push(
                        DiagnosticBuilder::new(
                            "E-SYNTH-SCHEM-003",
                            Severity::Warning,
                            "decoupling capacitor separation",
                        )
                        .message(format!(
                            "decoupling capacitor {} is {dist:.1} mm from its target IC {} \
                         (rail net \"{}\"), exceeding the {max_mm} mm limit",
                            cap.describe(),
                            ic.describe(),
                            net.name,
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
/// bounding box; past A2 it stops growing and the §P26 split takes
/// over (§MULTI-SHEET). This rule makes any residual overflow
/// visible: a component placement or wire point beyond the sheet's
/// landscape dimensions is flagged. Multi-sheet boards run this per
/// sheet (`check_sheets`), so a split board no longer trips it.
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
///
/// A rail carrying a declared voltage (`power "VCC" 3.3v`) is exempt:
/// the voltage declaration is exactly the explicitness the rule asks
/// for, even though the name itself is generic.
fn check_ambiguous_power_rails(layout: &Layout, board: &Board) -> Vec<Diagnostic> {
    rendered_net_names(layout)
        .into_iter()
        .filter(|name| AMBIGUOUS_RAIL_LABELS.contains(name))
        .filter(|name| {
            !board
                .nets
                .iter()
                .any(|net| net.name == *name && net.voltage.is_some())
        })
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

// ----- E-SYNTH-SCHEM-011: overlapping text runs ------------------------------

/// Slack (mm) below which two text runs count as merely touching —
/// far below any glyph size, only absorbing float noise.
const TEXT_OVERLAP_EPSILON_MM: f64 = 1e-6;

/// Axis-aligned box of a free-text run, mirroring
/// `synth_layout`'s overlap pass: `(x0, y0, x1, y1)` with `y` the
/// baseline anchor and KiCad's ~0.72 em stroke-font advance.
fn annotation_rect(text: &synth_layout::TextAnnotation) -> (f64, f64, f64, f64) {
    let (x, y) = text.at_mm;
    // u32 cast is bounded: annotation strings are far shorter than u32::MAX chars.
    let chars = u32::try_from(text.text.chars().count()).unwrap_or(u32::MAX);
    (
        x,
        y - text.size_mm,
        x + f64::from(chars) * text.size_mm * 0.72,
        y,
    )
}

/// Shorten a run for messages: first `max` chars plus `…` when cut.
fn ellipsize(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    format!("{}…", text.chars().take(max).collect::<String>())
}

/// `E-SYNTH-SCHEM-011`: two free-text runs (captions, note lines,
/// legend lines) still overlapping after the layout
/// `resolve_text_overlaps` pass (schematic-quality plan Phase A4).
/// One diagnostic per overlapping pair, in annotation order — the
/// measurement that keeps the resolve pass honest.
fn check_text_overlaps(layout: &Layout) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (i, a) in layout.annotations.iter().enumerate() {
        let ra = annotation_rect(a);
        for b in layout.annotations.iter().skip(i + 1) {
            let rb = annotation_rect(b);
            let overlaps = ra.0 < rb.2 - TEXT_OVERLAP_EPSILON_MM
                && rb.0 < ra.2 - TEXT_OVERLAP_EPSILON_MM
                && ra.1 < rb.3 - TEXT_OVERLAP_EPSILON_MM
                && rb.1 < ra.3 - TEXT_OVERLAP_EPSILON_MM;
            if !overlaps {
                continue;
            }
            out.push(
                DiagnosticBuilder::new(
                    "E-SYNTH-SCHEM-011",
                    Severity::Warning,
                    "overlapping text runs",
                )
                .message(format!(
                    "text runs \"{}\" and \"{}\" still overlap after the resolve pass; \
                     move one of them or shorten the text",
                    ellipsize(&a.text, 32),
                    ellipsize(&b.text, 32),
                ))
                .expected("disjoint text runs")
                .found(format!(
                    "\"{}\" overlaps \"{}\"",
                    ellipsize(&a.text, 32),
                    ellipsize(&b.text, 32),
                ))
                .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-011")
                .build(),
            );
        }
    }
    out
}

// ----- E-SYNTH-SCHEM-012: sheet fill ratio -----------------------------------
/// `E-SYNTH-SCHEM-012`: the content bounding box covers less than
/// `min_ratio` of the chosen sheet's area (schematic-quality plan
/// Phase A5, defect D6 — content in the top 40 % of an A3 page while
/// the bottom half sits empty). Info, never blocking: a roomy sheet
/// is wasteful, not wrong. Names the smaller standard sheet that
/// would fit when one exists.
fn check_sheet_fill(board: &Board, layout: &Layout, min_ratio: f64) -> Vec<Diagnostic> {
    let Some((min_x, max_x, min_y, max_y)) = synth_layout::content_bounds(board, layout) else {
        return Vec::new();
    };
    let (sheet_w, sheet_h) = layout.sheet_size.dims_mm();
    if sheet_w <= 0.0 || sheet_h <= 0.0 {
        return Vec::new();
    }
    let content_w = (max_x - min_x).max(0.0);
    let content_h = (max_y - min_y).max(0.0);
    let ratio = content_w * content_h / (sheet_w * sheet_h);
    if ratio >= min_ratio {
        return Vec::new();
    }
    let smaller = synth_layout::fit_sheet_size(min_x, max_x, min_y, max_y);
    let (smaller_w, smaller_h) = smaller.dims_mm();
    let smaller_hint = if smaller_w < sheet_w && smaller_h < sheet_h {
        format!(" — content would fit {smaller:?}")
    } else {
        String::new()
    };
    vec![DiagnosticBuilder::new(
        "E-SYNTH-SCHEM-012",
        Severity::Info,
        "sheet fill ratio below threshold",
    )
    .message(format!(
        "content ({content_w:.0} × {content_h:.0} mm) covers only {:.0}% of the \
         {sheet_w:.0} × {sheet_h:.0} mm sheet, below the {:.0}% threshold{smaller_hint}; \
         compact the layout or move to a smaller sheet",
        ratio * 100.0,
        min_ratio * 100.0,
    ))
    .expected(format!(
        "at least {:.0}% of the sheet covered",
        min_ratio * 100.0
    ))
    .found(format!("{:.0}% covered", ratio * 100.0))
    .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-012")
    .build()]
}

// ----- E-SYNTH-SCHEM-013: group regions overlap ------------------------------

/// `E-SYNTH-SCHEM-013`: the region placement (Phase C1) failed to keep
/// groups apart. Two failure modes, both reported as warnings:
///
/// - two group boxes overlap — a caption would title another region's
///   parts;
/// - a component whose declared group differs from a box's group lies
///   inside that box — the regions interleave even if the boxes
///   themselves only touch.
///
/// Boards with no declared groups have one implicit region and can
/// never fire. One diagnostic per offending pair, in box order.
fn check_group_regions(board: &Board, layout: &Layout) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let boxes = &layout.group_boxes;
    // Box-vs-box overlap.
    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            let a = &boxes[i];
            let b = &boxes[j];
            let disjoint = a.max_mm.0 <= b.min_mm.0
                || b.max_mm.0 <= a.min_mm.0
                || a.max_mm.1 <= b.min_mm.1
                || b.max_mm.1 <= a.min_mm.1;
            if disjoint {
                continue;
            }
            out.push(
                DiagnosticBuilder::new(
                    "E-SYNTH-SCHEM-013",
                    Severity::Warning,
                    "group regions overlap",
                )
                .message(format!(
                    "region boxes \"{}\" and \"{}\" overlap; a caption would title \
                     another region's parts — check the region placement",
                    a.group, b.group,
                ))
                .expected("disjoint group regions")
                .found(format!("\"{}\" overlaps \"{}\"", a.group, b.group))
                .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-013")
                .build(),
            );
        }
    }
    // A component outside its own group's box (or inside another's).
    // The component's region is resolved through the layout helper, so
    // the implicit `MOUNTING` region of a mechanical part counts as
    // its own region, exactly as placement treats it.
    for placement in &layout.components {
        let Some(component) = board.component(placement.id) else {
            continue;
        };
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let (bw, bh) = synth_layout::body_size_for_part(part);
        let (cx, cy) = placement.center_mm;
        let (x0, x1) = (cx - bw / 2.0, cx + bw / 2.0);
        let (y0, y1) = (cy - bh / 2.0, cy + bh / 2.0);
        for box_ in boxes {
            // Inside this box?
            let inside = x0 >= box_.min_mm.0
                && x1 <= box_.max_mm.0
                && y0 >= box_.min_mm.1
                && y1 <= box_.max_mm.1;
            if !inside {
                continue;
            }
            // Its own region, or a nested declaration (a component may
            // only belong to one group; a mismatch is the interleave).
            if synth_layout::effective_group(board, placement.id) == Some(box_.group.as_str()) {
                break;
            }
            out.push(
                DiagnosticBuilder::new(
                    "E-SYNTH-SCHEM-013",
                    Severity::Warning,
                    "component outside its group region",
                )
                .message(format!(
                    "component {} sits inside region \"{}\" but declares a different \
                     group; the regions are not contiguous",
                    component.describe(),
                    box_.group,
                ))
                .entity(EntityRef::Component {
                    id: component.refdes.clone(),
                })
                .expected("every component inside its own group's region")
                .found(format!("inside \"{}\"", box_.group))
                .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-013")
                .build(),
            );
            break;
        }
    }
    out
}

// ----- E-SYNTH-SCHEM-015: group has no notes ---------------------------------

/// `E-SYNTH-SCHEM-015`: a declared `group` carries no `notes` block
/// (schematic-quality plan §2 mechanism 3, info).
///
/// The reference sheet's every region explains its intent in prose
/// (*"VIN = 3.3 – 5.5 V, EN tied to VIN (always on)"*) — text the
/// netlist cannot carry. Advisory, never blocking: a group without
/// notes is under-documented, not wrong. One diagnostic per group, in
/// declaration order.
fn check_group_notes(board: &Board) -> Vec<Diagnostic> {
    board
        .groups
        .iter()
        .filter(|group| {
            !board
                .notes
                .iter()
                .any(|note| note.group.as_deref() == Some(group.name.as_str()))
        })
        .map(|group| {
            DiagnosticBuilder::new("E-SYNTH-SCHEM-015", Severity::Info, "group has no notes")
                .message(format!(
                    "group \"{}\" carries no `notes` block; add one so the region explains \
                 intent the netlist cannot (e.g. voltage range, always-on tie-off)",
                    group.name,
                ))
                .expected("a `notes` block inside the group")
                .found("no notes")
                .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-015")
                .build()
        })
        .collect()
}

/// Whether a rendered net name is an auto-generated placeholder
/// (`net_3`, `NET_3`) rather than a human name.
fn is_auto_net_label(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    lower
        .strip_prefix("net_")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

// ----- E-SYNTH-SCHEM-014: auto-named net rendered on the sheet ---------------

/// `E-SYNTH-SCHEM-014`: an auto-named net (`net_N`) reaching the sheet
/// as a rendered label (schematic-quality plan Phase D3).
///
/// A placeholder name carries no intent — the reference sheet names
/// every net it draws (`SCL`, `SDA`, `VIN`). The diagnostic suggests a
/// name derived from the net's first endpoint pin, which is exactly
/// the vocabulary `synth_layout::pick_net_label` uses, so an author
/// can name the net at its source.
///
/// Nets drawn only as wires are *not* rendered by name and never fire;
/// power rails get derived labels (`GND`, `VCC`), not placeholders.
/// One diagnostic per offending net, in label order.
fn check_auto_named_nets(board: &Board, layout: &Layout) -> Vec<Diagnostic> {
    let mut seen: HashSet<NetId> = HashSet::new();
    let mut out = Vec::new();
    for label in &layout.net_labels {
        if !is_auto_net_label(&label.label) || !seen.insert(label.net) {
            continue;
        }
        let net = board.net(label.net);
        // Suggest the first endpoint pin's name, uppercased — the same
        // token `pick_net_label` would fall back to.
        let suggestion = net
            .and_then(|n| n.endpoints.first())
            .and_then(|ep| board.pin(ep.component, ep.pin))
            .map(|p| p.name.to_ascii_uppercase());
        let message = match &suggestion {
            Some(name) => format!(
                "net rendered as \"{}\" is auto-named; give it a real name at its \
                 source (`net \"{name}\" {{ … }}` or `connect … as \"{name}\"`)",
                label.label,
            ),
            None => format!(
                "net rendered as \"{}\" is auto-named; give it a real name at its source",
                label.label,
            ),
        };
        out.push(
            DiagnosticBuilder::new(
                "E-SYNTH-SCHEM-014",
                Severity::Warning,
                "auto-named net rendered on the sheet",
            )
            .message(message)
            .entity(EntityRef::Net {
                name: label.label.clone(),
            })
            .expected("a declared net name (e.g. SDA, VIN)")
            .found(label.label.clone())
            .explanation_url("synth.docs/diagnostics/E-SYNTH-SCHEM-014")
            .build(),
        );
    }
    out
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
            hierarchical_labels: Vec::new(),
            group_boxes: Vec::new(),
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

    /// A one-pin part, so the SCHEM-014 suggestion can name a pin.
    fn single_pin_part(pin_name: &str) -> synth_registry::Part {
        let mut part = cap_part();
        part.pins = vec![synth_registry::Pin {
            name: pin_name.to_string(),
            number: synth_registry::PinNumber("1".to_string()),
            electrical_type: synth_registry::ElectricalType::Bidirectional,
            capabilities: Vec::new(),
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }];
        part
    }

    #[test]
    fn decoupling_cap_far_from_ic_is_flagged() {
        // IC at (10, 10); a VCC net connecting IC pin 0 and cap C1 at
        // (100, 10) — 90 mm apart, beyond the 15 mm default.
        let board = Board {
            groups: Vec::new(),
            legends: false,
            name: "b".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: vec![
                Component {
                    id: ComponentId(0),
                    refdes: "U1".to_string(),
                    kind: "mcu".to_string(),
                    part: Some(part_with_decoupling()),
                    value: None,
                    dnp: false,
                    properties: std::collections::BTreeMap::new(),
                    placement_hint: None,
                    group: None,
                    sheet: None,
                    source_span: Span::new(0, 0),
                },
                Component {
                    id: ComponentId(1),
                    refdes: "C1".to_string(),
                    kind: "capacitor".to_string(),
                    part: Some(cap_part()),
                    value: None,
                    dnp: false,
                    properties: std::collections::BTreeMap::new(),
                    placement_hint: None,
                    group: None,
                    sheet: None,
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
                netclass: None,
                voltage: None,
            }],
            diff_pairs: Vec::new(),
            notes: vec![],
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
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
            groups: Vec::new(),
            legends: false,
            name: "b".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: vec![
                Component {
                    id: ComponentId(0),
                    refdes: "U1".to_string(),
                    kind: "mcu".to_string(),
                    part: Some(part_with_decoupling()),
                    value: None,
                    dnp: false,
                    properties: std::collections::BTreeMap::new(),
                    placement_hint: None,
                    group: None,
                    sheet: None,
                    source_span: Span::new(0, 0),
                },
                Component {
                    id: ComponentId(1),
                    refdes: "C1".to_string(),
                    kind: "capacitor".to_string(),
                    part: Some(cap_part()),
                    value: None,
                    dnp: false,
                    properties: std::collections::BTreeMap::new(),
                    placement_hint: None,
                    group: None,
                    sheet: None,
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
                netclass: None,
                voltage: None,
            }],
            diff_pairs: Vec::new(),
            notes: vec![],
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
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

    fn board_with_nets(nets: Vec<Net>) -> Board {
        Board {
            groups: Vec::new(),
            legends: false,
            name: "b".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: Vec::new(),
            nets,
            diff_pairs: Vec::new(),
            notes: vec![],
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    fn undeclared_net(name: &str) -> Net {
        Net {
            id: NetId(0),
            name: name.to_string(),
            endpoints: Vec::new(),
            netclass: None,
            voltage: None,
        }
    }

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
        let board = board_with_nets(vec![undeclared_net("VCC")]);
        let violations = check_ambiguous_power_rails(&layout, &board);
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
        let board = board_with_nets(Vec::new());
        assert!(check_ambiguous_power_rails(&layout, &board).is_empty());
    }

    #[test]
    fn declared_voltage_vcc_is_silent() {
        // `power "VCC" 3.3v` states the voltage explicitly — the name
        // alone is no longer ambiguous.
        let layout = layout(
            Vec::new(),
            Vec::new(),
            vec![flag(ComponentId(0), PinId(0), PowerFlagKind::Vcc, "VCC")],
            Vec::new(),
        );
        let board = board_with_nets(vec![Net {
            id: NetId(0),
            name: "VCC".to_string(),
            endpoints: Vec::new(),
            netclass: None,
            voltage: Some(synth_ir::Voltage::from_v(3.3)),
        }]);
        assert!(check_ambiguous_power_rails(&layout, &board).is_empty());
    }

    // ----- E-SYNTH-SCHEM-011 ----------------------------------------------

    fn annotation(text: &str, size_mm: f64, x: f64, y: f64) -> synth_layout::TextAnnotation {
        synth_layout::TextAnnotation {
            text: text.to_string(),
            at_mm: (x, y),
            size_mm,
            kind: synth_layout::TextKind::NoteLine,
        }
    }

    #[test]
    fn overlapping_annotations_are_flagged() {
        let mut layout = layout(Vec::new(), Vec::new(), Vec::new(), Vec::new());
        layout.annotations = vec![
            annotation("J1 pinout", 2.0, 20.0, 100.0),
            annotation("J1 pinout", 2.0, 20.0, 100.0),
        ];
        let violations = check_text_overlaps(&layout);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-011");
        assert_eq!(violations[0].severity, Severity::Warning);
    }

    #[test]
    fn disjoint_annotations_are_silent() {
        let mut layout = layout(Vec::new(), Vec::new(), Vec::new(), Vec::new());
        layout.annotations = vec![
            annotation("Input", 2.0, 20.0, 100.0),
            annotation("Output", 2.0, 20.0, 120.0),
        ];
        assert!(check_text_overlaps(&layout).is_empty());
    }

    // ----- E-SYNTH-SCHEM-012 ----------------------------------------------

    #[test]
    fn roomy_sheet_is_info() {
        let board = board_with_nets(Vec::new());
        let mut layout = layout(
            vec![placement(ComponentId(0), 30.0, 30.0, Rotation::Zero)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        layout.sheet_size = SheetSize::A3;
        let violations = check_sheet_fill(&board, &layout, 0.45);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-012");
        assert_eq!(violations[0].severity, Severity::Info);
    }

    #[test]
    fn full_sheet_is_silent() {
        let board = board_with_nets(Vec::new());
        // Content spanning most of A4: well above the 45 % bar.
        let layout = layout(
            vec![
                placement(ComponentId(0), 30.0, 30.0, Rotation::Zero),
                placement(ComponentId(1), 260.0, 180.0, Rotation::Zero),
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert!(check_sheet_fill(&board, &layout, 0.45).is_empty());
    }

    // ----- E-SYNTH-SCHEM-014 ----------------------------------------------

    #[test]
    fn auto_named_rendered_net_is_flagged_with_a_suggestion() {
        let mut board = board_with_nets(Vec::new());
        board.components.push(Component {
            id: ComponentId(0),
            refdes: "U1".to_string(),
            kind: "mcu".to_string(),
            part: Some(single_pin_part("sda")),
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        });
        board.nets.push(Net {
            id: NetId(0),
            name: "net_3".to_string(),
            endpoints: vec![NetEndpoint {
                component: ComponentId(0),
                pin: PinId(0),
                source_span: Span::new(0, 0),
            }],
            netclass: None,
            voltage: None,
        });
        let layout = layout(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![net_label(NetId(0), ComponentId(0), "NET_3")],
        );
        let violations = check_auto_named_nets(&board, &layout);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-014");
        assert!(
            violations[0].message.as_deref().unwrap().contains("SDA"),
            "suggestion should name the endpoint pin: {:?}",
            violations[0].message
        );
    }

    #[test]
    fn named_net_is_silent_for_014() {
        let board = board_with_nets(Vec::new());
        let layout = layout(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![net_label(NetId(0), ComponentId(0), "SDA")],
        );
        assert!(check_auto_named_nets(&board, &layout).is_empty());
    }

    // ----- E-SYNTH-SCHEM-013 / 015 ----------------------------------------

    fn group_box(name: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> synth_layout::GroupBox {
        synth_layout::GroupBox {
            group: name.to_string(),
            min_mm: (x0, y0),
            max_mm: (x1, y1),
            color: [0, 0, 0],
            caption_inside: true,
        }
    }

    #[test]
    fn overlapping_group_boxes_are_flagged() {
        let board = board_with_nets(Vec::new());
        let mut layout = layout(Vec::new(), Vec::new(), Vec::new(), Vec::new());
        layout.group_boxes = vec![
            group_box("A", 0.0, 0.0, 100.0, 100.0),
            group_box("B", 50.0, 50.0, 150.0, 150.0),
        ];
        let violations = check_group_regions(&board, &layout);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-013");
    }

    #[test]
    fn disjoint_group_boxes_are_silent() {
        let board = board_with_nets(Vec::new());
        let mut layout = layout(Vec::new(), Vec::new(), Vec::new(), Vec::new());
        layout.group_boxes = vec![
            group_box("A", 0.0, 0.0, 40.0, 40.0),
            group_box("B", 60.0, 0.0, 100.0, 40.0),
        ];
        assert!(check_group_regions(&board, &layout).is_empty());
    }

    #[test]
    fn foreign_component_inside_a_region_is_flagged() {
        let mut board = board_with_nets(Vec::new());
        // Two grouped components: one in A, one in B.
        for (i, (refdes, group)) in [("R1", "A"), ("R2", "B")].iter().enumerate() {
            board.components.push(Component {
                id: ComponentId(i as u32),
                refdes: refdes.to_string(),
                kind: "resistor".to_string(),
                part: Some(single_pin_part("p1")),
                value: None,
                dnp: false,
                properties: std::collections::BTreeMap::new(),
                placement_hint: None,
                group: Some(group.to_string()),
                sheet: None,
                source_span: Span::new(0, 0),
            });
        }
        let mut layout = layout(
            vec![
                placement(ComponentId(0), 10.0, 10.0, Rotation::Zero),
                // R2 (group B) physically inside A's box.
                placement(ComponentId(1), 12.0, 12.0, Rotation::Zero),
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        layout.group_boxes = vec![group_box("A", 0.0, 0.0, 40.0, 40.0)];
        let violations = check_group_regions(&board, &layout);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-013");
    }

    #[test]
    fn mechanical_component_in_mounting_region_is_silent() {
        // Regression: the implicit MOUNTING region is not a declared
        // `group` on the component, so a naive group comparison fired
        // 013 on every mechanical part. Resolving through the layout's
        // `effective_group` fixes it.
        let mut board = board_with_nets(Vec::new());
        let mut hole = single_pin_part("1");
        hole.kind = "mounting_hole".to_string();
        board.components.push(Component {
            id: ComponentId(0),
            refdes: "H1".to_string(),
            kind: "mounting_hole".to_string(),
            part: Some(hole),
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        });
        let mut layout = layout(
            vec![placement(ComponentId(0), 10.0, 10.0, Rotation::Zero)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        layout.group_boxes = vec![group_box(
            synth_layout::MOUNTING_REGION,
            0.0,
            0.0,
            40.0,
            40.0,
        )];
        assert!(check_group_regions(&board, &layout).is_empty());
    }

    #[test]
    fn group_without_notes_is_info() {
        use synth_ir::Group;
        let mut board = board_with_nets(Vec::new());
        board.groups = vec![Group {
            name: "Power".to_string(),
            title: None,
            color: None,
            region: None,
            source_span: Span::new(0, 0),
        }];
        let violations = check_group_notes(&board);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, "E-SYNTH-SCHEM-015");
        assert_eq!(violations[0].severity, Severity::Info);
        // A matching notes block silences it.
        board.notes.push(synth_ir::Note {
            title: "Power notes".to_string(),
            lines: vec!["VIN = 3.3 - 5.5 V".to_string()],
            group: Some("Power".to_string()),
            sheet: None,
            source_span: Span::new(0, 0),
        });
        assert!(check_group_notes(&board).is_empty());
    }

    // ----- aggregate entry point ------------------------------------------
    #[test]
    fn aggregate_check_returns_warnings_in_rule_order() {
        let board = Board {
            groups: Vec::new(),
            legends: false,
            name: "b".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: vec![
                Component {
                    id: ComponentId(0),
                    refdes: "U1".to_string(),
                    kind: "mcu".to_string(),
                    part: Some(part_with_decoupling()),
                    value: None,
                    dnp: false,
                    properties: std::collections::BTreeMap::new(),
                    placement_hint: None,
                    group: None,
                    sheet: None,
                    source_span: Span::new(0, 0),
                },
                Component {
                    id: ComponentId(1),
                    refdes: "C1".to_string(),
                    kind: "capacitor".to_string(),
                    part: Some(cap_part()),
                    value: None,
                    dnp: false,
                    properties: std::collections::BTreeMap::new(),
                    placement_hint: None,
                    group: None,
                    sheet: None,
                    source_span: Span::new(0, 0),
                },
                Component {
                    id: ComponentId(2),
                    refdes: "U2".to_string(),
                    kind: "mcu".to_string(),
                    part: Some(cap_part()),
                    value: None,
                    dnp: false,
                    properties: std::collections::BTreeMap::new(),
                    placement_hint: None,
                    group: None,
                    sheet: None,
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
                netclass: None,
                voltage: None,
            }],
            diff_pairs: Vec::new(),
            notes: vec![],
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
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
        // 009 (long name), 010 (ambiguous VCC rail), 012 (roomy A4
        // sheet — no annotations, so 011 stays silent).
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
                "E-SYNTH-SCHEM-012",
            ]
        );
    }

    #[test]
    fn check_sheets_delegates_for_single_sheet_and_prefixes_multi() {
        use synth_layout::sheets::SheetLayout;
        // A VCC rail with no explicit voltage name trips SCHEM-010
        // wherever it is rendered — a stable diagnostic to attribute.
        let b = Board {
            groups: Vec::new(),
            legends: false,
            name: "b".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: vec![],
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
                netclass: None,
                voltage: None,
            }],
            diff_pairs: Vec::new(),
            notes: Vec::new(),
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        };
        let l = layout(
            vec![
                placement(ComponentId(0), 10.0, 10.0, Rotation::Zero),
                placement(ComponentId(1), 20.0, 10.0, Rotation::Zero),
            ],
            Vec::new(),
            Vec::new(),
            vec![net_label(NetId(0), ComponentId(0), "VCC")],
        );
        let single = check_sheets(
            &b,
            &[SheetLayout {
                name: None,
                layout: l.clone(),
            }],
        );
        assert_eq!(single.len(), check(&l, &b).len(), "single sheet delegates");

        let multi = check_sheets(
            &b,
            &[
                SheetLayout {
                    name: None,
                    layout: l.clone(),
                },
                SheetLayout {
                    name: Some("Power".to_string()),
                    layout: l,
                },
            ],
        );
        assert!(multi.iter().any(|d| d.title.starts_with("[root] ")));
        assert!(multi.iter().any(|d| d.title.starts_with("[Power] ")));
    }
}
