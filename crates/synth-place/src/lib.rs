// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::match_same_arms
)]

//! Deterministic PCB placement engine.
//!
//! Phase 7 (implementation plan §9). Same compiler-correctness
//! discipline as the earlier phases: byte-deterministic output,
//! integer-nanometer arithmetic, structured failure diagnostics
//! instead of best-effort placements.
//!
//! ## Phased rollout
//!
//! - **Slice 1A — foundation (shipped).** Crate skeleton +
//!   `Placement` IR + trivial deterministic grid algorithm.
//! - **Slice 1B — `.kicad_pcb` emitter (shipped).** Pcbnew opens
//!   the placement.
//! - **Slice 1C — `*.kicad_mod` footprint reader (shipped).**
//!   Courtyard bboxes and pad geometry come from KiCad's bundled
//!   footprint libraries.
//! - **Slice 2 — Stage 1 hard-constraint solver (this slice).**
//!   Per plan §9.2 stage 1: pure constraint satisfaction, no
//!   optimization. Returns *a* legal placement (no courtyard
//!   overlaps, components inside the board outline) or a
//!   structured [`PlaceError`] when none exists.
//! - **Slice 3 — Stage 2 cost-driven refinement.** Bounded-
//!   iteration swap optimisation reducing HPWL + estimated
//!   crossings, preserving all hard constraints.
//!
//! ## Slice 2 algorithm
//!
//! Greedy first-fit on a 1 mm grid. Deterministic by construction:
//!
//! 1. **Compute area budget.** Sum the courtyard area of every
//!    component, multiply by `1 / FILL_RATIO` to leave room for
//!    routing channels, pick the smallest standard sheet (A4/A3/A2)
//!    that fits.
//! 2. **Order components.** By net-graph degree descending (most-
//!    connected first), with `ComponentId` as the deterministic
//!    tie-breaker. Highly-connected anchors land near the centre;
//!    leaf-pull-ups fill the periphery.
//! 3. **Place each component.** Walk grid positions in row-major
//!    order (top-left to bottom-right). Snap the component's
//!    centre to the grid. Compute its courtyard rectangle. Accept
//!    the first position where:
//!    - the courtyard lies entirely inside the board outline,
//!    - the courtyard does not intersect any previously-placed
//!      component's courtyard (boundary-inclusive — touching
//!      courtyards collide, per IPC convention).
//! 4. **Failure.** If no grid cell accommodates a component,
//!    return [`PlaceError::NoLegalPosition`] with the component's
//!    refdes and the area we explored. If the very first
//!    component doesn't fit (board too small), return
//!    [`PlaceError::AreaInsufficient`] with the area numbers.
//!
//! No backtracking yet. First-fit on this ordering is sufficient
//! for the bounded class (≤200 components on a rectangular
//! 4-layer board, no fixed-position constraints, no keepouts).
//! Slice 3 adds the cost-driven refinement; slice 2.x adds full
//! backtracking + connector / RF / fixed-component ordering.

#![forbid(unsafe_code)]
#![allow(
    // i64 arithmetic on grid offsets (positive, bounded by board
    // dimensions << 2^32 nm). The casts are deliberate.
    clippy::cast_possible_wrap,
    // Paired x/y, w/h identifiers are intentionally similar.
    clippy::similar_names,
)]

pub mod advisor;
pub mod board_family;
pub mod cem;
pub mod floorplan;
pub mod modules;
pub mod outline_packer;
pub mod score;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity, Span};
use synth_geometry::{mm_to_nm, Layer, Point, Rect, Rotation};

use synth_ir::{Board, ComponentId, PinId};
use thiserror::Error;

mod routability;
pub use routability::{routability, RoutabilityEstimate};

/// Final position + orientation of a single component on the PCB.
///
/// `center` is the **courtyard-bbox centre** in nanometres — the coordinate
/// space the placer, DRC and exporter reason about. It is not the footprint's
/// physical origin and must not be published as one: for an asymmetric
/// footprint the origin sits at `center - rot(courtyard_offset)`. Convert
/// through [`to_external`] before crossing the process boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComponentPlacement {
    pub id: ComponentId,
    /// Courtyard-bbox centre of the component, in nanometres.
    pub center: Point,
    pub rotation: Rotation,
    pub layer: Layer,
}

/// Complete PCB placement output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub board_outline: Rect,
    pub components: Vec<ComponentPlacement>,
}

/// A [`ComponentPlacement`] in the external coordinate space that agents and
/// the SaaS (`synth-ee`) consume.
///
/// **Stable contract.** `center` is the point KiCad writes as a footprint's
/// `(at x y)` — the footprint's physical origin on the board — in nanometres.
/// This is the coordinate space the exported `.kicad_pcb` is in, so an overlay
/// of the placement JSON on the PCB needs no correction.
///
/// It deliberately differs from [`ComponentPlacement::center`], which is the
/// internal courtyard-bbox centre. Publishing the internal value here would
/// shift every downstream consumer by the footprint's courtyard offset
/// (`center - rot(courtyard_offset)`) for asymmetric footprints such as USB
/// connectors and DIP headers, while the exported PCB places the origin
/// correctly — the two would silently disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalComponent {
    pub id: ComponentId,
    /// Footprint origin (KiCad `at`), in nanometres.
    pub center: Point,
    pub rotation: Rotation,
    pub layer: Layer,
}

/// The external placement payload: board outline plus every component in the
/// footprint-origin coordinate space ([`ExternalComponent::center`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalPlacement {
    pub board_outline: Rect,
    pub components: Vec<ExternalComponent>,
}

/// The board-space point KiCad writes as a footprint's `(at x y)`, given the
/// internal courtyard-centre placement.
///
/// `part` supplies the courtyard offset; `None` (or a part with no resolvable
/// footprint) degrades to the identity, where origin and courtyard centre
/// coincide. The KiCad exporter and [`to_external`] must both go through this
/// so the exported board and the published placement cannot drift apart.
#[must_use]
pub fn footprint_origin(
    part: Option<&synth_registry::Part>,
    placement: &ComponentPlacement,
) -> Point {
    let (offset_x_mm, offset_y_mm) = part.map_or((0.0, 0.0), |p| {
        synth_layout::pcb_courtyard_geometry_for_part(p).0
    });
    let (rot_x, rot_y) = placement
        .rotation
        .rotate_offset(mm_to_nm(offset_x_mm), mm_to_nm(offset_y_mm));
    Point::new(placement.center.x_nm - rot_x, placement.center.y_nm - rot_y)
}

/// Convert an internal placement into the stable external coordinate space.
///
/// Use this for every placement payload that leaves the process — the CLI's
/// `synth place` and the MCP `component_placements` field. Never serialize
/// [`Placement`] directly to an external consumer.
#[must_use]
pub fn to_external(board: &Board, placement: &Placement) -> ExternalPlacement {
    let components = placement
        .components
        .iter()
        .map(|p| {
            let part = board.component(p.id).and_then(|c| c.part.as_ref());
            ExternalComponent {
                id: p.id,
                center: footprint_origin(part, p),
                rotation: p.rotation,
                layer: p.layer,
            }
        })
        .collect();
    ExternalPlacement {
        board_outline: placement.board_outline,
        components,
    }
}

/// Structured failure modes for the constraint solver. Each
/// variant corresponds to a `E-SYNTH-PLACE-*` code in the
/// diagnostic catalogue (plan §9.5); slice 5 promotes these to
/// full diagnostics with patch primitives.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum PlaceError {
    /// Total courtyard area exceeds the largest supported board
    /// size. The user must either select a larger sheet, split
    /// the design into sub-boards, or shrink footprints.
    #[error(
        "E-SYNTH-PLACE-001 area insufficient: {placed_mm2:.0} mm² courtyard, \
         {board_mm2:.0} mm² largest standard board"
    )]
    AreaInsufficient { placed_mm2: f64, board_mm2: f64 },

    /// No grid cell on the chosen board outline accommodates
    /// `refdes` without overlapping a previously-placed
    /// component. Either the board is too crowded (raise
    /// `FILL_RATIO`) or the ordering heuristic boxed the placer
    /// in (slice 2.x's backtracking pass fixes this).
    #[error(
        "E-SYNTH-PLACE-002 no legal position for `{refdes}` on \
         {board_w_mm:.1} × {board_h_mm:.1} mm board after \
         {tried} positions tried"
    )]
    NoLegalPosition {
        refdes: String,
        board_w_mm: f64,
        board_h_mm: f64,
        tried: usize,
    },

    /// Hard placement hint conflicts with board constraints or courtyard space.
    #[error("E-SYNTH-PLACE-HINT-001 hard placement hint conflict for `{refdes}`")]
    HintConflict { refdes: String },
}

impl PlaceError {
    /// Convert this failure into one or more [`Diagnostic`]
    /// objects suitable for emission via the
    /// `synth-diagnostics` protocol. Agent-facing tooling
    /// (`synth fix`, `synth validate --format json`) consumes
    /// these to decide on patches.
    ///
    /// `board` is used to attach component source spans where
    /// available; `file` is the input filename for the
    /// `Location`. The returned vector has one diagnostic
    /// today (one error → one diagnostic), but the signature
    /// is plural so future failure modes can fan out into
    /// multiple correlated diagnostics without a breaking
    /// change.
    #[must_use]
    pub fn to_diagnostics(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        match self {
            Self::AreaInsufficient {
                placed_mm2,
                board_mm2,
            } => vec![DiagnosticBuilder::new(
                "E-SYNTH-PLACE-001",
                Severity::Error,
                "Placement area insufficient",
            )
            .message(format!(
                "Total footprint courtyard area ({placed_mm2:.0} mm²) exceeds the \
                 largest supported board ({board_mm2:.0} mm²). The placer cannot \
                 fit all components on any standard sheet."
            ))
            .location(Location::from_span(file, board_span(board)))
            .build()],

            Self::NoLegalPosition {
                refdes,
                board_w_mm,
                board_h_mm,
                tried,
            } => {
                let mut builder = DiagnosticBuilder::new(
                    "E-SYNTH-PLACE-002",
                    Severity::Error,
                    format!("No legal placement for {refdes}"),
                )
                .message(format!(
                    "After {tried} grid-cell attempts on a \
                     {board_w_mm:.1} × {board_h_mm:.1} mm board, no position \
                     accommodates `{refdes}` without colliding with a previously \
                     placed component's courtyard."
                ));
                if let Some(span) = component_span(board, refdes) {
                    builder = builder.location(Location::from_span(file, span));
                }
                vec![builder.build()]
            }

            Self::HintConflict { refdes } => {
                let mut builder = DiagnosticBuilder::new(
                    "E-SYNTH-PLACE-HINT-001",
                    Severity::Error,
                    format!("Hard placement hint conflict for {refdes}"),
                )
                .message(format!(
                    "The hard placement hint for `{refdes}` could not be satisfied without \
                     violating courtyard placement rules or board boundary."
                ));
                if let Some(span) = component_span(board, refdes) {
                    builder = builder.location(Location::from_span(file, span));
                }
                vec![builder.build()]
            }
        }
    }
}

/// Locate the source span of a named component for diagnostic
/// attribution. Returns `None` when the refdes isn't in the IR
/// (shouldn't happen on a real failure, but defensive).
fn component_span(board: &Board, refdes: &str) -> Option<Span> {
    board
        .components
        .iter()
        .find(|c| c.refdes == refdes)
        .map(|c| c.source_span)
}

/// Span of the board declaration itself, used as the fallback
/// location for diagnostics that aren't tied to a specific
/// component (e.g. `E-SYNTH-PLACE-001 area insufficient`).
fn board_span(board: &Board) -> Span {
    board.source_span
}

/// Placement grid pitch in millimetres. The placer snaps every
/// component's centre to a 1 mm grid. Fine enough to pack 0603
/// passives without huge gaps; coarse enough that the greedy
/// scan stays fast on 200-component designs.
const GRID_PITCH_MM: f64 = 1.0;

/// Offset from `ComponentPlacement::center` to the courtyard centre that the
/// solver and legalization passes use. The KiCad exporter puts the footprint
/// origin at `center - rot(courtyard_offset)`, so on the exported board the
/// courtyard is centred exactly on `center`, and the router and DRC model it
/// the same way. Adding the footprint's courtyard offset here would make
/// the placer clear space one offset away from where the part actually
/// lands (a DIP-28's real courtyard then overlaps its neighbours).
const PLACEMENT_COURTYARD_OFFSET_MM: (f64, f64) = (0.0, 0.0);

/// Margin between the outermost placed component and the board
/// edge. Set to 6.0 mm to guarantee open routing channels around
/// component courtyards along the board perimeter.
const BOARD_MARGIN_MM: f64 = 6.0;

const COPPER_EDGE_CLEARANCE_MM: f64 = 0.5;

const MAX_DOCKING_SLIDE_STEPS: i64 = 40;

const EDGE_MARGIN_MM: f64 = 4.0;

/// Target fill ratio (placed-component courtyard area ÷ board
/// area). 0.45 leaves 55% of the board for routing channels and
/// keep-outs — a reasonable starting point for unconstrained
/// 2-layer boards. Manufacturer profiles will tune this later.
const FILL_RATIO: f64 = 0.45;

/// Lay out `board`'s components on the PCB.
///
/// Phase 7 slice 2: greedy first-fit constraint solver. Returns
/// either a legal placement or a structured [`PlaceError`] —
/// never a best-effort partial result. All components land on
/// [`Layer::Top`] at [`Rotation::Zero`]; rotation and layer
/// assignment come in slice 3.
const MAX_BACKTRACKS: usize = 500;

pub fn fallback_courtyard(kind: &str) -> (f64, f64) {
    match kind {
        "sensor" => (3.5, 3.5),
        "mcu" | "processor" | "ic" => (12.0, 12.0),
        "connector" => (10.0, 5.0),
        "memory" => (6.0, 5.0),
        "regulator" | "power" => (6.0, 6.0),
        "switch" | "button" => (6.0, 6.0),
        "diode" | "led" => (3.0, 2.0),
        "resistor" | "capacitor" | "inductor" => (2.0, 1.2),
        _ => (4.0, 4.0),
    }
}

/// Apply sidecar component placement overrides onto a Placement.
pub fn apply_sidecar_overrides(
    board: &Board,
    placement: &mut Placement,
    sidecar: &synth_layout::sidecar::SidecarLayout,
) {
    // Absolute entries first; TOML map order is not meaningful.
    for comp in &board.components {
        if let Some(sidecar_comp) = sidecar.components.get(&comp.refdes) {
            if sidecar_comp.relative_to.is_some() {
                continue;
            }
            if let Some(p) = placement.components.iter_mut().find(|c| c.id == comp.id) {
                p.center = Point::new(mm_to_nm(sidecar_comp.x), mm_to_nm(sidecar_comp.y));
                p.rotation = match sidecar_comp.rotation {
                    90 => Rotation::Ninety,
                    180 => Rotation::OneEighty,
                    270 => Rotation::TwoSeventy,
                    _ => Rotation::Zero,
                };
            }
        }
    }

    // Resolve relative entries after absolute anchors. Repeated bounded
    // passes support short chains while leaving cyclic/missing anchors at
    // their automatic placement instead of producing invalid coordinates.
    for _ in 0..sidecar.components.len() {
        let mut changed = false;
        for comp in &board.components {
            let Some(sidecar_comp) = sidecar.components.get(&comp.refdes) else {
                continue;
            };
            let Some(anchor_refdes) = sidecar_comp.relative_to.as_deref() else {
                continue;
            };
            let Some(anchor_id) = board
                .components
                .iter()
                .find(|candidate| candidate.refdes == anchor_refdes)
                .map(|candidate| candidate.id)
            else {
                continue;
            };
            let Some(anchor_center) = placement
                .components
                .iter()
                .find(|candidate| candidate.id == anchor_id)
                .map(|candidate| candidate.center)
            else {
                continue;
            };
            if let Some(p) = placement.components.iter_mut().find(|c| c.id == comp.id) {
                let center = Point::new(
                    anchor_center.x_nm + mm_to_nm(sidecar_comp.dx),
                    anchor_center.y_nm + mm_to_nm(sidecar_comp.dy),
                );
                if p.center != center {
                    changed = true;
                }
                p.center = center;
                p.rotation = match sidecar_comp.rotation {
                    90 => Rotation::Ninety,
                    180 => Rotation::OneEighty,
                    270 => Rotation::TwoSeventy,
                    _ => Rotation::Zero,
                };
            }
        }
        if !changed {
            break;
        }
    }

    // Agent/human overrides are authoritative only within the physical
    // placement contract. If an override collides with another courtyard or
    // leaves the generated outline, move that component to the nearest legal
    // half-millimetre slot. Exact legal overrides remain verbatim; this keeps
    // sidecars expressive without allowing an invalid KiCad board to reach
    // routing or export.
    legalize_sidecar_overrides(board, placement, sidecar);
}

/// Tighten an override-aware outline around the actual footprint courtyards.
/// The outline origin is allowed to be non-zero so absolute sidecar coordinates
/// remain authoritative; only unused perimeter area is removed.
fn tighten_outline_after_sidecar(board: &Board, placement: &mut Placement) {
    if let Some(outline) = outline_with_flush_connectors(
        board,
        &placement.components,
        mm_to_nm(EDGE_MARGIN_MM),
        sidecar_courtyard_rect,
        false,
    ) {
        placement.board_outline = outline;
    }
}

/// Fit the outline round an automatic placement. The flush side of an edge
/// connector comes from its hint, not from where it happens to stick out,
/// because nothing here was dragged by hand.
fn tighten_outline_to_hints(board: &Board, placement: &mut Placement) {
    if let Some(outline) = outline_with_flush_connectors(
        board,
        &placement.components,
        mm_to_nm(EDGE_MARGIN_MM),
        sidecar_courtyard_rect,
        true,
    ) {
        placement.board_outline = outline;
    }
}

fn rotated_courtyard_rect(placed: &ComponentPlacement, (width, height): (f64, f64)) -> Rect {
    let (w, h) = if placed.rotation.swaps_extents() {
        (height, width)
    } else {
        (width, height)
    };
    Rect::from_center_half_extents(placed.center, mm_to_nm(w) / 2, mm_to_nm(h) / 2)
}

fn padded_courtyard_rect(
    lookup: &std::collections::HashMap<ComponentId, (f64, f64)>,
    placed: &ComponentPlacement,
) -> Rect {
    rotated_courtyard_rect(placed, lookup[&placed.id])
}

fn is_edge_flush_connector(component: &synth_ir::Component) -> bool {
    matches!(component.kind.as_str(), "connector" | "jack")
        && component.placement_hint.as_ref().is_some_and(|hint| {
            hint.priority == synth_ir::PlacementPriority::Hard && hint.edge.is_some()
        })
}

/// The index the outline code gives each board edge: left 0, top 1, right 2,
/// bottom 3. Even indices face along x, odd ones along y.
fn edge_index(edge: &synth_ir::PlacementEdge) -> usize {
    match edge {
        synth_ir::PlacementEdge::Left => 0,
        synth_ir::PlacementEdge::Top => 1,
        synth_ir::PlacementEdge::Right => 2,
        synth_ir::PlacementEdge::Bottom => 3,
    }
}

/// The edge an edge-flush connector is hinted to, as an [`edge_index`].
fn flush_edge_index(board: &Board, id: ComponentId) -> Option<usize> {
    board
        .component(id)
        .filter(|c| is_edge_flush_connector(c))
        .and_then(|c| c.placement_hint.as_ref()?.edge.as_ref())
        .map(edge_index)
}

/// The reach of an edge connector: its courtyard, or its copper plus the
/// clearance the board edge keeps from it, whichever goes further.
fn connector_reach(real: Rect, copper: Rect) -> Rect {
    let clearance = mm_to_nm(COPPER_EDGE_CLEARANCE_MM);
    Rect::new(
        Point::new(
            real.min.x_nm.min(copper.min.x_nm - clearance),
            real.min.y_nm.min(copper.min.y_nm - clearance),
        ),
        Point::new(
            real.max.x_nm.max(copper.max.x_nm + clearance),
            real.max.y_nm.max(copper.max.y_nm + clearance),
        ),
    )
}

fn outline_with_flush_connectors(
    board: &Board,
    placements: &[ComponentPlacement],
    margin_nm: i64,
    courtyard: impl Fn(&Board, &synth_ir::Component, &ComponentPlacement) -> Rect,
    side_from_hint: bool,
) -> Option<Rect> {
    let parts: Vec<_> = placements
        .iter()
        .filter_map(|placed| {
            let component = board.component(placed.id)?;
            Some((component, placed, courtyard(board, component, placed)))
        })
        .collect();
    let body = parts
        .iter()
        .filter(|(component, _, _)| !is_edge_flush_connector(component))
        .map(|(_, _, rect)| *rect)
        .reduce(|a, b| {
            Rect::new(
                Point::new(a.min.x_nm.min(b.min.x_nm), a.min.y_nm.min(b.min.y_nm)),
                Point::new(a.max.x_nm.max(b.max.x_nm), a.max.y_nm.max(b.max.y_nm)),
            )
        });
    let mut outline = [i64::MAX, i64::MAX, i64::MIN, i64::MIN];
    for (component, placed, rect) in &parts {
        // A mounting hole, test point or fiducial belongs in a corner, so the
        // outline ends at its courtyard instead of leaving a margin round it.
        let margin_nm = if synth_layout::is_board_feature_kind(&component.kind) {
            0
        } else {
            margin_nm
        };
        let mut reach = [
            rect.min.x_nm - margin_nm,
            rect.min.y_nm - margin_nm,
            rect.max.x_nm + margin_nm,
            rect.max.y_nm + margin_nm,
        ];
        if is_edge_flush_connector(component) {
            let real = sidecar_courtyard_rect(board, component, placed);
            let copper = exported_pad_bounds(component, placed).unwrap_or(real);
            let flush = connector_reach(real, copper);
            let flush = [
                flush.min.x_nm,
                flush.min.y_nm,
                flush.max.x_nm,
                flush.max.y_nm,
            ];
            let side = match body.filter(|_| !side_from_hint) {
                Some(body) => [
                    body.min.x_nm - real.min.x_nm,
                    body.min.y_nm - real.min.y_nm,
                    real.max.x_nm - body.max.x_nm,
                    real.max.y_nm - body.max.y_nm,
                ]
                .iter()
                .enumerate()
                .max_by_key(|(_, p)| **p)
                .map(|(side, _)| side),
                None => component
                    .placement_hint
                    .as_ref()
                    .and_then(|hint| hint.edge.as_ref())
                    .map(edge_index),
            };
            if let Some(side) = side {
                reach[side] = flush[side];
            }
        }
        outline = [
            outline[0].min(reach[0]),
            outline[1].min(reach[1]),
            outline[2].max(reach[2]),
            outline[3].max(reach[3]),
        ];
    }
    (outline[0] != i64::MAX).then(|| {
        Rect::new(
            Point::new(outline[0], outline[1]),
            Point::new(outline[2], outline[3]),
        )
    })
}

fn sidecar_courtyard_rect(
    _board: &Board,
    component: &synth_ir::Component,
    placement: &ComponentPlacement,
) -> Rect {
    // Use the same offset, dimensions, and rotation convention as the visual
    // review and KiCad exporter. A simpler unrotated bbox here can accept a
    // sidecar that later collides once the real footprint courtyard is used.
    let size = component.part.as_ref().map_or_else(
        || fallback_courtyard(&component.kind),
        |part| synth_layout::pcb_courtyard_geometry_for_part(part).1,
    );
    rotated_courtyard_rect(placement, size)
}

fn sidecar_position_is_legal(
    board: &Board,
    placement: &Placement,
    component_index: usize,
    candidate: ComponentPlacement,
) -> bool {
    let candidate_component = &board.components[component_index];
    let candidate_rect = sidecar_courtyard_rect(board, candidate_component, &candidate);
    if candidate_rect.min.x_nm < placement.board_outline.min.x_nm
        || candidate_rect.min.y_nm < placement.board_outline.min.y_nm
        || candidate_rect.max.x_nm > placement.board_outline.max.x_nm
        || candidate_rect.max.y_nm > placement.board_outline.max.y_nm
    {
        return false;
    }
    placement.components.iter().all(|other| {
        if other.id == candidate.id {
            return true;
        }
        board.component(other.id).is_none_or(|other_component| {
            !candidate_rect.intersects(&sidecar_courtyard_rect(board, other_component, other))
        })
    })
}

fn legalize_sidecar_overrides(
    board: &Board,
    placement: &mut Placement,
    sidecar: &synth_layout::sidecar::SidecarLayout,
) {
    let overridden: std::collections::HashSet<ComponentId> = board
        .components
        .iter()
        .filter(|component| sidecar.components.contains_key(&component.refdes))
        .map(|component| component.id)
        .collect();
    let pitch_nm = mm_to_nm(0.5);

    // Snapshot every override's requested slot before anything moves.
    //
    // Legalising a conflicting override displaces a component, and a later
    // iteration reads its "requested" slot from the live placement. If that
    // component was the one displaced, the read returns where it was parked
    // rather than where the sidecar asked for it — which is how a part ends
    // up at the board origin instead of its overridden coordinate.
    let requested_by_id: std::collections::HashMap<ComponentId, ComponentPlacement> = placement
        .components
        .iter()
        .filter(|placed| overridden.contains(&placed.id))
        .map(|placed| (placed.id, *placed))
        .collect();

    // Honour hard overrides before soft ones, and refdes within a priority,
    // so which override wins a contested square is a property of the design
    // file rather than of iteration order.
    let priority_of = |id: ComponentId| {
        board
            .component(id)
            .and_then(|c| sidecar.components.get(&c.refdes))
            .map_or(synth_layout::sidecar::OverridePriority::Hard, |entry| {
                entry.priority
            })
    };
    let mut order: Vec<ComponentId> = overridden.iter().copied().collect();
    order.sort_by_key(|id| {
        let reversed = matches!(
            priority_of(*id),
            synth_layout::sidecar::OverridePriority::Soft
        );
        (
            reversed,
            board
                .component(*id)
                .map_or(String::new(), |c| c.refdes.clone()),
        )
    });

    for target in order {
        let Some(component_index) = board
            .components
            .iter()
            .position(|component| component.id == target)
        else {
            continue;
        };
        let Some(placement_index) = placement
            .components
            .iter()
            .position(|placed| placed.id == target)
        else {
            continue;
        };
        let requested = requested_by_id
            .get(&target)
            .copied()
            .unwrap_or(placement.components[placement_index]);
        if sidecar_position_is_legal(board, placement, component_index, requested) {
            placement.components[placement_index] = requested;
            continue;
        }

        // The requested square is occupied. A drag is an instruction, not a
        // suggestion: whoever is in the way moves, because leaving the board
        // with two overlapping courtyards is worse than moving a part the
        // designer did not place by hand.
        let blockers: Vec<ComponentId> = placement
            .components
            .iter()
            .filter(|other| other.id != target)
            .filter(|other| {
                let rect =
                    sidecar_courtyard_rect(board, &board.components[component_index], &requested);
                rect.intersects(&sidecar_courtyard_rect(
                    board,
                    board
                        .component(other.id)
                        .unwrap_or(&board.components[component_index]),
                    other,
                ))
            })
            .map(|other| other.id)
            .collect();

        // Move each movable blocker to its *own* nearest legal slot rather
        // than parking it at the outline corner. A corner parking spot overlaps
        // whatever already sits there, and a blocker that is itself overridden
        // would later be read back from the corner instead of its sidecar
        // coordinate.
        let mut displaced: Vec<(usize, ComponentPlacement)> = Vec::new();
        for blocker in &blockers {
            // A blocker that is itself a hard override outranks this drag and
            // is left alone; the caller surfaced that conflict already.
            if overridden.contains(blocker)
                && priority_of(*blocker) == synth_layout::sidecar::OverridePriority::Hard
            {
                continue;
            }
            let Some(blocker_component_index) = board
                .components
                .iter()
                .position(|component| component.id == *blocker)
            else {
                continue;
            };
            let Some(blocker_index) = placement
                .components
                .iter()
                .position(|placed| placed.id == *blocker)
            else {
                continue;
            };
            let current = placement.components[blocker_index];
            if let Some(slot) =
                nearest_local_slot(board, placement, blocker_component_index, current, pitch_nm)
            {
                displaced.push((blocker_index, current));
                placement.components[blocker_index] = slot;
            }
        }

        if sidecar_position_is_legal(board, placement, component_index, requested) {
            placement.components[placement_index] = requested;
            continue;
        }

        // Something un-movable is still in the way. Put back whatever moved,
        // then place the target as near its request as the board allows; if
        // even that fails, it keeps its automatic position for visual review
        // to report.
        for (index, original) in displaced {
            placement.components[index] = original;
        }
        if let Some(candidate) =
            nearest_local_slot(board, placement, component_index, requested, pitch_nm)
        {
            placement.components[placement_index] = candidate;
        }
    }
}

/// Nearest legal square to `requested`, searched as an expanding ring.
///
/// Deterministic: the first legal square found on the smallest ring wins,
/// and a ring is scanned in a fixed order.
fn nearest_local_slot(
    board: &Board,
    placement: &Placement,
    component_index: usize,
    requested: ComponentPlacement,
    pitch_nm: i64,
) -> Option<ComponentPlacement> {
    // Do not silently move an agent's requested component across the board: if
    // no nearby slot exists, leave the coordinate for visual review to report.
    const MAX_RING: i64 = 10;
    for radius in 1..=MAX_RING {
        for dx in -radius..=radius {
            for dy in -radius..=radius {
                if dx.abs().max(dy.abs()) != radius {
                    continue;
                }
                let candidate = ComponentPlacement {
                    center: Point::new(
                        requested.center.x_nm + dx * pitch_nm,
                        requested.center.y_nm + dy * pitch_nm,
                    ),
                    ..requested
                };
                if sidecar_position_is_legal(board, placement, component_index, candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Run placement with the optional PCB-placement sidecar.
///
/// `sidecar_path` holds **board millimetres** and must come from
/// `synth_layout::placement_sidecar_path` (or an explicit
/// `layout_file_path`). The schematic sidecar is a different file in sheet
/// millimetres and must not be passed here; see
/// [`synth_layout::SidecarKind`] for why the two are kept apart.
pub fn place_with_sidecar(
    board: &Board,
    sidecar_path: Option<&std::path::Path>,
) -> Result<Placement, PlaceError> {
    // Start from the same production defaults as `place()`, then apply the
    // agent/human sidecar so exact coordinates and rotations can take control.
    let has_sidecar = sidecar_path.is_some_and(std::path::Path::exists);
    let mut placement = place_with_default_tuning(board, !has_sidecar)?;
    if let Some(path) = sidecar_path {
        if path.exists() {
            if let Some(sidecar) = synth_layout::sidecar::SidecarLayout::load_from_file(path) {
                apply_sidecar_overrides(board, &mut placement, &sidecar);
            }
        }
    }
    if has_sidecar {
        tighten_outline_after_sidecar(board, &mut placement);
    }
    Ok(placement)
}

/// Run placement with iteration tuning (extra courtyard margin +
/// rotation overrides) and the optional PCB-placement sidecar (board
/// millimetres).
///
/// Sidecar entries record manual component drags
/// (`synth_write_layout_override`) and must come from
/// `synth_layout::placement_sidecar_path`. They are applied *after* the
/// solver runs so recorded human intent wins over the automatic
/// placement; DSL `placement_hint`s are honoured *inside* the solver
/// (`place_with_outline`). Export pipelines call this between
/// placement and routing so traces follow the overridden positions.
pub fn place_with_tuning_and_sidecar<S: ::std::hash::BuildHasher>(
    board: &Board,
    extra_margin_mm: f64,
    rotation_overrides: &std::collections::HashMap<ComponentId, synth_geometry::Rotation, S>,
    sidecar_path: Option<&std::path::Path>,
) -> Result<Placement, PlaceError> {
    place_with_tuning_and_sidecar_seeded(
        board,
        extra_margin_mm,
        rotation_overrides,
        sidecar_path,
        0,
    )
}

/// [`place_with_tuning_and_sidecar`] with a deterministic placement `seed`.
///
/// Used by the place→route→re-place loop to ask for a different legal
/// arrangement of the same board (see [`place_with_tuning_seeded`]). Seed `0`
/// is the canonical arrangement every other caller gets.
pub fn place_with_tuning_and_sidecar_seeded<S: ::std::hash::BuildHasher>(
    board: &Board,
    extra_margin_mm: f64,
    rotation_overrides: &std::collections::HashMap<ComponentId, synth_geometry::Rotation, S>,
    sidecar_path: Option<&std::path::Path>,
    seed: u64,
) -> Result<Placement, PlaceError> {
    let has_sidecar = sidecar_path.is_some_and(std::path::Path::exists);
    let loaded = sidecar_path
        .filter(|path| path.exists())
        .and_then(synth_layout::sidecar::SidecarLayout::load_from_file);

    // The author's absolute entries become constraints the solver is given,
    // not corrections applied to its answer. The old order ran the whole
    // greedy placement first and only then moved the pinned parts, so a
    // design the solver could not place was refused however completely its
    // floorplan was specified -- the overrides were never reached. A relative
    // entry still resolves afterwards, because it is defined against an
    // anchor that has to be placed before it can be measured from.
    let pinned: PinnedPlacements = loaded
        .as_ref()
        .map_or_else(PinnedPlacements::new, |sidecar| {
            board
                .components
                .iter()
                .filter_map(|component| {
                    let entry = sidecar.components.get(&component.refdes)?;
                    if entry.relative_to.is_some() {
                        return None;
                    }
                    Some((
                        component.id,
                        (
                            Point::new(mm_to_nm(entry.x), mm_to_nm(entry.y)),
                            match entry.rotation {
                                90 => Rotation::Ninety,
                                180 => Rotation::OneEighty,
                                270 => Rotation::TwoSeventy,
                                _ => Rotation::Zero,
                            },
                        ),
                    ))
                })
                .collect()
        });

    let mut placement = place_with_tuning_impl(
        board,
        extra_margin_mm,
        rotation_overrides,
        seed,
        !has_sidecar,
        &pinned,
    )?;
    if let Some(sidecar) = loaded.as_ref() {
        apply_sidecar_overrides(board, &mut placement, sidecar);
    }
    if sidecar_path.is_some_and(std::path::Path::exists) {
        tighten_outline_after_sidecar(board, &mut placement);
    }
    Ok(placement)
}

#[allow(clippy::too_many_lines)]
pub fn place(board: &Board) -> Result<Placement, PlaceError> {
    place_with_default_tuning(board, true)
}

/// `place()` with the production defaults, optionally without compaction.
fn place_with_default_tuning(board: &Board, compact: bool) -> Result<Placement, PlaceError> {
    let mut rotation_overrides = std::collections::HashMap::new();
    let has_rp2350 = board.components.iter().any(|component| {
        component
            .part
            .as_ref()
            .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("rp2350"))
    });
    for component in &board.components {
        let is_flash = has_rp2350
            && component
                .part
                .as_ref()
                .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("w25q"));
        if is_flash {
            rotation_overrides.insert(component.id, synth_geometry::Rotation::Zero);
        }
    }
    place_with_tuning_impl(
        board,
        1.5,
        &rotation_overrides,
        0,
        compact,
        &PinnedPlacements::new(),
    )
}

/// Place a board inside an explicit rectangular outline in millimetres.
///
/// This is the compiler-owned sizing seam used by agents and integrations.
/// Unlike the automatic placer, it never silently falls back to a larger
/// outline: an infeasible request returns the normal structured placement
/// diagnostic.
pub fn place_with_dimensions(
    board: &Board,
    width_mm: f64,
    height_mm: f64,
) -> Result<Placement, PlaceError> {
    use synth_layout::pcb_courtyard_geometry_for_part;

    if !width_mm.is_finite() || !height_mm.is_finite() || width_mm <= 0.0 || height_mm <= 0.0 {
        return Err(PlaceError::AreaInsufficient {
            placed_mm2: 0.0,
            board_mm2: 0.0,
        });
    }

    let courtyards_mm: Vec<(ComponentId, (f64, f64), (f64, f64))> = board
        .components
        .iter()
        .map(|c| {
            let (w, h) = c.part.as_ref().map_or_else(
                || fallback_courtyard(&c.kind),
                |part| pcb_courtyard_geometry_for_part(part).1,
            );
            (c.id, PLACEMENT_COURTYARD_OFFSET_MM, (w + 1.5, h + 1.5))
        })
        .collect();

    let outline = Rect::new(
        Point::new(0, 0),
        Point::new(mm_to_nm(width_mm), mm_to_nm(height_mm)),
    );
    let placement = place_with_outline(
        board,
        outline,
        &courtyards_mm,
        &std::collections::HashMap::new(),
        false,
        0,
        &PinnedPlacements::new(),
    )?;

    // `place_with_outline` tightens automatic outlines after solving. For an
    // explicit request, retain the requested rectangle only when the solved
    // geometry actually fits inside it; otherwise return a normal placement
    // diagnostic rather than silently accepting a larger board.
    let solved_width = synth_geometry::nm_to_mm(placement.board_outline.width_nm());
    let solved_height = synth_geometry::nm_to_mm(placement.board_outline.height_nm());
    if solved_width > width_mm || solved_height > height_mm {
        return Err(PlaceError::NoLegalPosition {
            refdes: board
                .components
                .first()
                .map_or_else(|| "board".to_string(), |component| component.refdes.clone()),
            board_w_mm: width_mm,
            board_h_mm: height_mm,
            tried: board.components.len(),
        });
    }

    Ok(Placement {
        board_outline: outline,
        components: placement.components,
    })
}

/// Closed-loop placement generator with support for iteration hints (rotation & margin tuning).
pub fn place_with_tuning<S: ::std::hash::BuildHasher>(
    board: &Board,
    extra_margin_mm: f64,
    rotation_overrides: &std::collections::HashMap<ComponentId, synth_geometry::Rotation, S>,
) -> Result<Placement, PlaceError> {
    place_with_tuning_seeded(board, extra_margin_mm, rotation_overrides, 0)
}

/// Soft placement hints that pull each of `hard_nets`' components toward the
/// one carrying most of that net's pins.
///
/// The place→route→re-place loop routes a candidate, learns which nets were
/// hardest, and calls this to ask for one more arrangement with those nets'
/// parts grouped. Components that already declare a hint are left alone, and a
/// net with fewer than two components offers nothing. Deterministic.
#[must_use]
pub fn hard_net_proximity_hints(board: &Board, hard_nets: &[String]) -> Vec<(ComponentId, String)> {
    let mut out = Vec::new();
    for name in hard_nets {
        let Some(net) = board.nets.iter().find(|net| net.name == *name) else {
            continue;
        };
        let mut pin_counts: BTreeMap<ComponentId, usize> = BTreeMap::new();
        for endpoint in &net.endpoints {
            *pin_counts.entry(endpoint.component).or_default() += 1;
        }
        if pin_counts.len() < 2 {
            continue;
        }
        let Some(anchor_id) = pin_counts
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(id, _)| *id)
        else {
            continue;
        };
        let Some(anchor_refdes) = board
            .component(anchor_id)
            .map(|component| component.refdes.clone())
        else {
            continue;
        };
        for component in pin_counts.keys().copied() {
            if component == anchor_id {
                continue;
            }
            if board
                .component(component)
                .is_some_and(|component| component.placement_hint.is_none())
            {
                out.push((component, anchor_refdes.clone()));
            }
        }
    }
    out
}

/// A copy of `board` with each `(component, anchor)` from
/// [`hard_net_proximity_hints`] added as a soft `near` constraint.
///
/// Components that already declare a hint are left alone, so an author's own
/// hints win over the loop's grouping.
#[must_use]
pub fn with_hard_net_hints(board: &Board, hints: &[(ComponentId, String)]) -> Board {
    let mut hinted = board.clone();
    for (id, anchor) in hints {
        if let Some(component) = hinted
            .components
            .iter_mut()
            .find(|component| component.id == *id)
        {
            if component.placement_hint.is_none() {
                component.placement_hint = Some(synth_ir::PlacementConstraint {
                    region: None,
                    edge: None,
                    near: Some(anchor.clone()),
                    side: None,
                    priority: synth_ir::PlacementPriority::Soft,
                });
            }
        }
    }
    hinted
}

/// [`place_with_tuning`] with a deterministic `seed`.
///
/// The seed varies only the order among components of equal net degree, which
/// changes the greedy first-fit packing without relaxing any hard constraint,
/// so distinct seeds give distinct-but-legal arrangements. Seed `0` is the
/// canonical order and is what every non-iterating caller uses.
pub fn place_with_tuning_seeded<S: ::std::hash::BuildHasher>(
    board: &Board,
    extra_margin_mm: f64,
    rotation_overrides: &std::collections::HashMap<ComponentId, synth_geometry::Rotation, S>,
    seed: u64,
) -> Result<Placement, PlaceError> {
    place_with_tuning_impl(
        board,
        extra_margin_mm,
        rotation_overrides,
        seed,
        true,
        &PinnedPlacements::new(),
    )
}

/// `compact` squeezes the finished layout. A caller that is about to apply a
/// placement sidecar passes false: the sidecar was written against the
/// uncompacted layout, and every part it leaves alone must stay where it was.
#[allow(clippy::too_many_lines)]
fn place_with_tuning_impl<S: ::std::hash::BuildHasher>(
    board: &Board,
    extra_margin_mm: f64,
    rotation_overrides: &std::collections::HashMap<ComponentId, synth_geometry::Rotation, S>,
    seed: u64,
    compact: bool,
    pinned: &PinnedPlacements,
) -> Result<Placement, PlaceError> {
    use synth_layout::pcb_courtyard_geometry_for_part;

    // Per-component courtyard geometry: (id, (cx, cy), (w, h)).
    let courtyards_mm: Vec<(ComponentId, (f64, f64), (f64, f64))> = board
        .components
        .iter()
        .map(|c| {
            let (w, h) = c.part.as_ref().map_or_else(
                || fallback_courtyard(&c.kind),
                |part| pcb_courtyard_geometry_for_part(part).1,
            );
            (
                c.id,
                PLACEMENT_COURTYARD_OFFSET_MM,
                (w + extra_margin_mm, h + extra_margin_mm),
            )
        })
        .collect();

    let total_courtyard_mm2: f64 = courtyards_mm.iter().map(|(_, _, (w, h))| w * h).sum();
    let max_board_mm2 = 230.0 * 230.0;
    if total_courtyard_mm2 / FILL_RATIO > max_board_mm2 {
        return Err(PlaceError::AreaInsufficient {
            placed_mm2: total_courtyard_mm2,
            board_mm2: max_board_mm2,
        });
    }

    let needed_area = total_courtyard_mm2 / FILL_RATIO;
    let compact_side = (needed_area.sqrt() + 15.0).clamp(40.0, 230.0);

    let candidates = [
        (compact_side, compact_side),
        (100.0, 80.0),
        (120.0, 100.0),
        (160.0, 120.0),
    ];

    let mut last_err = None;
    for (board_w_mm, board_h_mm) in candidates {
        let board_outline = Rect::new(
            Point::new(0, 0),
            Point::new(mm_to_nm(board_w_mm), mm_to_nm(board_h_mm)),
        );
        match place_with_outline(
            board,
            board_outline,
            &courtyards_mm,
            rotation_overrides,
            true,
            seed,
            pinned,
        ) {
            Ok(p) => {
                let packed = if compact {
                    compact_and_fit(board, p)
                } else {
                    p
                };
                return Ok(refine_placement(board, &packed, rotation_overrides));
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or(PlaceError::AreaInsufficient {
        placed_mm2: total_courtyard_mm2,
        board_mm2: 230.0 * 230.0,
    }))
}

/// A board feature carrying no net: a mounting hole, a fiducial, or a test
/// point nobody wired to anything.
///
/// Nothing in the solver has a reason to put one of these anywhere. It has no
/// pad to be near, no anchor to hug and no net to shorten, so the greedy
/// search places it wherever a legal cell happens to be free and the outline
/// then has to contain the result. They are placed after the circuit instead,
/// by [`place_board_features`].
fn is_netless_board_feature(board: &Board, id: ComponentId) -> bool {
    board.component(id).is_some_and(|component| {
        synth_layout::is_board_feature_kind(&component.kind)
            && !board.nets.iter().any(|net| {
                net.endpoints
                    .iter()
                    .any(|endpoint| endpoint.component == id)
            })
    })
}

/// The eight positions a board feature is offered, in the order an unhinted
/// feature takes them: the four corners first, then the four edge midpoints.
const FEATURE_SLOTS: [FeatureSlot; 8] = [
    FeatureSlot::TopLeft,
    FeatureSlot::TopRight,
    FeatureSlot::BottomLeft,
    FeatureSlot::BottomRight,
    FeatureSlot::TopMid,
    FeatureSlot::BottomMid,
    FeatureSlot::LeftMid,
    FeatureSlot::RightMid,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeatureSlot {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    TopMid,
    BottomMid,
    LeftMid,
    RightMid,
}

impl FeatureSlot {
    /// The point on the envelope this slot aims at: a corner, or the middle
    /// of an edge.
    fn anchor(self, envelope: Rect) -> Point {
        let mid_x = (envelope.min.x_nm + envelope.max.x_nm) / 2;
        let mid_y = (envelope.min.y_nm + envelope.max.y_nm) / 2;
        match self {
            Self::TopLeft => envelope.min,
            Self::TopRight => Point::new(envelope.max.x_nm, envelope.min.y_nm),
            Self::BottomLeft => Point::new(envelope.min.x_nm, envelope.max.y_nm),
            Self::BottomRight => envelope.max,
            Self::TopMid => Point::new(mid_x, envelope.min.y_nm),
            Self::BottomMid => Point::new(mid_x, envelope.max.y_nm),
            Self::LeftMid => Point::new(envelope.min.x_nm, mid_y),
            Self::RightMid => Point::new(envelope.max.x_nm, mid_y),
        }
    }
}

/// The slot a feature's own hint asks for, if it names one.
///
/// A corner region is read as that corner and an edge as that edge's
/// midpoint, so `region: top_left` and `edge: top` mean what they say on a
/// part whose only job is to sit somewhere on the border. `centre` and a
/// `near` hint name no slot and leave the caller's fallback to choose.
fn hinted_feature_slot(hint: &synth_ir::PlacementConstraint) -> Option<FeatureSlot> {
    use synth_ir::{PlacementEdge, PlacementRegion};
    if let Some(region) = &hint.region {
        return match region {
            PlacementRegion::TopLeft => Some(FeatureSlot::TopLeft),
            PlacementRegion::TopRight => Some(FeatureSlot::TopRight),
            PlacementRegion::BottomLeft => Some(FeatureSlot::BottomLeft),
            PlacementRegion::BottomRight => Some(FeatureSlot::BottomRight),
            PlacementRegion::TopEdge => Some(FeatureSlot::TopMid),
            PlacementRegion::BottomEdge => Some(FeatureSlot::BottomMid),
            PlacementRegion::LeftEdge => Some(FeatureSlot::LeftMid),
            PlacementRegion::RightEdge => Some(FeatureSlot::RightMid),
            PlacementRegion::Centre => None,
        };
    }
    match hint.edge.as_ref()? {
        PlacementEdge::Top => Some(FeatureSlot::TopMid),
        PlacementEdge::Bottom => Some(FeatureSlot::BottomMid),
        PlacementEdge::Left => Some(FeatureSlot::LeftMid),
        PlacementEdge::Right => Some(FeatureSlot::RightMid),
    }
}

/// Place every net-less board feature in the border band of the finished
/// circuit, as close as it can get to the corner or edge it asked for.
///
/// Placed with the circuit, these parts take whatever the greedy search had
/// free: on the Class-C SDRAM card the four mounting holes, each asking for a
/// different corner at `priority: hard`, land at the four quadrant *centres*
/// — 17 mm in from the left edge and 20 mm down from the top — which puts
/// four keep-outs in the middle of the routing area and no hole near a
/// corner. The two fiducials land mid-board, where a placement camera has no
/// use for them.
///
/// The band searched is the part's own extent plus a gap, inside the
/// envelope, which is where a hole or a fiducial belongs on a real board.
/// Only if nothing in the band is free does a feature step outside and grow
/// the outline, so a board pays area for a mounting hole exactly when its
/// border is genuinely full.
///
/// Deterministic: features are taken in `ComponentId` order, each slot is
/// claimed once, and candidates are ordered by distance to the slot's anchor
/// with ties broken by coordinate.
fn place_board_features(
    board: &Board,
    placements: &mut Vec<ComponentPlacement>,
    features: &[ComponentId],
    courtyard_lookup: &std::collections::HashMap<ComponentId, (f64, f64)>,
    courtyard_offset_lookup: &std::collections::HashMap<ComponentId, (f64, f64)>,
) {
    let rect_of = |placement: &ComponentPlacement| -> Rect {
        let (w_mm, h_mm) = courtyard_lookup[&placement.id];
        courtyard_rect_for_placement(
            placement,
            w_mm,
            h_mm,
            courtyard_offset_lookup.get(&placement.id).copied(),
            0,
        )
    };

    let mut occupied: Vec<Rect> = placements.iter().map(rect_of).collect();
    let step_nm = mm_to_nm(GRID_PITCH_MM);
    let gap_nm = mm_to_nm(0.5);
    let mut ordered = features.to_vec();
    ordered.sort_by_key(|id| id.0);

    let Some(envelope) = occupied.iter().copied().reduce(|a, b| {
        Rect::new(
            Point::new(a.min.x_nm.min(b.min.x_nm), a.min.y_nm.min(b.min.y_nm)),
            Point::new(a.max.x_nm.max(b.max.x_nm), a.max.y_nm.max(b.max.y_nm)),
        )
    }) else {
        // No circuit, so there is no border to take: these features are the
        // whole board. They were partitioned out of the solver, so nothing
        // has placed them and returning here drops them from the design
        // entirely. Lay them out in one deterministic row instead and let the
        // caller's outline sizing fit the board around it.
        let mut x_nm = 0;
        for id in ordered {
            let (w_mm, h_mm) = courtyard_lookup[&id];
            let half_w = mm_to_nm(w_mm) / 2;
            let half_h = mm_to_nm(h_mm) / 2;
            placements.push(ComponentPlacement {
                id,
                center: Point::new(x_nm + half_w, half_h),
                rotation: Rotation::Zero,
                layer: Layer::Top,
            });
            x_nm += half_w * 2 + gap_nm;
        }
        return;
    };

    let mut claimed: Vec<FeatureSlot> = Vec::with_capacity(features.len());

    for id in ordered {
        let (w_mm, h_mm) = courtyard_lookup[&id];
        let half_w = mm_to_nm(w_mm) / 2;
        let half_h = mm_to_nm(h_mm) / 2;
        let offset = courtyard_offset_lookup
            .get(&id)
            .copied()
            .unwrap_or((0.0, 0.0));
        let band_nm = half_w.max(half_h) * 2 + gap_nm;

        let asked = board
            .component(id)
            .and_then(|component| component.placement_hint.as_ref())
            .and_then(hinted_feature_slot);
        let start = asked
            .and_then(|slot| FEATURE_SLOTS.iter().position(|s| *s == slot))
            .unwrap_or(0);

        let legal = |centre: Point, occupied: &[Rect]| -> Option<Rect> {
            let candidate =
                courtyard_rect_at_anchor(centre, Rotation::Zero, half_w, half_h, offset, gap_nm);
            if occupied.iter().any(|rect| candidate.intersects(rect)) {
                return None;
            }
            if intersects_keepout(candidate, id, board, envelope, |pid| {
                placements
                    .iter()
                    .find(|placement| placement.id == pid)
                    .map(rect_of)
            }) {
                return None;
            }
            Some(candidate)
        };

        // Every grid centre whose courtyard sits inside the envelope and
        // touches its border band, nearest the asked-for anchor first. A
        // second ring outside the envelope follows, for a border with no room
        // left.
        let mut chosen: Option<(Point, Rect)> = None;
        'slots: for turn in 0..FEATURE_SLOTS.len() {
            let slot = FEATURE_SLOTS[(start + turn) % FEATURE_SLOTS.len()];
            if claimed.contains(&slot) && asked != Some(slot) {
                continue;
            }
            let anchor = slot.anchor(envelope);
            for ring in 0..2 {
                let search = if ring == 0 {
                    envelope
                } else {
                    Rect::new(
                        Point::new(envelope.min.x_nm - band_nm, envelope.min.y_nm - band_nm),
                        Point::new(envelope.max.x_nm + band_nm, envelope.max.y_nm + band_nm),
                    )
                };
                let mut candidates: Vec<Point> = Vec::new();
                let mut y = search.min.y_nm + half_h;
                while y <= search.max.y_nm - half_h {
                    let mut x = search.min.x_nm + half_w;
                    while x <= search.max.x_nm - half_w {
                        let in_band = x - half_w <= search.min.x_nm + band_nm
                            || x + half_w >= search.max.x_nm - band_nm
                            || y - half_h <= search.min.y_nm + band_nm
                            || y + half_h >= search.max.y_nm - band_nm;
                        if in_band {
                            candidates.push(Point::new(x, y));
                        }
                        x += step_nm;
                    }
                    y += step_nm;
                }
                candidates.sort_by_key(|point| {
                    (
                        (point.x_nm - anchor.x_nm).abs() + (point.y_nm - anchor.y_nm).abs(),
                        point.y_nm,
                        point.x_nm,
                    )
                });
                for centre in candidates {
                    if let Some(rect) = legal(centre, &occupied) {
                        chosen = Some((centre, rect));
                        claimed.push(slot);
                        break 'slots;
                    }
                }
            }
        }

        if let Some((centre, rect)) = chosen {
            occupied.push(rect);
            placements.push(ComponentPlacement {
                id,
                center: centre,
                rotation: Rotation::Zero,
                layer: Layer::Top,
            });
        } else {
            // Nothing legal anywhere on either ring. Clear of the envelope's
            // far corner is still legal, and is better than on top of a part.
            let centre = Point::new(envelope.max.x_nm + half_w, envelope.max.y_nm + half_h);
            occupied.push(courtyard_rect_at_anchor(
                centre,
                Rotation::Zero,
                half_w,
                half_h,
                offset,
                gap_nm,
            ));
            placements.push(ComponentPlacement {
                id,
                center: centre,
                rotation: Rotation::Zero,
                layer: Layer::Top,
            });
        }
    }
}

/// Half-extents of a component's courtyard after applying its rotation.
///
/// `unrot_half_w`/`unrot_half_h` are the footprint's own (unrotated) half
/// extents; a quarter turn swaps them. This is the single definition used by
/// the greedy search, the legalization sweep, outline compaction and the
/// exporter, so all four agree on where the copper boundary actually is.
fn rotated_half_extents(rotation: Rotation, unrot_half_w: i64, unrot_half_h: i64) -> (i64, i64) {
    match rotation {
        Rotation::Zero | Rotation::OneEighty => (unrot_half_w, unrot_half_h),
        Rotation::Ninety | Rotation::TwoSeventy => (unrot_half_h, unrot_half_w),
    }
}

/// The courtyard rectangle a component occupies, given its placement anchor.
///
/// A placement anchor is footprint-specific — for a long header it is commonly
/// pad 1, not the part's geometric centre — so the courtyard can sit up to
/// half a footprint away from the anchor. Every overlap test must therefore be
/// run against the offset courtyard rather than an anchor-centred box, or the
/// solver can accept a placement that later stages (and the exported board)
/// consider an overlap.
///
/// `extra_nm` widens the result symmetrically; pass a non-zero value to keep a
/// deliberate spacing margin around small passives.
/// Components whose position the design author fixed, as anchor and rotation.
///
/// A pinned component is a constraint on the solver, not an adjustment made
/// after it: see [`place_with_tuning_and_sidecar_seeded`].
pub(crate) type PinnedPlacements = std::collections::HashMap<ComponentId, (Point, Rotation)>;

fn courtyard_rect_at_anchor(
    anchor: Point,
    rotation: Rotation,
    unrot_half_w: i64,
    unrot_half_h: i64,
    offset_mm: (f64, f64),
    extra_nm: i64,
) -> Rect {
    let (half_w, half_h) = rotated_half_extents(rotation, unrot_half_w, unrot_half_h);
    let (off_x, off_y) = rotation.rotate_offset(mm_to_nm(offset_mm.0), mm_to_nm(offset_mm.1));
    Rect::from_center_half_extents(
        Point::new(anchor.x_nm + off_x, anchor.y_nm + off_y),
        half_w + extra_nm,
        half_h + extra_nm,
    )
}

/// The authoritative courtyard rect for an existing placement.
///
/// Thin wrapper over [`courtyard_rect_at_anchor`] for the many call sites that
/// already hold a [`ComponentPlacement`] rather than a raw anchor point.
/// `offset_mm` is the footprint's courtyard-origin offset and may be `None`
/// for parts with no offset data.
pub(crate) fn courtyard_rect_for_placement(
    placement: &ComponentPlacement,
    width_mm: f64,
    height_mm: f64,
    offset_mm: Option<(f64, f64)>,
    extra_nm: i64,
) -> Rect {
    courtyard_rect_at_anchor(
        placement.center,
        placement.rotation,
        mm_to_nm(width_mm) / 2,
        mm_to_nm(height_mm) / 2,
        offset_mm.unwrap_or((0.0, 0.0)),
        extra_nm,
    )
}

/// Deterministic order key for a component under a placement `seed`.
///
/// A splitmix64-style avalanche of `(seed, component id)`. Only used to break
/// ties between components of equal net degree, so it never overrides a
/// meaningful ordering — only the arbitrary one among equals.
fn seeded_order_key(seed: u64, id: u32) -> u64 {
    let mut x = seed ^ u64::from(id).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn place_with_outline<S: ::std::hash::BuildHasher>(
    board: &Board,
    board_outline: Rect,
    courtyards_mm: &[(ComponentId, (f64, f64), (f64, f64))],
    rotation_overrides: &std::collections::HashMap<ComponentId, synth_geometry::Rotation, S>,
    tighten_outline: bool,
    seed: u64,
    pinned: &PinnedPlacements,
) -> Result<Placement, PlaceError> {
    use synth_geometry::nm_to_mm;
    let mut resolved_rotation_overrides: std::collections::HashMap<
        ComponentId,
        synth_geometry::Rotation,
    > = rotation_overrides
        .iter()
        .map(|(id, rotation)| (*id, *rotation))
        .collect();
    let board_w_mm = nm_to_mm(board_outline.width_nm());
    let board_h_mm = nm_to_mm(board_outline.height_nm());
    let is_rp2350_board = board.components.iter().any(|component| {
        component
            .part
            .as_ref()
            .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("rp2350"))
    });
    // Usable area: board outline minus the page margin on every
    // side. Components must keep their courtyards inside this.
    let margin_nm = mm_to_nm(BOARD_MARGIN_MM);
    let usable = Rect::new(
        Point::new(margin_nm, margin_nm),
        Point::new(
            board_outline.max.x_nm - margin_nm,
            board_outline.max.y_nm - margin_nm,
        ),
    );

    // Ordering: Keepout anchors first, then net-graph degree descending.
    // Highly-connected anchors (MCU, USB-C connector) and keepout anchors
    // (Antennas) want priority placement so peripheral components don't block
    // central slots or enter keepout radii.
    let is_keepout_anchor = |id: ComponentId| -> bool {
        let Some(comp) = board.component(id) else {
            return false;
        };
        board.keepouts.iter().any(|k| {
            comp.refdes.to_lowercase() == k.name.to_lowercase()
                || comp.kind.to_lowercase() == k.name.to_lowercase()
                || comp
                    .refdes
                    .to_lowercase()
                    .starts_with(&k.name.to_lowercase())
        })
    };

    let is_macro = |id: ComponentId| -> bool {
        let Some(comp) = board.component(id) else {
            return false;
        };
        if is_keepout_anchor(id) {
            return true;
        }
        match comp.kind.as_str() {
            "mcu" | "processor" | "sensor" | "connector" | "ic" | "regulator" | "power"
            | "memory" => true,
            _ => comp.part.as_ref().is_some_and(|p| p.pins.len() > 2),
        }
    };

    let net_degree = compute_net_degree(board);
    let mut all_components: Vec<ComponentId> = courtyards_mm.iter().map(|(id, _, _)| *id).collect();
    let has_near = |id: ComponentId| -> bool {
        board
            .component(id)
            .is_some_and(|c| c.placement_hint.as_ref().is_some_and(|h| h.near.is_some()))
    };
    let is_dense_connector = |id: ComponentId| -> bool {
        board.component(id).is_some_and(|c| {
            matches!(c.kind.as_str(), "connector" | "jack")
                && c.part.as_ref().is_some_and(|part| part.pins.len() >= 8)
        })
    };
    all_components.sort_by(|a, b| {
        // Functional macros must claim their connected escape corridors
        // before small hinted passives are packed around them. Otherwise a
        // flash or connector can be displaced to the far side of its anchor
        // even though every individual courtyard remains legal.
        let ma = is_macro(*a);
        let mb = is_macro(*b);
        if ma != mb {
            return mb.cmp(&ma);
        }
        let dca = is_dense_connector(*a);
        let dcb = is_dense_connector(*b);
        if dca != dcb {
            return dcb.cmp(&dca);
        }
        let na = has_near(*a);
        let nb = has_near(*b);
        if na != nb {
            return na.cmp(&nb);
        }
        let ka = is_keepout_anchor(*a);
        let kb = is_keepout_anchor(*b);
        if ka != kb {
            return kb.cmp(&ka);
        }
        let da = net_degree.get(&a.0).copied().unwrap_or(0);
        let db = net_degree.get(&b.0).copied().unwrap_or(0);
        if db != da {
            return db.cmp(&da);
        }
        // Equal net degree: the seed reorders only here, so seed 0 reproduces
        // the canonical `ComponentId` order exactly and a non-zero seed gives
        // a different but equally legal packing.
        if seed != 0 {
            let ka = seeded_order_key(seed, a.0);
            let kb = seeded_order_key(seed, b.0);
            if ka != kb {
                return ka.cmp(&kb);
            }
        }
        a.0.cmp(&b.0)
    });

    let has_hint = |id: ComponentId| -> bool {
        board
            .component(id)
            .is_some_and(|c| c.placement_hint.is_some())
    };

    // A mounting hole or fiducial carries no net, so nothing in the solver
    // pulls it toward anything. Placed with the circuit it competes for the
    // same cells, lands wherever the greedy search has room, and then holds
    // the automatic outline out from the inside: on the Class-C SDRAM card
    // four holes asking for the four corners landed in one 32 x 6 mm huddle
    // mid-board and two 1 mm fiducials were packed 25 mm clear of every part
    // that has a net, which is where a third of that board's height came
    // from. They are placed last instead, against the finished circuit
    // envelope, by `place_board_features`.
    let (all_components, board_features): (Vec<ComponentId>, Vec<ComponentId>) = all_components
        .into_iter()
        .partition(|&id| !is_netless_board_feature(board, id));

    let (order, mut passives): (Vec<ComponentId>, Vec<ComponentId>) = all_components
        .into_iter()
        .partition(|&id| is_macro(id) || has_hint(id));
    passives.sort_by_key(|&id| has_near(id));

    // A pinned component is a constraint, so it comes out of both searches:
    // the greedy loop below and the passive packer after it. Leaving it in
    // meant the solver had to find a position for a part whose position was
    // already decided, and then the override moved it anyway -- which is how
    // a board with every part pinned could still fail to place.
    let (order, pinned_from_order): (Vec<ComponentId>, Vec<ComponentId>) =
        order.into_iter().partition(|id| !pinned.contains_key(id));
    let (passives, pinned_from_passives): (Vec<ComponentId>, Vec<ComponentId>) = passives
        .into_iter()
        .partition(|id| !pinned.contains_key(id));
    let (board_features, pinned_from_features): (Vec<ComponentId>, Vec<ComponentId>) =
        board_features
            .into_iter()
            .partition(|id| !pinned.contains_key(id));

    let courtyard_offset_lookup: std::collections::HashMap<ComponentId, (f64, f64)> = courtyards_mm
        .iter()
        .map(|(id, offset, _)| (*id, *offset))
        .collect();
    let courtyard_lookup: std::collections::HashMap<ComponentId, (f64, f64)> = courtyards_mm
        .iter()
        .map(|(id, _, size)| (*id, *size))
        .collect();

    let (modules, _claimed_children) =
        modules::extract_functional_modules(board, &courtyard_lookup);
    let mut fp_targets = floorplan::compute_floorplan_targets(board, usable, &courtyard_lookup);
    // A differential pair may turn the far end of its run so the pair's
    // pads escape toward the near end — but only where the floorplan left
    // the component at identity. The floorplan's non-identity rotations are
    // deliberate (a connector's mating face, a header's row order), and a
    // pair does not get to overrule them.
    for escape in
        modules::pair_escape_rotations(board, &fp_targets, &build_pad_offset_lookup(board))
    {
        if let Some(target) = fp_targets.get_mut(&escape.component) {
            if target.rotation == Rotation::Zero {
                target.rotation = escape.rotation;
            }
        }
    }
    let region_hints = cem::cem_region_assign(board, usable);

    // Child lookup for relative module offset placement and auto-rotation
    let mut child_module_map: std::collections::HashMap<
        ComponentId,
        (ComponentId, Point, Rotation),
    > = std::collections::HashMap::new();
    for module in &modules {
        for member in &module.members {
            child_module_map.insert(
                member.id,
                (module.anchor_id, member.offset_nm, member.rotation),
            );
        }
    }

    // Pad-aware near placement needs the same footprint geometry used by the
    // router. Keeping this lookup here ensures a hard-near passive targets the
    // actual anchor pad, not just the anchor courtyard centre.
    let pad_offsets = build_pad_offset_lookup(board);

    let pitch_nm = mm_to_nm(GRID_PITCH_MM);
    let center_x = usable.min.x_nm + usable.width_nm() / 2;
    let center_y = usable.min.y_nm + usable.height_nm() / 2;

    // Each entry is (component id, placement anchor, courtyard rect). The anchor
    // is what gets exported and what pad offsets are relative to; the rect is
    // the authoritative courtyard, offset-corrected so overlap tests here agree
    // with the legalization sweep and the exported board.
    let mut placed: Vec<(ComponentId, Point, Rect)> = Vec::with_capacity(board.components.len());

    // The pinned courtyards, occupied before the solver takes its first step.
    // No passive clearance is added: the 1.5 mm the greedy search reserves
    // around a loose passive is a spacing choice, and on a pinned part the
    // author has already made it.
    for id in pinned_from_order
        .iter()
        .chain(&pinned_from_passives)
        .chain(&pinned_from_features)
    {
        let Some(&(anchor, rotation)) = pinned.get(id) else {
            continue;
        };
        let Some(&(w_mm, h_mm)) = courtyard_lookup.get(id) else {
            continue;
        };
        let offset = courtyard_offset_lookup
            .get(id)
            .copied()
            .unwrap_or(PLACEMENT_COURTYARD_OFFSET_MM);
        let courtyard = courtyard_rect_at_anchor(
            anchor,
            rotation,
            mm_to_nm(w_mm) / 2,
            mm_to_nm(h_mm) / 2,
            offset,
            0,
        );
        placed.push((*id, anchor, courtyard));
        // So the emitted rotation is the pinned one rather than a floorplan
        // target computed for a part the solver never positioned.
        resolved_rotation_overrides.insert(*id, rotation);
    }

    let mut grid_start_indices = vec![0_usize; order.len()];
    let mut backtracks = 0_usize;

    let mut order_idx = 0_usize;
    while order_idx < order.len() {
        let id = order[order_idx];
        let (w_mm, h_mm) = courtyard_lookup[&id];
        let unrot_half_w = mm_to_nm(w_mm) / 2;
        let unrot_half_h = mm_to_nm(h_mm) / 2;

        let mut rotation = if let Some(&rot) = resolved_rotation_overrides.get(&id) {
            rot
        } else if let Some((_, _, rot)) = child_module_map.get(&id) {
            *rot
        } else if let Some(target) = fp_targets.get(&id) {
            target.rotation
        } else {
            Rotation::Zero
        };

        let comp_kind = board.component(id).map_or("", |c| c.kind.as_str());
        let is_passive =
            comp_kind == "capacitor" || comp_kind == "resistor" || comp_kind == "diode";
        // Clearance the greedy search enforces around small passives, on top
        // of the courtyard. 0.5 mm left too little to route between adjacent
        // 0603s: a 0603 is 1.46 mm tall, so 0.5 mm of slack leaves roughly a
        // 1 mm channel — about two cells on the router's 0.254 mm fine grid,
        // which is marginal once trace width and clearance are subtracted.
        //
        // 1.5 mm is the smallest clearance in a sweep of {0.5, 1.0, 1.5, 2.0} mm
        // that minimises unrouted nets across the repository's example designs
        // (11 at 1.5 and 2.0 mm, 12 at 0.5 mm, 13 at 1.0 mm) — 2.0 mm buys no
        // additional routability but costs ~9% more board area, so 1.5 mm
        // dominates it. This is an empirically tuned value, not a derived one.
        let pad_extra_nm = if is_passive { mm_to_nm(1.5) } else { 0 };
        let (mut half_w, mut half_h) = match rotation {
            Rotation::Zero | Rotation::OneEighty => {
                (unrot_half_w + pad_extra_nm, unrot_half_h + pad_extra_nm)
            }
            Rotation::Ninety | Rotation::TwoSeventy => {
                (unrot_half_h + pad_extra_nm, unrot_half_w + pad_extra_nm)
            }
        };

        // Determine target position for component
        let mut hint_target = None;
        let mut hard_region: Option<Rect> = None;
        let mut dense_connector_edge_override = false;
        if let Some(comp) = board.component(id) {
            if let Some(hint) = &comp.placement_hint {
                let is_dense_connector = matches!(comp.kind.as_str(), "connector" | "jack")
                    && comp.part.as_ref().is_some_and(|part| part.pins.len() >= 8);
                let mut effective_hint = hint.clone();
                if is_dense_connector && hint.priority == synth_ir::PlacementPriority::Hard {
                    // A hard near/region hint on a long header can satisfy
                    // the semantic constraint while placing the header in
                    // the middle of the board, where its pins cannot escape.
                    // Preserve explicit edge hints, but infer the natural
                    // edge from a quadrant when the source omitted one.
                    let is_usb_footprint = comp.part.as_ref().is_some_and(|part| {
                        let id = part.id.0.to_ascii_lowercase();
                        id.contains("usb") || id.contains("type-c")
                    });
                    let has_side_expansion_headers = is_rp2350_board
                        && board.components.iter().any(|candidate| {
                            matches!(candidate.kind.as_str(), "connector" | "jack")
                                && candidate
                                    .part
                                    .as_ref()
                                    .is_some_and(|part| part.pins.len() >= 16)
                                && candidate.placement_hint.as_ref().is_some_and(|other| {
                                    other.priority == synth_ir::PlacementPriority::Hard
                                        && matches!(
                                            other.edge,
                                            Some(
                                                synth_ir::PlacementEdge::Left
                                                    | synth_ir::PlacementEdge::Right
                                            )
                                        )
                                })
                        });
                    let inferred_edge = if is_usb_footprint && has_side_expansion_headers {
                        // Put the USB receptacle on the short top edge when
                        // both long expansion rows consume the side edges.
                        // This is the compact development-board topology used
                        // by human layouts and leaves the side rows as clean
                        // MCU fanout corridors.
                        synth_ir::PlacementEdge::Top
                    } else {
                        hint.edge.clone().unwrap_or(match hint.region {
                            Some(
                                synth_ir::PlacementRegion::TopLeft
                                | synth_ir::PlacementRegion::TopRight
                                | synth_ir::PlacementRegion::TopEdge,
                            ) => synth_ir::PlacementEdge::Top,
                            Some(
                                synth_ir::PlacementRegion::BottomLeft
                                | synth_ir::PlacementRegion::BottomRight
                                | synth_ir::PlacementRegion::BottomEdge,
                            ) => synth_ir::PlacementEdge::Bottom,
                            Some(synth_ir::PlacementRegion::LeftEdge) => {
                                synth_ir::PlacementEdge::Left
                            }
                            Some(synth_ir::PlacementRegion::RightEdge) => {
                                synth_ir::PlacementEdge::Right
                            }
                            _ => synth_ir::PlacementEdge::Bottom,
                        })
                    };
                    effective_hint.edge = Some(inferred_edge.clone());
                    effective_hint.near = None;
                    effective_hint.side = None;
                    let ((_, _), (footprint_w, footprint_h)) = comp.part.as_ref().map_or(
                        ((0.0, 0.0), fallback_courtyard(&comp.kind)),
                        synth_layout::pcb_courtyard_geometry_for_part,
                    );
                    rotation = if is_usb_footprint {
                        // USB-C is a dense connector too, but its mating face
                        // must point away from the selected board edge. Use
                        // the registry metadata so the contact row and shell
                        // are oriented physically, rather than silently
                        // leaving the receptacle facing into the PCB.
                        let edge = match inferred_edge {
                            synth_ir::PlacementEdge::Top => floorplan::BoardEdge::Top,
                            synth_ir::PlacementEdge::Right => floorplan::BoardEdge::Right,
                            synth_ir::PlacementEdge::Bottom => floorplan::BoardEdge::Bottom,
                            synth_ir::PlacementEdge::Left => floorplan::BoardEdge::Left,
                        };
                        comp.part
                            .as_ref()
                            .and_then(|part| part.footprint_dimensions.as_ref())
                            .and_then(|dimensions| dimensions.mating_face)
                            .map_or(Rotation::Zero, |face| {
                                floorplan::rotation_for_mating_edge(face, edge)
                            })
                    } else {
                        match inferred_edge {
                            synth_ir::PlacementEdge::Top | synth_ir::PlacementEdge::Bottom => {
                                if footprint_w >= footprint_h {
                                    Rotation::Zero
                                } else {
                                    Rotation::Ninety
                                }
                            }
                            synth_ir::PlacementEdge::Left | synth_ir::PlacementEdge::Right => {
                                if footprint_w >= footprint_h {
                                    Rotation::Ninety
                                } else {
                                    Rotation::Zero
                                }
                            }
                        }
                    };
                    resolved_rotation_overrides.insert(id, rotation);
                    let (rotated_w, rotated_h) = match rotation {
                        Rotation::Zero | Rotation::OneEighty => (w_mm, h_mm),
                        Rotation::Ninety | Rotation::TwoSeventy => (h_mm, w_mm),
                    };
                    half_w = mm_to_nm(rotated_w) / 2;
                    half_h = mm_to_nm(rotated_h) / 2;
                    dense_connector_edge_override = true;
                }
                let placed_refdes_rects: std::collections::HashMap<String, Rect> = placed
                    .iter()
                    .filter_map(|(pid, _, r)| board.component(*pid).map(|c| (c.refdes.clone(), *r)))
                    .collect();
                let res = resolve_hint_target(&effective_hint, usable, &placed_refdes_rects);
                hint_target = res.target;
                hard_region = res.hard_region;
            }
        }

        // Computed after the dense-connector rewrite above, which is the last
        // point at which `rotation` (and therefore the rotated courtyard offset)
        // can change.
        let courtyard_offset = courtyard_offset_lookup
            .get(&id)
            .copied()
            .unwrap_or((0.0, 0.0));
        let (rot_off_x, rot_off_y) =
            rotation.rotate_offset(mm_to_nm(courtyard_offset.0), mm_to_nm(courtyard_offset.1));
        // Anchor bounds that keep the offset-corrected courtyard inside `usable`.
        let scan_min_x = usable.min.x_nm + half_w - rot_off_x;
        let scan_max_x = usable.max.x_nm - half_w - rot_off_x;
        let scan_min_y = usable.min.y_nm + half_h - rot_off_y;
        let scan_max_y = usable.max.y_nm - half_h - rot_off_y;

        let mut target_point = if let Some(t) = hint_target {
            t
        } else if let Some((anchor_id, rel_offset, _rot)) = child_module_map.get(&id) {
            if let Some((_, _, anchor_rect)) = placed.iter().find(|(pid, _, _)| pid == anchor_id) {
                let ac = Point::new(
                    (anchor_rect.min.x_nm + anchor_rect.max.x_nm) / 2,
                    (anchor_rect.min.y_nm + anchor_rect.max.y_nm) / 2,
                );
                Point::new(ac.x_nm + rel_offset.x_nm, ac.y_nm + rel_offset.y_nm)
            } else {
                fp_targets
                    .get(&id)
                    .map_or_else(|| Point::new(center_x, center_y), |t| t.point)
            }
        } else if let Some(target) = fp_targets.get(&id) {
            target.point
        } else if let Some(target) = region_hints.hints.get(&id) {
            *target
        } else {
            // Target placed connected component or board center
            let mut conn_pos = None;
            for net in &board.nets {
                if net.endpoints.iter().any(|ep| ep.component == id) {
                    for ep in &net.endpoints {
                        if ep.component != id {
                            if let Some((_, _, r)) =
                                placed.iter().find(|(pid, _, _)| *pid == ep.component)
                            {
                                conn_pos = Some(Point::new(
                                    (r.min.x_nm + r.max.x_nm) / 2,
                                    (r.min.y_nm + r.max.y_nm) / 2,
                                ));
                                break;
                            }
                        }
                    }
                }
                if conn_pos.is_some() {
                    break;
                }
            }
            conn_pos.unwrap_or_else(|| Point::new(center_x, center_y))
        };

        // For a hard near hint, prefer the pad where the two components are
        // directly connected. This is especially important for short local
        // paths such as USB series resistors and reset networks: a broad
        // courtyard halo can be legal while still forcing an unnecessarily
        // long or impossible route.
        if let Some(comp) = board.component(id) {
            if let Some(hint) = &comp.placement_hint {
                if hint.priority == synth_ir::PlacementPriority::Hard
                    && !dense_connector_edge_override
                {
                    if let Some(anchor_refdes) = hint.near.as_deref() {
                        let (anchor_id, anchor_pin) = resolve_near_target(board, anchor_refdes)
                            .unwrap_or((ComponentId(u32::MAX), None));
                        if let Some(anchor) = board.component(anchor_id) {
                            if let Some((_, anchor_point, anchor_rect)) = placed
                                .iter()
                                .find(|(placed_id, _, _)| *placed_id == anchor.id)
                            {
                                // Pad offsets are expressed relative to the
                                // placement anchor, not the courtyard centre,
                                // so the synthetic placement below must carry
                                // the anchor.
                                let anchor_center = *anchor_point;
                                let _ = anchor_rect;
                                let anchor_rotation = resolved_rotation_overrides
                                    .get(&anchor.id)
                                    .copied()
                                    .or_else(|| {
                                        child_module_map
                                            .get(&anchor.id)
                                            .map(|(_, _, rotation)| *rotation)
                                    })
                                    .or_else(|| {
                                        fp_targets.get(&anchor.id).map(|target| target.rotation)
                                    })
                                    .unwrap_or(Rotation::Zero);
                                let anchor_placement = ComponentPlacement {
                                    id: anchor.id,
                                    center: anchor_center,
                                    rotation: anchor_rotation,
                                    layer: Layer::Top,
                                };
                                // A `near` that names a pin aims at that pin's pad. Otherwise any
                                // net the two share will do, and the first is
                                // as good as any.
                                'direct_net: for net in &board.nets {
                                    if !net.endpoints.iter().any(|ep| ep.component == id) {
                                        continue;
                                    }
                                    let Some(anchor_endpoint) =
                                        net.endpoints.iter().find(|ep| ep.component == anchor.id)
                                    else {
                                        continue;
                                    };
                                    if anchor_pin.is_some_and(|pin| anchor_endpoint.pin != pin) {
                                        continue;
                                    }
                                    let Some(anchor_offset) = pad_offsets
                                        .lookup(anchor.id, anchor_endpoint.pin.0 as usize)
                                    else {
                                        continue;
                                    };
                                    let Some(component_endpoint) =
                                        net.endpoints.iter().find(|ep| ep.component == id)
                                    else {
                                        continue;
                                    };
                                    let Some(component_offset) =
                                        pad_offsets.lookup(id, component_endpoint.pin.0 as usize)
                                    else {
                                        continue;
                                    };
                                    let anchor_pad =
                                        apply_rotation(anchor_placement, anchor_offset);
                                    let dx = anchor_pad.x_nm - anchor_center.x_nm;
                                    let dy = anchor_pad.y_nm - anchor_center.y_nm;
                                    let (outward_x, outward_y) = if dx.abs() >= dy.abs() {
                                        (dx.signum(), 0)
                                    } else {
                                        (0, dy.signum())
                                    };
                                    let (component_pad_x, component_pad_y) = rotation
                                        .rotate_offset(component_offset.0, component_offset.1);
                                    let clearance = mm_to_nm(1.0);
                                    if is_macro(id)
                                        && !matches!(comp.kind.as_str(), "connector" | "jack")
                                    {
                                        // Macro packages need room for a
                                        // breakout corridor. Target twice
                                        // the anchor-pad vector so the body
                                        // sits outside the anchor courtyard;
                                        // the candidate search then resolves
                                        // the exact legal grid slot.
                                        target_point = Point::new(
                                            anchor_center.x_nm + dx * 3,
                                            anchor_center.y_nm + dy * 3,
                                        );
                                    } else {
                                        target_point = Point::new(
                                            anchor_pad.x_nm + outward_x * clearance
                                                - component_pad_x,
                                            anchor_pad.y_nm + outward_y * clearance
                                                - component_pad_y,
                                        );
                                    }
                                    break 'direct_net;
                                }
                            }
                        }
                    }
                }
            }
        }

        // An explicit hard side relation is a stronger placement contract
        // than the pad-local shortcut above. The shortcut is useful for
        // routing, but it can otherwise put (for example) a flash or USB
        // resistor on the wrong side of the anchor while still being close to
        // one electrically connected pad. Set a directional target here and
        // let the normal legal-grid search resolve collisions between several
        // parts requesting the same side.
        if let Some(comp) = board.component(id) {
            if let Some(hint) = &comp.placement_hint {
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    if let (Some(anchor_refdes), Some(side)) =
                        (hint.near.as_deref(), hint.side.as_ref())
                    {
                        if let Some(anchor) = board
                            .components
                            .iter()
                            .find(|candidate| candidate.refdes.eq_ignore_ascii_case(anchor_refdes))
                        {
                            if let Some((_, _, anchor_rect)) = placed
                                .iter()
                                .find(|(placed_id, _, _)| *placed_id == anchor.id)
                            {
                                let gap = mm_to_nm(1.0);
                                let anchor_center = Point::new(
                                    (anchor_rect.min.x_nm + anchor_rect.max.x_nm) / 2,
                                    (anchor_rect.min.y_nm + anchor_rect.max.y_nm) / 2,
                                );
                                target_point = match side {
                                    synth_ir::PlacementSide::Above => Point::new(
                                        anchor_center.x_nm,
                                        anchor_rect.min.y_nm - half_h - gap,
                                    ),
                                    synth_ir::PlacementSide::Below => Point::new(
                                        anchor_center.x_nm,
                                        anchor_rect.max.y_nm + half_h + gap,
                                    ),
                                    synth_ir::PlacementSide::Left => Point::new(
                                        anchor_rect.min.x_nm - half_w - gap,
                                        anchor_center.y_nm,
                                    ),
                                    synth_ir::PlacementSide::Right => Point::new(
                                        anchor_rect.max.x_nm + half_w + gap,
                                        anchor_center.y_nm,
                                    ),
                                };
                            }
                        }
                    }
                }
            }
        }

        // Apply 2D Density Spreading force only when component has no explicit placement hint
        if hint_target.is_none() {
            let mut rep_dx = 0_i64;
            let mut rep_dy = 0_i64;
            for (_, _, r) in &placed {
                let cx = (r.min.x_nm + r.max.x_nm) / 2;
                let cy = (r.min.y_nm + r.max.y_nm) / 2;
                let dx = target_point.x_nm - cx;
                let dy = target_point.y_nm - cy;
                let dist_sq = (dx * dx + dy * dy).max(mm_to_nm(1.0) * mm_to_nm(1.0));
                if dist_sq < mm_to_nm(15.0) * mm_to_nm(15.0) {
                    rep_dx += (dx * mm_to_nm(5.0)) / (dist_sq / mm_to_nm(1.0));
                    rep_dy += (dy * mm_to_nm(5.0)) / (dist_sq / mm_to_nm(1.0));
                }
            }
            target_point.x_nm += rep_dx;
            target_point.y_nm += rep_dy;
        }

        // Generate grid candidate cells sorted radially by distance to target_point.
        // These are placement *anchor* positions; the overlap test below turns
        // each into the offset-corrected courtyard.
        let mut candidates = Vec::new();
        let mut cy = scan_min_y;
        while cy <= scan_max_y {
            let mut cx = scan_min_x;
            while cx <= scan_max_x {
                let pt = Point::new(cx, cy);
                if let Some(hr) = hard_region {
                    if hr.contains(pt) {
                        candidates.push(pt);
                    }
                } else {
                    candidates.push(pt);
                }
                cx += pitch_nm;
            }
            cy += pitch_nm;
        }

        // Fallback to full grid if hard region filter leaves 0 candidates
        if candidates.is_empty() && hard_region.is_some() {
            let mut cy = scan_min_y;
            while cy <= scan_max_y {
                let mut cx = scan_min_x;
                while cx <= scan_max_x {
                    candidates.push(Point::new(cx, cy));
                    cx += pitch_nm;
                }
                cy += pitch_nm;
            }
        }

        // A hard `near` hint is a routing preference, not permission to make
        // the board unsatisfiable. Rotating an edge-mounted USB connector to
        // face outward can make its local halo too small for an ESD diode or
        // fuse. Search the full legal board after the preferred region while
        // retaining preferred candidates first. Dense edge connectors remain
        // strict and cannot escape their declared edge.
        if hard_region.is_some() && !dense_connector_edge_override {
            let mut cy = scan_min_y;
            while cy <= scan_max_y {
                let mut cx = scan_min_x;
                while cx <= scan_max_x {
                    let pt = Point::new(cx, cy);
                    if !candidates.contains(&pt) {
                        candidates.push(pt);
                    }
                    cx += pitch_nm;
                }
                cy += pitch_nm;
            }
        }

        candidates.sort_by(|a, b| {
            let da = (a.x_nm - target_point.x_nm).abs() + (a.y_nm - target_point.y_nm).abs();
            let db = (b.x_nm - target_point.x_nm).abs() + (b.y_nm - target_point.y_nm).abs();
            da.cmp(&db)
                .then(a.y_nm.cmp(&b.y_nm))
                .then(a.x_nm.cmp(&b.x_nm))
        });

        let mut tried = 0_usize;
        let mut found: Option<Point> = None;
        let start_idx = grid_start_indices[order_idx];

        for (cell_idx, pt) in candidates.iter().enumerate().skip(start_idx) {
            tried += 1;
            let candidate = courtyard_rect_at_anchor(
                *pt,
                rotation,
                unrot_half_w,
                unrot_half_h,
                courtyard_offset,
                pad_extra_nm,
            );
            if placed.iter().all(|(_, _, r)| !candidate.intersects(r))
                && !intersects_keepout(candidate, id, board, board_outline, |pid| {
                    placed
                        .iter()
                        .find(|(placed_id, _, _)| *placed_id == pid)
                        .map(|(_, _, r)| *r)
                })
            {
                found = Some(*pt);
                grid_start_indices[order_idx] = cell_idx;
                break;
            }
        }

        if let Some(centre) = found {
            let courtyard = courtyard_rect_at_anchor(
                centre,
                rotation,
                unrot_half_w,
                unrot_half_h,
                courtyard_offset,
                pad_extra_nm,
            );
            placed.push((id, centre, courtyard));
            order_idx += 1;
        } else {
            if order_idx == 0 || backtracks >= MAX_BACKTRACKS {
                let refdes = board
                    .components
                    .iter()
                    .find(|c| c.id == id)
                    .map_or_else(|| format!("#{}", id.0), |c| c.refdes.clone());
                return Err(PlaceError::NoLegalPosition {
                    refdes,
                    board_w_mm,
                    board_h_mm,
                    tried,
                });
            }
            backtracks += 1;
            placed.pop();
            grid_start_indices[order_idx] = 0;
            order_idx -= 1;
            grid_start_indices[order_idx] += 1;
        }
    }

    // Build the output in IR order so consumers iterating by
    // index see deterministic ordering. `center` is the placement anchor, which
    // is what pad offsets and the exporter are expressed relative to.
    let mut placements: Vec<ComponentPlacement> = placed
        .iter()
        .map(|(id, anchor, _rect)| {
            let center = *anchor;
            let rotation = if let Some(rot) = resolved_rotation_overrides.get(id) {
                *rot
            } else if let Some((_, _, rot)) = child_module_map.get(id) {
                *rot
            } else if let Some(target) = fp_targets.get(id) {
                target.rotation
            } else {
                Rotation::Zero
            };
            ComponentPlacement {
                id: *id,
                center,
                rotation,
                layer: Layer::Top,
            }
        })
        .collect();
    placements.sort_by_key(|p| p.id.0);

    if !passives.is_empty() {
        outline_packer::pack_passives_along_outline(
            board,
            &mut placements,
            &passives,
            &courtyard_lookup,
            &courtyard_offset_lookup,
            &pad_offsets,
            usable,
        )?;
    }

    // Stage 2 refinement: disabled because macro floorplan and pin-relative passive packing
    // already establish human-quality semantic placement. Swapping distorts macro positions.
    // if !board.nets.is_empty() {
    //     refine_swaps(board, &mut placements, &courtyard_lookup, usable);
    // }

    // Closed-Loop Placement DRC Legalization Pass:
    // Verify zero courtyard overlaps among all placed components.
    // If any 2 component courtyards overlap after initial placement/swapping,
    // automatically shift them apart along grid axes until clean pass.
    let mut resolved_overlaps = false;
    let mut pass = 0;
    while !resolved_overlaps && pass < 10 {
        resolved_overlaps = true;
        pass += 1;

        for i in 0..placements.len() {
            let (w_i, h_i) = courtyard_lookup[&placements[i].id];
            let (off_x_i, off_y_i) = courtyard_offset_lookup
                .get(&placements[i].id)
                .copied()
                .unwrap_or((0.0, 0.0));
            let unrot_w_i = mm_to_nm(w_i) / 2;
            let unrot_h_i = mm_to_nm(h_i) / 2;
            let (rw_i, rh_i) = match placements[i].rotation {
                Rotation::Zero | Rotation::OneEighty => (unrot_w_i, unrot_h_i),
                Rotation::Ninety | Rotation::TwoSeventy => (unrot_h_i, unrot_w_i),
            };
            let (rot_cx_i, rot_cy_i) = placements[i]
                .rotation
                .rotate_offset(mm_to_nm(off_x_i), mm_to_nm(off_y_i));
            let c_i = Point::new(
                placements[i].center.x_nm + rot_cx_i,
                placements[i].center.y_nm + rot_cy_i,
            );
            let rect_i = Rect::from_center_half_extents(c_i, rw_i, rh_i);

            for j in (i + 1)..placements.len() {
                let (w_j, h_j) = courtyard_lookup[&placements[j].id];
                let (off_x_j, off_y_j) = courtyard_offset_lookup
                    .get(&placements[j].id)
                    .copied()
                    .unwrap_or((0.0, 0.0));
                let unrot_w_j = mm_to_nm(w_j) / 2;
                let unrot_h_j = mm_to_nm(h_j) / 2;
                let (rw_j, rh_j) = match placements[j].rotation {
                    Rotation::Zero | Rotation::OneEighty => (unrot_w_j, unrot_h_j),
                    Rotation::Ninety | Rotation::TwoSeventy => (unrot_h_j, unrot_w_j),
                };
                let (rot_cx_j, rot_cy_j) = placements[j]
                    .rotation
                    .rotate_offset(mm_to_nm(off_x_j), mm_to_nm(off_y_j));
                let c_j = Point::new(
                    placements[j].center.x_nm + rot_cx_j,
                    placements[j].center.y_nm + rot_cy_j,
                );
                let rect_j = Rect::from_center_half_extents(c_j, rw_j, rh_j);

                if rect_i.intersects(&rect_j) {
                    resolved_overlaps = false;
                    let dx = placements[j].center.x_nm - placements[i].center.x_nm;
                    let dy = placements[j].center.y_nm - placements[i].center.y_nm;
                    let shift_x = if dx >= 0 { pitch_nm * 2 } else { -pitch_nm * 2 };
                    let shift_y = if dy >= 0 { pitch_nm * 2 } else { -pitch_nm * 2 };
                    if dx.abs() >= dy.abs() {
                        placements[j].center.x_nm += shift_x;
                    } else {
                        placements[j].center.y_nm += shift_y;
                    }
                }
            }
        }
    }

    // The circuit is settled, so the envelope it needs is now known and the
    // board features can take the border instead of defining it.
    if !board_features.is_empty() {
        place_board_features(
            board,
            &mut placements,
            &board_features,
            &courtyard_lookup,
            &courtyard_offset_lookup,
        );
        placements.sort_by_key(|p| p.id.0);
    }

    // Tight Adaptive PCB Sizing:
    // Calculate tight bounding box of placed component courtyards using their
    // exact rotated half-extents, add a uniform 5.0mm edge margin on all 4 sides,
    // and shift all component coordinates so (x_min, y_min) aligns to (5mm, 5mm).
    if placements.is_empty() {
        return Ok(Placement {
            board_outline,
            components: placements,
        });
    }

    // Explicit board dimensions are a user/agent constraint. Preserve that
    // rectangle after solving; only automatic sizing should compact the
    // outline around the component envelope.
    if !tighten_outline {
        return Ok(Placement {
            board_outline,
            components: placements,
        });
    }

    let mut min_x_nm = i64::MAX;
    let mut max_x_nm = i64::MIN;
    let mut min_y_nm = i64::MAX;
    let mut max_y_nm = i64::MIN;

    for p in &placements {
        let (w_mm, h_mm) = courtyard_lookup[&p.id];
        let (off_x_mm, off_y_mm) = courtyard_offset_lookup
            .get(&p.id)
            .copied()
            .unwrap_or((0.0, 0.0));
        let unrot_half_w = mm_to_nm(w_mm) / 2;
        let unrot_half_h = mm_to_nm(h_mm) / 2;
        let (rot_half_w, rot_half_h) = match p.rotation {
            Rotation::Zero | Rotation::OneEighty => (unrot_half_w, unrot_half_h),
            Rotation::Ninety | Rotation::TwoSeventy => (unrot_half_h, unrot_half_w),
        };
        let (rot_cx, rot_cy) = p
            .rotation
            .rotate_offset(mm_to_nm(off_x_mm), mm_to_nm(off_y_mm));
        let court_center_x = p.center.x_nm + rot_cx;
        let court_center_y = p.center.y_nm + rot_cy;

        min_x_nm = min_x_nm.min(court_center_x - rot_half_w);
        max_x_nm = max_x_nm.max(court_center_x + rot_half_w);
        min_y_nm = min_y_nm.min(court_center_y - rot_half_h);
        max_y_nm = max_y_nm.max(court_center_y + rot_half_h);
    }

    // Keep a practical assembly/Edge.Cuts margin without letting the
    // automatic outline grow around large empty perimeter bands. Small
    // boards need a proportionate margin; larger boards retain 4 mm. Explicit
    // board dimensions remain authoritative above and do not enter this path.
    let component_span_mm = nm_to_mm((max_x_nm - min_x_nm).max(max_y_nm - min_y_nm));
    let edge_margin_nm = mm_to_nm(if component_span_mm < 30.0 { 2.0 } else { 4.0 });

    let target_left_margin = edge_margin_nm;

    // When a top-edge connector is present, the board's y=0 is the mating face.
    // To keep all OTHER components below the connector's courtyard (and preserve
    // their routing clearance), we compute the y-shift based on the minimum
    // courtyard-top of NON-connector components so they land at `edge_margin_nm`.
    // The connector itself was placed at y = min_y + half_h so its courtyard
    // top (= min_y_nm) lands exactly at y=0 after the shift.
    let shift_y_nm = edge_margin_nm - min_y_nm;

    let shift_x_nm = target_left_margin - min_x_nm;

    for p in &mut placements {
        p.center.x_nm += shift_x_nm;
        p.center.y_nm += shift_y_nm;
    }

    // Run a final legalization pass in the coordinates that will actually be
    // exported. The exporter consumes these final placements verbatim.
    let mut final_legalization_changed = false;
    for _ in 0..32 {
        let mut changed = false;
        for i in 0..placements.len() {
            let (w_i, h_i) = courtyard_lookup[&placements[i].id];
            let (ox_i, oy_i) = courtyard_offset_lookup
                .get(&placements[i].id)
                .copied()
                .unwrap_or((0.0, 0.0));
            let (rcx_i, rcy_i) = placements[i]
                .rotation
                .rotate_offset(mm_to_nm(ox_i), mm_to_nm(oy_i));
            let (hw_i, hh_i) = match placements[i].rotation {
                Rotation::Zero | Rotation::OneEighty => (mm_to_nm(w_i) / 2, mm_to_nm(h_i) / 2),
                Rotation::Ninety | Rotation::TwoSeventy => (mm_to_nm(h_i) / 2, mm_to_nm(w_i) / 2),
            };
            let rect_i = Rect::from_center_half_extents(
                Point::new(
                    placements[i].center.x_nm + rcx_i,
                    placements[i].center.y_nm + rcy_i,
                ),
                hw_i,
                hh_i,
            );
            for j in (i + 1)..placements.len() {
                let i_kind = board
                    .component(placements[i].id)
                    .map_or("", |c| c.kind.as_str());
                let j_kind = board
                    .component(placements[j].id)
                    .map_or("", |c| c.kind.as_str());
                let connector_pair = [i_kind, j_kind]
                    .iter()
                    .all(|kind| *kind == "connector" || *kind == "jack");
                if !connector_pair {
                    continue;
                }
                let (w_j, h_j) = courtyard_lookup[&placements[j].id];
                let (ox_j, oy_j) = courtyard_offset_lookup
                    .get(&placements[j].id)
                    .copied()
                    .unwrap_or((0.0, 0.0));
                let (rcx_j, rcy_j) = placements[j]
                    .rotation
                    .rotate_offset(mm_to_nm(ox_j), mm_to_nm(oy_j));
                let (hw_j, hh_j) = match placements[j].rotation {
                    Rotation::Zero | Rotation::OneEighty => (mm_to_nm(w_j) / 2, mm_to_nm(h_j) / 2),
                    Rotation::Ninety | Rotation::TwoSeventy => {
                        (mm_to_nm(h_j) / 2, mm_to_nm(w_j) / 2)
                    }
                };
                let rect_j = Rect::from_center_half_extents(
                    Point::new(
                        placements[j].center.x_nm + rcx_j,
                        placements[j].center.y_nm + rcy_j,
                    ),
                    hw_j,
                    hh_j,
                );
                if !rect_i.intersects(&rect_j) {
                    continue;
                }
                // Prefer separating along X for edge-mounted connectors so
                // their mating faces remain on the same edge.  The extra
                // pitch makes the invariant strict rather than borderline.
                let overlap_x = rect_i.max.x_nm - rect_j.min.x_nm;
                let separation = overlap_x.max(pitch_nm * 2);
                if placements[j].center.x_nm >= placements[i].center.x_nm {
                    placements[j].center.x_nm += separation;
                } else {
                    placements[j].center.x_nm -= separation;
                }
                changed = true;
                final_legalization_changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Recompute the bounds after final legalization and edge docking.  Using
    // the earlier bounds can clip a connector that was moved to resolve a
    // collision, producing a board that is syntactically valid but physically
    // invalid in KiCad.
    if final_legalization_changed {
        min_x_nm = i64::MAX;
        max_x_nm = i64::MIN;
        min_y_nm = i64::MAX;
        max_y_nm = i64::MIN;
    }
    if final_legalization_changed {
        for p in &placements {
            let (w_mm, h_mm) = courtyard_lookup[&p.id];
            let (ox_mm, oy_mm) = courtyard_offset_lookup
                .get(&p.id)
                .copied()
                .unwrap_or((0.0, 0.0));
            let (rcx, rcy) = p.rotation.rotate_offset(mm_to_nm(ox_mm), mm_to_nm(oy_mm));
            let (hw, hh) = match p.rotation {
                Rotation::Zero | Rotation::OneEighty => (mm_to_nm(w_mm) / 2, mm_to_nm(h_mm) / 2),
                Rotation::Ninety | Rotation::TwoSeventy => (mm_to_nm(h_mm) / 2, mm_to_nm(w_mm) / 2),
            };
            let cx = p.center.x_nm + rcx;
            let cy = p.center.y_nm + rcy;
            min_x_nm = min_x_nm.min(cx - hw);
            max_x_nm = max_x_nm.max(cx + hw);
            min_y_nm = min_y_nm.min(cy - hh);
            max_y_nm = max_y_nm.max(cy + hh);
        }
    }
    // Recompute the final courtyard envelope after every legalization and the
    // initial edge shift. The earlier bounds are in the pre-shift coordinate
    // system and can retain a large empty band, especially when a connector
    // was initially docked to an edge. Normalize the final envelope once more
    // so the generated board is sized from what will actually be exported.
    let mut final_min_x_nm = i64::MAX;
    let mut final_max_x_nm = i64::MIN;
    let mut final_min_y_nm = i64::MAX;
    let mut final_max_y_nm = i64::MIN;
    for p in &placements {
        let (w_mm, h_mm) = courtyard_lookup[&p.id];
        let (off_x_mm, off_y_mm) = courtyard_offset_lookup
            .get(&p.id)
            .copied()
            .unwrap_or((0.0, 0.0));
        let (offset_x, offset_y) = p
            .rotation
            .rotate_offset(mm_to_nm(off_x_mm), mm_to_nm(off_y_mm));
        let (half_w, half_h) = match p.rotation {
            Rotation::Zero | Rotation::OneEighty => (mm_to_nm(w_mm) / 2, mm_to_nm(h_mm) / 2),
            Rotation::Ninety | Rotation::TwoSeventy => (mm_to_nm(h_mm) / 2, mm_to_nm(w_mm) / 2),
        };
        let cx = p.center.x_nm + offset_x;
        let cy = p.center.y_nm + offset_y;
        final_min_x_nm = final_min_x_nm.min(cx - half_w);
        final_max_x_nm = final_max_x_nm.max(cx + half_w);
        final_min_y_nm = final_min_y_nm.min(cy - half_h);
        final_max_y_nm = final_max_y_nm.max(cy + half_h);
    }

    let compact_shift_x_nm = target_left_margin - final_min_x_nm;
    let compact_shift_y_nm = edge_margin_nm - final_min_y_nm;
    for p in &mut placements {
        p.center.x_nm += compact_shift_x_nm;
        p.center.y_nm += compact_shift_y_nm;
    }

    let adaptive_width_nm = (final_max_x_nm - final_min_x_nm) + target_left_margin + edge_margin_nm;
    let adaptive_height_nm = (final_max_y_nm - final_min_y_nm) + edge_margin_nm * 2;

    let mut adaptive_board_outline = Rect::new(
        Point::new(0, 0),
        Point::new(adaptive_width_nm, adaptive_height_nm),
    );

    // The bottom header's edge contract makes the raw envelope include a
    // large vertical gap after the core compaction above. For RP2350 boards,
    // reclaim a bounded portion of that empty span before re-docking the
    // header. Keep a positive margin and let the final placement validation
    // catch any future floorplan that cannot fit this compacting step.
    if is_rp2350_board
        && adaptive_board_outline.height_nm() > mm_to_nm(55.0)
        && placements.iter().any(|placed| {
            let Some(component) = board.component(placed.id) else {
                return false;
            };
            matches!(component.kind.as_str(), "connector" | "jack")
                && component
                    .part
                    .as_ref()
                    .is_some_and(|part| part.pins.len() >= 8)
                && component.placement_hint.as_ref().is_some_and(|hint| {
                    hint.priority == synth_ir::PlacementPriority::Hard
                        && hint.edge == Some(synth_ir::PlacementEdge::Bottom)
                })
        })
    {
        adaptive_board_outline.max.y_nm -= mm_to_nm(8.0);
    }

    if is_rp2350_board {
        let has_bottom_dense_connector = placements.iter().any(|placed| {
            let Some(component) = board.component(placed.id) else {
                return false;
            };
            matches!(component.kind.as_str(), "connector" | "jack")
                && component
                    .part
                    .as_ref()
                    .is_some_and(|part| part.pins.len() >= 8)
                && component.placement_hint.as_ref().is_some_and(|hint| {
                    hint.priority == synth_ir::PlacementPriority::Hard
                        && hint.edge == Some(synth_ir::PlacementEdge::Bottom)
                })
        });
        let has_top_dense_connector = placements.iter().any(|placed| {
            let Some(component) = board.component(placed.id) else {
                return false;
            };
            matches!(component.kind.as_str(), "connector" | "jack")
                && component
                    .part
                    .as_ref()
                    .is_some_and(|part| part.pins.len() >= 8)
                && component.placement_hint.as_ref().is_some_and(|hint| {
                    hint.priority == synth_ir::PlacementPriority::Hard
                        && hint.edge == Some(synth_ir::PlacementEdge::Top)
                })
        });
        if has_bottom_dense_connector && !has_top_dense_connector {
            for placed in &mut placements {
                let Some(component) = board.component(placed.id) else {
                    continue;
                };
                let keep_on_edge = is_edge_flush_connector(component);
                if !keep_on_edge {
                    placed.center.y_nm += mm_to_nm(10.0);
                }
            }
            let lowest_nm = placements
                .iter()
                .filter(|placed| {
                    board
                        .component(placed.id)
                        .is_some_and(|c| !is_edge_flush_connector(c))
                })
                .map(|placed| padded_courtyard_rect(&courtyard_lookup, placed).max.y_nm)
                .max();
            if let Some(lowest_nm) = lowest_nm {
                adaptive_board_outline.max.y_nm = adaptive_board_outline
                    .max
                    .y_nm
                    .max(lowest_nm + edge_margin_nm);
            }
        }
    }

    let has_side_expansion_headers = is_rp2350_board
        && placements.iter().any(|candidate| {
            let Some(other) = board.component(candidate.id) else {
                return false;
            };
            matches!(other.kind.as_str(), "connector" | "jack")
                && other
                    .part
                    .as_ref()
                    .is_some_and(|part| part.pins.len() >= 16)
                && other.placement_hint.as_ref().is_some_and(|hint| {
                    hint.priority == synth_ir::PlacementPriority::Hard
                        && matches!(
                            hint.edge,
                            Some(synth_ir::PlacementEdge::Left | synth_ir::PlacementEdge::Right)
                        )
                })
        });
    for index in 0..placements.len() {
        let mut placement = placements[index];
        let Some(component) = board.component(placement.id) else {
            continue;
        };
        if !is_edge_flush_connector(component) {
            continue;
        }
        let Some(requested_edge) = component
            .placement_hint
            .as_ref()
            .and_then(|hint| hint.edge.as_ref())
        else {
            continue;
        };
        let usb_on_compact_top_edge = is_rp2350_board
            && component.part.as_ref().is_some_and(|part| {
                let id = part.id.0.to_ascii_lowercase();
                id.contains("usb") || id.contains("type-c")
            })
            && has_side_expansion_headers;
        let edge = if usb_on_compact_top_edge {
            &synth_ir::PlacementEdge::Top
        } else {
            requested_edge
        };
        let courtyard = sidecar_courtyard_rect(board, component, &placement);
        let (half_w, half_h) = (courtyard.width_nm() / 2, courtyard.height_nm() / 2);
        let min_center_x = adaptive_board_outline.min.x_nm + edge_margin_nm + half_w;
        let max_center_x = adaptive_board_outline.max.x_nm - edge_margin_nm - half_w;
        let min_center_y = adaptive_board_outline.min.y_nm + edge_margin_nm + half_h;
        let max_center_y = adaptive_board_outline.max.y_nm - edge_margin_nm - half_h;
        match edge {
            synth_ir::PlacementEdge::Top => {
                placement.center.y_nm = adaptive_board_outline.min.y_nm + half_h;
                placement.center.x_nm = placement.center.x_nm.clamp(min_center_x, max_center_x);
            }
            synth_ir::PlacementEdge::Bottom => {
                placement.center.y_nm = adaptive_board_outline.max.y_nm - half_h;
                placement.center.x_nm = placement.center.x_nm.clamp(min_center_x, max_center_x);
            }
            synth_ir::PlacementEdge::Left => {
                placement.center.x_nm = adaptive_board_outline.min.x_nm + half_w;
                placement.center.y_nm = placement.center.y_nm.clamp(min_center_y, max_center_y);
            }
            synth_ir::PlacementEdge::Right => {
                placement.center.x_nm = adaptive_board_outline.max.x_nm - half_w;
                placement.center.y_nm = placement.center.y_nm.clamp(min_center_y, max_center_y);
            }
        }
        enforce_edge_copper_clearance(component, &mut placement, adaptive_board_outline, edge);
        let along_x = matches!(
            edge,
            synth_ir::PlacementEdge::Top | synth_ir::PlacementEdge::Bottom
        );
        let (min_along, max_along) = if along_x {
            (min_center_x, max_center_x)
        } else {
            (min_center_y, max_center_y)
        };
        let is_clear = |candidate: &ComponentPlacement| {
            placements.iter().all(|other| {
                other.id == candidate.id
                    || !padded_courtyard_rect(&courtyard_lookup, candidate)
                        .intersects(&padded_courtyard_rect(&courtyard_lookup, other))
            })
        };
        let pitch_nm = mm_to_nm(GRID_PITCH_MM);
        let slot = std::iter::once(0)
            .chain((1..=MAX_DOCKING_SLIDE_STEPS).flat_map(|step| [step, -step]))
            .find_map(|step| {
                let mut candidate = placement;
                let along = if along_x {
                    &mut candidate.center.x_nm
                } else {
                    &mut candidate.center.y_nm
                };
                *along += step * pitch_nm;
                ((min_along..=max_along).contains(along) && is_clear(&candidate))
                    .then_some(candidate)
            });
        if let Some(slot) = slot {
            placements[index] = slot;
        }
    }

    // The RP2350 edge connectors are legalized after the first adaptive
    // outline is computed. Recompute the outline from the final courtyard
    // envelope so the initial solver canvas cannot leave a large unused
    // band below or beside the board. This preserves every relative
    // component position and the hard edge contracts while producing the
    // compact outline expected of a development board.
    if is_rp2350_board {
        let outline = outline_with_flush_connectors(
            board,
            &placements,
            mm_to_nm(EDGE_MARGIN_MM),
            |_, _, placed| padded_courtyard_rect(&courtyard_lookup, placed),
            false,
        );
        if let Some(outline) = outline {
            for placement in &mut placements {
                placement.center.x_nm -= outline.min.x_nm;
                placement.center.y_nm -= outline.min.y_nm;
            }
            adaptive_board_outline = Rect::new(
                Point::new(0, 0),
                Point::new(outline.width_nm(), outline.height_nm()),
            );
        }
    }

    Ok(Placement {
        board_outline: adaptive_board_outline,
        components: placements,
    })
}

fn exported_pad_bounds(
    component: &synth_ir::Component,
    placement: &ComponentPlacement,
) -> Option<Rect> {
    let part = component.part.as_ref()?;
    let pads = part
        .kicad_footprint
        .as_deref()
        .and_then(synth_layout::kicad_footprint_loader::pads)
        .or_else(|| synth_layout::kicad_footprint_loader::synth_part_pads(part))?;
    let origin = footprint_origin(Some(part), placement);
    let mut min_x = i64::MAX;
    let mut max_x = i64::MIN;
    let mut min_y = i64::MAX;
    let mut max_y = i64::MIN;
    for pad in pads {
        let local = placement
            .rotation
            .rotate_offset(mm_to_nm(pad.center_mm.0), mm_to_nm(pad.center_mm.1));
        let center = Point::new(origin.x_nm + local.0, origin.y_nm + local.1);
        let (w, h) = if placement.rotation.swaps_extents() {
            (mm_to_nm(pad.size_mm.1), mm_to_nm(pad.size_mm.0))
        } else {
            (mm_to_nm(pad.size_mm.0), mm_to_nm(pad.size_mm.1))
        };
        min_x = min_x.min(center.x_nm - w / 2);
        max_x = max_x.max(center.x_nm + w / 2);
        min_y = min_y.min(center.y_nm - h / 2);
        max_y = max_y.max(center.y_nm + h / 2);
    }
    (min_x <= max_x).then(|| Rect::new(Point::new(min_x, min_y), Point::new(max_x, max_y)))
}

/// Shift an edge-mounted connector using its actual transformed pad bounds.
/// Courtyard offsets describe the body, not necessarily the outermost PTH
/// copper, and some stock footprints have a large local anchor offset. The
/// exported KiCad board must keep every copper pad at least the manufacturer
/// edge clearance inside Edge.Cuts.
fn enforce_edge_copper_clearance(
    component: &synth_ir::Component,
    placement: &mut ComponentPlacement,
    outline: Rect,
    edge: &synth_ir::PlacementEdge,
) {
    let Some(pads) = exported_pad_bounds(component, placement) else {
        return;
    };
    let (min_x, max_x, min_y, max_y) = (pads.min.x_nm, pads.max.x_nm, pads.min.y_nm, pads.max.y_nm);
    let margin = mm_to_nm(COPPER_EDGE_CLEARANCE_MM);
    match edge {
        synth_ir::PlacementEdge::Left => {
            placement.center.x_nm += (outline.min.x_nm + margin - min_x).max(0);
            placement.center.y_nm += (outline.min.y_nm + margin - min_y).max(0);
            placement.center.y_nm -= (max_y - (outline.max.y_nm - margin)).max(0);
        }
        synth_ir::PlacementEdge::Right => {
            placement.center.x_nm -= (max_x - (outline.max.x_nm - margin)).max(0);
            placement.center.y_nm += (outline.min.y_nm + margin - min_y).max(0);
            placement.center.y_nm -= (max_y - (outline.max.y_nm - margin)).max(0);
        }
        synth_ir::PlacementEdge::Top => {
            placement.center.y_nm += (outline.min.y_nm + margin - min_y).max(0);
            placement.center.x_nm += (outline.min.x_nm + margin - min_x).max(0);
            placement.center.x_nm -= (max_x - (outline.max.x_nm - margin)).max(0);
        }
        synth_ir::PlacementEdge::Bottom => {
            placement.center.y_nm -= (max_y - (outline.max.y_nm - margin)).max(0);
            placement.center.x_nm += (outline.min.x_nm + margin - min_x).max(0);
            placement.center.x_nm -= (max_x - (outline.max.x_nm - margin)).max(0);
        }
    }
}

/// True when `candidate` falls inside any keepout circle.
///
/// `lookup` resolves an already-placed component's courtyard rect; callers
/// differ in how they track placements, so this is a closure rather than a
/// concrete slice type. A missing rect means "not placed yet", which falls back
/// to the board centre — the same fallback used when no anchor matches.
pub(crate) fn intersects_keepout(
    candidate: Rect,
    id: ComponentId,
    board: &Board,
    board_outline: Rect,
    lookup: impl Fn(ComponentId) -> Option<Rect>,
) -> bool {
    let candidate_center = Point::new(
        (candidate.min.x_nm + candidate.max.x_nm) / 2,
        (candidate.min.y_nm + candidate.max.y_nm) / 2,
    );
    for keepout in &board.keepouts {
        let radius_mm = keepout.radius.map_or(0.0, synth_ir::Length::to_mm);
        let keepout_radius_nm = mm_to_nm(radius_mm);
        if keepout_radius_nm <= 0 {
            continue;
        }

        let anchor = board.components.iter().find(|c| {
            c.refdes.to_lowercase() == keepout.name.to_lowercase()
                || c.kind.to_lowercase() == keepout.name.to_lowercase()
                || c.refdes
                    .to_lowercase()
                    .starts_with(&keepout.name.to_lowercase())
        });

        let keepout_center = if let Some(anchor) = anchor {
            if anchor.id == id {
                continue;
            }
            lookup(anchor.id)
                .map(|r| Point::new((r.min.x_nm + r.max.x_nm) / 2, (r.min.y_nm + r.max.y_nm) / 2))
                .or_else(|| {
                    Some(Point::new(
                        board_outline.max.x_nm / 2,
                        board_outline.max.y_nm / 2,
                    ))
                })
        } else {
            Some(Point::new(
                board_outline.max.x_nm / 2,
                board_outline.max.y_nm / 2,
            ))
        };

        if let Some(ac) = keepout_center {
            let dx = (candidate_center.x_nm - ac.x_nm).abs();
            let dy = (candidate_center.y_nm - ac.y_nm).abs();
            if dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
                < keepout_radius_nm.saturating_mul(keepout_radius_nm)
            {
                return true;
            }
        }
    }

    false
}

#[allow(dead_code)]
fn placements_valid(
    board: &Board,
    placements: &[ComponentPlacement],
    courtyard_lookup: &std::collections::HashMap<ComponentId, (f64, f64)>,
    usable: Rect,
) -> bool {
    let placed_rects: Vec<(ComponentId, Rect)> = placements
        .iter()
        .map(|p| {
            let (w_mm, h_mm) = courtyard_lookup.get(&p.id).copied().unwrap_or((10.0, 10.0));
            let unrot_half_w = mm_to_nm(w_mm) / 2;
            let unrot_half_h = mm_to_nm(h_mm) / 2;
            let (rw, rh) = match p.rotation {
                Rotation::Zero | Rotation::OneEighty => (unrot_half_w, unrot_half_h),
                Rotation::Ninety | Rotation::TwoSeventy => (unrot_half_h, unrot_half_w),
            };
            let r = Rect::from_center_half_extents(p.center, rw, rh);
            (p.id, r)
        })
        .collect();

    let margin_nm = mm_to_nm(BOARD_MARGIN_MM);
    let board_outline = Rect::new(
        Point::new(0, 0),
        Point::new(usable.max.x_nm + margin_nm, usable.max.y_nm + margin_nm),
    );

    for (id, r) in &placed_rects {
        if r.min.x_nm < usable.min.x_nm
            || r.min.y_nm < usable.min.y_nm
            || r.max.x_nm > usable.max.x_nm
            || r.max.y_nm > usable.max.y_nm
        {
            return false;
        }
        if intersects_keepout(*r, *id, board, board_outline, |pid| {
            placed_rects
                .iter()
                .find(|(placed_id, _)| *placed_id == pid)
                .map(|(_, r)| *r)
        }) {
            return false;
        }
    }

    for i in 0..placed_rects.len() {
        for j in (i + 1)..placed_rects.len() {
            if placed_rects[i].1.intersects(&placed_rects[j].1) {
                return false;
            }
        }
    }

    courtyards_non_overlapping(placements, courtyard_lookup)
}

#[allow(dead_code)]
const REFINE_MAX_ATTEMPTS: usize = 200_000;

#[allow(dead_code)]
fn refine_swaps(
    board: &Board,
    placements: &mut [ComponentPlacement],
    courtyard_lookup: &std::collections::HashMap<ComponentId, (f64, f64)>,
    usable: Rect,
) {
    let pad_offsets = build_pad_offset_lookup(board);
    let cluster_pairs = build_cluster_pairs(board);
    let mut cost = total_cost(board, placements, &pad_offsets, &cluster_pairs);
    let mut attempts = 0_usize;
    loop {
        let mut improved = false;
        for i in 0..placements.len() {
            for j in (i + 1)..placements.len() {
                attempts += 1;
                if attempts > REFINE_MAX_ATTEMPTS {
                    return;
                }
                let size_i = courtyard_lookup.get(&placements[i].id);
                let size_j = courtyard_lookup.get(&placements[j].id);
                let same_size = match (size_i, size_j) {
                    (Some((wi, hi)), Some((wj, hj))) => {
                        (wi - wj).abs() <= 0.01 && (hi - hj).abs() <= 0.01
                    }
                    _ => false,
                };
                if !same_size {
                    continue;
                }

                // Kind safety guard: ICs, sensors, connectors must match kind exactly;
                // passives may swap with passives if dimensions match.
                let comp_i = board.component(placements[i].id);
                let comp_j = board.component(placements[j].id);
                let kinds_compatible = match (comp_i, comp_j) {
                    (Some(ci), Some(cj)) => {
                        if ci.kind == cj.kind {
                            true
                        } else {
                            let is_passive = |k: &str| {
                                matches!(k, "resistor" | "capacitor" | "inductor" | "diode" | "led")
                            };
                            is_passive(&ci.kind) && is_passive(&cj.kind)
                        }
                    }
                    _ => false,
                };
                if !kinds_compatible {
                    continue;
                }
                // Try swapping centres.
                let centre_i = placements[i].center;
                let centre_j = placements[j].center;
                placements[i].center = centre_j;
                placements[j].center = centre_i;

                let valid = placements_valid(board, placements, courtyard_lookup, usable);
                let new_cost = if valid {
                    total_cost(board, placements, &pad_offsets, &cluster_pairs)
                } else {
                    i64::MAX
                };

                if valid && new_cost < cost {
                    cost = new_cost;
                    improved = true;
                } else {
                    placements[i].center = centre_i;
                    placements[j].center = centre_j;
                }
            }
        }
        if !improved {
            break;
        }
    }
}

/// Weight applied to cluster-cohesion cost, per nanometer of
/// L1 distance between an IC and its decoupling cap. The HPWL
/// term is in nm; multiplying cluster distance by ~16× pushes
/// caps into adjacent grid cells of their IC even when the
/// power net's bbox doesn't change much from a swap.
///
/// Why a multiplier matters: VCC nets typically span the whole
/// board (every IC and every cap touches them), so the HPWL
/// bbox is dominated by the outliers — moving one cap closer
/// to its IC barely changes the net's bbox. The cluster term
/// is the placer's only signal that "this cap belongs *here*".
#[allow(dead_code)]
const CLUSTER_WEIGHT: i64 = 16;

/// Combined cost: HPWL + cluster cohesion. The refinement loop
/// minimises this single scalar; both terms are in nm-units so
/// they add directly.
#[allow(dead_code)]
fn total_cost(
    board: &Board,
    placements: &[ComponentPlacement],
    pad_offsets: &PadOffsetLookup,
    cluster_pairs: &[(ComponentId, ComponentId)],
) -> i64 {
    hpwl_total(board, placements, pad_offsets)
        .saturating_add(cluster_cohesion(placements, cluster_pairs))
}

/// Sum of L1 distance between every (anchor, member) cluster
/// pair, multiplied by [`CLUSTER_WEIGHT`]. L1 (Manhattan) is
/// the natural metric for orthogonal PCB routes and is cheaper
/// than Euclidean — square roots in a hot inner loop are
/// unnecessary.
#[allow(dead_code)]
fn cluster_cohesion(
    placements: &[ComponentPlacement],
    pairs: &[(ComponentId, ComponentId)],
) -> i64 {
    let by_id: std::collections::HashMap<ComponentId, Point> =
        placements.iter().map(|p| (p.id, p.center)).collect();
    let mut total = 0_i64;
    for (anchor, member) in pairs {
        let Some(a) = by_id.get(anchor) else { continue };
        let Some(m) = by_id.get(member) else { continue };
        let dx = (a.x_nm - m.x_nm).abs();
        let dy = (a.y_nm - m.y_nm).abs();
        total = total.saturating_add(dx.saturating_add(dy).saturating_mul(CLUSTER_WEIGHT));
    }
    total
}

/// Recognise (anchor, member) pairs that the placer should
/// pull tight.
///
/// Every functional cluster the compiler recognizes becomes one
/// cohesion pair per member: an IC with its decoupling caps, a crystal
/// with its load caps, a connector with its ESD diodes, an antenna with
/// its matching network. The pairs come from the shared recognition
/// pass, so whatever the schematic groups with an anchor is what the
/// board placer is asked to keep together — the same fact, read by two
/// consumers.
///
/// A member belongs to exactly one cluster, so the first anchor to claim
/// it wins and a capacitor cannot be pulled toward two parts at once.
///
/// Returned pairs are deterministic: recognition order is IR component
/// order, and member order within a cluster is net-endpoint order.
#[allow(dead_code)]
fn build_cluster_pairs(board: &Board) -> Vec<(ComponentId, ComponentId)> {
    let mut out = Vec::new();
    let mut claimed: std::collections::HashSet<ComponentId> = std::collections::HashSet::new();
    for cluster in synth_ir::recognize_clusters(board) {
        for member in &cluster.members {
            if claimed.insert(member.component) {
                out.push((cluster.anchor, member.component));
            }
        }
    }
    out
}

/// Half-perimeter wire length across every multi-endpoint net.
/// Standard placement metric — bounding-box semiperimeter of the
/// pad positions on the net.
pub(crate) fn hpwl_total(
    board: &Board,
    placements: &[ComponentPlacement],
    pad_offsets: &PadOffsetLookup,
) -> i64 {
    let by_id: std::collections::HashMap<ComponentId, &ComponentPlacement> =
        placements.iter().map(|p| (p.id, p)).collect();
    let mut total: i64 = 0;
    for net in &board.nets {
        if net.endpoints.len() < 2 {
            continue;
        }
        let mut min_x = i64::MAX;
        let mut max_x = i64::MIN;
        let mut min_y = i64::MAX;
        let mut max_y = i64::MIN;
        let mut found = false;
        for endpoint in &net.endpoints {
            let Some(placement) = by_id.get(&endpoint.component) else {
                continue;
            };
            let pad_offset_nm = pad_offsets
                .lookup(endpoint.component, endpoint.pin.0 as usize)
                .unwrap_or((0, 0));
            let pos = apply_rotation(**placement, pad_offset_nm);
            min_x = min_x.min(pos.x_nm);
            max_x = max_x.max(pos.x_nm);
            min_y = min_y.min(pos.y_nm);
            max_y = max_y.max(pos.y_nm);
            found = true;
        }
        if found {
            total = total.saturating_add((max_x - min_x) + (max_y - min_y));
        }
    }
    total
}

/// True when no two placed courtyards intersect. The slice-2
/// hard invariant; the refinement loop must never break it.
#[allow(dead_code)]
fn courtyards_non_overlapping(
    placements: &[ComponentPlacement],
    courtyard_lookup: &std::collections::HashMap<ComponentId, (f64, f64)>,
) -> bool {
    let rects: Vec<Rect> = placements
        .iter()
        .map(|p| {
            let (w_mm, h_mm) = courtyard_lookup.get(&p.id).copied().unwrap_or((10.0, 10.0));
            let unrot_half_w = mm_to_nm(w_mm) / 2;
            let unrot_half_h = mm_to_nm(h_mm) / 2;
            let (rw, rh) = match p.rotation {
                Rotation::Zero | Rotation::OneEighty => (unrot_half_w, unrot_half_h),
                Rotation::Ninety | Rotation::TwoSeventy => (unrot_half_h, unrot_half_w),
            };
            Rect::from_center_half_extents(p.center, rw, rh)
        })
        .collect();
    for i in 0..rects.len() {
        for j in (i + 1)..rects.len() {
            if rects[i].intersects(&rects[j]) {
                return false;
            }
        }
    }
    true
}

/// Apply a placement's rotation to a footprint-local pad offset
/// and return the pad's absolute nanometer position. Uses the
/// shared KiCad file-convention helper so placer beliefs about
/// pin positions match what pcbnew renders from the export.
fn apply_rotation(placement: ComponentPlacement, offset_nm: (i64, i64)) -> Point {
    let (rx, ry) = placement.rotation.rotate_offset(offset_nm.0, offset_nm.1);
    Point::new(
        placement.center.x_nm.saturating_add(rx),
        placement.center.y_nm.saturating_add(ry),
    )
}

/// Per-(component, pin-index) → footprint pad offset in nm.
pub(crate) struct PadOffsetLookup {
    /// `map[component][pin_idx]` → offset. Outer key is dense
    /// in `ComponentId.0`; we use a `HashMap` to avoid relying
    /// on contiguous ids.
    map: std::collections::HashMap<ComponentId, Vec<Option<(i64, i64)>>>,
}

impl PadOffsetLookup {
    pub(crate) fn lookup(&self, component: ComponentId, pin_idx: usize) -> Option<(i64, i64)> {
        self.map.get(&component)?.get(pin_idx).copied().flatten()
    }
}

/// For every component in `board`, look up its footprint and
/// build a `pin_idx → pad_offset_nm` table. Components without
/// a footprint mapping contribute a vector of `None`s and
/// effectively place all their pads at the component centre —
/// the HPWL contribution is still valid, just less precise.
pub(crate) fn build_pad_offset_lookup(board: &Board) -> PadOffsetLookup {
    use synth_layout::{kicad_footprint_loader, pcb_courtyard_geometry_for_part};
    let mut map = std::collections::HashMap::new();
    for component in &board.components {
        let pins = component
            .part
            .as_ref()
            .map_or(&[][..], |p| p.pins.as_slice());
        let footprint_pads = component
            .part
            .as_ref()
            .and_then(|p| p.kicad_footprint.as_deref())
            .and_then(kicad_footprint_loader::pads);
        // ComponentPlacement.center is the courtyard centre. The KiCad
        // exporter puts the footprint origin at center - rot(courtyard
        // offset), so a pad sits at center + rot(pad - offset). Every
        // consumer of pad positions must subtract the same offset or
        // placement will target coordinates different from the exported PCB
        // (the router and DRC already do).
        let (origin_x_mm, origin_y_mm) = component
            .part
            .as_ref()
            .map_or((0.0, 0.0), |part| pcb_courtyard_geometry_for_part(part).0);
        let origin_offset = (mm_to_nm(origin_x_mm), mm_to_nm(origin_y_mm));
        let mut row: Vec<Option<(i64, i64)>> = Vec::with_capacity(pins.len());
        for pin in pins {
            let entry = footprint_pads
                .as_ref()
                .and_then(|pads| pads.iter().find(|p| p.number == pin.number.0))
                .map(|pad| {
                    (
                        mm_to_nm(pad.center_mm.0) - origin_offset.0,
                        mm_to_nm(pad.center_mm.1) - origin_offset.1,
                    )
                });
            row.push(entry);
        }
        map.insert(component.id, row);
    }
    PadOffsetLookup { map }
}

/// Split a `near` hint into the component it anchors to and, when the hint
/// names one, the pin.
///
/// `near: U1` anchors to the part; `near: U1.dvdd` anchors to a specific
/// pad of it, which is what an author needs when a part's pins sit on
/// different sides and only one of them is the right neighbour — a
/// decoupling capacitor belongs beside the rail pin it decouples, not
/// beside the part's centre of mass.
pub(crate) fn resolve_near_target(
    board: &Board,
    near: &str,
) -> Option<(ComponentId, Option<PinId>)> {
    let (refdes, pin_name) = match near.split_once('.') {
        Some((refdes, pin)) => (refdes, Some(pin)),
        None => (near, None),
    };
    let component = board
        .components
        .iter()
        .find(|candidate| candidate.refdes.eq_ignore_ascii_case(refdes))?;
    let pin = match pin_name {
        Some(name) => {
            let part = component.part.as_ref()?;
            let index = part
                .pins
                .iter()
                .position(|p| p.name.eq_ignore_ascii_case(name))?;
            Some(PinId(u32::try_from(index).ok()?))
        }
        None => None,
    };
    Some((component.id, pin))
}

/// Net-graph degree per component: how many distinct nets
/// include at least one pin on that component. Used as the
/// placement-ordering heuristic.
fn compute_net_degree(board: &Board) -> std::collections::HashMap<u32, usize> {
    let mut counts = std::collections::HashMap::new();
    for net in &board.nets {
        let mut seen = std::collections::HashSet::new();
        for endpoint in &net.endpoints {
            if seen.insert(endpoint.component) {
                *counts.entry(endpoint.component.0).or_insert(0_usize) += 1;
            }
        }
    }
    counts
}

impl Placement {
    pub fn component_by_refdes<'a>(
        &'a self,
        board: &Board,
        refdes: &str,
    ) -> Option<&'a ComponentPlacement> {
        let comp = board.components.iter().find(|c| c.refdes == refdes)?;
        self.components.iter().find(|p| p.id == comp.id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExternalHint {
    #[serde(default)]
    pub component: Option<String>,
    #[serde(default)]
    pub components: Vec<String>,
    pub region: Option<String>,
    pub edge: Option<String>,
    pub near: Option<String>,
    pub side: Option<String>,
    pub priority: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HintSatisfactionReport {
    pub hints: Vec<HintOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HintOutcome {
    pub component: String,
    pub status: HintStatus,
    pub actual_region: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HintStatus {
    Honoured,
    RelaxedToSoft,
    FallbackUsed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementDescription {
    pub board_size_mm: [f64; 2],
    pub component_regions: Vec<ComponentRegionEntry>,
    pub cluster_summary: String,
    /// `Some(true)`/`Some(false)` only when a DRC run has actually been
    /// performed on this placement; `None` means "not evaluated".
    ///
    /// Placement review does not run DRC, so this is always `None` here. It
    /// must never be defaulted to `true`: a placement that has not been routed
    /// or checked cannot be asserted clean.
    pub drc_clean: Option<bool>,
    /// `Some(count)` only when routing has actually been performed; `None`
    /// means "not evaluated". Always `None` from placement review.
    pub unrouted_nets: Option<usize>,
    pub dense_regions: Vec<DenseRegionWarning>,
    /// Actionable warnings for an agent reviewing whether the placement is
    /// electrically purposeful and visually production-like.
    pub functional_warnings: Vec<String>,
    /// Structural visual review emitted before routing. This is the
    /// machine-readable equivalent of a quick human PCB-layout scan; an
    /// agent should revise placement when `requires_revision` is true.
    pub visual_review: VisualPlacementReview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualPlacementReview {
    pub score: u8,
    pub requires_revision: bool,
    pub findings: Vec<String>,
    /// Machine-actionable suggestions for the next placement iteration. These
    /// are advisory: the agent or human must review the resulting geometry.
    #[serde(default)]
    pub recommended_actions: Vec<PlacementRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementRecommendation {
    pub action: String,
    pub refdes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_x_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_y_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_rotation_deg: Option<u32>,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentRegionEntry {
    pub component: String,
    pub kind: String,
    pub region: String,
    pub mm: [f64; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DenseRegionWarning {
    pub region: String,
    pub density: f64,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct HintResolution {
    pub target: Option<Point>,
    pub hard_region: Option<Rect>,
}

#[allow(clippy::implicit_hasher)]
pub fn resolve_hint_target(
    hint: &synth_ir::PlacementConstraint,
    usable: Rect,
    placed_refdes: &std::collections::HashMap<String, Rect>,
) -> HintResolution {
    let width_nm = usable.width_nm();
    let height_nm = usable.height_nm();
    let min_x = usable.min.x_nm;
    let min_y = usable.min.y_nm;
    let max_x = usable.max.x_nm;
    let max_y = usable.max.y_nm;
    let cx = min_x + width_nm / 2;
    let cy = min_y + height_nm / 2;

    let edge_band_nm = mm_to_nm(15.0).min(width_nm / 3).min(height_nm / 3);

    let (mut target, mut hard_region) = (None, None);

    if let Some(region) = &hint.region {
        match region {
            synth_ir::PlacementRegion::TopLeft => {
                target = Some(Point::new(min_x + width_nm / 4, min_y + height_nm / 4));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(usable.min, Point::new(cx, cy)));
                }
            }
            synth_ir::PlacementRegion::TopRight => {
                target = Some(Point::new(
                    min_x + (3 * width_nm) / 4,
                    min_y + height_nm / 4,
                ));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(Point::new(cx, min_y), Point::new(max_x, cy)));
                }
            }
            synth_ir::PlacementRegion::BottomLeft => {
                target = Some(Point::new(
                    min_x + width_nm / 4,
                    min_y + (3 * height_nm) / 4,
                ));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(Point::new(min_x, cy), Point::new(cx, max_y)));
                }
            }
            synth_ir::PlacementRegion::BottomRight => {
                target = Some(Point::new(
                    min_x + (3 * width_nm) / 4,
                    min_y + (3 * height_nm) / 4,
                ));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(Point::new(cx, cy), usable.max));
                }
            }
            synth_ir::PlacementRegion::Centre => {
                target = Some(Point::new(cx, cy));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::from_center_half_extents(
                        Point::new(cx, cy),
                        width_nm / 4,
                        height_nm / 4,
                    ));
                }
            }
            synth_ir::PlacementRegion::TopEdge => {
                target = Some(Point::new(cx, min_y + edge_band_nm / 2));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(
                        usable.min,
                        Point::new(max_x, min_y + edge_band_nm),
                    ));
                }
            }
            synth_ir::PlacementRegion::BottomEdge => {
                target = Some(Point::new(cx, max_y - edge_band_nm / 2));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(
                        Point::new(min_x, max_y - edge_band_nm),
                        usable.max,
                    ));
                }
            }
            synth_ir::PlacementRegion::LeftEdge => {
                target = Some(Point::new(min_x + edge_band_nm / 2, cy));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(
                        usable.min,
                        Point::new(min_x + edge_band_nm, max_y),
                    ));
                }
            }
            synth_ir::PlacementRegion::RightEdge => {
                target = Some(Point::new(max_x - edge_band_nm / 2, cy));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(
                        Point::new(max_x - edge_band_nm, min_y),
                        usable.max,
                    ));
                }
            }
        }
    }

    if let Some(edge) = &hint.edge {
        match edge {
            synth_ir::PlacementEdge::Top => {
                target = Some(Point::new(
                    target.map_or(cx, |t| t.x_nm),
                    min_y + edge_band_nm / 2,
                ));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(
                        usable.min,
                        Point::new(max_x, min_y + edge_band_nm),
                    ));
                }
            }
            synth_ir::PlacementEdge::Bottom => {
                target = Some(Point::new(
                    target.map_or(cx, |t| t.x_nm),
                    max_y - edge_band_nm / 2,
                ));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(
                        Point::new(min_x, max_y - edge_band_nm),
                        usable.max,
                    ));
                }
            }
            synth_ir::PlacementEdge::Left => {
                target = Some(Point::new(
                    min_x + edge_band_nm / 2,
                    target.map_or(cy, |t| t.y_nm),
                ));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(
                        usable.min,
                        Point::new(min_x + edge_band_nm, max_y),
                    ));
                }
            }
            synth_ir::PlacementEdge::Right => {
                target = Some(Point::new(
                    max_x - edge_band_nm / 2,
                    target.map_or(cy, |t| t.y_nm),
                ));
                if hint.priority == synth_ir::PlacementPriority::Hard {
                    hard_region = Some(Rect::new(
                        Point::new(max_x - edge_band_nm, min_y),
                        usable.max,
                    ));
                }
            }
        }
    }

    if let Some(anchor_refdes) = &hint.near {
        // `near` may name a pin (`U1.dvdd`); the halo and the side offset
        // are relative to the part either way, so only the refdes is
        // needed here. The pin is honoured by the pad-local target.
        let refdes = anchor_refdes
            .split_once('.')
            .map_or(anchor_refdes.as_str(), |(refdes, _)| refdes);
        let anchor_lower = refdes.to_lowercase();
        let anchor_rect = placed_refdes
            .iter()
            .find(|(k, _)| k.to_lowercase() == anchor_lower)
            .map(|(_, v)| *v);
        if let Some(ar) = anchor_rect {
            let ac = Point::new(
                (ar.min.x_nm + ar.max.x_nm) / 2,
                (ar.min.y_nm + ar.max.y_nm) / 2,
            );
            let offset_nm = mm_to_nm(3.0);
            let near_target = match hint.side {
                Some(synth_ir::PlacementSide::Above) => {
                    Point::new(ac.x_nm, ar.min.y_nm - offset_nm)
                }
                Some(synth_ir::PlacementSide::Below) => {
                    Point::new(ac.x_nm, ar.max.y_nm + offset_nm)
                }
                Some(synth_ir::PlacementSide::Left) => Point::new(ar.min.x_nm - offset_nm, ac.y_nm),
                Some(synth_ir::PlacementSide::Right) => {
                    Point::new(ar.max.x_nm + offset_nm, ac.y_nm)
                }
                None => Point::new(ar.max.x_nm + offset_nm, ac.y_nm),
            };
            target = Some(near_target);
            if hint.priority == synth_ir::PlacementPriority::Hard {
                // A hard `near` hint is a connectivity constraint, not merely
                // a region preference. Keep the candidate centre within a
                // bounded 8 mm halo around the anchor courtyard while the
                // pad-aware target above ranks the locally useful positions.
                // local connections (USB series parts, reset parts, etc.) do
                // not silently land on the far side of the board. If several
                // hard-near parts cannot all fit, the normal bounded fallback
                // still preserves a legal placement rather than failing the
                // whole board.
                let margin = mm_to_nm(8.0);
                hard_region = Some(Rect::new(
                    Point::new(ar.min.x_nm - margin, ar.min.y_nm - margin),
                    Point::new(ar.max.x_nm + margin, ar.max.y_nm + margin),
                ));
            }
        }
    }

    HintResolution {
        target,
        hard_region,
    }
}

pub fn place_with_hints(
    board: &Board,
    external_hints: &[ExternalHint],
) -> Result<(Placement, HintSatisfactionReport), PlaceError> {
    place_with_hints_compacting(board, external_hints, true)
}

/// [`place_with_hints`], optionally without compaction. A caller that is about
/// to apply a placement sidecar passes false: the sidecar was written against
/// the uncompacted layout, and every part it leaves alone must stay put.
pub fn place_with_hints_compacting(
    board: &Board,
    external_hints: &[ExternalHint],
    compact: bool,
) -> Result<(Placement, HintSatisfactionReport), PlaceError> {
    let modified_board = board_with_external_hints(board, external_hints);
    let placement = place_with_default_tuning(&modified_board, compact)?;
    Ok((
        placement.clone(),
        hint_satisfaction(&modified_board, &placement),
    ))
}

/// Place with semantic hints inside a caller-requested board outline. This is
/// intentionally a placement-only seam: it does not route or relax visual
/// review, and an infeasible outline returns the normal structured error.
pub fn place_with_hints_and_dimensions(
    board: &Board,
    external_hints: &[ExternalHint],
    width_mm: f64,
    height_mm: f64,
) -> Result<(Placement, HintSatisfactionReport), PlaceError> {
    let modified_board = board_with_external_hints(board, external_hints);
    let placement = place_with_dimensions(&modified_board, width_mm, height_mm)?;
    Ok((
        placement.clone(),
        hint_satisfaction(&modified_board, &placement),
    ))
}

fn board_with_external_hints(board: &Board, external_hints: &[ExternalHint]) -> Board {
    let mut modified_board = board.clone();
    for hint in external_hints {
        let mut target_refdes = Vec::new();
        if let Some(r) = &hint.component {
            target_refdes.push(r.clone());
        }
        target_refdes.extend(hint.components.clone());

        let parsed_constraint = synth_ir::PlacementConstraint {
            region: hint.region.as_deref().and_then(parse_region_str),
            edge: hint.edge.as_deref().and_then(parse_edge_str),
            near: hint.near.clone(),
            side: hint.side.as_deref().and_then(parse_side_str),
            priority: match hint.priority.as_deref() {
                Some("hard") => synth_ir::PlacementPriority::Hard,
                _ => synth_ir::PlacementPriority::Soft,
            },
        };

        for refdes in target_refdes {
            if let Some(comp) = modified_board
                .components
                .iter_mut()
                .find(|c| c.refdes == refdes)
            {
                comp.placement_hint = Some(parsed_constraint.clone());
            }
        }
    }
    modified_board
}

fn hint_satisfaction(board: &Board, placement: &Placement) -> HintSatisfactionReport {
    let mut outcomes = Vec::new();
    let margin_nm = mm_to_nm(BOARD_MARGIN_MM);
    let usable = Rect::new(
        Point::new(
            placement.board_outline.min.x_nm + margin_nm,
            placement.board_outline.min.y_nm + margin_nm,
        ),
        Point::new(
            placement.board_outline.max.x_nm - margin_nm,
            placement.board_outline.max.y_nm - margin_nm,
        ),
    );

    for comp in &board.components {
        if let Some(hint) = &comp.placement_hint {
            if let Some(placed_comp) = placement.components.iter().find(|p| p.id == comp.id) {
                let actual_region = classify_region_name(placed_comp.center, usable);
                let cx = usable.min.x_nm + usable.width_nm() / 2;
                let cy = usable.min.y_nm + usable.height_nm() / 2;

                let status = if let Some(req_region) = &hint.region {
                    let is_honoured = match req_region {
                        synth_ir::PlacementRegion::TopLeft => {
                            placed_comp.center.x_nm <= cx && placed_comp.center.y_nm <= cy
                        }
                        synth_ir::PlacementRegion::TopRight => {
                            placed_comp.center.x_nm >= cx && placed_comp.center.y_nm <= cy
                        }
                        synth_ir::PlacementRegion::BottomLeft => {
                            placed_comp.center.x_nm <= cx && placed_comp.center.y_nm >= cy
                        }
                        synth_ir::PlacementRegion::BottomRight => {
                            placed_comp.center.x_nm >= cx && placed_comp.center.y_nm >= cy
                        }
                        synth_ir::PlacementRegion::Centre => {
                            (placed_comp.center.x_nm - cx).abs() <= usable.width_nm() / 4
                                && (placed_comp.center.y_nm - cy).abs() <= usable.height_nm() / 4
                        }
                        synth_ir::PlacementRegion::TopEdge => {
                            placed_comp.center.y_nm <= usable.min.y_nm + mm_to_nm(15.0)
                        }
                        synth_ir::PlacementRegion::BottomEdge => {
                            placed_comp.center.y_nm >= usable.max.y_nm - mm_to_nm(15.0)
                        }
                        synth_ir::PlacementRegion::LeftEdge => {
                            placed_comp.center.x_nm <= usable.min.x_nm + mm_to_nm(15.0)
                        }
                        synth_ir::PlacementRegion::RightEdge => {
                            placed_comp.center.x_nm >= usable.max.x_nm - mm_to_nm(15.0)
                        }
                    };

                    if is_honoured {
                        HintStatus::Honoured
                    } else if hint.priority == synth_ir::PlacementPriority::Hard {
                        HintStatus::RelaxedToSoft
                    } else {
                        HintStatus::FallbackUsed
                    }
                } else {
                    HintStatus::Honoured
                };

                outcomes.push(HintOutcome {
                    component: comp.refdes.clone(),
                    status,
                    actual_region,
                });
            }
        }
    }

    HintSatisfactionReport { hints: outcomes }
}

#[allow(dead_code)]
fn region_to_snake(r: &synth_ir::PlacementRegion) -> &'static str {
    match r {
        synth_ir::PlacementRegion::TopLeft => "top_left",
        synth_ir::PlacementRegion::TopRight => "top_right",
        synth_ir::PlacementRegion::BottomLeft => "bottom_left",
        synth_ir::PlacementRegion::BottomRight => "bottom_right",
        synth_ir::PlacementRegion::Centre => "centre",
        synth_ir::PlacementRegion::TopEdge => "top_edge",
        synth_ir::PlacementRegion::BottomEdge => "bottom_edge",
        synth_ir::PlacementRegion::LeftEdge => "left_edge",
        synth_ir::PlacementRegion::RightEdge => "right_edge",
    }
}

fn parse_region_str(s: &str) -> Option<synth_ir::PlacementRegion> {
    match s.to_lowercase().as_str() {
        "top_left" => Some(synth_ir::PlacementRegion::TopLeft),
        "top_right" => Some(synth_ir::PlacementRegion::TopRight),
        "bottom_left" => Some(synth_ir::PlacementRegion::BottomLeft),
        "bottom_right" => Some(synth_ir::PlacementRegion::BottomRight),
        "centre" | "center" => Some(synth_ir::PlacementRegion::Centre),
        "top_edge" => Some(synth_ir::PlacementRegion::TopEdge),
        "bottom_edge" => Some(synth_ir::PlacementRegion::BottomEdge),
        "left_edge" => Some(synth_ir::PlacementRegion::LeftEdge),
        "right_edge" => Some(synth_ir::PlacementRegion::RightEdge),
        _ => None,
    }
}

fn parse_edge_str(s: &str) -> Option<synth_ir::PlacementEdge> {
    match s.to_lowercase().as_str() {
        "top" => Some(synth_ir::PlacementEdge::Top),
        "bottom" => Some(synth_ir::PlacementEdge::Bottom),
        "left" => Some(synth_ir::PlacementEdge::Left),
        "right" => Some(synth_ir::PlacementEdge::Right),
        _ => None,
    }
}

fn parse_side_str(s: &str) -> Option<synth_ir::PlacementSide> {
    match s.to_lowercase().as_str() {
        "above" => Some(synth_ir::PlacementSide::Above),
        "below" => Some(synth_ir::PlacementSide::Below),
        "left" => Some(synth_ir::PlacementSide::Left),
        "right" => Some(synth_ir::PlacementSide::Right),
        _ => None,
    }
}

pub fn classify_region_name(pt: Point, usable: Rect) -> String {
    let cx = usable.min.x_nm + usable.width_nm() / 2;
    let cy = usable.min.y_nm + usable.height_nm() / 2;
    let edge_band = mm_to_nm(15.0)
        .min(usable.width_nm() / 3)
        .min(usable.height_nm() / 3);

    if pt.y_nm <= usable.min.y_nm + edge_band {
        return "top_edge".into();
    }
    if pt.y_nm >= usable.max.y_nm - edge_band {
        return "bottom_edge".into();
    }
    if pt.x_nm <= usable.min.x_nm + edge_band {
        return "left_edge".into();
    }
    if pt.x_nm >= usable.max.x_nm - edge_band {
        return "right_edge".into();
    }

    match (pt.x_nm <= cx, pt.y_nm <= cy) {
        (true, true) => "top_left".into(),
        (false, true) => "top_right".into(),
        (true, false) => "bottom_left".into(),
        (false, false) => "bottom_right".into(),
    }
}

// Area-ratio math in the body casts nanometer integers to f64.
// Board envelopes stay far below 2^53 nm, so the cast is exact for
// every representable board; the allow documents that bound.
#[allow(clippy::cast_precision_loss)]
pub fn describe_placement(board: &Board, placement: &Placement) -> PlacementDescription {
    let board_w_mm = synth_geometry::nm_to_mm(placement.board_outline.width_nm());
    let board_h_mm = synth_geometry::nm_to_mm(placement.board_outline.height_nm());
    let margin_nm = mm_to_nm(BOARD_MARGIN_MM);
    let usable = Rect::new(
        Point::new(
            placement.board_outline.min.x_nm + margin_nm,
            placement.board_outline.min.y_nm + margin_nm,
        ),
        Point::new(
            placement.board_outline.max.x_nm - margin_nm,
            placement.board_outline.max.y_nm - margin_nm,
        ),
    );

    let mut component_regions = Vec::new();
    let mut region_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for comp in &board.components {
        if let Some(p) = placement.components.iter().find(|p| p.id == comp.id) {
            let region = classify_region_name(p.center, usable);
            *region_counts.entry(region.clone()).or_default() += 1;
            component_regions.push(ComponentRegionEntry {
                component: comp.refdes.clone(),
                kind: comp.kind.clone(),
                region,
                mm: [
                    synth_geometry::nm_to_mm(p.center.x_nm),
                    synth_geometry::nm_to_mm(p.center.y_nm),
                ],
            });
        }
    }

    let mut kind_groups: std::collections::HashMap<String, (String, Vec<String>)> =
        std::collections::HashMap::new();
    for entry in &component_regions {
        let (_region, list) = kind_groups
            .entry(entry.kind.clone())
            .or_insert_with(|| (entry.region.clone(), Vec::new()));
        list.push(entry.component.clone());
    }

    let mut summaries = Vec::new();
    for (kind, (region, comps)) in kind_groups {
        let comp_str = comps.join(", ");
        summaries.push(format!("{kind} ({comp_str}) in {region}"));
    }
    let cluster_summary = if summaries.is_empty() {
        "Empty placement".into()
    } else {
        summaries.join(". ")
    };

    let total_comps = board.components.len();
    let mut dense_regions = Vec::new();
    for (region, count) in region_counts {
        #[allow(clippy::cast_precision_loss)]
        let ratio = (count as f64) / (total_comps.max(1) as f64);
        if ratio > 0.6 && total_comps > 5 {
            dense_regions.push(DenseRegionWarning {
                region: region.clone(),
                density: ratio,
                note: format!("high component concentration in {region}"),
            });
        }
    }

    let center_of = |id: ComponentId| {
        placement
            .components
            .iter()
            .find(|placed| placed.id == id)
            .map(|placed| placed.center)
    };
    let distance_mm = |a: Point, b: Point| {
        let dx = synth_geometry::nm_to_mm(a.x_nm - b.x_nm);
        let dy = synth_geometry::nm_to_mm(a.y_nm - b.y_nm);
        (dx * dx + dy * dy).sqrt()
    };
    let distance_to_segment_mm = |point: Point, start: Point, end: Point| {
        let px = synth_geometry::nm_to_mm(point.x_nm);
        let py = synth_geometry::nm_to_mm(point.y_nm);
        let sx = synth_geometry::nm_to_mm(start.x_nm);
        let sy = synth_geometry::nm_to_mm(start.y_nm);
        let ex = synth_geometry::nm_to_mm(end.x_nm);
        let ey = synth_geometry::nm_to_mm(end.y_nm);
        let dx = ex - sx;
        let dy = ey - sy;
        let length_squared = dx * dx + dy * dy;
        let t = if length_squared <= f64::EPSILON {
            0.0
        } else {
            (((px - sx) * dx + (py - sy) * dy) / length_squared).clamp(0.0, 1.0)
        };
        let closest_x = sx + t * dx;
        let closest_y = sy + t * dy;
        let distance = ((px - closest_x).powi(2) + (py - closest_y).powi(2)).sqrt();
        (distance, t)
    };
    let mut functional_warnings = Vec::new();
    let mut visual_findings = Vec::new();
    let mut recommended_actions = Vec::new();
    let rp2350_id = board.components.iter().find_map(|component| {
        component.part.as_ref().and_then(|part| {
            part.id
                .0
                .to_ascii_lowercase()
                .contains("rp2350")
                .then_some(component.id)
        })
    });
    if let Some(mcu_id) = rp2350_id {
        if let Some(mcu_center) = center_of(mcu_id) {
            if let Some(flash) = board.components.iter().find(|component| {
                component.kind == "memory"
                    || component.kind == "flash"
                    || component
                        .part
                        .as_ref()
                        .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("w25q"))
            }) {
                if let Some(flash_center) = center_of(flash.id) {
                    let distance = distance_mm(mcu_center, flash_center);
                    if distance > 12.0 {
                        functional_warnings.push(format!(
                            "QSPI flash {} is {:.1} mm from RP2350; target <= 12 mm and keep it on the MCU side of the board",
                            flash.refdes, distance
                        ));
                    }
                }
            }
            for component in &board.components {
                if component.kind != "capacitor" {
                    continue;
                }
                let is_mcu_decoupler = component.placement_hint.as_ref().is_some_and(|hint| {
                    board
                        .component(mcu_id)
                        .is_some_and(|mcu| hint.near.as_deref() == Some(mcu.refdes.as_str()))
                });
                if is_mcu_decoupler {
                    if let Some(cap_center) = center_of(component.id) {
                        let distance = distance_mm(mcu_center, cap_center);
                        if distance > 8.0 {
                            functional_warnings.push(format!(
                                "MCU decoupler {} is {:.1} mm from RP2350; target <= 8 mm",
                                component.refdes, distance
                            ));
                        }
                    }
                }
            }

            // USB_DP/USB_DM series resistors are short escape components,
            // not generic MCU-near passives. Keep them close enough for the
            // agent to refine the pad-side approach without hard-coding a
            // potentially wrong orientation into the deterministic placer.
            for component in &board.components {
                if component.kind != "resistor" {
                    continue;
                }
                let is_usb_series = board.nets.iter().any(|net| {
                    net.endpoints
                        .iter()
                        .any(|endpoint| endpoint.component == component.id)
                        && net.endpoints.iter().any(|endpoint| {
                            board
                                .pin(endpoint.component, endpoint.pin)
                                .is_some_and(|pin| matches!(pin.name.as_str(), "USB_DP" | "USB_DM"))
                        })
                });
                if !is_usb_series {
                    continue;
                }
                if let Some(resistor_center) = center_of(component.id) {
                    let distance = distance_mm(mcu_center, resistor_center);
                    if distance > 8.0 {
                        functional_warnings.push(format!(
                            "USB series resistor {} is {:.1} mm from RP2350; refine it near the MCU USB pad edge (target <= 8 mm)",
                            component.refdes, distance
                        ));
                    }
                }
            }
        }
    }

    // Sidecar placement is intentionally authoritative, but it must remain
    // observable as it is refined by an agent or a human. Report physical
    // courtyard collisions here so the next placement iteration can fix them
    // before spending time in the router. This also catches relative sidecar
    // overrides that move a component into an otherwise legal automatic slot.
    let mut courtyard_rects = Vec::with_capacity(placement.components.len());
    for placed in &placement.components {
        let Some(component) = board.component(placed.id) else {
            continue;
        };
        let (width, height) = component.part.as_ref().map_or_else(
            || fallback_courtyard(&component.kind),
            |part| synth_layout::pcb_courtyard_geometry_for_part(part).1,
        );
        let (rotated_width, rotated_height) = match placed.rotation {
            Rotation::Zero | Rotation::OneEighty => (width, height),
            Rotation::Ninety | Rotation::TwoSeventy => (height, width),
        };
        courtyard_rects.push((
            component.refdes.as_str(),
            Rect::from_center_half_extents(
                placed.center,
                mm_to_nm(rotated_width) / 2,
                mm_to_nm(rotated_height) / 2,
            ),
        ));
    }

    // A generic electrical-cluster shortcut may intentionally choose a
    // nearby legal slot instead of the exact relative side requested by the
    // source hint. That can still be a poor human layout (and can make the
    // intended breakout direction impossible), so expose the mismatch before
    // routing. Keep this as a review finding rather than silently moving the
    // part: the agent may have a stronger mechanical or connector constraint
    // that should decide the next iteration.
    for component in &board.components {
        let Some(hint) = component.placement_hint.as_ref() else {
            continue;
        };
        let (Some(near), Some(side)) = (hint.near.as_deref(), hint.side.as_ref()) else {
            continue;
        };
        if hint.priority != synth_ir::PlacementPriority::Hard {
            continue;
        }
        let Some(anchor) = board
            .components
            .iter()
            .find(|candidate| candidate.refdes == near)
        else {
            continue;
        };
        let Some(component_placement) = placement.components.iter().find(|p| p.id == component.id)
        else {
            continue;
        };
        let Some(anchor_placement) = placement.components.iter().find(|p| p.id == anchor.id) else {
            continue;
        };
        let component_center = component_placement.center;
        let anchor_center = anchor_placement.center;
        let delta_x = synth_geometry::nm_to_mm(component_center.x_nm - anchor_center.x_nm);
        let delta_y = synth_geometry::nm_to_mm(component_center.y_nm - anchor_center.y_nm);
        let tolerance_mm = 0.5;
        let side_honored = match side {
            synth_ir::PlacementSide::Above => delta_y <= -tolerance_mm,
            synth_ir::PlacementSide::Below => delta_y >= tolerance_mm,
            synth_ir::PlacementSide::Left => delta_x <= -tolerance_mm,
            synth_ir::PlacementSide::Right => delta_x >= tolerance_mm,
        };
        if side_honored {
            continue;
        }
        let side_name = match side {
            synth_ir::PlacementSide::Above => "above",
            synth_ir::PlacementSide::Below => "below",
            synth_ir::PlacementSide::Left => "left of",
            synth_ir::PlacementSide::Right => "right of",
        };
        let actual_relation = if delta_x.abs() >= delta_y.abs() {
            if delta_x < 0.0 {
                "left of"
            } else {
                "right of"
            }
        } else if delta_y < 0.0 {
            "above"
        } else {
            "below"
        };
        let warning = format!(
            "hard placement hint not honored: {} should be {} {}, but its courtyard is {} {} (delta {:.1}, {:.1} mm); revise placement before routing",
            component.refdes, side_name, near, actual_relation, near, delta_x, delta_y
        );
        functional_warnings.push(warning.clone());
        visual_findings.push(warning.clone());
        let suggested_offset_mm = 6.0;
        let (suggested_x, suggested_y) = match side {
            synth_ir::PlacementSide::Above => (
                synth_geometry::nm_to_mm(anchor_placement.center.x_nm),
                synth_geometry::nm_to_mm(anchor_placement.center.y_nm) - suggested_offset_mm,
            ),
            synth_ir::PlacementSide::Below => (
                synth_geometry::nm_to_mm(anchor_placement.center.x_nm),
                synth_geometry::nm_to_mm(anchor_placement.center.y_nm) + suggested_offset_mm,
            ),
            synth_ir::PlacementSide::Left => (
                synth_geometry::nm_to_mm(anchor_placement.center.x_nm) - suggested_offset_mm,
                synth_geometry::nm_to_mm(anchor_placement.center.y_nm),
            ),
            synth_ir::PlacementSide::Right => (
                synth_geometry::nm_to_mm(anchor_placement.center.x_nm) + suggested_offset_mm,
                synth_geometry::nm_to_mm(anchor_placement.center.y_nm),
            ),
        };
        recommended_actions.push(PlacementRecommendation {
            action: "honor_hard_relative_side_hint".into(),
            refdes: component.refdes.clone(),
            relative_to: Some(anchor.refdes.clone()),
            suggested_x_mm: Some(suggested_x),
            suggested_y_mm: Some(suggested_y),
            suggested_rotation_deg: None,
            rationale: format!(
                "place {} {} {} with its physical courtyard center at least {:.1} mm from {}",
                component.refdes, side_name, near, suggested_offset_mm, near
            ),
        });
    }

    for i in 0..courtyard_rects.len() {
        for j in (i + 1)..courtyard_rects.len() {
            if courtyard_rects[i].1.intersects(&courtyard_rects[j].1) {
                let overlap_warning = format!(
                    "courtyard overlap: {} with {}; adjust placement before routing",
                    courtyard_rects[i].0, courtyard_rects[j].0
                );
                functional_warnings.push(overlap_warning.clone());
                // A physical courtyard collision is not merely advisory:
                // KiCad/assembly will reject it and routing cannot repair
                // the footprint geometry. Surface it as a visual-gate
                // finding so the agent must revise the sidecar first.
                visual_findings.push(overlap_warning);

                let first = courtyard_rects[i].1;
                let second = courtyard_rects[j].1;
                let overlap_x = (first.max.x_nm.min(second.max.x_nm)
                    - first.min.x_nm.max(second.min.x_nm))
                .max(mm_to_nm(0.5));
                let overlap_y = (first.max.y_nm.min(second.max.y_nm)
                    - first.min.y_nm.max(second.min.y_nm))
                .max(mm_to_nm(0.5));
                let Some(second_component) = board
                    .components
                    .iter()
                    .find(|component| component.refdes == courtyard_rects[j].0)
                else {
                    continue;
                };
                let Some(second_center) = center_of(second_component.id) else {
                    continue;
                };
                let first_center = Point::new(
                    (first.min.x_nm + first.max.x_nm) / 2,
                    (first.min.y_nm + first.max.y_nm) / 2,
                );
                let (shift_x, shift_y, axis) = if overlap_x <= overlap_y {
                    let direction = if second_center.x_nm >= first_center.x_nm {
                        1
                    } else {
                        -1
                    };
                    (direction * (overlap_x + mm_to_nm(1.0)), 0, "X")
                } else {
                    let direction = if second_center.y_nm >= first_center.y_nm {
                        1
                    } else {
                        -1
                    };
                    (0, direction * (overlap_y + mm_to_nm(1.0)), "Y")
                };
                recommended_actions.push(PlacementRecommendation {
                    action: "separate_courtyards".into(),
                    refdes: second_component.refdes.clone(),
                    relative_to: None,
                    suggested_x_mm: Some(synth_geometry::nm_to_mm(second_center.x_nm + shift_x)),
                    suggested_y_mm: Some(synth_geometry::nm_to_mm(second_center.y_nm + shift_y)),
                    suggested_rotation_deg: None,
                    rationale: format!(
                        "move {} away from {} by the minimum separating distance on the {} axis",
                        second_component.refdes, courtyard_rects[i].0, axis
                    ),
                });
            }
        }
    }

    // Sidecar coordinates are intentionally authoritative, but an exact
    // manual/agent move must not leave a courtyard outside Edge.Cuts. Catch
    // this before routing; otherwise the router can spend time on geometry
    // that KiCad will reject immediately.
    let board_keepin_nm = mm_to_nm(1.0);
    for (refdes, rect) in &courtyard_rects {
        let outside = rect.min.x_nm < placement.board_outline.min.x_nm
            || rect.min.y_nm < placement.board_outline.min.y_nm
            || rect.max.x_nm > placement.board_outline.max.x_nm
            || rect.max.y_nm > placement.board_outline.max.y_nm;
        if !outside {
            continue;
        }
        visual_findings.push(format!(
            "courtyard for {refdes} extends outside the board outline; move it inside Edge.Cuts before routing"
        ));
        functional_warnings.push(format!(
            "courtyard for {refdes} is outside the board outline"
        ));
        let Some(component) = board.components.iter().find(|c| c.refdes == *refdes) else {
            continue;
        };
        let Some(current) = center_of(component.id) else {
            continue;
        };
        let half_w = rect.width_nm() / 2;
        let half_h = rect.height_nm() / 2;
        let min_center_x = placement.board_outline.min.x_nm + board_keepin_nm + half_w;
        let max_center_x = placement.board_outline.max.x_nm - board_keepin_nm - half_w;
        let min_center_y = placement.board_outline.min.y_nm + board_keepin_nm + half_h;
        let max_center_y = placement.board_outline.max.y_nm - board_keepin_nm - half_h;
        let clamp_center = |value: i64, min_value: i64, max_value: i64| {
            if min_value <= max_value {
                value.clamp(min_value, max_value)
            } else {
                (min_value + max_value) / 2
            }
        };
        let target = Point::new(
            clamp_center(current.x_nm, min_center_x, max_center_x),
            clamp_center(current.y_nm, min_center_y, max_center_y),
        );
        recommended_actions.push(PlacementRecommendation {
            action: "keep_courtyard_inside_board".into(),
            refdes: (*refdes).to_string(),
            relative_to: None,
            suggested_x_mm: Some(synth_geometry::nm_to_mm(target.x_nm)),
            suggested_y_mm: Some(synth_geometry::nm_to_mm(target.y_nm)),
            suggested_rotation_deg: None,
            rationale: "keep the complete courtyard at least 1 mm inside Edge.Cuts".into(),
        });
    }

    // Human-style first-pass review of dense connector composition. A
    // high-pin connector should be edge-mounted, oriented so its long pin
    // row follows that edge, and have a clear first escape corridor to the
    // core IC. These checks intentionally produce findings instead of
    // silently overriding an agent's explicit placement decision.
    for connector in &board.components {
        if !matches!(connector.kind.as_str(), "connector" | "jack")
            || connector
                .part
                .as_ref()
                .is_none_or(|part| part.pins.len() < 8)
        {
            continue;
        }
        let Some(connector_placement) = placement.components.iter().find(|p| p.id == connector.id)
        else {
            continue;
        };
        let Some((_, connector_rect)) = courtyard_rects
            .iter()
            .find(|(refdes, _)| *refdes == connector.refdes.as_str())
        else {
            continue;
        };
        let edge_distances = [
            (
                connector_rect.min.x_nm - placement.board_outline.min.x_nm,
                "left",
            ),
            (
                placement.board_outline.max.x_nm - connector_rect.max.x_nm,
                "right",
            ),
            (
                connector_rect.min.y_nm - placement.board_outline.min.y_nm,
                "top",
            ),
            (
                placement.board_outline.max.y_nm - connector_rect.max.y_nm,
                "bottom",
            ),
        ];
        let (nearest_distance, nearest_edge) = edge_distances
            .iter()
            .min_by_key(|(distance, _)| *distance)
            .copied()
            .unwrap_or((i64::MAX, "unknown"));
        let edge_mounted = nearest_distance <= mm_to_nm(6.0);
        // The automatic placer reserves a 5 mm manufacturing/keep-in margin;
        // a courtyard within 6 mm of the outline is therefore already
        // edge-mounted for review purposes.
        if !edge_mounted {
            visual_findings.push(format!(
                "dense connector {} is {:.1} mm from its nearest board edge; move it to an edge before routing",
                connector.refdes,
                synth_geometry::nm_to_mm(nearest_distance)
            ));

            if let Some(hint) = connector.placement_hint.as_ref() {
                if hint.priority == synth_ir::PlacementPriority::Hard
                    && hint.edge.is_none()
                    && (hint.near.is_some() || hint.region.is_some())
                {
                    let anchor = hint.near.as_deref().map_or_else(
                        || "its requested region".to_string(),
                        |near| format!("near {near}"),
                    );
                    let hint_warning = format!(
                        "hard placement hint for dense connector {} keeps it {} instead of an edge breakout; revise the source hint to use edge: bottom/right (or make near/region soft)",
                        connector.refdes, anchor
                    );
                    visual_findings.push(hint_warning.clone());
                    recommended_actions.push(PlacementRecommendation {
                        action: "revise_hard_connector_hint".into(),
                        refdes: connector.refdes.clone(),
                        relative_to: None,
                        suggested_x_mm: None,
                        suggested_y_mm: None,
                        suggested_rotation_deg: None,
                        rationale: hint_warning,
                    });
                }
            }

            // Give the agent a concrete sidecar target rather than requiring
            // it to guess a coordinate from a prose warning. Preserve the
            // current orthogonal coordinate.
            let half_w_nm = connector_rect.width_nm() / 2;
            let half_h_nm = connector_rect.height_nm() / 2;
            let current = connector_placement.center;
            let flush = is_edge_flush_connector(connector);
            let edge_margin_nm = if flush { 0 } else { mm_to_nm(EDGE_MARGIN_MM) };
            let target = match nearest_edge {
                "left" => Point::new(
                    placement.board_outline.min.x_nm + edge_margin_nm + half_w_nm,
                    current.y_nm,
                ),
                "right" => Point::new(
                    placement.board_outline.max.x_nm - edge_margin_nm - half_w_nm,
                    current.y_nm,
                ),
                "top" => Point::new(
                    current.x_nm,
                    placement.board_outline.min.y_nm + edge_margin_nm + half_h_nm,
                ),
                "bottom" => Point::new(
                    current.x_nm,
                    placement.board_outline.max.y_nm - edge_margin_nm - half_h_nm,
                ),
                _ => current,
            };
            recommended_actions.push(PlacementRecommendation {
                action: "move_connector_to_edge".into(),
                refdes: connector.refdes.clone(),
                relative_to: None,
                suggested_x_mm: Some(synth_geometry::nm_to_mm(target.x_nm)),
                suggested_y_mm: Some(synth_geometry::nm_to_mm(target.y_nm)),
                suggested_rotation_deg: None,
                rationale: format!(
                    "place the {} connector courtyard {} the {} edge",
                    connector.refdes,
                    if flush {
                        "flush with"
                    } else {
                        "approximately 4 mm inside"
                    },
                    nearest_edge
                ),
            });
        }

        let ((_, _), (footprint_w, footprint_h)) = connector.part.as_ref().map_or(
            ((0.0, 0.0), (0.0, 0.0)),
            synth_layout::pcb_courtyard_geometry_for_part,
        );
        let is_usb = connector
            .part
            .as_ref()
            .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("usb"));
        if !is_usb && edge_mounted {
            let long_axis_horizontal = match connector_placement.rotation {
                Rotation::Zero | Rotation::OneEighty => footprint_w >= footprint_h,
                Rotation::Ninety | Rotation::TwoSeventy => footprint_h >= footprint_w,
            };
            let edge_expects_horizontal = matches!(nearest_edge, "top" | "bottom");
            if long_axis_horizontal != edge_expects_horizontal {
                visual_findings.push(format!(
                    "dense connector {} has its long pin row perpendicular to the {} board edge; rotate it parallel to the edge",
                    connector.refdes, nearest_edge
                ));
                // Choose the quarter-turn from the footprint's native
                // aspect ratio, not from the board edge alone. A vertical
                // pin-header footprint needs 90° to become horizontal on a
                // top/bottom edge; a horizontal footprint needs 0°.
                let suggested_rotation_deg = if edge_expects_horizontal {
                    if footprint_w >= footprint_h {
                        0
                    } else {
                        90
                    }
                } else if footprint_w >= footprint_h {
                    90
                } else {
                    0
                };
                recommended_actions.push(PlacementRecommendation {
                    action: "rotate_connector_parallel_to_edge".into(),
                    refdes: connector.refdes.clone(),
                    relative_to: None,
                    // Include the current center as well as the rotation so
                    // an agent can persist this as a complete absolute
                    // sidecar override without accidentally moving the part.
                    suggested_x_mm: Some(synth_geometry::nm_to_mm(connector_placement.center.x_nm)),
                    suggested_y_mm: Some(synth_geometry::nm_to_mm(connector_placement.center.y_nm)),
                    suggested_rotation_deg: Some(suggested_rotation_deg),
                    rationale: format!(
                        "align the connector's long pin row with the {nearest_edge} board edge"
                    ),
                });
            }
        }

        if let Some(mcu) = board
            .components
            .iter()
            .find(|component| matches!(component.kind.as_str(), "mcu" | "processor" | "ic"))
        {
            if let Some(mcu_center) = center_of(mcu.id) {
                let connector_center = connector_placement.center;
                let is_connector_mcu_chain_component = |component_id: ComponentId| {
                    let touches_connector = board.nets.iter().any(|net| {
                        net.endpoints
                            .iter()
                            .any(|endpoint| endpoint.component == connector.id)
                            && net
                                .endpoints
                                .iter()
                                .any(|endpoint| endpoint.component == component_id)
                    });
                    let touches_mcu = board.nets.iter().any(|net| {
                        net.endpoints
                            .iter()
                            .any(|endpoint| endpoint.component == component_id)
                            && net
                                .endpoints
                                .iter()
                                .any(|endpoint| endpoint.component == mcu.id)
                    });
                    touches_connector && touches_mcu
                };
                // A rectangular bounding box around a diagonal connector-to-
                // MCU path over-reports nearly every component between the
                // endpoints. Review the actual line segment instead, with a
                // modest width for the first breakout channel and courtyard
                // extent. This keeps the warning useful for agent revisions.
                let blocker_names: Vec<&str> = courtyard_rects
                    .iter()
                    .filter(|(refdes, _)| {
                        *refdes != connector.refdes.as_str() && *refdes != mcu.refdes.as_str()
                    })
                    .filter(|(refdes, _)| {
                        let Some(blocker) = board
                            .components
                            .iter()
                            .find(|component| component.refdes == *refdes)
                        else {
                            return false;
                        };
                        let Some(blocker_center) = center_of(blocker.id) else {
                            return false;
                        };
                        // A required series/conditioning part can be
                        // intentionally located in this breakout corridor.
                        // Do not classify it as a visual blocker when its
                        // connectivity forms the connector-to-MCU chain.
                        if is_connector_mcu_chain_component(blocker.id) {
                            return false;
                        }
                        let (distance, t) =
                            distance_to_segment_mm(blocker_center, connector_center, mcu_center);
                        (0.0..=1.0).contains(&t) && distance <= 3.5
                    })
                    .map(|(refdes, _)| *refdes)
                    .collect();
                if blocker_names.len() >= 3 {
                    visual_findings.push(format!(
                        "connector {} to {} has {} courtyard blockers ({}) in its direct escape corridor; move those parts or revise the header/core relationship before routing",
                        connector.refdes,
                        mcu.refdes,
                        blocker_names.len(),
                        blocker_names.join(", ")
                    ));

                    // Recommend deterministic, local escape moves for each
                    // blocker. Moving perpendicular to the connector-to-MCU
                    // corridor preserves the MCU relationship while opening
                    // the first breakout channel. The exact coordinates are
                    // suggestions and still go through the sidecar review.
                    let corridor_dx =
                        synth_geometry::nm_to_mm(mcu_center.x_nm - connector_center.x_nm);
                    let corridor_dy =
                        synth_geometry::nm_to_mm(mcu_center.y_nm - connector_center.y_nm);
                    let corridor_len = (corridor_dx * corridor_dx + corridor_dy * corridor_dy)
                        .sqrt()
                        .max(0.001);
                    let perp_x = -corridor_dy / corridor_len;
                    let perp_y = corridor_dx / corridor_len;
                    for blocker in blocker_names {
                        let Some(blocker_center) = board
                            .components
                            .iter()
                            .find(|component| component.refdes == blocker)
                            .and_then(|component| center_of(component.id))
                        else {
                            continue;
                        };
                        let relative_x =
                            synth_geometry::nm_to_mm(blocker_center.x_nm - connector_center.x_nm);
                        let relative_y =
                            synth_geometry::nm_to_mm(blocker_center.y_nm - connector_center.y_nm);
                        let side = if relative_x * perp_x + relative_y * perp_y >= 0.0 {
                            1.0
                        } else {
                            -1.0
                        };
                        recommended_actions.push(PlacementRecommendation {
                            action: "clear_connector_escape_corridor".into(),
                            refdes: blocker.to_string(),
                            relative_to: None,
                            suggested_x_mm: Some(
                                synth_geometry::nm_to_mm(blocker_center.x_nm)
                                    + side * perp_x * 5.0,
                            ),
                            suggested_y_mm: Some(
                                synth_geometry::nm_to_mm(blocker_center.y_nm)
                                    + side * perp_y * 5.0,
                            ),
                            suggested_rotation_deg: None,
                            rationale: format!(
                                "move {} approximately 5 mm perpendicular to the {}-{} breakout corridor",
                                blocker, connector.refdes, mcu.refdes
                            ),
                        });
                    }
                }
            }
        }
    }

    if !placement.components.is_empty() {
        let min_x = placement
            .components
            .iter()
            .map(|component| component.center.x_nm)
            .min()
            .unwrap_or(0);
        let max_x = placement
            .components
            .iter()
            .map(|component| component.center.x_nm)
            .max()
            .unwrap_or(0);
        let min_y = placement
            .components
            .iter()
            .map(|component| component.center.y_nm)
            .min()
            .unwrap_or(0);
        let max_y = placement
            .components
            .iter()
            .map(|component| component.center.y_nm)
            .max()
            .unwrap_or(0);
        // Use the actual footprint courtyard envelope rather than component
        // centers. Long headers and connectors can legitimately span a large
        // area while their reference points remain close together; measuring
        // only centers would report a false unused-space warning.
        let (envelope_min_x, envelope_max_x, envelope_min_y, envelope_max_y) =
            if courtyard_rects.is_empty() {
                (min_x, max_x, min_y, max_y)
            } else {
                (
                    courtyard_rects
                        .iter()
                        .map(|(_, rect)| rect.min.x_nm)
                        .min()
                        .unwrap_or(min_x),
                    courtyard_rects
                        .iter()
                        .map(|(_, rect)| rect.max.x_nm)
                        .max()
                        .unwrap_or(max_x),
                    courtyard_rects
                        .iter()
                        .map(|(_, rect)| rect.min.y_nm)
                        .min()
                        .unwrap_or(min_y),
                    courtyard_rects
                        .iter()
                        .map(|(_, rect)| rect.max.y_nm)
                        .max()
                        .unwrap_or(max_y),
                )
            };
        let occupied_area = (envelope_max_x - envelope_min_x).max(mm_to_nm(1.0)) as f64
            * (envelope_max_y - envelope_min_y).max(mm_to_nm(1.0)) as f64;
        let board_area =
            placement.board_outline.width_nm() as f64 * placement.board_outline.height_nm() as f64;
        if board_area / occupied_area > 1.8 {
            let finding = format!(
                "large unused board area (outline is {board_w_mm:.1}x{board_h_mm:.1} mm around a sparse component envelope); tighten placement or request a smaller outline"
            );
            functional_warnings.push(finding.clone());
            visual_findings.push(finding);
        }
    }

    let visual_score = 100u8.saturating_sub((visual_findings.len() as u8).saturating_mul(20));
    let visual_review = VisualPlacementReview {
        score: visual_score,
        requires_revision: !visual_findings.is_empty(),
        findings: visual_findings,
        recommended_actions,
    };

    PlacementDescription {
        board_size_mm: [board_w_mm, board_h_mm],
        component_regions,
        cluster_summary,
        // Placement review runs neither the router nor DRC, so reporting
        // `true`/`0` here would assert a clean board that was never checked.
        drc_clean: None,
        unrouted_nets: None,
        dense_regions,
        functional_warnings,
        visual_review,
    }
}

/// Review passes `refine_placement` may spend. Each pass applies every
/// recommended move at once, so a few are enough for the moves to settle.
const REFINE_MAX_PASSES: usize = 4;

/// Apply the visual review's recommended moves to `placement`, re-reviewing
/// after each pass, and return the version with the fewest findings.
///
/// Moves are applied together, so two of them can undo each other, and a
/// move that closes one finding can open a collision. A pass only counts when
/// it adds no overlap or off-board part, so a refined placement is never
/// worse than the one passed in and never less legal. The review's findings
/// are then re-checked after the wirelength pass, which is dropped if it
/// reopens any.
/// Overlap and off-board parts are what the independent validator rejects,
/// so a pass may not add any. Each defect emits exactly one of these actions,
/// so counting them counts the defects; a new emitter for either defect must
/// keep that pairing or the gate stops seeing it.
fn physical_defects(review: &VisualPlacementReview) -> usize {
    review
        .recommended_actions
        .iter()
        .filter(|a| {
            matches!(
                a.action.as_str(),
                "separate_courtyards" | "keep_courtyard_inside_board"
            )
        })
        .count()
}

/// A finding with its measurements dropped, so one that only changed a
/// distance is still the same finding.
fn finding_identity(finding: &str) -> String {
    finding
        .split_whitespace()
        .filter(|word| {
            let bare = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
            bare.is_empty()
                || !bare
                    .chars()
                    .all(|c| c.is_ascii_digit() || "-.x".contains(c))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `candidate` has a finding that `kept` does not. A count cannot
/// tell, because a move can close one finding and open another.
fn introduces_finding(kept: &VisualPlacementReview, candidate: &VisualPlacementReview) -> bool {
    let known: std::collections::HashSet<String> =
        kept.findings.iter().map(|f| finding_identity(f)).collect();
    candidate
        .findings
        .iter()
        .any(|f| !known.contains(&finding_identity(f)))
}

/// The wirelength pass only checks overlap, outline and keepouts, so a move
/// can still reopen a review finding (decoupling distance, breakout
/// corridor). Keep its result only when the review does not get worse.
fn keep_unless_worse(board: &Board, kept: Placement, candidate: Placement) -> Placement {
    let review_of = |placement: &Placement| describe_placement(board, placement).visual_review;
    let (kept_review, candidate_review) = (review_of(&kept), review_of(&candidate));
    if physical_defects(&candidate_review) <= physical_defects(&kept_review)
        && !introduces_finding(&kept_review, &candidate_review)
    {
        candidate
    } else {
        kept
    }
}

fn refine_placement<S: ::std::hash::BuildHasher>(
    board: &Board,
    placement: &Placement,
    rotation_overrides: &std::collections::HashMap<ComponentId, Rotation, S>,
) -> Placement {
    let review_of = |candidate: &Placement| describe_placement(board, candidate).visual_review;
    let mut best = placement.clone();
    let mut best_review = review_of(&best);
    let mut current = placement.clone();
    let mut review = best_review.clone();
    for _ in 0..REFINE_MAX_PASSES {
        if best_review.findings.is_empty() {
            break;
        }
        let before = current.clone();
        for action in &review.recommended_actions {
            let hits = keepout_hits(board, &current);
            let undo = current.clone();
            apply_recommendation(board, &mut current, action, rotation_overrides);
            // The validator rejects a part inside a keepout, so a move that
            // would put one there is dropped.
            if keepout_hits(board, &current) > hits {
                current = undo;
            }
        }
        if current == before {
            break;
        }
        review = review_of(&current);
        if physical_defects(&review) <= physical_defects(&best_review)
            && review.findings.len() < best_review.findings.len()
            && !introduces_finding(&best_review, &review)
        {
            best = current.clone();
            best_review = review.clone();
        }
    }
    let shortened = shorten_wires(board, best.clone());
    keep_unless_worse(board, best, shortened)
}

/// Scale factors (in thousandths) `compact_placement` tries each round,
/// biggest squeeze first.
const COMPACT_PERMILLE: [i64; 3] = [900, 950, 980];

/// Parts with at least this many pins get a wider keep-out ring, in
/// compaction and in the wirelength pass, so the router can still escape their
/// pins.
const DENSE_PART_PINS: usize = 12;

/// Extra width and height added to a dense part's courtyard, in millimetres:
/// half of it on each side. Compaction separates parts by it and the
/// wirelength pass keeps passives out of it, so a decoupling cap sits about
/// half this far from a dense part, not against it.
/// Calibrated on the reference boards: smaller rings strand USB-C and MCU
/// nets in the native router, and the right size is not monotonic, so
/// re-check `sensor_logger_routes_cleanly`, `iot_sensor_board_routes_cleanly`
/// and the `routability_report` gate before changing it.
const DENSE_PART_RING_MM: f64 = 7.0;

/// Whether a component has enough pins to need a keep-out ring.
fn is_dense_part(board: &Board, id: ComponentId) -> bool {
    board
        .component(id)
        .and_then(|c| c.part.as_ref())
        .is_some_and(|part| part.pins.len() >= DENSE_PART_PINS)
}

/// Rounds of squeezing before the pass gives up. Each accepted round shrinks
/// the layout, so this only bounds a layout that keeps finding room.
const COMPACT_ROUNDS: usize = 60;

/// Sweeps `separate_overlaps` may spend pushing courtyards apart.
const SEPARATION_SWEEPS: usize = 200;

/// Courtyard size of every component, in `placement.components` order. Looked
/// up once per pass: the lookup is far too slow to repeat inside the loops.
fn courtyard_sizes(board: &Board, placement: &Placement) -> Vec<(f64, f64)> {
    placement
        .components
        .iter()
        .map(|placed| {
            board.component(placed.id).map_or((0.0, 0.0), |component| {
                component.part.as_ref().map_or_else(
                    || fallback_courtyard(&component.kind),
                    |part| synth_layout::pcb_courtyard_geometry_for_part(part).1,
                )
            })
        })
        .collect()
}

/// The box around every courtyard.
fn courtyard_envelope(placement: &Placement, sizes: &[(f64, f64)]) -> Option<Rect> {
    placement
        .components
        .iter()
        .zip(sizes)
        .map(|(placed, size)| rotated_courtyard_rect(placed, *size))
        .reduce(|a, b| {
            Rect::new(
                Point::new(a.min.x_nm.min(b.min.x_nm), a.min.y_nm.min(b.min.y_nm)),
                Point::new(a.max.x_nm.max(b.max.x_nm), a.max.y_nm.max(b.max.y_nm)),
            )
        })
}

/// Area of a rectangle in square millimetres.
fn area_mm2(rect: &Rect) -> f64 {
    synth_geometry::nm_to_mm(rect.width_nm()) * synth_geometry::nm_to_mm(rect.height_nm())
}

/// Area of the box around every courtyard, in square millimetres. The board
/// outline is this box plus a margin, so it is the size being minimised.
#[cfg(test)]
fn courtyard_envelope_area_mm2(board: &Board, placement: &Placement) -> f64 {
    courtyard_envelope(placement, &courtyard_sizes(board, placement)).map_or(0.0, |r| area_mm2(&r))
}

/// Push overlapping courtyards apart along the axis they overlap least. Both
/// parts move, the lighter one further, so a squeezed row of equal parts
/// spreads out evenly instead of drifting; a part in `frozen` never moves.
/// Returns false when the overlaps could not be removed.
fn separate_overlaps(placement: &mut Placement, sizes: &[(f64, f64)], frozen: &[bool]) -> bool {
    let pitch_nm = mm_to_nm(0.5);
    let rect_of = |placement: &Placement, index: usize| {
        rotated_courtyard_rect(&placement.components[index], sizes[index])
    };
    let weight = area_mm2;
    for _ in 0..SEPARATION_SWEEPS {
        let mut moved = false;
        for i in 0..placement.components.len() {
            for j in (i + 1)..placement.components.len() {
                let (a, b) = (rect_of(placement, i), rect_of(placement, j));
                if !a.intersects(&b) {
                    continue;
                }
                // Share of the push each part takes: the lighter one moves
                // further, a frozen one not at all.
                let share_i = match (frozen[i], frozen[j]) {
                    (true, true) => return false,
                    (true, false) => 0.0,
                    (false, true) => 1.0,
                    (false, false) => weight(&b) / (weight(&a) + weight(&b)),
                };
                let overlap_x = a.max.x_nm.min(b.max.x_nm) - a.min.x_nm.max(b.min.x_nm);
                let overlap_y = a.max.y_nm.min(b.max.y_nm) - a.min.y_nm.max(b.min.y_nm);
                let along_x = overlap_x <= overlap_y;
                let push =
                    ((if along_x { overlap_x } else { overlap_y }) / pitch_nm + 1) * pitch_nm;
                let move_i =
                    (synth_geometry::nm_to_mm(push) * share_i / 0.5).round() as i64 * pitch_nm;
                let move_j = push - move_i;
                let i_is_low = if along_x {
                    a.min.x_nm + a.max.x_nm < b.min.x_nm + b.max.x_nm
                } else {
                    a.min.y_nm + a.max.y_nm < b.min.y_nm + b.max.y_nm
                };
                // i moves away from j, j the other way.
                let (delta_i, delta_j) = if i_is_low {
                    (-move_i, move_j)
                } else {
                    (move_i, -move_j)
                };
                for (index, delta) in [(i, delta_i), (j, delta_j)] {
                    let centre = &mut placement.components[index].center;
                    if along_x {
                        centre.x_nm += delta;
                    } else {
                        centre.y_nm += delta;
                    }
                }
                moved = true;
            }
        }
        if !moved {
            return true;
        }
    }
    false
}

/// Compact the finished placement and fit the outline round the result, with
/// the origin back at the corner. A placement compaction cannot improve comes
/// back untouched, outline included.
fn compact_and_fit(board: &Board, placement: Placement) -> Placement {
    let packed = compact_placement(board, placement.clone());
    if packed.components == placement.components {
        return placement;
    }
    let mut fitted = packed;
    tighten_outline_to_hints(board, &mut fitted);
    let origin = fitted.board_outline.min;
    for placed in &mut fitted.components {
        placed.center = Point::new(
            placed.center.x_nm - origin.x_nm,
            placed.center.y_nm - origin.y_nm,
        );
    }
    fitted.board_outline = Rect::new(
        Point::new(0, 0),
        Point::new(
            fitted.board_outline.width_nm(),
            fitted.board_outline.height_nm(),
        ),
    );
    fitted
}

/// Parts whose reach (courtyard plus the edge margin the outline keeps round
/// it) passes the outer side of an edge connector row. Such a part would hold
/// the outline out past the connector, leaving it inside the board instead of
/// on its edge.
fn edge_overhang(
    board: &Board,
    placement: &Placement,
    sizes: &[(f64, f64)],
    flush_edge: &dyn Fn(ComponentId) -> Option<usize>,
) -> usize {
    let rects: Vec<(Option<usize>, Rect)> = placement
        .components
        .iter()
        .zip(sizes)
        .map(|(p, size)| {
            let real = rotated_courtyard_rect(p, *size);
            let component = board.component(p.id);
            let edge = flush_edge(p.id);
            // A connector's reach is its copper plus the clearance the
            // outline keeps from it, as in `outline_with_flush_connectors`.
            if let (Some(_), Some(component)) = (edge, component) {
                let copper = exported_pad_bounds(component, p).unwrap_or(real);
                return (edge, connector_reach(real, copper));
            }
            let margin = component.map_or(0, |c| {
                if synth_layout::is_board_feature_kind(&c.kind) {
                    0
                } else {
                    mm_to_nm(EDGE_MARGIN_MM)
                }
            });
            (
                edge,
                Rect::new(
                    Point::new(real.min.x_nm - margin, real.min.y_nm - margin),
                    Point::new(real.max.x_nm + margin, real.max.y_nm + margin),
                ),
            )
        })
        .collect();
    (0..4)
        .map(|edge| {
            let row = || {
                rects
                    .iter()
                    .filter(|(e, _)| *e == Some(edge))
                    .map(|(_, r)| r)
            };
            let outer = match edge {
                0 => row().map(|r| r.min.x_nm).min(),
                1 => row().map(|r| r.min.y_nm).min(),
                2 => row().map(|r| r.max.x_nm).max(),
                _ => row().map(|r| r.max.y_nm).max(),
            };
            let Some(outer) = outer else { return 0 };
            rects
                .iter()
                .filter(|(e, r)| {
                    *e != Some(edge)
                        && match edge {
                            0 => r.min.x_nm < outer,
                            1 => r.min.y_nm < outer,
                            2 => r.max.x_nm > outer,
                            _ => r.max.y_nm > outer,
                        }
                })
                .count()
        })
        .sum()
}

/// Pack each edge's row of connectors together along that edge, in their
/// present order and centred on where the row is now. Each connector keeps its
/// spaced courtyard, so nothing overlaps. This is one-dimensional compaction:
/// the row only ever moves along its own axis.
fn pack_edge_rows(
    placement: &mut Placement,
    spaced: &[(f64, f64)],
    flush_edge: &dyn Fn(ComponentId) -> Option<usize>,
) {
    let pitch_nm = mm_to_nm(0.5);
    for edge in 0..4 {
        // Top and bottom rows run along x, left and right rows along y.
        let runs_along_x = edge % 2 == 1;
        let extent_and_centre = |placement: &Placement, index: usize| {
            let rect = rotated_courtyard_rect(&placement.components[index], spaced[index]);
            if runs_along_x {
                (rect.width_nm(), placement.components[index].center.x_nm)
            } else {
                (rect.height_nm(), placement.components[index].center.y_nm)
            }
        };
        let mut row: Vec<usize> = (0..placement.components.len())
            .filter(|&i| flush_edge(placement.components[i].id) == Some(edge))
            .collect();
        if row.len() < 2 {
            continue;
        }
        row.sort_by_key(|&i| extent_and_centre(placement, i).1);
        // Touching courtyards count as overlapping, so leave one grid step
        // between neighbours.
        let total: i64 = row
            .iter()
            .map(|&i| extent_and_centre(placement, i).0 + pitch_nm)
            .sum();
        let first = row[0];
        let last = row[row.len() - 1];
        let middle =
            (extent_and_centre(placement, first).1 + extent_and_centre(placement, last).1) / 2;
        let mut cursor = middle - total / 2;
        for &i in &row {
            let (extent, _) = extent_and_centre(placement, i);
            let centre = (cursor + extent / 2) / pitch_nm * pitch_nm;
            let component = &mut placement.components[i];
            if runs_along_x {
                component.center.x_nm = centre;
            } else {
                component.center.y_nm = centre;
            }
            cursor += extent + pitch_nm;
        }
    }
}

/// Label geometry of the PCB exporter's reference-label placer
/// (`synth-kicad/src/silk_labels.rs`), which this crate cannot call. If those
/// numbers change there, change them here too. The exporter also honours
/// design and part overrides (hidden labels, preferred sides); the check here
/// ignores them, which only makes it stricter.
const LABEL_CHAR_WIDTH_MM: f64 = 1.0;
const LABEL_TEXT_HEIGHT_MM: f64 = 1.3;
const LABEL_SIDE_GAP_MM: f64 = 0.3;
const LABEL_EDGE_CLEARANCE_MM: f64 = 0.2;

/// How many parts have no free side for their reference label. The exporter
/// puts each label against its part on the first side that is clear of other
/// courtyards, other labels and the board edge, and falls back to a far default
/// that collides when none is. Parts are visited in board order, as there.
fn labels_without_room(board: &Board, placement: &Placement, sizes: &[(f64, f64)]) -> usize {
    let courtyards: Vec<Rect> = placement
        .components
        .iter()
        .zip(sizes)
        .map(|(placed, size)| rotated_courtyard_rect(placed, *size))
        .collect();
    let edge = mm_to_nm(LABEL_EDGE_CLEARANCE_MM);
    let outline = placement.board_outline;
    let inside = |b: &Rect| {
        b.min.x_nm >= outline.min.x_nm + edge
            && b.min.y_nm >= outline.min.y_nm + edge
            && b.max.x_nm <= outline.max.x_nm - edge
            && b.max.y_nm <= outline.max.y_nm - edge
    };
    let mut taken: Vec<Rect> = Vec::new();
    let mut without_room = 0;
    for component in &board.components {
        let Some(i) = placement
            .components
            .iter()
            .position(|p| p.id == component.id)
        else {
            continue;
        };
        let c = courtyards[i];
        // A designator is a handful of characters, so the cast is exact.
        #[allow(clippy::cast_precision_loss)]
        let width = mm_to_nm(component.refdes.chars().count() as f64 * LABEL_CHAR_WIDTH_MM);
        let height = mm_to_nm(LABEL_TEXT_HEIGHT_MM);
        let gap = mm_to_nm(LABEL_SIDE_GAP_MM);
        let (mid_x, mid_y) = ((c.min.x_nm + c.max.x_nm) / 2, (c.min.y_nm + c.max.y_nm) / 2);
        let free = [
            (mid_x, c.min.y_nm - gap - height / 2),
            (mid_x, c.max.y_nm + gap + height / 2),
            (c.min.x_nm - gap - width / 2, mid_y),
            (c.max.x_nm + gap + width / 2, mid_y),
        ]
        .iter()
        .map(|&(x, y)| Rect::from_center_half_extents(Point::new(x, y), width / 2, height / 2))
        .find(|b| {
            inside(b)
                && !courtyards
                    .iter()
                    .enumerate()
                    .any(|(j, o)| j != i && o.intersects(b))
                && !taken.iter().any(|t| t.intersects(b))
        });
        match free {
            Some(b) => taken.push(b),
            None => without_room += 1,
        }
    }
    without_room
}

/// Slide every movable part along one axis, toward the low or the high side,
/// until it touches the part in its way or reaches the edge of the layout. A
/// part alone in its band moves into a free spot in the next band over, which
/// scaling toward the middle never does. Parts move in order from the target
/// side outward, so none passes another.
fn slide_parts(
    placement: &mut Placement,
    sizes: &[(f64, f64)],
    movable: &[bool],
    along_y: bool,
    toward_low: bool,
) {
    let pitch_nm = mm_to_nm(0.5);
    let Some(envelope) = courtyard_envelope(placement, sizes) else {
        return;
    };
    let mut rects: Vec<Rect> = placement
        .components
        .iter()
        .zip(sizes)
        .map(|(placed, size)| rotated_courtyard_rect(placed, *size))
        .collect();
    let along = |r: &Rect| {
        if along_y {
            (r.min.y_nm, r.max.y_nm)
        } else {
            (r.min.x_nm, r.max.x_nm)
        }
    };
    let across = |r: &Rect| {
        if along_y {
            (r.min.x_nm, r.max.x_nm)
        } else {
            (r.min.y_nm, r.max.y_nm)
        }
    };
    let (envelope_low, envelope_high) = along(&envelope);
    let mut order: Vec<usize> = (0..rects.len()).filter(|&i| movable[i]).collect();
    if toward_low {
        order.sort_by_key(|&i| along(&rects[i]).0);
    } else {
        order.sort_by_key(|&i| std::cmp::Reverse(along(&rects[i]).1));
    }
    let mut settled: Vec<usize> = (0..rects.len()).filter(|&i| !movable[i]).collect();
    for i in order {
        let (low, high) = along(&rects[i]);
        let (across_low, across_high) = across(&rects[i]);
        let mut bound = if toward_low {
            envelope_low
        } else {
            envelope_high
        };
        for &other in &settled {
            let (other_across_low, other_across_high) = across(&rects[other]);
            if !(across_low < other_across_high && other_across_low < across_high) {
                continue;
            }
            let (other_low, other_high) = along(&rects[other]);
            if toward_low && other_high <= low {
                bound = bound.max(other_high + pitch_nm);
            } else if !toward_low && other_low >= high {
                bound = bound.min(other_low - pitch_nm);
            }
        }
        // Whole grid steps only, and never away from the target side.
        let wanted = if toward_low {
            (bound - low).min(0)
        } else {
            (bound - high).max(0)
        };
        let shift = wanted / pitch_nm * pitch_nm;
        let centre = &mut placement.components[i].center;
        let r = &mut rects[i];
        if along_y {
            centre.y_nm += shift;
            r.min.y_nm += shift;
            r.max.y_nm += shift;
        } else {
            centre.x_nm += shift;
            r.min.x_nm += shift;
            r.max.x_nm += shift;
        }
        settled.push(i);
    }
}

/// Move each edge's connector row inward, as one body, until it meets the
/// parts beside it or its outer side comes level with the rest of the board.
/// Sliding the body makes room, and this is how the outline follows.
fn shift_rows_inward(
    placement: &mut Placement,
    sizes: &[(f64, f64)],
    flush_edge: &dyn Fn(ComponentId) -> Option<usize>,
) {
    let pitch_nm = mm_to_nm(0.5);
    let margin_nm = mm_to_nm(EDGE_MARGIN_MM);
    let rects: Vec<Rect> = placement
        .components
        .iter()
        .zip(sizes)
        .map(|(placed, size)| rotated_courtyard_rect(placed, *size))
        .collect();
    let edges: Vec<Option<usize>> = placement
        .components
        .iter()
        .map(|placed| flush_edge(placed.id))
        .collect();
    for edge in 0..4 {
        let members: Vec<usize> = (0..rects.len())
            .filter(|&i| edges[i] == Some(edge))
            .collect();
        let others: Vec<usize> = (0..rects.len()).filter(|&i| edges[i].is_none()).collect();
        if members.is_empty() || others.is_empty() {
            continue;
        }
        // Top and bottom rows move in y, left and right rows in x. Rows on a
        // low edge (left, top) move toward high coordinates.
        let moves_along_y = edge % 2 == 1;
        let toward_high = edge < 2;
        let span = |r: &Rect| {
            if moves_along_y {
                (r.min.y_nm, r.max.y_nm)
            } else {
                (r.min.x_nm, r.max.x_nm)
            }
        };
        let side = |r: &Rect| {
            if moves_along_y {
                (r.min.x_nm, r.max.x_nm)
            } else {
                (r.min.y_nm, r.max.y_nm)
            }
        };
        let mut room = i64::MAX;
        for &m in &members {
            let (m_low, m_high) = span(&rects[m]);
            let (m_side_low, m_side_high) = side(&rects[m]);
            for &o in &others {
                let (o_side_low, o_side_high) = side(&rects[o]);
                if !(m_side_low < o_side_high && o_side_low < m_side_high) {
                    continue;
                }
                let (o_low, o_high) = span(&rects[o]);
                if toward_high && o_low >= m_high {
                    room = room.min(o_low - m_high - pitch_nm);
                } else if !toward_high && o_high <= m_low {
                    room = room.min(m_low - o_high - pitch_nm);
                }
            }
        }
        // The outer side may come level with the other parts' reach, no more.
        let row_outer = if toward_high {
            members.iter().map(|&m| span(&rects[m]).0).min()
        } else {
            members.iter().map(|&m| span(&rects[m]).1).max()
        };
        let reach = if toward_high {
            others.iter().map(|&o| span(&rects[o]).0 - margin_nm).min()
        } else {
            others.iter().map(|&o| span(&rects[o]).1 + margin_nm).max()
        };
        if let (Some(row_outer), Some(reach)) = (row_outer, reach) {
            room = room.min(if toward_high {
                reach - row_outer
            } else {
                row_outer - reach
            });
        }
        let shift = room.max(0) / pitch_nm * pitch_nm;
        if shift == 0 {
            continue;
        }
        let signed = if toward_high { shift } else { -shift };
        for &m in &members {
            let centre = &mut placement.components[m].center;
            if moves_along_y {
                centre.y_nm += signed;
            } else {
                centre.x_nm += signed;
            }
        }
    }
}

/// Pull the layout toward `middle` by `permille` thousandths of each distance.
/// Free parts scale about the middle; each edge's connector row moves inward
/// as one body, by the amount its own middle would have scaled, so the row
/// keeps its spacing.
fn squeeze(
    placement: &mut Placement,
    permille: i64,
    middle: Point,
    frozen: &[bool],
    flush_edge: &dyn Fn(ComponentId) -> Option<usize>,
) {
    let pitch_nm = mm_to_nm(0.5);
    let scale = |value: i64, centre: i64| {
        (centre + (value - centre) * permille / 1000) / pitch_nm * pitch_nm
    };
    let mut row_shift = [0i64; 4];
    for (edge, shift) in row_shift.iter_mut().enumerate() {
        // Left and right rows move in x, top and bottom rows in y.
        let faces_x = edge % 2 == 0;
        let row: Vec<i64> = placement
            .components
            .iter()
            .filter(|p| flush_edge(p.id) == Some(edge))
            .map(|p| {
                if faces_x {
                    p.center.x_nm
                } else {
                    p.center.y_nm
                }
            })
            .collect();
        if row.is_empty() {
            continue;
        }
        let mean = row.iter().sum::<i64>() / row.len() as i64;
        *shift = scale(mean, if faces_x { middle.x_nm } else { middle.y_nm }) - mean;
    }
    for (placed, frozen) in placement.components.iter_mut().zip(frozen) {
        if let Some(edge) = flush_edge(placed.id) {
            if edge % 2 == 0 {
                placed.center.x_nm += row_shift[edge];
            } else {
                placed.center.y_nm += row_shift[edge];
            }
        } else if !frozen {
            placed.center = Point::new(
                scale(placed.center.x_nm, middle.x_nm),
                scale(placed.center.y_nm, middle.y_nm),
            );
        }
    }
}

/// Global compaction: pull every free part toward the middle of the layout,
/// push apart whatever then overlaps, and keep the step only when the box
/// around the courtyards shrinks while wiring gets no longer, nothing enters a
/// keepout and the review does not get worse. Scaling the whole layout together
/// keeps neighbours near each other, and a step that lengthens the wiring (an
/// IC leaving its caps) is rejected by the wirelength check.
/// Hard-hinted parts stay put, except an edge connector, which the outline
/// follows: each edge's row of connectors moves inward as one body. The
/// caller fits the outline around the result.
fn compact_placement(board: &Board, placement: Placement) -> Placement {
    let frozen_ids: std::collections::HashSet<ComponentId> = board
        .components
        .iter()
        .filter(|c| is_hard_hinted(c) && !is_edge_flush_connector(c))
        .map(|c| c.id)
        .collect();
    // Edge connectors keep their spacing: each edge's row moves as one body,
    // inward from its own edge, and the separation pass never nudges them.
    let flush_edge = |id: ComponentId| flush_edge_index(board, id);
    // Judge every step against an outline fitted to its parts, so a part
    // pushed past the old edge is not mistaken for an off-board one.
    let mut current = placement;
    tighten_outline_to_hints(board, &mut current);
    let sizes = courtyard_sizes(board, &current);
    // A part with many pins needs room for its traces to leave, so it gets a
    // wider ring.
    let spaced: Vec<(f64, f64)> = current
        .components
        .iter()
        .zip(&sizes)
        .map(|(placed, (w, h))| {
            let ring = if is_dense_part(board, placed.id) {
                DENSE_PART_RING_MM
            } else {
                0.0
            };
            (w + ring, h + ring)
        })
        .collect();
    let frozen: Vec<bool> = current
        .components
        .iter()
        .map(|p| frozen_ids.contains(&p.id))
        .collect();
    let pinned: Vec<bool> = current
        .components
        .iter()
        .zip(&frozen)
        .map(|(p, frozen)| *frozen || flush_edge(p.id).is_some())
        .collect();
    let movable: Vec<bool> = pinned.iter().map(|p| !p).collect();
    // The placer docks each edge connector to the edge later, by a short
    // slide. That only works when nothing sits beyond the connector row, so a
    // layout that already has such a part is left as it is.
    if edge_overhang(board, &current, &sizes, &flush_edge) > 0 {
        return current;
    }
    for _ in 0..COMPACT_ROUNDS {
        let Some(envelope) = courtyard_envelope(&current, &sizes) else {
            break;
        };
        let area = area_mm2(&envelope);
        let middle = Point::new(
            (envelope.min.x_nm + envelope.max.x_nm) / 2,
            (envelope.min.y_nm + envelope.max.y_nm) / 2,
        );
        let hpwl = total_hpwl_nm(board, &current);
        let hits = keepout_hits(board, &current);
        let labels = labels_without_room(board, &current, &sizes);
        // Separate what overlaps, then keep the candidate only if the box
        // around the courtyards is smaller and nothing got worse.
        let settle = |mut candidate: Placement| -> Option<Placement> {
            let fitted = separate_overlaps(&mut candidate, &spaced, &pinned);
            let smaller = courtyard_envelope(&candidate, &sizes)
                .is_some_and(|candidate_envelope| area_mm2(&candidate_envelope) < area);
            if !(fitted && smaller) {
                return None;
            }
            tighten_outline_to_hints(board, &mut candidate);
            let better = total_hpwl_nm(board, &candidate) <= hpwl
                && keepout_hits(board, &candidate) <= hits
                && edge_overhang(board, &candidate, &sizes, &flush_edge) == 0
                && labels_without_room(board, &candidate, &sizes) <= labels;
            (better && keep_unless_worse(board, current.clone(), candidate.clone()) == candidate)
                .then_some(candidate)
        };
        let squeezed = COMPACT_PERMILLE.iter().find_map(|permille| {
            let mut candidate = current.clone();
            squeeze(&mut candidate, *permille, middle, &frozen, &flush_edge);
            settle(candidate)
        });
        // When nothing squeezes any further, let parts slide into free spots
        // in the next band, and let the connector rows follow.
        let slid = || {
            [(true, true), (true, false), (false, true), (false, false)]
                .iter()
                .find_map(|&(along_y, toward_low)| {
                    let mut candidate = current.clone();
                    slide_parts(&mut candidate, &spaced, &movable, along_y, toward_low);
                    shift_rows_inward(&mut candidate, &spaced, &flush_edge);
                    settle(candidate)
                })
        };
        // A row of connectors along an edge may still be setting the board
        // width: pack it.
        let packed = || {
            let mut packed = current.clone();
            pack_edge_rows(&mut packed, &spaced, &flush_edge);
            settle(packed)
        };
        let next = squeezed.or_else(slid).or_else(packed);
        match next {
            Some(candidate) => current = candidate,
            None => break,
        }
    }
    current
}

/// Nets with more pins than this are power rails or buses. They are
/// routed as pours or trunks, so pulling parts toward their middle only
/// scatters everything else.
const HPWL_MAX_NET_MEMBERS: usize = 6;

/// Wirelength passes `shorten_wires` may spend.
const HPWL_PASSES: usize = 3;

/// Sum over nets of the half-perimeter of the box around the centres of the
/// parts on the net: the standard cheap stand-in for routed wire length.
fn total_hpwl_nm(board: &Board, placement: &Placement) -> i64 {
    board
        .nets
        .iter()
        .filter_map(|net| {
            let centres: Vec<Point> = net
                .endpoints
                .iter()
                .filter_map(|e| {
                    placement
                        .components
                        .iter()
                        .find(|p| p.id == e.component)
                        .map(|p| p.center)
                })
                .collect();
            if centres.len() < 2 || centres.len() > HPWL_MAX_NET_MEMBERS {
                return None;
            }
            let span = |value: fn(&Point) -> i64| {
                centres.iter().map(value).max().unwrap_or(0)
                    - centres.iter().map(value).min().unwrap_or(0)
            };
            Some(span(|p| p.x_nm) + span(|p| p.y_nm))
        })
        .sum()
}

/// The nearest centre to `centre` whose rectangle (`half` wide and tall on each
/// side) is clear of every ring: whichever ring it overlaps, it moves out along
/// the axis with the smaller distance to travel.
fn centre_outside_rings(
    mut centre: Point,
    half: (i64, i64),
    rings: &[Rect],
    pitch_nm: i64,
) -> Point {
    for _ in 0..=rings.len() {
        let rect = Rect::from_center_half_extents(centre, half.0, half.1);
        let Some(ring) = rings.iter().find(|ring| ring.intersects(&rect)) else {
            break;
        };
        let ring_middle = Point::new(
            (ring.min.x_nm + ring.max.x_nm) / 2,
            (ring.min.y_nm + ring.max.y_nm) / 2,
        );
        let x = if centre.x_nm >= ring_middle.x_nm {
            ring.max.x_nm + half.0 + pitch_nm
        } else {
            ring.min.x_nm - half.0 - pitch_nm
        };
        let y = if centre.y_nm >= ring_middle.y_nm {
            ring.max.y_nm + half.1 + pitch_nm
        } else {
            ring.min.y_nm - half.1 - pitch_nm
        };
        if (x - centre.x_nm).abs() <= (y - centre.y_nm).abs() {
            centre.x_nm = x / pitch_nm * pitch_nm;
        } else {
            centre.y_nm = y / pitch_nm * pitch_nm;
        }
    }
    centre
}

/// Wirelength-driven detailed placement: move each small two-pin part to the
/// median of the boxes its nets form around the other parts (the spot that
/// minimises its nets' half-perimeter), snap it to a legal slot nearby, and
/// keep the move only if total wirelength drops. ICs, connectors and
/// hard-hinted parts stay where they are, so only passives travel toward the
/// parts they serve, and a move can never make the wiring longer.
/// A passive does not move into the keep-out ring around a many-pin part
/// (see `DENSE_PART_RING_MM`), so a decoupling cap stays about half the ring
/// away from such a part, not against it. The router needs that room.
fn shorten_wires(board: &Board, mut placement: Placement) -> Placement {
    let pitch_nm = mm_to_nm(0.5);
    let mut hpwl = total_hpwl_nm(board, &placement);
    let mut hits = keepout_hits(board, &placement);
    let sizes = courtyard_sizes(board, &placement);
    let mut labels = labels_without_room(board, &placement, &sizes);
    // The router needs room to leave a many-pin part's pins, so a passive may
    // not move into the ring compaction keeps around such a part. Only
    // passives move here, so these rings stay where they are.
    let half_ring_nm = mm_to_nm(DENSE_PART_RING_MM) / 2;
    let rings: Vec<Rect> = placement
        .components
        .iter()
        .zip(&sizes)
        .filter(|(placed, _)| is_dense_part(board, placed.id))
        .map(|(placed, size)| {
            let r = rotated_courtyard_rect(placed, *size);
            Rect::new(
                Point::new(r.min.x_nm - half_ring_nm, r.min.y_nm - half_ring_nm),
                Point::new(r.max.x_nm + half_ring_nm, r.max.y_nm + half_ring_nm),
            )
        })
        .collect();
    for _ in 0..HPWL_PASSES {
        let mut improved = false;
        for (component_index, component) in board.components.iter().enumerate() {
            let two_pin = component.part.as_ref().is_some_and(|p| p.pins.len() == 2);
            if !two_pin || !is_passive(component) || is_hard_hinted(component) {
                continue;
            }
            let Some(current) = placement
                .components
                .iter()
                .find(|p| p.id == component.id)
                .copied()
            else {
                continue;
            };
            // Both ends of every other-part box the net would have without
            // this part: the optimum lies between the two middle values.
            let (mut xs, mut ys) = (Vec::new(), Vec::new());
            for net in &board.nets {
                if !net.endpoints.iter().any(|e| e.component == component.id) {
                    continue;
                }
                let others: Vec<Point> = net
                    .endpoints
                    .iter()
                    .filter(|e| e.component != component.id)
                    .filter_map(|e| {
                        placement
                            .components
                            .iter()
                            .find(|p| p.id == e.component)
                            .map(|p| p.center)
                    })
                    .collect();
                if others.is_empty() || others.len() + 1 > HPWL_MAX_NET_MEMBERS {
                    continue;
                }
                xs.extend(others.iter().map(|p| p.x_nm).min());
                xs.extend(others.iter().map(|p| p.x_nm).max());
                ys.extend(others.iter().map(|p| p.y_nm).min());
                ys.extend(others.iter().map(|p| p.y_nm).max());
            }
            if xs.is_empty() {
                continue;
            }
            let middle = |values: &mut Vec<i64>| {
                values.sort_unstable();
                // Each net adds its low and high end, so the count is even.
                let mid = values.len() / 2;
                (values[mid - 1] + values[mid]) / 2 / pitch_nm * pitch_nm
            };
            let Some(index) = placement
                .components
                .iter()
                .position(|p| p.id == component.id)
            else {
                continue;
            };
            let half = {
                let r = rotated_courtyard_rect(&current, sizes[index]);
                (r.width_nm() / 2, r.height_nm() / 2)
            };
            let target = ComponentPlacement {
                center: centre_outside_rings(
                    Point::new(middle(&mut xs), middle(&mut ys)),
                    half,
                    &rings,
                    pitch_nm,
                ),
                ..current
            };
            let slot = if sidecar_position_is_legal(board, &placement, component_index, target) {
                Some(target)
            } else {
                nearest_local_slot(board, &placement, component_index, target, pitch_nm)
            };
            let Some(slot) = slot else { continue };
            // The legal slot search knows nothing about rings, so it can still
            // land inside one; then the part stays where it is.
            let slot_rect = rotated_courtyard_rect(&slot, sizes[index]);
            if rings.iter().any(|ring| ring.intersects(&slot_rect)) {
                continue;
            }
            let mut trial = placement.clone();
            if let Some(placed) = trial.components.iter_mut().find(|p| p.id == component.id) {
                *placed = slot;
            }
            let (trial_hpwl, trial_hits) =
                (total_hpwl_nm(board, &trial), keepout_hits(board, &trial));
            let trial_labels = labels_without_room(board, &trial, &sizes);
            if trial_hpwl < hpwl && trial_hits <= hits && trial_labels <= labels {
                hpwl = trial_hpwl;
                hits = trial_hits;
                labels = trial_labels;
                placement = trial;
                improved = true;
            }
        }
        if !improved {
            break;
        }
    }
    placement
}

/// Parts whose centre lies inside a keepout circle, the rule the independent
/// validator enforces (E-SYNTH-PLACE-004).
fn keepout_hits(board: &Board, placement: &Placement) -> usize {
    let centre_rect = |id: ComponentId| {
        placement
            .components
            .iter()
            .find(|placed| placed.id == id)
            .map(|placed| Rect::new(placed.center, placed.center))
    };
    placement
        .components
        .iter()
        .filter(|placed| {
            intersects_keepout(
                Rect::new(placed.center, placed.center),
                placed.id,
                board,
                placement.board_outline,
                centre_rect,
            )
        })
        .count()
}

fn apply_recommendation<S: ::std::hash::BuildHasher>(
    board: &Board,
    placement: &mut Placement,
    action: &PlacementRecommendation,
    rotation_overrides: &std::collections::HashMap<ComponentId, Rotation, S>,
) {
    let Some(component) = board
        .components
        .iter()
        .find(|component| component.refdes == action.refdes)
    else {
        return;
    };
    // A hard hint is the author's decision, so the refiner never overrides it.
    if is_hard_hinted(component) {
        return;
    }
    let id = component.id;
    let Some(placed) = placement.components.iter_mut().find(|p| p.id == id) else {
        return;
    };
    if let Some(x) = action.suggested_x_mm {
        placed.center.x_nm = mm_to_nm(x);
    }
    if let Some(y) = action.suggested_y_mm {
        placed.center.y_nm = mm_to_nm(y);
    }
    // The router's repair loop chooses these rotations and hands them in.
    if let Some(degrees) = action.suggested_rotation_deg {
        if !rotation_overrides.contains_key(&id) {
            placed.rotation = rotation_from_degrees(degrees);
        }
    }
}

/// Parts that exist to serve another part and can sit anywhere near it.
/// Connectors, crystals and switches are placed for the board, not the net.
fn is_passive(component: &synth_ir::Component) -> bool {
    matches!(
        component.kind.as_str(),
        "resistor" | "capacitor" | "inductor" | "diode" | "led" | "fuse"
    )
}

fn is_hard_hinted(component: &synth_ir::Component) -> bool {
    component
        .placement_hint
        .as_ref()
        .is_some_and(|hint| hint.priority == synth_ir::PlacementPriority::Hard)
}

fn rotation_from_degrees(degrees: u32) -> Rotation {
    match degrees {
        90 => Rotation::Ninety,
        180 => Rotation::OneEighty,
        270 => Rotation::TwoSeventy,
        _ => Rotation::Zero,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_geometry::nm_to_mm;
    use synth_ir::Board;

    /// The authoritative courtyard rect for every placement, in the same
    /// order as `placement.components`.
    ///
    /// Tests must use this rather than a rect centred on `placement.center`:
    /// a placement anchor is footprint-relative (a DIP anchors near one corner,
    /// with a courtyard offset of several millimetres), so anchor-centring
    /// invents phantom overlaps and phantom out-of-bounds violations.
    fn authoritative_courtyards(board: &Board, placement: &Placement) -> Vec<Rect> {
        placement
            .components
            .iter()
            .map(|p| {
                let comp = board.component(p.id).expect("component");
                let size = comp.part.as_ref().map_or_else(
                    || fallback_courtyard(&comp.kind),
                    |part| synth_layout::pcb_courtyard_geometry_for_part(part).1,
                );
                courtyard_rect_for_placement(
                    p,
                    size.0,
                    size.1,
                    Some(PLACEMENT_COURTYARD_OFFSET_MM),
                    0,
                )
            })
            .collect()
    }

    /// Load a fixture against the real registry so the placer
    /// sees real `Part` metadata.
    fn load_board(path: &str) -> Board {
        load_board_replacing(path, &[])
    }

    fn load_board_replacing(path: &str, replacements: &[(&str, &str)]) -> Board {
        let mut source = std::fs::read_to_string(path).expect("read fixture");
        for (from, to) in replacements {
            assert!(source.contains(from), "{from} not in {path}");
            source = source.replace(from, to);
        }
        board_from_source(&source, path)
    }

    /// Lower a SynthSpec source held in the test itself.
    ///
    /// No shipped design declares a mounting hole, a fiducial or a test point,
    /// so a board carrying the furniture cannot be exercised through a fixture
    /// file without adding one to the repository for the placer's sake alone.
    fn board_from_source(source: &str, path: &str) -> Board {
        let file = path.to_string();
        let parse = synth_parser::parse(source, file.clone());
        let ast = parse.ast.as_ref().expect("parse");
        let registry_dir = std::path::Path::new("../..").join("registry").join("parts");
        let registry = synth_registry::load_dir(&registry_dir).expect("registry");
        let loader = synth_ir::FsImportLoader {
            root: std::path::PathBuf::from("../.."),
        };
        let resolved = synth_ir::resolve_imports(ast, &loader, &file);
        let lowered = synth_ir::lower(&resolved.program, &registry, &file);
        lowered.board.expect("board")
    }

    #[test]
    fn a_rigid_translation_does_not_change_the_routability_estimate() {
        // The estimate is built from net bounding boxes, so moving the whole
        // board cannot change it. If it did, the exporter would pick between
        // placements on absolute position rather than on routing.
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let mut shifted = placement.clone();
        for component in &mut shifted.components {
            component.center = Point::new(
                component.center.x_nm + mm_to_nm(20.0),
                component.center.y_nm + mm_to_nm(15.0),
            );
        }
        assert_eq!(
            routability(&board, &placement),
            routability(&board, &shifted)
        );
    }

    #[test]
    fn pulling_one_part_away_from_its_nets_scores_worse() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let baseline = routability(&board, &placement);

        let mut stretched = placement.clone();
        let mut ids: Vec<ComponentId> = board.components.iter().map(|c| c.id).collect();
        ids.sort();
        let victim = ids[0];
        if let Some(placed) = stretched.components.iter_mut().find(|p| p.id == victim) {
            placed.center = Point::new(
                placed.center.x_nm + mm_to_nm(30.0),
                placed.center.y_nm + mm_to_nm(30.0),
            );
        }
        assert!(
            routability(&board, &stretched).key() > baseline.key(),
            "a part dragged off its nets must not score better"
        );
    }

    #[test]
    fn placement_is_deterministic_across_100_runs() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let reference = place(&board).expect("place");
        for run in 1..=100 {
            let current = place(&board).expect("place");
            assert_eq!(
                current, reference,
                "run {run} of place(&board) must be byte-deterministic"
            );
        }
    }

    #[test]
    fn every_component_is_placed_exactly_once() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        assert_eq!(
            placement.components.len(),
            board.components.len(),
            "every IR component must have a placement"
        );
    }

    #[test]
    fn no_two_courtyards_overlap() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let rects = authoritative_courtyards(&board, &placement);
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                assert!(
                    !rects[i].intersects(&rects[j]),
                    "courtyards {} ({}) and {} ({}) overlap: {:?} vs {:?}",
                    i,
                    placement.components[i].id.0,
                    j,
                    placement.components[j].id.0,
                    rects[i],
                    rects[j]
                );
            }
        }
    }

    #[test]
    #[ignore = "Stage 2 swap refinement replaced by semantic floorplanning"]
    fn refinement_never_worsens_hpwl() {
        // Lock in the slice-3 invariant: post-refinement HPWL ≤
        // pre-refinement HPWL. The refinement loop only accepts
        // strictly-decreasing swaps, so the inequality is by
        // construction; the test guards against future changes
        // that would break it.
        use synth_layout::pcb_courtyard_for_part;

        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");

        // Reconstruct the pre-refinement placement: run the
        // greedy first-fit and grab the HPWL before refinement
        // would have happened. Easiest way is to call `place`,
        // which gives the *refined* HPWL — and then construct
        // an "unrefined" baseline by sorting components onto
        // the grid in IR order, like slice-1A did.
        let placement = place(&board).expect("place");
        let pad_offsets = build_pad_offset_lookup(&board);
        let cluster_pairs = build_cluster_pairs(&board);
        let refined_cost = total_cost(&board, &placement.components, &pad_offsets, &cluster_pairs);

        // Synthesize an alternative arrangement by reversing
        // every pair of components and confirm `place`'s output
        // is no worse. Using the *worst* arrangement is
        // overkill; checking that no single swap improves the
        // returned placement is the local-optimum invariant the
        // refinement promises.
        let courtyard_map: std::collections::HashMap<ComponentId, (f64, f64)> = board
            .components
            .iter()
            .map(|c| {
                let (w, h) = c
                    .part
                    .as_ref()
                    .map_or_else(|| fallback_courtyard(&c.kind), pcb_courtyard_for_part);
                (c.id, (w + 1.0, h + 1.0))
            })
            .collect();
        let margin_nm = mm_to_nm(BOARD_MARGIN_MM);
        let usable = Rect::new(
            Point::new(margin_nm, margin_nm),
            Point::new(
                placement.board_outline.max.x_nm - margin_nm,
                placement.board_outline.max.y_nm - margin_nm,
            ),
        );

        for i in 0..placement.components.len() {
            for j in (i + 1)..placement.components.len() {
                let mut alt = placement.components.clone();
                let centre_i = alt[i].center;
                let centre_j = alt[j].center;
                alt[i].center = centre_j;
                alt[j].center = centre_i;
                if !placements_valid(&board, &alt, &courtyard_map, usable) {
                    continue; // illegal swap; refinement would reject.
                }
                let alt_cost = total_cost(&board, &alt, &pad_offsets, &cluster_pairs);
                assert!(
                    alt_cost >= refined_cost,
                    "swap of {:?} and {:?} would lower cost from {refined_cost} to {alt_cost}; \
                     refinement is supposed to be a local minimum",
                    alt[i].id,
                    alt[j].id,
                );
            }
        }
    }

    #[test]
    fn cluster_cohesion_reduces_decoupling_distance() {
        // Slice 4 contract (relaxed): cluster cohesion in the
        // refinement pass strictly reduces the *summed* L1
        // distance of every (IC, decoupling-cap) pair vs a
        // placement where no swap was accepted on the cluster
        // term. Per-pair tightness within a few millimetres
        // requires either hard constraints (registry-declared
        // "decap within N mm" rules) or multi-component moves
        // (slice 3 only does pair swaps, which can't reach
        // some local minima when the IC is much bigger than
        // the cap).
        //
        // We measure the property by comparing the actual
        // refined placement against the *grid-major* baseline
        // (slice 1A's algorithm — components placed in
        // IR order on a fixed grid). The cluster-weighted
        // refinement must improve the sum-of-L1.
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let refined = place(&board).expect("place");
        let cluster_pairs = build_cluster_pairs(&board);
        if cluster_pairs.is_empty() {
            return;
        }

        let l1_sum = |components: &[ComponentPlacement]| -> i64 {
            let by_id: std::collections::HashMap<ComponentId, Point> =
                components.iter().map(|p| (p.id, p.center)).collect();
            let mut total = 0_i64;
            for (anchor, member) in &cluster_pairs {
                let (Some(a), Some(m)) = (by_id.get(anchor), by_id.get(member)) else {
                    continue;
                };
                total += (a.x_nm - m.x_nm).abs() + (a.y_nm - m.y_nm).abs();
            }
            total
        };
        let refined_l1 = l1_sum(&refined.components);

        // Synthesize the IR-order grid baseline (no clustering,
        // no HPWL refinement). 30 mm pitch, 8 columns — same as
        // slice 1A.
        let baseline: Vec<ComponentPlacement> = board
            .components
            .iter()
            .enumerate()
            .map(|(idx, c)| {
                let col = idx % 8;
                let row = idx / 8;
                let cx = mm_to_nm(5.0) + mm_to_nm(15.0) + col as i64 * mm_to_nm(30.0);
                let cy = mm_to_nm(5.0) + mm_to_nm(15.0) + row as i64 * mm_to_nm(30.0);
                ComponentPlacement {
                    id: c.id,
                    center: Point::new(cx, cy),
                    rotation: Rotation::Zero,
                    layer: Layer::Top,
                }
            })
            .collect();
        let baseline_l1 = l1_sum(&baseline);

        assert!(
            refined_l1 < baseline_l1,
            "refined cluster-distance sum {refined_l1} nm should be \
             less than naïve-grid baseline {baseline_l1} nm"
        );
    }

    #[test]
    fn all_placements_lie_inside_board_outline() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let courtyards = authoritative_courtyards(&board, &placement);
        for (p, courtyard) in placement.components.iter().zip(&courtyards) {
            assert!(
                courtyard.min.x_nm >= placement.board_outline.min.x_nm
                    && courtyard.max.x_nm <= placement.board_outline.max.x_nm
                    && courtyard.min.y_nm >= placement.board_outline.min.y_nm
                    && courtyard.max.y_nm <= placement.board_outline.max.y_nm,
                "component courtyard {:?} at ({:.2}, {:.2}) mm extends outside board outline",
                p.id,
                nm_to_mm(p.center.x_nm),
                nm_to_mm(p.center.y_nm)
            );
        }
    }

    #[test]
    fn cem_macro_floorplanning_assigns_valid_hints() {
        let board = load_board("../../fixtures/designs/secure_tracker.synth");
        let outline = Rect::new(
            Point::new(0, 0),
            Point::new(mm_to_nm(100.0), mm_to_nm(80.0)),
        );
        let assignment = cem::cem_region_assign(&board, outline);
        assert!(
            !assignment.hints.is_empty(),
            "CEM should assign region hints to macro components"
        );
    }

    /// A board that is nothing but a circuit and the furniture round it.
    const FEATURE_BOARD: &str = r#"
board "features" {
  layers 2

  component U1: ic "sn74hc595_shift"
  component C1: capacitor "c_generic_0603" value "100nF" { placement_hint { near: "U1" priority: hard } }
  component C2: capacitor "c_generic_0603" value "100nF" { placement_hint { near: "U1" priority: hard } }
  component C3: capacitor "c_generic_0603" value "100nF" { placement_hint { near: "U1" priority: hard } }
  component R1: resistor "r_generic_0603" value "10k"
  component J1: connector "header_1x4"

  component H1: mounting_hole "mounting_hole_m2_5" { placement_hint { region: top_left priority: hard } }
  component H2: mounting_hole "mounting_hole_m2_5" { placement_hint { region: top_right priority: hard } }
  component H3: mounting_hole "mounting_hole_m2_5" { placement_hint { region: bottom_left priority: hard } }
  component H4: mounting_hole "mounting_hole_m2_5" { placement_hint { region: bottom_right priority: hard } }
  component FID1: fiducial "fiducial_1mm"
  component FID2: fiducial "fiducial_1mm"

  connect J1.p1 -> U1.vcc as "V3V3"
  connect J1.p2 -> U1.gnd as "GND"
  connect U1.vcc -> C1.p1 as "V3V3"
  connect C1.p2 -> U1.gnd as "GND"
  connect U1.vcc -> C2.p1 as "V3V3"
  connect C2.p2 -> U1.gnd as "GND"
  connect U1.vcc -> C3.p1 as "V3V3"
  connect C3.p2 -> U1.gnd as "GND"
  connect J1.p3 -> R1.p1 as "SRCLK_H"
  connect R1.p2 -> U1.srclk as "SRCLK"
  connect J1.p4 -> U1.ser as "SER"
}
"#;

    /// The courtyard each placed component occupies, by refdes.
    fn courtyards_by_refdes(board: &Board, placement: &Placement) -> BTreeMap<String, Rect> {
        let mut out = BTreeMap::new();
        for placed in &placement.components {
            let Some(component) = board.component(placed.id) else {
                continue;
            };
            let (w, h) = component.part.as_ref().map_or_else(
                || fallback_courtyard(&component.kind),
                |part| synth_layout::pcb_courtyard_geometry_for_part(part).1,
            );
            out.insert(
                component.refdes.clone(),
                courtyard_rect_for_placement(placed, w, h, Some(PLACEMENT_COURTYARD_OFFSET_MM), 0),
            );
        }
        out
    }

    fn envelope_of<'a>(rects: impl Iterator<Item = &'a Rect>) -> Rect {
        rects
            .copied()
            .reduce(|a, b| {
                Rect::new(
                    Point::new(a.min.x_nm.min(b.min.x_nm), a.min.y_nm.min(b.min.y_nm)),
                    Point::new(a.max.x_nm.max(b.max.x_nm), a.max.y_nm.max(b.max.y_nm)),
                )
            })
            .expect("at least one rect")
    }

    /// A mounting hole asking for a corner belongs on the border, not in the
    /// middle of the routing area.
    ///
    /// Placed with the circuit, all four landed at the quadrant centres — 17 mm
    /// in from the edge on the Class-C SDRAM card — because a part with no net
    /// has nothing pulling it anywhere and is ordered last by net degree.
    #[test]
    fn a_net_less_board_feature_takes_the_border_and_not_the_middle() {
        let board = board_from_source(FEATURE_BOARD, "features.synth");
        let placement = place(&board).expect("place");
        let courtyards = courtyards_by_refdes(&board, &placement);
        let circuit = envelope_of(
            courtyards
                .iter()
                .filter(|(refdes, _)| !refdes.starts_with('H') && !refdes.starts_with("FID"))
                .map(|(_, rect)| rect),
        );

        for feature in ["H1", "H2", "H3", "H4", "FID1", "FID2"] {
            let rect = courtyards[feature];
            let band = mm_to_nm(6.0);
            let on_border = rect.min.x_nm <= circuit.min.x_nm + band
                || rect.max.x_nm >= circuit.max.x_nm - band
                || rect.min.y_nm <= circuit.min.y_nm + band
                || rect.max.y_nm >= circuit.max.y_nm - band;
            assert!(
                on_border,
                "{feature} is not within 6 mm of an edge of the circuit envelope"
            );
        }
    }

    const FEATURES_ONLY_BOARD: &str = r#"
board "features_only" {
  layers 2

  component H1: mounting_hole "mounting_hole_m2_5"
  component H2: mounting_hole "mounting_hole_m2_5"
  component FID1: fiducial "fiducial_1mm"
  component FID2: fiducial "fiducial_1mm"
}
"#;

    /// A board whose only components are net-less features still places them.
    ///
    /// Features are partitioned out of the solver, so the border pass is the
    /// only thing that can place them. With no circuit there is no envelope to
    /// hug, and returning on that absence dropped all four: the placement came
    /// back with an outline and an empty component list, so a panel coupon
    /// exported with neither a hole nor a fiducial on it.
    #[test]
    fn a_board_of_only_features_still_places_every_one_of_them() {
        let board = board_from_source(FEATURES_ONLY_BOARD, "features_only.synth");
        let placement = place(&board).expect("place");

        assert_eq!(
            placement.components.len(),
            board.components.len(),
            "every component must be placed, not only the ones carrying a net"
        );

        // And they must not be stacked on each other at the same point.
        let courtyards = courtyards_by_refdes(&board, &placement);
        for (left, a) in &courtyards {
            for (right, b) in &courtyards {
                if left >= right {
                    continue;
                }
                let disjoint = a.max.x_nm <= b.min.x_nm
                    || b.max.x_nm <= a.min.x_nm
                    || a.max.y_nm <= b.min.y_nm
                    || b.max.y_nm <= a.min.y_nm;
                assert!(disjoint, "{left} and {right} overlap");
            }
        }
    }

    #[test]
    fn hard_region_hint_lands_in_correct_quadrant() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let hints = vec![ExternalHint {
            component: Some("U1".into()),
            region: Some("top_left".into()),
            priority: Some("hard".into()),
            ..Default::default()
        }];
        let (placement, report) = place_with_hints(&board, &hints).expect("place_with_hints");
        println!("REPORT: {report:?}");
        let u1 = placement
            .component_by_refdes(&board, "U1")
            .expect("U1 component placement");
        let margin_nm = mm_to_nm(BOARD_MARGIN_MM);
        let usable = Rect::new(
            Point::new(margin_nm, margin_nm),
            Point::new(
                placement.board_outline.max.x_nm - margin_nm,
                placement.board_outline.max.y_nm - margin_nm,
            ),
        );
        let cx = usable.min.x_nm + usable.width_nm() / 2;
        let cy = usable.min.y_nm + usable.height_nm() / 2;
        assert!(u1.center.x_nm <= cx && u1.center.y_nm <= cy);
        assert!(report
            .hints
            .iter()
            .any(|h| h.component == "U1" && h.status == HintStatus::Honoured));
    }

    /// Lock in why the placer has no orientation objective for small parts.
    ///
    /// Turning one 0603 changes board HPWL by well under 1% - far less than the
    /// spread between candidate *positions* - so any search that scores
    /// position and rotation together settles a passive's orientation by
    /// tie-break. This test records the measurement so a future orientation
    /// objective can be justified against it, and so the gap does not close
    /// silently without anyone noticing why it existed.
    ///
    /// Macros are excluded deliberately: rotating a connector or a DIP *does*
    /// move HPWL substantially, so those are already oriented meaningfully by
    /// the floorplan's mating-face rules. It is specifically the passives - the
    /// parts a human turns so their pads face the part they connect to - that
    /// the current cost model cannot resolve.
    #[test]
    fn hpwl_is_insensitive_to_passive_rotation() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let pad_offsets = build_pad_offset_lookup(&board);
        let baseline = hpwl_total(&board, &placement.components, &pad_offsets);

        let rotations = [
            Rotation::Zero,
            Rotation::Ninety,
            Rotation::OneEighty,
            Rotation::TwoSeventy,
        ];

        let passives: Vec<ComponentId> = placement
            .components
            .iter()
            .filter(|p| {
                board.component(p.id).is_some_and(|c| {
                    matches!(
                        c.kind.as_str(),
                        "resistor" | "capacitor" | "diode" | "inductor" | "led"
                    )
                })
            })
            .map(|p| p.id)
            .collect();
        assert!(!passives.is_empty(), "expected passive components");

        let mut worst_delta_pct = 0.0_f64;
        for id in passives {
            let original = placement
                .components
                .iter()
                .find(|p| p.id == id)
                .expect("passive placement")
                .rotation;
            for rot in rotations {
                if rot == original {
                    continue;
                }
                let mut trial = placement.clone();
                trial
                    .components
                    .iter_mut()
                    .find(|q| q.id == id)
                    .expect("passive placement")
                    .rotation = rot;
                let cost = hpwl_total(&board, &trial.components, &pad_offsets);
                #[allow(clippy::cast_precision_loss)]
                let delta_pct = 100.0 * (cost - baseline).abs() as f64 / baseline as f64;
                worst_delta_pct = worst_delta_pct.max(delta_pct);
            }
        }

        assert!(
            worst_delta_pct < 5.0,
            "expected HPWL to be near-blind to passive rotation, but some rotation \
             moved it by {worst_delta_pct:.2}%"
        );
    }

    #[test]
    fn describe_placement_returns_cluster_summary() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let desc = describe_placement(&board, &placement);
        assert!(!desc.cluster_summary.is_empty());
        assert!(!desc.component_regions.is_empty());
    }

    // ----- structural visual review gate -------------------------------
    //
    // The gate is what blocks routing, so it needs its own coverage: a review
    // that silently stops firing is indistinguishable from a good placement
    // until a board reaches fabrication.

    /// `describe_placement` runs neither the router nor DRC, so it must not
    /// claim a clean board.
    #[test]
    fn visual_review_does_not_assert_unevaluated_drc_or_routing() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let desc = describe_placement(&board, &placement);
        assert_eq!(
            desc.drc_clean, None,
            "placement review must not report drc_clean without running DRC"
        );
        assert_eq!(
            desc.unrouted_nets, None,
            "placement review must not report unrouted_nets without routing"
        );
    }

    /// A real placement of a real design should clear the gate. If this fails,
    /// the placer regressed or the gate thresholds drifted.
    #[test]
    fn visual_review_passes_for_a_placed_example() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let desc = describe_placement(&board, &placement);
        assert!(
            !desc.visual_review.requires_revision,
            "expected a clean visual review, got: {:?}",
            desc.visual_review.findings
        );
        assert_eq!(desc.visual_review.score, 100);
        assert!(desc.visual_review.findings.is_empty());
    }

    /// Score must be `100 - 20 * findings`, saturating at zero.
    #[test]
    fn visual_review_score_tracks_finding_count() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");

        // Push one component clear of the right-hand edge, adding exactly one
        // "outside board outline" finding.
        let mut broken = placement.clone();
        let edge_x = broken.board_outline.max.x_nm;
        broken.components[0].center.x_nm = edge_x + mm_to_nm(20.0);
        let desc = describe_placement(&board, &broken);

        let outside = desc
            .visual_review
            .findings
            .iter()
            .filter(|f| f.contains("extends outside the board outline"))
            .count();
        assert_eq!(outside, 1, "expected one outside-outline finding");
        assert_eq!(
            desc.visual_review.score,
            100u8.saturating_sub((desc.visual_review.findings.len() as u8) * 20)
        );
        assert!(desc.visual_review.requires_revision);
        assert!(
            desc.visual_review
                .recommended_actions
                .iter()
                .any(|a| a.action == "keep_courtyard_inside_board"),
            "expected a keep_courtyard_inside_board recommendation, got {:?}",
            desc.visual_review.recommended_actions
        );
    }

    /// Two components sharing a spot must be reported with a separating move.
    #[test]
    fn visual_review_reports_overlapping_courtyards_with_a_separation() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");

        // Collapse the second component onto the first.
        let mut broken = placement.clone();
        broken.components[1].center = broken.components[0].center;
        let desc = describe_placement(&board, &broken);

        assert!(
            desc.visual_review
                .findings
                .iter()
                .any(|f| f.contains("courtyard overlap")),
            "expected a courtyard overlap finding, got {:?}",
            desc.visual_review.findings
        );
        assert!(desc.visual_review.requires_revision);
        let sep = desc
            .visual_review
            .recommended_actions
            .iter()
            .find(|a| a.action == "separate_courtyards")
            .expect("expected a separate_courtyards recommendation");
        // The suggestion must actually move the component.
        assert!(
            sep.suggested_x_mm.is_some() || sep.suggested_y_mm.is_some(),
            "separate_courtyards must propose a new position"
        );
    }

    /// Every recommendation must name the component it is about and carry a
    /// human-readable rationale, otherwise it is not actionable.
    #[test]
    fn visual_review_recommendations_are_actionable() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let mut broken = placement.clone();
        broken.components[0].center.x_nm = broken.board_outline.max.x_nm + mm_to_nm(20.0);
        broken.components[1].center = broken.components[0].center;
        let desc = describe_placement(&board, &broken);

        let refdeses: std::collections::HashSet<&str> =
            board.components.iter().map(|c| c.refdes.as_str()).collect();
        assert!(!desc.visual_review.recommended_actions.is_empty());
        for action in &desc.visual_review.recommended_actions {
            assert!(
                refdeses.contains(action.refdes.as_str()),
                "recommendation names unknown refdes {:?}",
                action.refdes
            );
            assert!(
                !action.rationale.trim().is_empty(),
                "recommendation for {} has no rationale",
                action.refdes
            );
            assert!(
                !action.action.is_empty(),
                "recommendation for {} has no action",
                action.refdes
            );
        }
    }

    fn refine(board: &Board, placement: &Placement) -> Placement {
        refine_placement(board, placement, &std::collections::HashMap::new())
    }

    const TWO_LOOSE_RESISTORS: &str = r#"
        component R1: resistor "r_generic_0603" value "10k"
        component R2: resistor "r_generic_0603" value "10k"
    "#;

    /// The refiner applies the review's own moves, so a part pushed off the
    /// board comes back inside it and the review stops firing.
    #[test]
    fn refine_placement_pulls_a_part_back_inside_the_outline() {
        let (board, mut broken) = row_board(TWO_LOOSE_RESISTORS, &[10.0, 50.0]);
        move_part(&mut broken, &board, "R1", 80.0, 15.0);

        let refined = refine(&board, &broken);
        let review = describe_placement(&board, &refined).visual_review;

        assert!(
            !review
                .findings
                .iter()
                .any(|f| f.contains("extends outside the board outline")),
            "still outside: {:?}",
            review.findings
        );
    }

    /// Two parts stacked on one spot are pulled apart.
    #[test]
    fn refine_placement_separates_overlapping_courtyards() {
        let (board, broken) = row_board(TWO_LOOSE_RESISTORS, &[30.0, 30.0]);

        let refined = refine(&board, &broken);
        let review = describe_placement(&board, &refined).visual_review;

        assert!(
            !review
                .findings
                .iter()
                .any(|f| f.contains("courtyard overlap")),
            "still overlapping: {:?}",
            review.findings
        );
    }

    /// Refining an already-refined placement neither reopens a finding nor
    /// lengthens the wiring.
    #[test]
    fn refining_again_reopens_nothing_and_does_not_lengthen_the_wiring() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let refined = refine(&board, &placement);
        assert!(
            !describe_placement(&board, &refined)
                .visual_review
                .requires_revision
        );
        assert!(total_hpwl_nm(&board, &refined) <= total_hpwl_nm(&board, &placement));
    }

    #[test]
    fn refine_placement_is_deterministic() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let mut broken = place(&board).expect("place");
        broken.components[1].center = broken.components[0].center;
        assert_eq!(refine(&board, &broken), refine(&board, &broken));
    }

    /// Lower `components` into a board on a 60 x 30 mm outline and put them in
    /// a row at `x_mm`, 15 mm down, in declaration order.
    fn row_board(components: &str, x_mm: &[f64]) -> (Board, Placement) {
        let source = format!("board \"row\" {{ layers 2 {components} }}");
        let file = "row.synth".to_string();
        let parse = synth_parser::parse(&source, file.clone());
        let ast = parse.ast.as_ref().expect("parses");
        let registry =
            synth_registry::load_dir(&std::path::Path::new("../..").join("registry").join("parts"))
                .expect("registry");
        let board = synth_ir::lower(ast, &registry, &file)
            .board
            .expect("lowers");
        let mut placement = place(&board).expect("place");
        placement.board_outline =
            Rect::new(Point::new(0, 0), Point::new(mm_to_nm(60.0), mm_to_nm(30.0)));
        assert_eq!(board.components.len(), x_mm.len(), "one x per part");
        for (component, x) in board.components.iter().zip(x_mm) {
            move_part(&mut placement, &board, &component.refdes, *x, 15.0);
        }
        (board, placement)
    }

    fn move_part(placement: &mut Placement, board: &Board, refdes: &str, x_mm: f64, y_mm: f64) {
        let id = board
            .components
            .iter()
            .find(|c| c.refdes == refdes)
            .expect("part exists")
            .id;
        let placed = placement
            .components
            .iter_mut()
            .find(|p| p.id == id)
            .expect("part placed");
        placed.center = Point::new(mm_to_nm(x_mm), mm_to_nm(y_mm));
    }

    fn centre_of(board: &Board, placement: &Placement, refdes: &str) -> Point {
        let id = board
            .components
            .iter()
            .find(|c| c.refdes == refdes)
            .expect("part exists")
            .id;
        placement
            .components
            .iter()
            .find(|p| p.id == id)
            .expect("placed")
            .center
    }

    const TWO_RESISTORS: &str = r#"
        component R1: resistor "r_generic_0603" value "10k"
        component R2: resistor "r_generic_0603" value "10k"
        connect R1.p2 -> R2.p1
    "#;

    /// Connected passives drifted far apart are pulled together: that is the
    /// wirelength objective, and the refiner keeps a move only if it lowers it.
    #[test]
    fn refine_placement_shortens_the_wire_between_connected_parts() {
        let (board, placement) = row_board(TWO_RESISTORS, &[10.0, 50.0]);
        let refined = refine(&board, &placement);
        assert!(
            total_hpwl_nm(&board, &refined) < total_hpwl_nm(&board, &placement) / 2,
            "wire not shortened"
        );
    }

    /// An IC is never moved to shorten a wire: other parts are placed around
    /// it, so only the small passive travels.
    #[test]
    fn refine_placement_never_moves_an_ic() {
        let components = r#"
            component U1: mcu "rp2350"
            component R1: resistor "r_generic_0603" value "1k"
            connect U1.usb_dp -> R1.p1
        "#;
        let (board, placement) = row_board(components, &[15.0, 50.0]);
        let refined = refine(&board, &placement);
        assert_eq!(
            centre_of(&board, &refined, "U1"),
            centre_of(&board, &placement, "U1")
        );
        assert!(total_hpwl_nm(&board, &refined) < total_hpwl_nm(&board, &placement));
    }

    /// The router's repair loop picks rotations and hands them to the placer;
    /// a review move must not rotate those parts back.
    #[test]
    fn a_review_move_never_rotates_a_part_the_caller_pinned() {
        let (board, placement) = row_board(TWO_LOOSE_RESISTORS, &[10.0, 50.0]);
        let r1 = board
            .components
            .iter()
            .find(|c| c.refdes == "R1")
            .expect("R1")
            .id;
        let rotate = PlacementRecommendation {
            action: "rotate_connector_parallel_to_edge".into(),
            refdes: "R1".into(),
            relative_to: None,
            suggested_x_mm: None,
            suggested_y_mm: None,
            suggested_rotation_deg: Some(90),
            rationale: String::new(),
        };
        let rotation_of = |p: &Placement| {
            p.components
                .iter()
                .find(|c| c.id == r1)
                .expect("placed")
                .rotation
        };

        let mut pinned = placement.clone();
        let overrides = std::collections::HashMap::from([(r1, Rotation::Zero)]);
        apply_recommendation(&board, &mut pinned, &rotate, &overrides);
        assert_eq!(rotation_of(&pinned), rotation_of(&placement));

        let mut free = placement.clone();
        apply_recommendation(
            &board,
            &mut free,
            &rotate,
            &std::collections::HashMap::new(),
        );
        assert_eq!(rotation_of(&free), Rotation::Ninety);
    }

    /// The wirelength pass is dropped when it reopens a review finding, and
    /// kept when it does not.
    #[test]
    fn a_wirelength_result_that_reopens_a_finding_is_dropped() {
        let (board, clean) = row_board(TWO_LOOSE_RESISTORS, &[10.0, 50.0]);
        let (_, stacked) = row_board(TWO_LOOSE_RESISTORS, &[30.0, 30.0]);
        assert!(
            describe_placement(&board, &stacked)
                .visual_review
                .requires_revision,
            "setup: stacked parts overlap"
        );

        assert_eq!(
            keep_unless_worse(&board, clean.clone(), stacked.clone()),
            clean
        );

        let (_, closer) = row_board(TWO_LOOSE_RESISTORS, &[10.0, 40.0]);
        assert_eq!(keep_unless_worse(&board, clean, closer.clone()), closer);
    }

    const FOUR_IN_A_CHAIN: &str = r#"
        component R1: resistor "r_generic_0603" value "10k"
        component R2: resistor "r_generic_0603" value "10k"
        component R3: resistor "r_generic_0603" value "10k"
        component R4: resistor "r_generic_0603" value "10k"
        connect R1.p2 -> R2.p1
        connect R2.p2 -> R3.p1
        connect R3.p2 -> R4.p1
    "#;

    fn overlaps(board: &Board, placement: &Placement) -> usize {
        describe_placement(board, placement)
            .visual_review
            .findings
            .iter()
            .filter(|f| f.contains("courtyard overlap"))
            .count()
    }

    /// A loose layout is squeezed toward its centre: the envelope around the
    /// courtyards shrinks, wiring gets no longer, and nothing overlaps.
    #[test]
    fn compaction_squeezes_a_loose_layout() {
        let (board, loose) = row_board(FOUR_IN_A_CHAIN, &[8.0, 24.0, 40.0, 56.0]);
        let packed = compact_placement(&board, loose.clone());
        assert!(
            courtyard_envelope_area_mm2(&board, &packed)
                < courtyard_envelope_area_mm2(&board, &loose) / 2.0,
            "envelope did not shrink"
        );
        assert!(total_hpwl_nm(&board, &packed) <= total_hpwl_nm(&board, &loose));
        assert_eq!(overlaps(&board, &packed), 0);
    }

    /// Parts that already touch cannot be squeezed through each other: the
    /// pass separates them again instead of leaving an overlap.
    #[test]
    fn compaction_never_leaves_an_overlap() {
        let (board, tight) = row_board(FOUR_IN_A_CHAIN, &[30.0, 33.5, 37.0, 40.5]);
        assert_eq!(overlaps(&board, &tight), 0, "setup: parts only touch");
        let packed = compact_placement(&board, tight);
        assert_eq!(overlaps(&board, &packed), 0);
    }

    /// A hard hint is the author's decision: those parts keep their places
    /// relative to each other (the whole board may shift to the origin).
    #[test]
    fn compaction_never_moves_a_hard_hinted_part() {
        let hard = "{ placement_hint { region: top_left priority: hard } }";
        let components = FOUR_IN_A_CHAIN
            .replace(
                r#"component R1: resistor "r_generic_0603" value "10k""#,
                &format!(r#"component R1: resistor "r_generic_0603" value "10k" {hard}"#),
            )
            .replace(
                r#"component R4: resistor "r_generic_0603" value "10k""#,
                &format!(r#"component R4: resistor "r_generic_0603" value "10k" {hard}"#),
            );
        let (board, loose) = row_board(&components, &[8.0, 24.0, 40.0, 56.0]);
        let packed = compact_placement(&board, loose.clone());
        let span = |p: &Placement| {
            let (r1, r4) = (centre_of(&board, p, "R1"), centre_of(&board, p, "R4"));
            (r4.x_nm - r1.x_nm, r4.y_nm - r1.y_nm)
        };
        assert_eq!(span(&packed), span(&loose));
    }

    /// Squeezing must not push a part into a keepout circle. With the antenna
    /// held in place and no keepout declared, the same squeeze does bring a
    /// part inside the circle, so the guard is what holds it back.
    #[test]
    fn compaction_never_moves_a_part_into_a_keepout() {
        let build = |keepout: &str| {
            let components = format!(
                r#"{FOUR_IN_A_CHAIN}
                   component ANT1: antenna "ant_chip_2g4" {{ placement_hint {{ region: top_left priority: hard }} }}
                   {keepout}"#
            );
            let (board, mut loose) = row_board(&components, &[8.0, 24.0, 40.0, 56.0, 30.0]);
            move_part(&mut loose, &board, "ANT1", 30.0, 30.0);
            (board, loose)
        };
        let nearest_part = |board: &Board, p: &Placement| {
            let ant = centre_of(board, p, "ANT1");
            ["R1", "R2", "R3", "R4"]
                .iter()
                .map(|r| {
                    let c = centre_of(board, p, r);
                    synth_geometry::nm_to_mm(c.x_nm - ant.x_nm)
                        .hypot(synth_geometry::nm_to_mm(c.y_nm - ant.y_nm))
                })
                .fold(f64::MAX, f64::min)
        };
        let radius_mm = 15.0;

        let (open_board, open) = build("");
        let squeezed = compact_placement(&open_board, open);
        assert!(
            nearest_part(&open_board, &squeezed) < radius_mm,
            "setup: without a keepout the squeeze enters the circle"
        );

        let (board, loose) = build("keepout antenna { radius 15mm }");
        assert_eq!(keepout_hits(&board, &loose), 0, "setup is clear");
        let packed = compact_placement(&board, loose);
        assert_eq!(keepout_hits(&board, &packed), 0);
    }

    /// A row of connectors along an edge is packed together, in the same
    /// order, so the row stops setting the board width.
    #[test]
    fn packing_an_edge_row_closes_its_gaps_and_keeps_the_order() {
        let edge = "{ placement_hint { edge: bottom priority: hard } }";
        let components = format!(
            r#"component J1: connector "header_1x3" {edge}
               component J2: connector "header_1x3" {edge}
               component J3: connector "header_1x3" {edge}
               component R1: resistor "r_generic_0603" value "10k""#
        );
        let (board, mut loose) = row_board(&components, &[8.0, 30.0, 52.0, 30.0]);
        for (refdes, x) in [("J1", 8.0), ("J2", 30.0), ("J3", 52.0)] {
            move_part(&mut loose, &board, refdes, x, 26.0);
        }
        let sizes = courtyard_sizes(&board, &loose);
        let flush_edge = |id: ComponentId| {
            board
                .component(id)
                .filter(|c| is_edge_flush_connector(c))
                .map(|_| 3)
        };

        let mut packed = loose.clone();
        pack_edge_rows(&mut packed, &sizes, &flush_edge);

        let x = |p: &Placement, r| centre_of(&board, p, r).x_nm;
        assert!(x(&packed, "J3") - x(&packed, "J1") < (x(&loose, "J3") - x(&loose, "J1")) / 2);
        assert!(x(&packed, "J1") < x(&packed, "J2") && x(&packed, "J2") < x(&packed, "J3"));
        assert_eq!(
            centre_of(&board, &packed, "R1"),
            centre_of(&board, &loose, "R1")
        );
        assert_eq!(overlaps(&board, &packed), 0);
    }

    /// A part alone in its band slides into the free spot in the band above,
    /// and the part it would have to pass stays put.
    #[test]
    fn a_lone_part_slides_into_the_band_above() {
        let components = r#"
            component R1: resistor "r_generic_0603" value "10k"
            component R2: resistor "r_generic_0603" value "10k"
            component R3: resistor "r_generic_0603" value "10k"
        "#;
        let (board, mut loose) = row_board(components, &[10.0, 30.0, 20.0]);
        move_part(&mut loose, &board, "R3", 20.0, 25.0);
        let sizes = courtyard_sizes(&board, &loose);
        let movable = vec![true; loose.components.len()];

        let mut slid = loose.clone();
        slide_parts(&mut slid, &sizes, &movable, true, true);

        let y = |p: &Placement, r| centre_of(&board, p, r).y_nm;
        assert_eq!(y(&slid, "R3"), y(&slid, "R1"), "R3 joins the band above");
        assert_eq!(y(&slid, "R1"), y(&loose, "R1"));
        assert_eq!(overlaps(&board, &slid), 0);
    }

    /// A part cannot slide through one that is in its way.
    #[test]
    fn a_sliding_part_stops_at_the_part_above_it() {
        let components = r#"
            component R1: resistor "r_generic_0603" value "10k"
            component R2: resistor "r_generic_0603" value "10k"
        "#;
        let (board, mut loose) = row_board(components, &[20.0, 20.0]);
        move_part(&mut loose, &board, "R2", 20.0, 25.0);
        let sizes = courtyard_sizes(&board, &loose);
        let movable = vec![true; loose.components.len()];

        let mut slid = loose.clone();
        slide_parts(&mut slid, &sizes, &movable, true, true);

        let y = |p: &Placement, r| centre_of(&board, p, r).y_nm;
        assert!(y(&slid, "R2") > y(&slid, "R1"), "order kept");
        assert!(y(&slid, "R2") < y(&loose, "R2"), "closed the gap");
        assert_eq!(overlaps(&board, &slid), 0);
    }

    /// A connector row follows the body inward as far as nothing is in its way.
    #[test]
    fn an_edge_row_moves_inward_to_meet_the_body() {
        let edge = "{ placement_hint { edge: bottom priority: hard } }";
        let components = format!(
            r#"component J1: connector "header_1x3" {edge}
               component R1: resistor "r_generic_0603" value "10k""#
        );
        let (board, mut loose) = row_board(&components, &[30.0, 30.0]);
        move_part(&mut loose, &board, "J1", 30.0, 40.0);
        move_part(&mut loose, &board, "R1", 30.0, 10.0);
        let sizes = courtyard_sizes(&board, &loose);
        let flush_edge = |id: ComponentId| {
            board
                .component(id)
                .filter(|c| is_edge_flush_connector(c))
                .map(|_| 3)
        };

        let mut moved = loose.clone();
        shift_rows_inward(&mut moved, &sizes, &flush_edge);

        let y = |p: &Placement, r| centre_of(&board, p, r).y_nm;
        assert!(y(&moved, "J1") < y(&loose, "J1"), "row moved up");
        assert!(
            y(&moved, "J1") > y(&moved, "R1"),
            "row still below the part"
        );
        assert_eq!(overlaps(&board, &moved), 0);
    }

    /// Reference labels need a free side to sit on. A grid of parts with
    /// long designators is squeezed only as far as every part keeps room for
    /// its label, so the silkscreen stays readable.
    #[test]
    fn compaction_keeps_a_free_side_for_every_label() {
        // Two columns, three rows, 8 mm apart: plenty of room for labels.
        let grid = [
            ("RLONG1", 15.0, 6.0),
            ("RLONG2", 40.0, 6.0),
            ("RLONG3", 15.0, 14.0),
            ("RLONG4", 40.0, 14.0),
            ("RLONG5", 15.0, 22.0),
            ("RLONG6", 40.0, 22.0),
        ];
        let components: String = grid
            .iter()
            .map(|(n, _, _)| format!("component {n}: resistor \"r_generic_0603\" value \"10k\"\n"))
            .collect::<Vec<_>>()
            .concat();
        let (board, mut loose) = row_board(&components, &[0.0; 6]);
        for (name, x, y) in grid {
            move_part(&mut loose, &board, name, x, y);
        }
        let sizes = courtyard_sizes(&board, &loose);
        assert_eq!(
            labels_without_room(&board, &loose, &sizes),
            0,
            "setup: every label has room"
        );

        let packed = compact_placement(&board, loose.clone());

        let sizes = courtyard_sizes(&board, &packed);
        assert_eq!(labels_without_room(&board, &packed, &sizes), 0);
        assert!(
            courtyard_envelope_area_mm2(&board, &packed)
                < courtyard_envelope_area_mm2(&board, &loose),
            "setup: the grid still compacts"
        );
    }

    /// The wirelength pass does not crowd a part so that its reference label
    /// loses the last free side: R2 is boxed in above, below and to the right,
    /// so a part parked on its left would leave its label nowhere to go.
    #[test]
    fn the_wirelength_pass_leaves_every_label_a_free_side() {
        let names = ["RLONG1", "RLONG2", "RLONG3", "RLONG4", "RLONG5", "RLONG6"];
        // Only R1 may move; the parts around R2 are held by hard hints.
        let held = "{ placement_hint { region: top_left priority: hard } }";
        let mut components: String = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let hint = if i == 0 { "" } else { held };
                format!("component {n}: resistor \"r_generic_0603\" value \"10k\" {hint}\n")
            })
            .collect::<Vec<_>>()
            .concat();
        components.push_str("connect RLONG1.p2 -> RLONG2.p1\n");
        let (board, mut loose) = row_board(&components, &[0.0; 6]);
        for (name, x, y) in [
            ("RLONG1", 8.0, 15.0),
            ("RLONG2", 30.0, 15.0),
            ("RLONG3", 30.0, 12.4),
            ("RLONG4", 30.0, 17.6),
            ("RLONG5", 33.4, 15.0),
            ("RLONG6", 27.0, 12.4),
        ] {
            move_part(&mut loose, &board, name, x, y);
        }
        let sizes = courtyard_sizes(&board, &loose);
        let before = labels_without_room(&board, &loose, &sizes);

        let moved = shorten_wires(&board, loose.clone());

        let after = labels_without_room(&board, &moved, &sizes);
        assert!(after <= before, "labels without room: {before} -> {after}");
    }

    /// With nothing but a spread-out row of edge connectors, only packing the
    /// row can shrink the board, so this is the compaction loop's packing step.
    #[test]
    fn compaction_packs_a_lone_connector_row() {
        let edge = "{ placement_hint { edge: bottom priority: hard } }";
        let components = format!(
            r#"component J1: connector "header_1x3" {edge}
               component J2: connector "header_1x3" {edge}
               component J3: connector "header_1x3" {edge}"#
        );
        let (board, mut loose) = row_board(&components, &[0.0; 3]);
        for (refdes, x) in [("J1", 8.0), ("J2", 30.0), ("J3", 52.0)] {
            move_part(&mut loose, &board, refdes, x, 26.0);
        }

        let packed = compact_placement(&board, loose.clone());

        let x = |p: &Placement, r| centre_of(&board, p, r).x_nm;
        assert!(x(&packed, "J3") - x(&packed, "J1") < (x(&loose, "J3") - x(&loose, "J1")) / 2);
        assert_eq!(overlaps(&board, &packed), 0);
    }

    /// A bottom connector row follows the body up when the body sits far
    /// above it, which is what makes the board shorter.
    #[test]
    fn compaction_moves_a_bottom_row_up_to_meet_the_body() {
        let edge = "{ placement_hint { edge: bottom priority: hard } }";
        let components = format!(
            r#"component J1: connector "header_1x3" {edge}
               component R1: resistor "r_generic_0603" value "10k"
               component R2: resistor "r_generic_0603" value "10k""#
        );
        let (board, mut loose) = row_board(&components, &[0.0; 3]);
        move_part(&mut loose, &board, "J1", 30.0, 40.0);
        move_part(&mut loose, &board, "R1", 25.0, 8.0);
        move_part(&mut loose, &board, "R2", 35.0, 8.0);

        let packed = compact_placement(&board, loose.clone());

        let y = |p: &Placement| centre_of(&board, p, "J1").y_nm;
        assert!(y(&packed) < y(&loose), "the row did not move up");
        assert_eq!(overlaps(&board, &packed), 0);
    }

    /// The flush side of an edge connector comes from its hint. Here the
    /// connector pokes out furthest on the left, so the geometry alone would
    /// pick the left edge; the hint says bottom.
    #[test]
    fn the_outline_is_flush_on_the_hinted_side() {
        let edge = "{ placement_hint { edge: bottom priority: hard } }";
        let components = format!(
            r#"component J1: connector "header_1x3" {edge}
               component R1: resistor "r_generic_0603" value "10k"
               component R2: resistor "r_generic_0603" value "10k""#
        );
        let (board, mut placement) = row_board(&components, &[0.0; 3]);
        move_part(&mut placement, &board, "J1", 8.0, 24.0);
        move_part(&mut placement, &board, "R1", 40.0, 10.0);
        move_part(&mut placement, &board, "R2", 46.0, 12.0);
        let j1 = board
            .components
            .iter()
            .find(|c| c.refdes == "J1")
            .expect("J1");
        let placed = placement.component_by_refdes(&board, "J1").expect("placed");
        let bottom_gap = |p: &Placement| {
            p.board_outline.max.y_nm - sidecar_courtyard_rect(&board, j1, placed).max.y_nm
        };

        let mut by_hint = placement.clone();
        tighten_outline_to_hints(&board, &mut by_hint);
        let mut by_geometry = placement.clone();
        tighten_outline_after_sidecar(&board, &mut by_geometry);

        assert!(
            bottom_gap(&by_hint) <= mm_to_nm(1.0),
            "flush on the hinted bottom"
        );
        assert!(
            bottom_gap(&by_geometry) > mm_to_nm(1.0),
            "setup: geometry picks another side"
        );
    }

    /// An IC keeps its decoupling caps beside it: squeezing the board does not
    /// leave them further away than they started.
    #[test]
    fn compaction_does_not_pull_an_ic_away_from_its_caps() {
        let components = r#"
            component U1: mcu "rp2350"
            component C1: capacitor "c_generic_0603" value "100n"
            component C2: capacitor "c_generic_0603" value "100n"
            component R1: resistor "r_generic_0603" value "10k"
            connect U1.dvdd -> C1.p1
            connect U1.gnd -> C1.p2
            connect U1.dvdd -> C2.p1
            connect U1.gnd -> C2.p2
        "#;
        let (board, mut loose) = row_board(components, &[0.0; 4]);
        move_part(&mut loose, &board, "U1", 30.0, 15.0);
        move_part(&mut loose, &board, "C1", 21.0, 15.0);
        move_part(&mut loose, &board, "C2", 39.0, 15.0);
        move_part(&mut loose, &board, "R1", 5.0, 5.0);
        let gap_mm = |p: &Placement, cap: &str| {
            let (u, c) = (centre_of(&board, p, "U1"), centre_of(&board, p, cap));
            nm_to_mm(c.x_nm - u.x_nm).hypot(nm_to_mm(c.y_nm - u.y_nm))
        };

        let packed = compact_placement(&board, loose.clone());

        for cap in ["C1", "C2"] {
            assert!(
                gap_mm(&packed, cap) <= gap_mm(&loose, cap) + 0.01,
                "{cap} moved away from U1: {:.1} -> {:.1} mm",
                gap_mm(&loose, cap),
                gap_mm(&packed, cap)
            );
        }
    }

    /// A caller that is about to apply a sidecar asks for the uncompacted
    /// layout; the default keeps compacting.
    #[test]
    fn place_with_hints_can_skip_compaction() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let (uncompacted, _) = place_with_hints_compacting(&board, &[], false).expect("place");
        assert_eq!(
            uncompacted,
            place_with_default_tuning(&board, false).expect("place")
        );
        let (compacted, _) = place_with_hints(&board, &[]).expect("place");
        assert_eq!(compacted, place(&board).expect("place"));
        assert_ne!(
            compacted, uncompacted,
            "setup: compaction changes this board"
        );
    }

    /// One squeeze step on its own: a free part scales toward the middle and
    /// a bottom connector row moves up by the same proportion, as one body.
    #[test]
    fn a_squeeze_step_moves_parts_and_edge_rows_toward_the_middle() {
        let edge = "{ placement_hint { edge: bottom priority: hard } }";
        let components = format!(
            r#"component J1: connector "header_1x3" {edge}
               component J2: connector "header_1x3" {edge}
               component R1: resistor "r_generic_0603" value "10k""#
        );
        let (board, mut loose) = row_board(&components, &[0.0; 3]);
        move_part(&mut loose, &board, "J1", 20.0, 40.0);
        move_part(&mut loose, &board, "J2", 40.0, 40.0);
        move_part(&mut loose, &board, "R1", 30.0, 10.0);
        let frozen = vec![false; loose.components.len()];
        let flush_edge = |id: ComponentId| flush_edge_index(&board, id);
        let middle = Point::new(mm_to_nm(30.0), mm_to_nm(25.0));

        let mut squeezed = loose.clone();
        squeeze(&mut squeezed, 900, middle, &frozen, &flush_edge);

        let at = |p: &Placement, r| centre_of(&board, p, r);
        // y: 40 -> 25 + 15 * 0.9 = 38.5, snapped to the grid; R1: 10 -> 11.5.
        assert_eq!(at(&squeezed, "J1").y_nm, mm_to_nm(38.5));
        assert_eq!(
            at(&squeezed, "J2").y_nm,
            at(&squeezed, "J1").y_nm,
            "one body"
        );
        assert_eq!(
            at(&squeezed, "J1").x_nm,
            at(&loose, "J1").x_nm,
            "row keeps its spacing"
        );
        assert_eq!(at(&squeezed, "R1").y_nm, mm_to_nm(11.5));
    }

    /// The router needs room to leave a many-pin part's pins, so the
    /// wirelength pass does not park a passive inside that part's ring even
    /// though it would shorten the wire.
    #[test]
    fn the_wirelength_pass_keeps_out_of_a_many_pin_part_s_ring() {
        let components = r#"
            component U1: mcu "rp2350" { placement_hint { region: top_left priority: hard } }
            component C1: capacitor "c_generic_0603" value "100n"
            connect U1.dvdd -> C1.p1
        "#;
        let (board, mut loose) = row_board(components, &[0.0; 2]);
        move_part(&mut loose, &board, "U1", 30.0, 15.0);
        move_part(&mut loose, &board, "C1", 5.0, 15.0);

        let moved = shorten_wires(&board, loose.clone());

        let rect_of = |p: &Placement, refdes: &str| {
            let component = board
                .components
                .iter()
                .find(|c| c.refdes == refdes)
                .expect(refdes);
            let placed = p.component_by_refdes(&board, refdes).expect(refdes);
            sidecar_courtyard_rect(&board, component, placed)
        };
        let (u1, c1) = (rect_of(&moved, "U1"), rect_of(&moved, "C1"));
        let gap_nm = (c1.min.x_nm - u1.max.x_nm)
            .max(u1.min.x_nm - c1.max.x_nm)
            .max(c1.min.y_nm - u1.max.y_nm)
            .max(u1.min.y_nm - c1.max.y_nm);
        assert!(
            gap_nm >= mm_to_nm(DENSE_PART_RING_MM / 2.0) - mm_to_nm(0.01),
            "C1 sits {:.1} mm from U1",
            nm_to_mm(gap_nm)
        );
        assert_ne!(moved, loose, "setup: C1 still moves toward U1");
    }

    #[test]
    fn compaction_is_deterministic() {
        let (board, loose) = row_board(FOUR_IN_A_CHAIN, &[8.0, 24.0, 40.0, 56.0]);
        assert_eq!(
            compact_placement(&board, loose.clone()),
            compact_placement(&board, loose)
        );
    }

    /// Swapping one finding for another leaves the count unchanged, and it
    /// still reopens something the kept placement did not have.
    #[test]
    fn a_result_that_swaps_one_finding_for_another_is_dropped() {
        let (board, mut r1_off) = row_board(TWO_LOOSE_RESISTORS, &[10.0, 50.0]);
        move_part(&mut r1_off, &board, "R1", 80.0, 15.0);
        let mut r2_off = r1_off.clone();
        move_part(&mut r2_off, &board, "R1", 10.0, 15.0);
        move_part(&mut r2_off, &board, "R2", 80.0, 15.0);
        let findings_of = |p: &Placement| describe_placement(&board, p).visual_review.findings;
        assert_eq!(findings_of(&r1_off).len(), findings_of(&r2_off).len());
        assert_ne!(findings_of(&r1_off), findings_of(&r2_off));

        assert_eq!(keep_unless_worse(&board, r1_off.clone(), r2_off), r1_off);
    }

    /// A finding whose distance changed is the same finding, so a move that
    /// only shortens it still counts as an improvement.
    #[test]
    fn a_finding_that_only_changes_its_distance_is_the_same_finding() {
        let review = |finding: &str| VisualPlacementReview {
            score: 0,
            requires_revision: true,
            findings: vec![finding.to_string()],
            recommended_actions: Vec::new(),
        };
        let far = review("dense connector J1 is 9.5 mm from its nearest board edge");
        let near = review("dense connector J1 is 4.0 mm from its nearest board edge");
        let other = review("dense connector J2 is 4.0 mm from its nearest board edge");
        assert!(!introduces_finding(&far, &near));
        assert!(introduces_finding(&far, &other));

        let hint = |delta: &str| {
            review(&format!(
                "hard placement hint not honored: U1 should be left of J1, but its courtyard is right of J1 (delta {delta} mm); revise placement before routing"
            ))
        };
        assert!(!introduces_finding(&hint("4.2, -3.0"), &hint("1.5, -3.0")));
    }

    /// A two-pin connector is still a connector: it sits where the board edge
    /// and the cable want it, so only the resistor moves.
    #[test]
    fn refine_placement_never_moves_a_two_pin_connector() {
        let components = r#"
            component J1: connector "jst_ph_2pin"
            component R1: resistor "r_generic_0603" value "1k"
            connect J1.p1 -> R1.p1
        "#;
        let (board, placement) = row_board(components, &[10.0, 50.0]);
        let refined = refine(&board, &placement);
        assert_eq!(
            centre_of(&board, &refined, "J1"),
            centre_of(&board, &placement, "J1")
        );
        assert_ne!(
            centre_of(&board, &refined, "R1"),
            centre_of(&board, &placement, "R1")
        );
    }

    /// A hard hint is the author's decision: the refiner never moves a part
    /// that carries one, so a hard-hinted pair stays exactly where it is.
    #[test]
    fn hard_hinted_parts_are_never_moved_by_the_refiner() {
        let hard = "{ placement_hint { edge: top priority: hard } }";
        let components = format!(
            r#"component R1: resistor "r_generic_0603" value "10k" {hard}
               component R2: resistor "r_generic_0603" value "10k" {hard}
               connect R1.p2 -> R2.p1"#
        );
        let (board, apart) = row_board(&components, &[10.0, 50.0]);
        assert_eq!(refine(&board, &apart), apart);

        let (board, stacked) = row_board(&components, &[30.0, 30.0]);
        assert_eq!(refine(&board, &stacked), stacked);
    }

    /// With one part hard-hinted and one free, the free one comes to it,
    /// whichever order the two are declared in.
    #[test]
    fn the_free_part_of_a_pair_moves_to_a_hard_hinted_one() {
        let hard = "{ placement_hint { edge: top priority: hard } }";
        for (r1_hint, r2_hint, free) in [(hard, "", "R2"), ("", hard, "R1")] {
            let components = format!(
                r#"component R1: resistor "r_generic_0603" value "10k" {r1_hint}
                   component R2: resistor "r_generic_0603" value "10k" {r2_hint}
                   connect R1.p2 -> R2.p1"#
            );
            let (board, placement) = row_board(&components, &[10.0, 50.0]);
            let refined = refine(&board, &placement);
            let pinned = if free == "R2" { "R1" } else { "R2" };
            assert_eq!(
                centre_of(&board, &refined, pinned),
                centre_of(&board, &placement, pinned)
            );
            assert_ne!(
                centre_of(&board, &refined, free),
                centre_of(&board, &placement, free)
            );
        }
    }

    /// A part must not be pulled into a keepout to shorten a wire. The same
    /// board without the keepout does move R1 into that circle, so the guard
    /// is what holds it back.
    #[test]
    fn the_refiner_never_moves_a_part_into_a_keepout() {
        let build = |keepout: &str| {
            let components = format!(
                r#"{TWO_RESISTORS}
                   component ANT1: antenna "ant_chip_2g4"
                   {keepout}"#
            );
            let (board, mut placement) = row_board(&components, &[10.0, 50.0, 30.0]);
            move_part(&mut placement, &board, "ANT1", 35.0, 15.0);
            (board, placement)
        };
        let radius_mm = 14.0;
        let distance_to_antenna = |board: &Board, p: &Placement| {
            let (r1, ant) = (centre_of(board, p, "R1"), centre_of(board, p, "ANT1"));
            nm_to_mm(r1.x_nm - ant.x_nm).hypot(nm_to_mm(r1.y_nm - ant.y_nm))
        };

        let (open_board, open) = build("");
        let moved_freely = refine(&open_board, &open);
        assert!(
            distance_to_antenna(&open_board, &moved_freely) < radius_mm,
            "setup: without a keepout R1 should be pulled inside the circle"
        );

        let (board, placement) = build("keepout antenna { radius 14mm }");
        assert_eq!(keepout_hits(&board, &placement), 0, "setup is clear");
        let refined = refine(&board, &placement);
        assert_eq!(keepout_hits(&board, &refined), 0);
    }

    #[test]
    fn a_diff_pair_records_its_skew_tolerance_and_coupling() {
        // A declared tolerance is the only way a design can say whether a
        // reported skew is acceptable, and `couple` is the author's statement
        // about how hard the router should try. Both must survive lowering.
        let source = r#"
        board "pair_spec" {
          layers 2
          component J1: connector "usb_c_receptacle"
          component U1: mcu "rp2350"
          connect J1.dp -> U1.usb_dp
          connect J1.dn -> U1.usb_dn
          connect J1.gnd -> U1.gnd

          diff_pair J1_dp J1_dn {
            impedance 90ohm
            max_skew 0.15mm
            couple tight
          }
        }
        "#;
        let file = "pair_spec.synth".to_string();
        let parse = synth_parser::parse(source, file.clone());
        let ast = parse.ast.as_ref().expect("parses");
        let registry =
            synth_registry::load_dir(&std::path::Path::new("../..").join("registry").join("parts"))
                .expect("registry");
        let lowered = synth_ir::lower(ast, &registry, &file);
        let board = lowered.board.expect("lowers");

        let pair = board.diff_pairs.first().expect("a pair is declared");
        assert_eq!(
            pair.max_skew.map(synth_ir::Length::to_mm),
            Some(0.15),
            "the declared skew tolerance reaches the IR"
        );
        assert_eq!(pair.couple, Some(synth_ir::Couple::Tight));
    }

    #[test]
    fn a_pin_qualified_near_hint_steers_placement_to_that_pin() {
        // `near: U1.dvdd` must parse as a component *and* a pin, and the
        // pad-local target must aim at that pin rather than at whichever
        // net the two parts happened to share first.
        let source = r#"
        board "pin_near" {
          layers 2
          component U1: mcu "rp2350"
          component C1: capacitor "c_generic_0805" value "100n" {
            placement_hint { near: U1.dvdd priority: hard }
          }
          connect U1.dvdd -> C1.p1
          connect U1.gnd -> C1.p2
        }
        "#;
        let file = "pin_near.synth".to_string();
        let parse = synth_parser::parse(source, file.clone());
        let ast = parse.ast.as_ref().expect("parses");
        let registry =
            synth_registry::load_dir(&std::path::Path::new("../..").join("registry").join("parts"))
                .expect("registry");
        let lowered = synth_ir::lower(ast, &registry, &file);
        let board = lowered.board.expect("lowers");

        // The hint survives lowering with the pin attached.
        let by_refdes = |r: &str| {
            board
                .components
                .iter()
                .find(|c| c.refdes == r)
                .expect("component")
        };
        let c1 = by_refdes("C1");
        let near = c1
            .placement_hint
            .as_ref()
            .and_then(|hint| hint.near.clone())
            .expect("near hint");
        assert_eq!(near, "U1.dvdd");

        // And it resolves to U1's dvdd pin, not to some other pin.
        let (anchor, pin) = resolve_near_target(&board, &near).expect("resolves");
        assert_eq!(anchor, by_refdes("U1").id);
        let pin_name = board
            .pin(anchor, pin.expect("a pin"))
            .expect("named pin")
            .name
            .clone();
        assert_eq!(pin_name, "dvdd");

        // Placement must still succeed with the pin-qualified hint.
        place(&board).expect("place with a pin-qualified near hint");
    }

    #[test]
    fn a_drag_onto_an_occupied_square_displaces_the_occupant() {
        // A drag is an instruction, not a suggestion. The old legalization
        // searched for a *nearby* legal square and, finding none, left the
        // requested coordinate in place — which put two courtyards on top of
        // each other and exported an illegal board. Whoever is in the way has
        // to move.
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let victim = placement
            .components
            .iter()
            .find(|p| board.component(p.id).is_some_and(|c| c.refdes == "C1"))
            .copied()
            .expect("C1 placed");
        let mover = placement
            .components
            .iter()
            .find(|p| board.component(p.id).is_some_and(|c| c.refdes == "C2"))
            .copied()
            .expect("C2 placed");

        // Drag C2 exactly onto C1's square.
        let mut dragged = placement.clone();
        let index = dragged
            .components
            .iter()
            .position(|p| p.id == mover.id)
            .expect("C2 placement");
        dragged.components[index].center = victim.center;

        let sidecar = synth_layout::sidecar::SidecarLayout {
            schema_version: synth_layout::sidecar::SIDECAR_SCHEMA_VERSION,
            components: [(
                "C2".to_string(),
                synth_layout::sidecar::SidecarPlacement {
                    x: synth_geometry::nm_to_mm(victim.center.x_nm),
                    y: synth_geometry::nm_to_mm(victim.center.y_nm),
                    rotation: 0,
                    sheet: None,
                    source: synth_layout::sidecar::OverrideSource::HumanDrag,
                    priority: synth_layout::sidecar::OverridePriority::Hard,
                    timestamp: None,
                    relative_to: None,
                    dx: 0.0,
                    dy: 0.0,
                },
            )]
            .into_iter()
            .collect(),
            forced_net_labels: Vec::new(),
            labels: std::collections::HashMap::new(),
            fit_sheet: false,
        };

        let mut legalized = dragged;
        apply_sidecar_overrides(&board, &mut legalized, &sidecar);

        // Every pair of courtyards must be clear of one another.
        let by_refdes = |r: &str| {
            legalized
                .component_by_refdes(&board, r)
                .expect("placed")
                .center
        };
        for (a, b) in [("C1", "C2"), ("C1", "R1"), ("C2", "R1")] {
            let gap_x = (by_refdes(a).x_nm - by_refdes(b).x_nm).abs();
            let gap_y = (by_refdes(a).y_nm - by_refdes(b).y_nm).abs();
            assert!(
                gap_x > mm_to_nm(0.5) || gap_y > mm_to_nm(0.5),
                "{a} and {b} overlap after a drag displaced them"
            );
        }
    }

    #[test]
    fn a_soft_override_that_blocks_another_does_not_strand_it_at_the_origin() {
        // Two soft overrides land on the same square. Legalising the first
        // displaces the second; if the second's "requested" slot were then
        // read back from the live placement it would return wherever it was
        // parked — the outline corner — and the part would be exported at
        // (0, 0) instead of where the sidecar asked.
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let auto = place(&board).expect("place");
        let square = |refdes: &str| {
            auto.component_by_refdes(&board, refdes)
                .expect("placed")
                .center
        };
        let contested = square("C2");
        let entry = |refdes: &str, center: synth_geometry::Point| {
            (
                refdes.to_string(),
                synth_layout::sidecar::SidecarPlacement {
                    x: synth_geometry::nm_to_mm(center.x_nm),
                    y: synth_geometry::nm_to_mm(center.y_nm),
                    rotation: 0,
                    sheet: None,
                    source: synth_layout::sidecar::OverrideSource::Agent,
                    priority: synth_layout::sidecar::OverridePriority::Soft,
                    timestamp: None,
                    relative_to: None,
                    dx: 0.0,
                    dy: 0.0,
                },
            )
        };
        let sidecar = synth_layout::sidecar::SidecarLayout {
            schema_version: synth_layout::sidecar::SIDECAR_SCHEMA_VERSION,
            components: [entry("C1", contested), entry("C2", contested)]
                .into_iter()
                .collect(),
            forced_net_labels: Vec::new(),
            labels: std::collections::HashMap::new(),
            fit_sheet: false,
        };

        let mut placement = auto.clone();
        apply_sidecar_overrides(&board, &mut placement, &sidecar);

        let origin = synth_geometry::Point::new(
            placement.board_outline.min.x_nm,
            placement.board_outline.min.y_nm,
        );
        // The exact landing spot depends on the local legalizer, but neither
        // part may be left at the parking corner, and both must be legal and
        // inside the board.
        for refdes in ["C1", "C2"] {
            let component_index = board
                .components
                .iter()
                .position(|component| component.refdes == refdes)
                .expect("component");
            let placement_index = placement
                .components
                .iter()
                .position(|placed| placed.id == board.components[component_index].id)
                .expect("placement");
            let current = placement.components[placement_index];
            assert_ne!(
                current.center, origin,
                "{refdes} was parked at the outline corner"
            );
            assert!(
                sidecar_position_is_legal(&board, &placement, component_index, current),
                "{refdes} ended somewhere illegal"
            );
        }
    }

    #[test]
    fn sidecar_overrides_are_applied_by_tuned_placement() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");

        // Without a sidecar the tuned entry point matches plain place.
        let plain = place(&board).expect("place");
        let untuned =
            place_with_tuning_and_sidecar(&board, 1.5, &std::collections::HashMap::new(), None)
                .expect("place");
        assert_eq!(plain, untuned);

        // With a sidecar drag on U1 the requested rotation is preserved. The
        // solver may move an invalid coordinate to the nearest legal slot.
        let sidecar_path = std::env::temp_dir().join(format!(
            "synth-place-sidecar-test-{}.toml",
            std::process::id()
        ));
        std::fs::write(
            &sidecar_path,
            "schema_version = 2\n\n[components.U1]\nx = 31.0\ny = 13.0\nrotation = 90\nsource = \"human_drag\"\npriority = \"hard\"\n",
        )
        .expect("sidecar TOML written");
        let overridden = place_with_tuning_and_sidecar(
            &board,
            1.5,
            &std::collections::HashMap::new(),
            Some(&sidecar_path),
        )
        .expect("place");
        std::fs::remove_file(&sidecar_path).ok();
        let u1 = overridden
            .component_by_refdes(&board, "U1")
            .expect("U1 placed");
        assert_eq!(u1.rotation, Rotation::Ninety);
        let u1_index = board
            .components
            .iter()
            .position(|component| component.refdes == "U1")
            .expect("U1 component");
        let u1_placement_index = overridden
            .components
            .iter()
            .position(|placed| placed.id == board.components[u1_index].id)
            .expect("U1 placement");
        assert!(sidecar_position_is_legal(
            &board,
            &overridden,
            u1_index,
            overridden.components[u1_placement_index]
        ));
    }

    /// A sidecar was written against the uncompacted layout, so a board that
    /// has one is placed without compaction and every part it leaves alone
    /// stays where the sidecar's author saw it.
    #[test]
    fn a_board_with_a_sidecar_is_placed_without_compaction() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let sidecar_path = std::env::temp_dir().join(format!(
            "synth-place-no-compact-{}.toml",
            std::process::id()
        ));
        std::fs::write(
            &sidecar_path,
            "schema_version = 2\n\n[components.U1]\nx = 31.0\ny = 13.0\nrotation = 90\nsource = \"human_drag\"\npriority = \"hard\"\n",
        )
        .expect("sidecar TOML written");

        let with_sidecar = place_with_sidecar(&board, Some(&sidecar_path)).expect("place");

        let mut expected = place_with_default_tuning(&board, false).expect("place");
        let sidecar = synth_layout::sidecar::SidecarLayout::load_from_file(&sidecar_path)
            .expect("sidecar loads");
        apply_sidecar_overrides(&board, &mut expected, &sidecar);
        tighten_outline_after_sidecar(&board, &mut expected);
        std::fs::remove_file(&sidecar_path).ok();

        assert_eq!(with_sidecar, expected);
        assert_ne!(
            with_sidecar,
            {
                let mut compacted = place(&board).expect("place");
                apply_sidecar_overrides(&board, &mut compacted, &sidecar);
                tighten_outline_after_sidecar(&board, &mut compacted);
                compacted
            },
            "setup: compaction changes this board"
        );
    }

    const EDGES: [&str; 4] = ["left", "right", "top", "bottom"];

    fn with_hint(board: &Board, refdes: &str, edge: &str, priority: &str) -> Board {
        board_with_external_hints(
            board,
            &[ExternalHint {
                component: Some(refdes.into()),
                edge: Some(edge.into()),
                priority: Some(priority.into()),
                ..ExternalHint::default()
            }],
        )
    }

    const FEATHER: &str = "../../fixtures/designs/feather_m4_express.synth";
    const WIDE_PAD_PLUG: &str = "Connector_USB:USB3_A_Plug_Wuerth_692112030100_Horizontal";
    const LITERAL_COPPER_CLEARANCE_NM: i64 = 500_000;

    fn feather_with_j2_hint(edge: &str, priority: &str) -> Board {
        with_hint(&load_board(FEATHER), "J2", edge, priority)
    }

    fn with_wide_pad_footprint(mut board: Board, refdes: &str) -> Board {
        let part = board
            .components
            .iter_mut()
            .find(|c| c.refdes == refdes)
            .and_then(|c| c.part.as_mut())
            .expect("part");
        part.kicad_footprint = Some(WIDE_PAD_PLUG.into());
        board
    }

    fn pad_extents_as_exported(board: &Board, placed: &ComponentPlacement) -> Rect {
        let part = board
            .component(placed.id)
            .and_then(|c| c.part.as_ref())
            .expect("part");
        let origin = footprint_origin(Some(part), placed);
        let pads = synth_layout::kicad_footprint_loader::pads(
            part.kicad_footprint.as_deref().expect("footprint"),
        )
        .expect("pads");
        let mut bounds = Rect::new(
            Point::new(i64::MAX, i64::MAX),
            Point::new(i64::MIN, i64::MIN),
        );
        for pad in pads {
            let (dx, dy) = placed
                .rotation
                .rotate_offset(mm_to_nm(pad.center_mm.0), mm_to_nm(pad.center_mm.1));
            let (w, h) = if placed.rotation.swaps_extents() {
                (pad.size_mm.1, pad.size_mm.0)
            } else {
                pad.size_mm
            };
            bounds.min.x_nm = bounds.min.x_nm.min(origin.x_nm + dx - mm_to_nm(w) / 2);
            bounds.min.y_nm = bounds.min.y_nm.min(origin.y_nm + dy - mm_to_nm(h) / 2);
            bounds.max.x_nm = bounds.max.x_nm.max(origin.x_nm + dx + mm_to_nm(w) / 2);
            bounds.max.y_nm = bounds.max.y_nm.max(origin.y_nm + dy + mm_to_nm(h) / 2);
        }
        bounds
    }

    fn gaps_to_outline(rect: Rect, outline: Rect) -> [i64; 4] {
        [
            rect.min.x_nm - outline.min.x_nm,
            outline.max.x_nm - rect.max.x_nm,
            rect.min.y_nm - outline.min.y_nm,
            outline.max.y_nm - rect.max.y_nm,
        ]
    }

    fn courtyard_gaps(board: &Board, placement: &Placement, refdes: &str) -> [i64; 4] {
        let placed = placement.component_by_refdes(board, refdes).expect(refdes);
        let component = board.component(placed.id).expect("component");
        gaps_to_outline(
            sidecar_courtyard_rect(board, component, placed),
            placement.board_outline,
        )
    }

    fn assert_flush_with_copper_clear(
        board: &Board,
        placement: &Placement,
        refdes: &str,
        edge: &str,
    ) {
        assert_connector_flush(board, placement, refdes, edge);
        let side = EDGES.iter().position(|e| *e == edge).expect("edge");
        for component in &board.components {
            let gaps = courtyard_gaps(board, placement, &component.refdes);
            for (index, gap) in gaps.iter().enumerate() {
                assert!(
                    (component.refdes == refdes && index == side) || *gap >= mm_to_nm(2.0),
                    "{} must keep its normal margin on {}, gap {gap} nm",
                    component.refdes,
                    EDGES[index]
                );
            }
        }
    }

    fn assert_connector_flush(board: &Board, placement: &Placement, refdes: &str, edge: &str) {
        let placed = placement.component_by_refdes(board, refdes).expect(refdes);
        let side = EDGES.iter().position(|e| *e == edge).expect("edge");
        let courtyard = courtyard_gaps(board, placement, refdes)[side];
        let copper = gaps_to_outline(
            pad_extents_as_exported(board, placed),
            placement.board_outline,
        )[side];
        assert!(
            courtyard >= -1_000 && copper >= LITERAL_COPPER_CLEARANCE_NM - 1_000,
            "{refdes} on {edge}: courtyard gap {courtyard} nm, copper gap {copper} nm"
        );
        assert!(
            courtyard <= 1_000 || copper <= LITERAL_COPPER_CLEARANCE_NM + 1_000,
            "{refdes} on {edge} is neither flush nor held back by copper: courtyard gap {courtyard} nm, copper gap {copper} nm"
        );
    }

    fn only_hinted_connectors(hints: &[(&str, &str)]) -> Board {
        let mut board = load_board(FEATHER);
        for (refdes, edge) in hints {
            board = with_hint(&board, refdes, edge, "hard");
        }
        board
            .components
            .retain(|c| hints.iter().any(|(refdes, _)| c.refdes == *refdes));
        for (index, component) in board.components.iter_mut().enumerate() {
            component.id = ComponentId(u32::try_from(index).expect("index"));
        }
        board.nets.clear();
        board
    }

    #[test]
    fn a_lone_hard_edge_hinted_connector_is_flush_in_the_solver_and_the_sidecar_pass() {
        for edge in EDGES {
            let board = only_hinted_connectors(&[("J2", edge)]);
            let placement = place(&board).expect("place");
            assert_flush_with_copper_clear(&board, &placement, "J2", edge);
            let mut tightened = placement.clone();
            tightened.board_outline = Rect::new(Point::new(0, 0), Point::new(1, 1));
            tighten_outline_after_sidecar(&board, &mut tightened);
            assert_flush_with_copper_clear(&board, &tightened, "J2", edge);
        }
    }

    #[test]
    fn two_hard_connectors_on_opposite_edges_are_both_flush() {
        for hints in [
            [("J2", "left"), ("J3", "right")],
            [("J2", "top"), ("J3", "bottom")],
        ] {
            let board = only_hinted_connectors(&hints);
            let mut placement = place(&board).expect("place");
            tighten_outline_after_sidecar(&board, &mut placement);
            for (refdes, edge) in hints {
                assert_connector_flush(&board, &placement, refdes, edge);
            }
            let solved = place(&board).expect("place");
            for (refdes, edge) in hints {
                assert_connector_flush(&board, &solved, refdes, edge);
            }
        }
    }

    #[test]
    fn hard_edge_hinted_connectors_are_flush_and_soft_ones_are_not_in_the_solver() {
        let header_1x20 = [(
            "component J4: connector \"header_1x4\"",
            "component J4: connector \"header_1x20\"",
        )];
        let mut cases: Vec<(&str, &[(&str, &str)], &str, &str, bool)> = Vec::new();
        for edge in EDGES {
            cases.push((FEATHER, &[], "J2", edge, false));
            cases.push((FEATHER, &[], "J2", edge, true));
            cases.push((
                "../../examples/env_logger/env_logger.synth",
                &[],
                "J2",
                edge,
                true,
            ));
        }
        cases.push((
            "../../fixtures/designs/iot_sensor_board.synth",
            &[],
            "J1",
            "top",
            false,
        ));
        cases.push((FEATHER, &[], "J1", "bottom", false));
        cases.push((FEATHER, &header_1x20, "J4", "bottom", false));
        for (path, replacements, refdes, edge, wide_pads) in cases {
            for priority in ["hard", "soft"] {
                let board = load_board_replacing(path, replacements);
                let board = if wide_pads {
                    with_wide_pad_footprint(board, refdes)
                } else {
                    board
                };
                let board = with_hint(&board, refdes, edge, priority);
                let placement = place(&board).expect("place");
                let rects = authoritative_courtyards(&board, &placement);
                for (i, a) in rects.iter().enumerate() {
                    for b in &rects[i + 1..] {
                        assert!(
                            !a.intersects(b),
                            "{refdes} {edge} {priority} wide={wide_pads}: {a:?} and {b:?} overlap"
                        );
                    }
                }
                if priority == "hard" {
                    assert_flush_with_copper_clear(&board, &placement, refdes, edge);
                } else {
                    let gaps = courtyard_gaps(&board, &placement, refdes);
                    assert!(
                        gaps.iter().all(|gap| *gap >= mm_to_nm(2.0)),
                        "soft hint for {refdes} on {edge} must not reach the edge, gaps {gaps:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn solver_keeps_overhanging_pads_clear_of_the_edge_in_every_orientation() {
        for edge in EDGES {
            for rotation in [
                Rotation::Zero,
                Rotation::Ninety,
                Rotation::OneEighty,
                Rotation::TwoSeventy,
            ] {
                let board = load_board("../../examples/env_logger/env_logger.synth");
                let board = with_wide_pad_footprint(board, "J2");
                let board = with_hint(&board, "J2", edge, "hard");
                let j2 = board
                    .components
                    .iter()
                    .find(|c| c.refdes == "J2")
                    .expect("J2")
                    .id;
                let turned = std::collections::HashMap::from([(j2, rotation)]);
                let placement = place_with_tuning(&board, 1.5, &turned).expect("place");
                let placed = placement.component_by_refdes(&board, "J2").expect("J2");
                if placed.rotation != rotation {
                    continue;
                }
                assert_flush_with_copper_clear(&board, &placement, "J2", edge);
            }
        }
    }

    fn j2_protruding(board: &Board, edge: &str, protrusion_mm: f64) -> Placement {
        let mut placement = place(&load_board(FEATHER)).expect("place");
        let j2 = board
            .components
            .iter()
            .find(|c| c.refdes == "J2")
            .expect("J2");
        let others: Vec<Rect> = placement
            .components
            .iter()
            .filter(|placed| placed.id != j2.id)
            .map(|placed| {
                let component = board.component(placed.id).expect("component");
                sidecar_courtyard_rect(board, component, placed)
            })
            .collect();
        let pull = mm_to_nm(protrusion_mm);
        let placed = placement
            .components
            .iter_mut()
            .find(|placed| placed.id == j2.id)
            .expect("J2 placement");
        let half = sidecar_courtyard_rect(board, j2, placed);
        let (half_w, half_h) = (half.width_nm() / 2, half.height_nm() / 2);
        match edge {
            "left" => {
                placed.center.x_nm =
                    others.iter().map(|r| r.min.x_nm).min().expect("others") - pull + half_w;
            }
            "right" => {
                placed.center.x_nm =
                    others.iter().map(|r| r.max.x_nm).max().expect("others") + pull - half_w;
            }
            "top" => {
                placed.center.y_nm =
                    others.iter().map(|r| r.min.y_nm).min().expect("others") - pull + half_h;
            }
            _ => {
                placed.center.y_nm =
                    others.iter().map(|r| r.max.y_nm).max().expect("others") + pull - half_h;
            }
        }
        placement
    }

    #[test]
    fn sidecar_outline_pass_makes_a_pulled_out_hinted_connector_flush() {
        for wide_pads in [false, true] {
            for edge in EDGES {
                let board = feather_with_j2_hint(edge, "hard");
                let board = if wide_pads {
                    with_wide_pad_footprint(board, "J2")
                } else {
                    board
                };
                let mut placement = j2_protruding(&board, edge, 12.0);
                tighten_outline_after_sidecar(&board, &mut placement);
                assert_flush_with_copper_clear(&board, &placement, "J2", edge);
            }
        }
    }

    #[test]
    fn sidecar_outline_pass_goes_flush_only_past_the_four_millimetre_margin() {
        let soft = feather_with_j2_hint("left", "soft");
        let mut placement = j2_protruding(&soft, "left", 12.0);
        tighten_outline_after_sidecar(&soft, &mut placement);
        assert_eq!(
            courtyard_gaps(&soft, &placement, "J2")[0],
            mm_to_nm(EDGE_MARGIN_MM)
        );

        let hard = feather_with_j2_hint("left", "hard");
        let mut short = j2_protruding(&hard, "left", 3.9);
        tighten_outline_after_sidecar(&hard, &mut short);
        let j2_gap = courtyard_gaps(&hard, &short, "J2")[0];
        assert!(
            (mm_to_nm(0.1) - 1_000..=mm_to_nm(0.1) + 1_000).contains(&j2_gap),
            "3.9 mm protrusion keeps the neighbours' 4 mm margin, J2 gap {j2_gap} nm"
        );
        let mut past = j2_protruding(&hard, "left", 4.1);
        tighten_outline_after_sidecar(&hard, &mut past);
        assert_flush_with_copper_clear(&hard, &past, "J2", "left");
    }

    /// Mounting holes, test points and fiducials are board features that sit
    /// in a corner, so the outline ends at their courtyard.
    #[test]
    fn a_board_feature_reaches_the_outline_without_the_four_millimetre_margin() {
        for (refdes, kind, part) in [
            ("H1", "mounting_hole", "mounting_hole_m2_5"),
            ("TP1", "testpoint", "testpoint_pad_1_5"),
            ("FID1", "fiducial", "fiducial_1mm"),
        ] {
            let board = load_board_replacing(
                FEATHER,
                &[(
                    "component J2:",
                    &format!("component {refdes}: {kind} \"{part}\"\n  component J2:"),
                )],
            );
            let mut placement = place(&board).expect("place");
            let feature = board
                .components
                .iter()
                .find(|c| c.refdes == refdes)
                .expect(refdes);
            let others_min_x = placement
                .components
                .iter()
                .filter(|placed| placed.id != feature.id)
                .map(|placed| {
                    let component = board.component(placed.id).expect("component");
                    sidecar_courtyard_rect(&board, component, placed).min.x_nm
                })
                .min()
                .expect("other parts");
            let placed = placement
                .components
                .iter_mut()
                .find(|placed| placed.id == feature.id)
                .expect("placement");
            let half_w = sidecar_courtyard_rect(&board, feature, placed).width_nm() / 2;
            placed.center.x_nm = others_min_x - mm_to_nm(10.0) + half_w;
            tighten_outline_after_sidecar(&board, &mut placement);
            assert_eq!(
                courtyard_gaps(&board, &placement, refdes)[0],
                0,
                "{kind}: the outline ends at its courtyard"
            );
        }
    }

    #[test]
    fn placer_and_exporter_agree_on_asymmetric_courtyards() {
        let board = load_board("../../fixtures/designs/feather_m4_express.synth");
        let template = board
            .components
            .iter()
            .find(|c| c.refdes == "J2")
            .expect("J2");
        for footprint in [
            "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12",
            "Connector_JST:JST_PH_B2B-PH-K_1x02_P2.00mm_Vertical",
        ] {
            let mut component = template.clone();
            let part = component.part.as_mut().expect("part");
            part.kicad_footprint = Some(footprint.into());
            let (offset, _) = synth_layout::pcb_courtyard_geometry_for_part(part);
            assert!(
                offset.0.abs() + offset.1.abs() > 0.1,
                "{footprint} must have an asymmetric courtyard"
            );
            let (cx, cy, w, h) =
                synth_layout::kicad_footprint_loader::courtyard_rect(footprint).expect("courtyard");
            for rotation in [
                Rotation::Zero,
                Rotation::Ninety,
                Rotation::OneEighty,
                Rotation::TwoSeventy,
            ] {
                let placed = ComponentPlacement {
                    id: component.id,
                    center: Point::new(mm_to_nm(31.0), mm_to_nm(17.0)),
                    rotation,
                    layer: Layer::Top,
                };
                let origin = footprint_origin(component.part.as_ref(), &placed);
                let corners =
                    [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].map(|(sx, sy)| {
                        let (dx, dy) = rotation.rotate_offset(
                            mm_to_nm(cx + sx * w / 2.0),
                            mm_to_nm(cy + sy * h / 2.0),
                        );
                        (origin.x_nm + dx, origin.y_nm + dy)
                    });
                let exported = Rect::new(
                    Point::new(
                        corners.iter().map(|c| c.0).min().expect("corners"),
                        corners.iter().map(|c| c.1).min().expect("corners"),
                    ),
                    Point::new(
                        corners.iter().map(|c| c.0).max().expect("corners"),
                        corners.iter().map(|c| c.1).max().expect("corners"),
                    ),
                );
                let placer = sidecar_courtyard_rect(&board, &component, &placed);
                assert!(
                    (placer.min.x_nm - exported.min.x_nm).abs() <= 2
                        && (placer.min.y_nm - exported.min.y_nm).abs() <= 2
                        && (placer.max.x_nm - exported.max.x_nm).abs() <= 2
                        && (placer.max.y_nm - exported.max.y_nm).abs() <= 2,
                    "{footprint} {rotation:?}: placer {placer:?} vs exporter {exported:?}"
                );
            }
        }
    }
}
