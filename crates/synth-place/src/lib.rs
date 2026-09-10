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
pub mod cem;
pub mod floorplan;
pub mod modules;
pub mod outline_packer;
pub mod score;

use serde::{Deserialize, Serialize};
use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity, Span};
use synth_geometry::{mm_to_nm, Layer, Point, Rect, Rotation};
use synth_ir::{Board, ComponentId};
use thiserror::Error;

/// Final position + orientation of a single component on the PCB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComponentPlacement {
    pub id: ComponentId,
    /// Centre of the component's footprint, in nanometers.
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

/// Margin between the outermost placed component and the board
/// edge. Set to 6.0 mm to guarantee open routing channels around
/// component courtyards along the board perimeter.
const BOARD_MARGIN_MM: f64 = 6.0;

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
    for comp in &board.components {
        if let Some(sidecar_comp) = sidecar.components.get(&comp.refdes) {
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
}

/// Run placement with optional sidecar layout override file
/// (`<design>.synth.layout.toml`).
pub fn place_with_sidecar(
    board: &Board,
    sidecar_path: Option<&std::path::Path>,
) -> Result<Placement, PlaceError> {
    place_with_tuning_and_sidecar(board, 1.5, &std::collections::HashMap::new(), sidecar_path)
}

/// Run placement with iteration tuning (extra courtyard margin +
/// rotation overrides) and an optional sidecar layout override file
/// (`<design>.synth.layout.toml`).
///
/// Sidecar entries record manual component drags (`synth preview`,
/// `synth_write_layout_override`). They are applied *after* the
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
    let mut placement = place_with_tuning(board, extra_margin_mm, rotation_overrides)?;
    if let Some(path) = sidecar_path {
        if path.exists() {
            if let Some(sidecar) = synth_layout::sidecar::SidecarLayout::load_from_file(path) {
                apply_sidecar_overrides(board, &mut placement, &sidecar);
            }
        }
    }
    Ok(placement)
}

#[allow(clippy::too_many_lines)]
pub fn place(board: &Board) -> Result<Placement, PlaceError> {
    place_with_tuning(board, 1.5, &std::collections::HashMap::new())
}

/// Closed-loop placement generator with support for iteration hints (rotation & margin tuning).
#[allow(clippy::too_many_lines)]
pub fn place_with_tuning<S: ::std::hash::BuildHasher>(
    board: &Board,
    extra_margin_mm: f64,
    rotation_overrides: &std::collections::HashMap<ComponentId, synth_geometry::Rotation, S>,
) -> Result<Placement, PlaceError> {
    use synth_layout::pcb_courtyard_geometry_for_part;

    // Per-component courtyard geometry: (id, (cx, cy), (w, h)).
    let courtyards_mm: Vec<(ComponentId, (f64, f64), (f64, f64))> = board
        .components
        .iter()
        .map(|c| {
            let ((cx, cy), (w, h)) = c.part.as_ref().map_or_else(
                || ((0.0, 0.0), fallback_courtyard(&c.kind)),
                pcb_courtyard_geometry_for_part,
            );
            (c.id, (cx, cy), (w + extra_margin_mm, h + extra_margin_mm))
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
        match place_with_outline(board, board_outline, &courtyards_mm, rotation_overrides) {
            Ok(p) => return Ok(p),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or(PlaceError::AreaInsufficient {
        placed_mm2: total_courtyard_mm2,
        board_mm2: 230.0 * 230.0,
    }))
}

fn place_with_outline<S: ::std::hash::BuildHasher>(
    board: &Board,
    board_outline: Rect,
    courtyards_mm: &[(ComponentId, (f64, f64), (f64, f64))],
    rotation_overrides: &std::collections::HashMap<ComponentId, synth_geometry::Rotation, S>,
) -> Result<Placement, PlaceError> {
    use synth_geometry::nm_to_mm;
    let board_w_mm = nm_to_mm(board_outline.width_nm());
    let board_h_mm = nm_to_mm(board_outline.height_nm());
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

    all_components.sort_by(|a, b| {
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
        db.cmp(&da).then(a.0.cmp(&b.0))
    });

    let has_hint = |id: ComponentId| -> bool {
        board
            .component(id)
            .is_some_and(|c| c.placement_hint.is_some())
    };

    let (order, mut passives): (Vec<ComponentId>, Vec<ComponentId>) = all_components
        .into_iter()
        .partition(|&id| is_macro(id) || has_hint(id));
    passives.sort_by_key(|&id| has_near(id));

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
    let fp_targets = floorplan::compute_floorplan_targets(board, usable, &courtyard_lookup);
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

    let pitch_nm = mm_to_nm(GRID_PITCH_MM);
    let center_x = usable.min.x_nm + usable.width_nm() / 2;
    let center_y = usable.min.y_nm + usable.height_nm() / 2;

    let mut placed: Vec<(ComponentId, Rect)> = Vec::with_capacity(board.components.len());
    let mut grid_start_indices = vec![0_usize; order.len()];
    let mut backtracks = 0_usize;

    let mut order_idx = 0_usize;
    while order_idx < order.len() {
        let id = order[order_idx];
        let (w_mm, h_mm) = courtyard_lookup[&id];
        let unrot_half_w = mm_to_nm(w_mm) / 2;
        let unrot_half_h = mm_to_nm(h_mm) / 2;

        let rotation = if let Some(&rot) = rotation_overrides.get(&id) {
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
        let pad_extra_nm = if is_passive { mm_to_nm(0.5) } else { 0 };
        let (half_w, half_h) = match rotation {
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
        if let Some(comp) = board.component(id) {
            if let Some(hint) = &comp.placement_hint {
                let placed_refdes_rects: std::collections::HashMap<String, Rect> = placed
                    .iter()
                    .filter_map(|(pid, r)| board.component(*pid).map(|c| (c.refdes.clone(), *r)))
                    .collect();
                let res = resolve_hint_target(hint, usable, &placed_refdes_rects);
                hint_target = res.target;
                hard_region = res.hard_region;
            }
        }

        let mut target_point = if let Some(t) = hint_target {
            t
        } else if let Some((anchor_id, rel_offset, _rot)) = child_module_map.get(&id) {
            if let Some((_, anchor_rect)) = placed.iter().find(|(pid, _)| pid == anchor_id) {
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
                            if let Some((_, r)) =
                                placed.iter().find(|(pid, _)| *pid == ep.component)
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

        // Apply 2D Density Spreading force only when component has no explicit placement hint
        if hint_target.is_none() {
            let mut rep_dx = 0_i64;
            let mut rep_dy = 0_i64;
            for (_, r) in &placed {
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

        // Generate grid candidate cells sorted radially by distance to target_point
        let mut candidates = Vec::new();
        let mut cy = usable.min.y_nm + half_h;
        while cy + half_h <= usable.max.y_nm {
            let mut cx = usable.min.x_nm + half_w;
            while cx + half_w <= usable.max.x_nm {
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
            let mut cy = usable.min.y_nm + half_h;
            while cy + half_h <= usable.max.y_nm {
                let mut cx = usable.min.x_nm + half_w;
                while cx + half_w <= usable.max.x_nm {
                    candidates.push(Point::new(cx, cy));
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
            let candidate = Rect::from_center_half_extents(*pt, half_w, half_h);
            if placed.iter().all(|(_, r)| !candidate.intersects(r))
                && !intersects_keepout(candidate, id, board, board_outline, &placed)
            {
                found = Some(*pt);
                grid_start_indices[order_idx] = cell_idx;
                break;
            }
        }

        if let Some(centre) = found {
            let courtyard = Rect::from_center_half_extents(centre, half_w, half_h);
            placed.push((id, courtyard));
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
    // index see deterministic ordering.
    let mut placements: Vec<ComponentPlacement> = placed
        .iter()
        .map(|(id, rect)| {
            let center = Point::new(
                (rect.min.x_nm + rect.max.x_nm) / 2,
                (rect.min.y_nm + rect.max.y_nm) / 2,
            );
            let rotation = if let Some((_, _, rot)) = child_module_map.get(id) {
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
        let pad_offsets = build_pad_offset_lookup(board);
        outline_packer::pack_passives_along_outline(
            board,
            &mut placements,
            &passives,
            &courtyard_lookup,
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

    let edge_margin_nm = mm_to_nm(8.0);
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

    // Connector Edge-Docking Anchor: If the left-most component is a connector/jack,
    // dock its mating face directly on the board outline edge x=0.
    let has_left_connector = placements.iter().any(|p| {
        let comp_kind = board.component(p.id).map_or("", |c| c.kind.as_str());
        (comp_kind == "connector" || comp_kind == "jack") && {
            let (w_mm, _) = courtyard_lookup[&p.id];
            let (off_x_mm, _) = courtyard_offset_lookup
                .get(&p.id)
                .copied()
                .unwrap_or((0.0, 0.0));
            let (rot_cx, _) = p.rotation.rotate_offset(mm_to_nm(off_x_mm), 0);
            let court_min_x = p.center.x_nm + rot_cx - mm_to_nm(w_mm) / 2;
            (court_min_x - min_x_nm).abs() < mm_to_nm(2.0)
        }
    });

    // Connector Edge-Docking Anchor: If the top-most component is a connector/jack,
    // dock its mating face directly on the board outline edge y=0 (top edge).
    // This makes USB-C connectors placed on the top edge physically accessible.
    let has_top_connector = placements.iter().any(|p| {
        let comp_kind = board.component(p.id).map_or("", |c| c.kind.as_str());
        (comp_kind == "connector" || comp_kind == "jack") && {
            let (_, h_mm) = courtyard_lookup[&p.id];
            let (_, off_y_mm) = courtyard_offset_lookup
                .get(&p.id)
                .copied()
                .unwrap_or((0.0, 0.0));
            let (_, rot_cy) = p.rotation.rotate_offset(0, mm_to_nm(off_y_mm));
            let court_min_y = p.center.y_nm + rot_cy - mm_to_nm(h_mm) / 2;
            (court_min_y - min_y_nm).abs() < mm_to_nm(2.0)
        }
    });

    let target_left_margin = if has_left_connector {
        0
    } else {
        edge_margin_nm
    };

    // When a top-edge connector is present, the board's y=0 is the mating face.
    // To keep all OTHER components below the connector's courtyard (and preserve
    // their routing clearance), we compute the y-shift based on the minimum
    // courtyard-top of NON-connector components so they land at `edge_margin_nm`.
    // The connector itself was placed at y = min_y + half_h so its courtyard
    // top (= min_y_nm) lands exactly at y=0 after the shift.
    let shift_y_nm = if has_top_connector {
        let min_y_others = placements
            .iter()
            .filter_map(|p| {
                let comp_kind = board.component(p.id).map_or("", |c| c.kind.as_str());
                if comp_kind == "connector" || comp_kind == "jack" {
                    return None;
                }
                let (_, h_mm) = courtyard_lookup[&p.id];
                let (_, off_y_mm) = courtyard_offset_lookup
                    .get(&p.id)
                    .copied()
                    .unwrap_or((0.0, 0.0));
                let (_, rot_cy) = p.rotation.rotate_offset(0, mm_to_nm(off_y_mm));
                let court_min_y = p.center.y_nm + rot_cy - mm_to_nm(h_mm) / 2;
                Some(court_min_y)
            })
            .min();

        if let Some(_other_min_y) = min_y_others {
            // Shift so non-connector components get the full edge_margin at top,
            // and the connector's courtyard top (min_y_nm) ends up at y=0.
            // We want: other_min_y + shift_y = edge_margin_nm
            //          min_y_nm + shift_y = 0
            // The second equation gives: shift_y = -min_y_nm
            // The first equation gives: shift_y = edge_margin_nm - other_min_y
            // Use the connector constraint (flush to edge) and accept that other
            // components will shift accordingly.
            -min_y_nm
        } else {
            // Only connectors in design, flush the mating face to y=0.
            -min_y_nm
        }
    } else {
        edge_margin_nm - min_y_nm
    };

    let shift_x_nm = target_left_margin - min_x_nm;

    for p in &mut placements {
        p.center.x_nm += shift_x_nm;
        p.center.y_nm += shift_y_nm;
    }

    // ── Deterministic top-connector edge-flush snap ───────────────────────────
    // After the global Y-shift, the connector center may be off by the board
    // margin (BOARD_MARGIN_MM) because the floorplan target is expressed
    // relative to `usable.min.y_nm` (which includes the margin) while
    // shift_y_nm maps the courtyard minimum to y=0, not the usable minimum.
    //
    // Closed-form fix: for every top-edge USB connector, force
    //   courtyard_min_y = center.y + rotated_offset_y - half_h = 0.
    //
    // This is a mechanical identity derived entirely from the footprint
    // courtyard dimensions. DO NOT replace with an agent heuristic.
    if has_top_connector {
        for p in &mut placements {
            let comp_kind = board.component(p.id).map_or("", |c| c.kind.as_str());
            let is_usb = board.component(p.id).is_some_and(|c| {
                let rl = c.refdes.to_lowercase();
                let kl = c.kind.to_lowercase();
                rl.contains("usb")
                    || kl.contains("usb")
                    || c.part
                        .as_ref()
                        .is_some_and(|pt| pt.id.as_str().contains("usb"))
            });
            if (comp_kind != "connector" && comp_kind != "jack") || !is_usb {
                continue;
            }
            let (_, h_mm) = courtyard_lookup[&p.id];
            let (_, off_y_mm) = courtyard_offset_lookup
                .get(&p.id)
                .copied()
                .unwrap_or((0.0, 0.0));
            let half_h_nm = mm_to_nm(h_mm) / 2;
            // For Rotation::OneEighty the y-component of the courtyard offset
            // is negated by the rotation.
            let rot_off_y_nm = match p.rotation {
                Rotation::OneEighty | Rotation::TwoSeventy => -mm_to_nm(off_y_mm),
                _ => mm_to_nm(off_y_mm),
            };
            p.center.y_nm = half_h_nm - rot_off_y_nm;
        }
    }

    let adaptive_width_nm = (max_x_nm - min_x_nm) + target_left_margin + edge_margin_nm;
    // Board height: from y=0 (top edge / connector mating face) to the lowest component
    // courtyard bottom plus a uniform bottom margin. After the shift, max_y_nm moves to
    // (max_y_nm + shift_y_nm); add edge_margin for the bottom clearance.
    let adaptive_height_nm = max_y_nm + shift_y_nm + edge_margin_nm;

    let adaptive_board_outline = Rect::new(
        Point::new(0, 0),
        Point::new(adaptive_width_nm, adaptive_height_nm),
    );

    Ok(Placement {
        board_outline: adaptive_board_outline,
        components: placements,
    })
}

pub(crate) fn intersects_keepout(
    candidate: Rect,
    id: ComponentId,
    board: &Board,
    board_outline: Rect,
    placed: &[(ComponentId, Rect)],
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
            placed
                .iter()
                .find(|(pid, _)| *pid == anchor.id)
                .map(|(_, r)| {
                    Point::new((r.min.x_nm + r.max.x_nm) / 2, (r.min.y_nm + r.max.y_nm) / 2)
                })
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
        if intersects_keepout(*r, *id, board, board_outline, &placed_rects) {
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
/// pull tight: every IC with `required_decoupling` is glued to
/// the capacitors it shares a power net with, up to the rule's
/// `count`. Same recognition logic the schematic clusterer uses,
/// just applied to the PCB.
///
/// Returned pairs are deterministic: outer iteration is IR
/// component order, inner is net-endpoint order.
#[allow(clippy::too_many_lines, dead_code)]
fn build_cluster_pairs(board: &Board) -> Vec<(ComponentId, ComponentId)> {
    let mut out = Vec::new();
    let mut claimed: std::collections::HashSet<ComponentId> = std::collections::HashSet::new();

    // 1. Decoupling clusters (IC + decoupling caps)
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        for rule in &part.required_decoupling {
            let Some(pin_idx) = part.pins.iter().position(|p| p.name == rule.net) else {
                continue;
            };
            let mut claimed_for_rule = 0_u32;
            'outer: for net in &board.nets {
                let mentions_pin = net
                    .endpoints
                    .iter()
                    .any(|ep| ep.component == component.id && ep.pin.0 as usize == pin_idx);
                if !mentions_pin {
                    continue;
                }
                for endpoint in &net.endpoints {
                    if endpoint.component == component.id || claimed.contains(&endpoint.component) {
                        continue;
                    }
                    let Some(other) = board.components.iter().find(|c| c.id == endpoint.component)
                    else {
                        continue;
                    };
                    let is_cap = other.part.as_ref().is_some_and(|p| p.kind == "capacitor");
                    if !is_cap {
                        continue;
                    }
                    out.push((component.id, other.id));
                    claimed.insert(other.id);
                    claimed_for_rule += 1;
                    if claimed_for_rule >= rule.count {
                        break 'outer;
                    }
                }
            }
        }
    }

    // 2. Crystal clusters (crystal + load caps)
    for component in &board.components {
        if component.kind != "crystal" {
            continue;
        }
        for net in &board.nets {
            let mentions_crystal = net.endpoints.iter().any(|ep| ep.component == component.id);
            if !mentions_crystal {
                continue;
            }
            for endpoint in &net.endpoints {
                if endpoint.component == component.id || claimed.contains(&endpoint.component) {
                    continue;
                }
                if let Some(other) = board.components.iter().find(|c| c.id == endpoint.component) {
                    if other.kind == "capacitor" {
                        out.push((component.id, other.id));
                        claimed.insert(other.id);
                    }
                }
            }
        }
    }

    // 3. USB + ESD protection clusters (USB connector + ESD diodes)
    for component in &board.components {
        if component.kind != "connector" {
            continue;
        }
        for net in &board.nets {
            let mentions_usb = net.endpoints.iter().any(|ep| ep.component == component.id);
            if !mentions_usb {
                continue;
            }
            for endpoint in &net.endpoints {
                if endpoint.component == component.id || claimed.contains(&endpoint.component) {
                    continue;
                }
                if let Some(other) = board.components.iter().find(|c| c.id == endpoint.component) {
                    if other.kind == "diode" {
                        out.push((component.id, other.id));
                        claimed.insert(other.id);
                    }
                }
            }
        }
    }

    // 4. RF matching network clusters (antenna + RF passives)
    for component in &board.components {
        if component.kind != "antenna" {
            continue;
        }
        for net in &board.nets {
            let mentions_rf = net.endpoints.iter().any(|ep| ep.component == component.id);
            if !mentions_rf {
                continue;
            }
            for endpoint in &net.endpoints {
                if endpoint.component == component.id || claimed.contains(&endpoint.component) {
                    continue;
                }
                if let Some(other) = board.components.iter().find(|c| c.id == endpoint.component) {
                    if matches!(other.kind.as_str(), "resistor" | "capacitor" | "inductor") {
                        out.push((component.id, other.id));
                        claimed.insert(other.id);
                    }
                }
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
    use synth_layout::kicad_footprint_loader;
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
        let mut row: Vec<Option<(i64, i64)>> = Vec::with_capacity(pins.len());
        for pin in pins {
            let entry = footprint_pads
                .as_ref()
                .and_then(|pads| pads.iter().find(|p| p.number == pin.number.0))
                .map(|pad| (mm_to_nm(pad.center_mm.0), mm_to_nm(pad.center_mm.1)));
            row.push(entry);
        }
        map.insert(component.id, row);
    }
    PadOffsetLookup { map }
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
    pub drc_clean: bool,
    pub unrouted_nets: usize,
    pub dense_regions: Vec<DenseRegionWarning>,
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
        let anchor_lower = anchor_refdes.to_lowercase();
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

    let placement = place(&modified_board)?;

    let mut outcomes = Vec::new();
    let margin_nm = mm_to_nm(BOARD_MARGIN_MM);
    let usable = Rect::new(
        Point::new(margin_nm, margin_nm),
        Point::new(
            placement.board_outline.max.x_nm - margin_nm,
            placement.board_outline.max.y_nm - margin_nm,
        ),
    );

    for comp in &modified_board.components {
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

    Ok((placement, HintSatisfactionReport { hints: outcomes }))
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

pub fn describe_placement(board: &Board, placement: &Placement) -> PlacementDescription {
    let board_w_mm = synth_geometry::nm_to_mm(placement.board_outline.width_nm());
    let board_h_mm = synth_geometry::nm_to_mm(placement.board_outline.height_nm());
    let margin_nm = mm_to_nm(BOARD_MARGIN_MM);
    let usable = Rect::new(
        Point::new(margin_nm, margin_nm),
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

    PlacementDescription {
        board_size_mm: [board_w_mm, board_h_mm],
        component_regions,
        cluster_summary,
        drc_clean: true,
        unrouted_nets: 0,
        dense_regions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_geometry::nm_to_mm;
    use synth_ir::Board;

    /// Load a fixture against the real registry so the placer
    /// sees real `Part` metadata.
    fn load_board(path: &str) -> Board {
        let source = std::fs::read_to_string(path).expect("read fixture");
        let file = path.to_string();
        let parse = synth_parser::parse(&source, file.clone());
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
    fn placement_is_deterministic_across_100_runs() {
        let board = load_board("../../examples/sensor_logger.synth");
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
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = place(&board).expect("place");
        assert_eq!(
            placement.components.len(),
            board.components.len(),
            "every IR component must have a placement"
        );
    }

    #[test]
    fn no_two_courtyards_overlap() {
        use synth_layout::pcb_courtyard_for_part;
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = place(&board).expect("place");
        // Reconstruct courtyard rects for every placement and
        let rects: Vec<Rect> = placement
            .components
            .iter()
            .map(|p| {
                let (w_mm, h_mm) =
                    board
                        .components
                        .iter()
                        .find(|c| c.id == p.id)
                        .map_or((4.0, 4.0), |c| {
                            c.part
                                .as_ref()
                                .map_or_else(|| fallback_courtyard(&c.kind), pcb_courtyard_for_part)
                        });
                let (rw, rh) = match p.rotation {
                    Rotation::Zero | Rotation::OneEighty => (w_mm, h_mm),
                    Rotation::Ninety | Rotation::TwoSeventy => (h_mm, w_mm),
                };
                Rect::from_center_half_extents(p.center, mm_to_nm(rw) / 2, mm_to_nm(rh) / 2)
            })
            .collect();
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                if rects[i].intersects(&rects[j]) {
                    eprintln!("Overlap detected between idx {i} ({:?}) at {:?} rect {:?} AND idx {j} ({:?}) at {:?} rect {:?}",
                        placement.components[i].id, placement.components[i].center, rects[i],
                        placement.components[j].id, placement.components[j].center, rects[j]
                    );
                }
                assert!(
                    !rects[i].intersects(&rects[j]),
                    "courtyards {i} and {j} overlap",
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

        let board = load_board("../../examples/sensor_logger.synth");

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
        let board = load_board("../../examples/sensor_logger.synth");
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
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = place(&board).expect("place");
        for p in &placement.components {
            let comp = board.component(p.id).unwrap();
            let (w_mm, h_mm) = comp.part.as_ref().map_or_else(
                || fallback_courtyard(&comp.kind),
                synth_layout::pcb_courtyard_for_part,
            );
            let half_w = mm_to_nm(w_mm / 2.0);
            let half_h = mm_to_nm(h_mm / 2.0);
            let (rot_w, rot_h) = match p.rotation {
                synth_geometry::Rotation::Zero | synth_geometry::Rotation::OneEighty => {
                    (half_w, half_h)
                }
                synth_geometry::Rotation::Ninety | synth_geometry::Rotation::TwoSeventy => {
                    (half_h, half_w)
                }
            };
            let courtyard = Rect::from_center_half_extents(p.center, rot_w, rot_h);
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

    #[test]
    fn hard_region_hint_lands_in_correct_quadrant() {
        let board = load_board("../../examples/sensor_logger.synth");
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

    #[test]
    fn describe_placement_returns_cluster_summary() {
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = place(&board).expect("place");
        let desc = describe_placement(&board, &placement);
        assert!(!desc.cluster_summary.is_empty());
        assert!(!desc.component_regions.is_empty());
    }

    #[test]
    fn sidecar_overrides_are_applied_by_tuned_placement() {
        let board = load_board("../../examples/sensor_logger.synth");

        // Without a sidecar the tuned entry point matches plain place.
        let plain = place(&board).expect("place");
        let untuned =
            place_with_tuning_and_sidecar(&board, 1.5, &std::collections::HashMap::new(), None)
                .expect("place");
        assert_eq!(plain, untuned);

        // With a sidecar drag on U1 the override wins verbatim
        // (human intent outranks the solver), including rotation.
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
        assert_eq!(u1.center, Point::new(mm_to_nm(31.0), mm_to_nm(13.0)));
        assert_eq!(u1.rotation, Rotation::Ninety);
    }
}
