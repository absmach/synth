// SPDX-License-Identifier: Apache-2.0

//! Canonical schematic wire routing (§7.8.6 of the implementation plan).
//!
//! Both consumers — the browser preview (`synth-web`) and the KiCad
//! export (`synth-kicad`) — consume [`super::Layout`]`::wires` produced
//! by [`route_board`] so the two views agree on geometry. This module
//! holds the A* grid pathfinder, the pin-approach corridors, the
//! L-route fallback, and junction detection that previously lived in
//! `synth-kicad`; moving it here is what actually populates
//! `Layout.wires` for the first time.
//!
//! The router is pure geometry: it depends only on `synth_ir` and
//! `synth_registry`, never on KiCad's s-expression or UUID formats.

mod grid;

use std::collections::{BTreeMap, HashMap, HashSet};

use synth_ir::{Board, ComponentId, NetId, PinId};
use synth_registry::{ElectricalType, Part, Pin, PinCapability};

use crate::{
    body_size_for_part, text_inclusive_half_width, ComponentPlacement, Layout, PinSide, Rotation,
    WirePath,
};

use grid::SchematicGrid;

// ----- Geometry constants ---------------------------------------------------

/// A* obstacle-grid step (mm) — matches KiCad's 50 mil schematic grid.
pub const GRID_STEP: f64 = 1.27;
/// Maximum depth of the pin-approach corridor (mm). Cluster spacing
/// (~17 mm) keeps the walk inside a part's own text-margin zone.
pub const CORRIDOR_MAX_MM: f64 = 14.0;

pub const PIN_PITCH: f64 = 2.54;
pub const BODY_HALF_WIDTH: f64 = 7.62;
pub const MIN_BODY_HEIGHT: f64 = 10.16;
pub const BODY_PIN_PADDING: f64 = 2.54;
pub const TWOPIN_HALF_W: f64 = 5.08;
pub const PIN_LENGTH: f64 = 2.54;

/// Tolerance for "same coordinate" comparisons on grid-snapped mm
/// values, which can carry ~1e-13 mm of float noise from repeated
/// `snap_grid_127` rounding — see `score::COORD_EPSILON_MM` for the
/// same reasoning (kept as a separate constant to avoid a cross-
/// module `pub(crate)` const import for one value).
const COORD_EPSILON_MM: f64 = 1e-6;

/// Snap a millimetre coordinate to the 1.27 mm (50 mil) grid used by
/// pin terminals, stubs and wire endpoints.
pub fn snap_grid_127(val: f64) -> f64 {
    (val / 1.27).round() * 1.27
}

/// Per-kind rotation offset in degrees applied when emitting a
/// symbol instance's `(at x y angle)`. Reconciles the divergence
/// between KiCad's symbol-natural orientation (which varies per
/// part) and our `Rotation` enum's logical convention
/// (`Zero` = horizontal, pin 0 on left).
///
/// - `LED`, `D`, `D_TVS` ship horizontal in `Device.kicad_sym`
///   (pin 1 on left) — already matches our convention → offset 0.
/// - `R`, `R_US`, `C`, `L` ship vertical (pin 1 on top) — needs
///   +90° to land pin 1 on the left → offset 90.
///
/// Without this offset, a resistor at `Rotation::Zero` renders
/// vertical (KiCad's natural) and a vertical LED chain with
/// `Rotation::TwoSeventy` ends up with the resistor horizontal
/// while the LED is vertical — the two pins never line up and
/// the wires look disconnected.
pub fn natural_rotation_offset(part: &Part) -> f64 {
    if let Some(sym) = part.kicad_symbol.as_deref() {
        // Strip the library prefix; only the symbol name matters.
        let name = sym.split_once(':').map_or(sym, |(_, n)| n);
        if matches!(name, "R" | "R_US" | "C" | "C_Small" | "L" | "L_Small") {
            return 90.0;
        }
    }
    0.0
}

/// Which side of a body a pin's terminal stub emerges from, derived
/// from the pin's electrical role (power up, ground down, clock/reset
/// right, data outputs right, everything else left).
///
/// The sides assigned here MUST agree with the sides the synthesized
/// symbol library actually draws pins on
/// (`synth_kicad::symbol_lib::classify_ic_pin`) — wire terminals are
/// computed from this classifier while the drawn pins come from that
/// one, and a disagreement would leave wires landing on a body edge
/// with no pin. Both copies implement the same schematic convention
/// (ProtoExpress "Schematic Design Rules": inputs on the left,
/// outputs on the right, power up, ground down); unifying them into
/// one shared function is still a known follow-up.
pub fn classify_ic_pin(pin: &Pin) -> PinSide {
    let lower = pin.name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "gnd" | "vss" | "vssa" | "gnda" | "vee" | "vneg" | "agnd" | "dgnd"
    ) {
        return PinSide::Bottom;
    }
    if matches!(
        pin.electrical_type,
        ElectricalType::PowerInput | ElectricalType::PowerOutput | ElectricalType::GroundReference
    ) {
        return PinSide::Top;
    }
    if pin.capabilities.iter().any(|c| {
        matches!(
            c,
            PinCapability::Reset
                | PinCapability::BootMode
                | PinCapability::ClockInput
                | PinCapability::ClockOutput
                | PinCapability::RfFeed
        )
    }) {
        return PinSide::Right;
    }
    // Plain data direction: outputs to the right, inputs (and
    // bidirectional/passive/analog/... — anything without a clearer
    // signal) stay left. This is the convention IC datasheets and
    // most hand-drawn schematics already follow.
    if pin.electrical_type == ElectricalType::Output {
        return PinSide::Right;
    }
    PinSide::Left
}

pub fn is_two_pin_symbol_kind(kind: &str) -> bool {
    matches!(
        kind,
        "resistor" | "capacitor" | "inductor" | "diode" | "led" | "crystal" | "switch"
    )
}

/// Absolute sheet coordinate of a pin terminal: `(x, y, dx, dy)` where
/// `(dx, dy)` is the outward unit direction of the pin's wire stub.
///
/// Prefers the part's real KiCad symbol geometry (`kicad_lib_loader`)
/// so terminals land exactly on the symbol's pins; falls back to the
/// synthesized body geometry used for `synth:` fallback symbols.
#[allow(clippy::implicit_hasher)]
pub fn pin_terminal_xy(
    board: &Board,
    component_id: ComponentId,
    pin_id: PinId,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> Option<(f64, f64, f64, f64)> {
    let component = board.component(component_id)?;
    let part = component.part.as_ref()?;
    let placement = placements.get(&component.id)?;
    let (cx, cy) = (
        snap_grid_127(placement.center_mm.0),
        snap_grid_127(placement.center_mm.1),
    );

    let pin = part.pins.get(pin_id.0 as usize)?;

    if let Some(lib_id) = part.kicad_symbol.as_deref() {
        if let Some(pin_map) = crate::kicad_lib_loader::pin_positions(lib_id) {
            let pin_data = pin_map
                .get(&pin.number.0)
                .or_else(|| pin_map.get(&pin.name.to_lowercase()));
            if let Some(&(px, py, pin_angle)) = pin_data {
                let natural_offset = natural_rotation_offset(part);
                let logical_deg = match placement.rotation {
                    Rotation::Zero => 0.0,
                    Rotation::Ninety => 90.0,
                    Rotation::OneEighty => 180.0,
                    Rotation::TwoSeventy => 270.0,
                };
                let total_deg = (logical_deg + natural_offset).rem_euclid(360.0);
                let rad = total_deg.to_radians();

                let rot_x = px * rad.cos() - py * rad.sin();
                let rot_y = px * rad.sin() + py * rad.cos();

                let term_x = cx + rot_x;
                let term_y = cy - rot_y;

                // KiCad's pin `angle` points INWARD (terminal →
                // body); the outward stub direction is angle+180°.
                // The loader frame is y-up while the sheet is
                // y-down (`term_y` negates), which flips the sign
                // of the y component: outward = (-cos θ, +sin θ).
                let dir_deg = (pin_angle + total_deg).rem_euclid(360.0);
                let dir_rad = dir_deg.to_radians();
                let dx = -dir_rad.cos().round();
                let dy = dir_rad.sin().round();

                return Some((snap_grid_127(term_x), snap_grid_127(term_y), dx, dy));
            }
        }
    }

    endpoint_xy(board, component_id, pin_id, placements)
}

/// Synthetic-body fallback for [`pin_terminal_xy`] — computes a pin's
/// terminal from the rectangular body we draw for parts without a
/// stock `kicad_symbol` mapping.
#[allow(clippy::too_many_lines)]
fn endpoint_xy(
    board: &Board,
    component_id: ComponentId,
    pin_id: PinId,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> Option<(f64, f64, f64, f64)> {
    let component = board.component(component_id)?;
    let part = component.part.as_ref()?;
    let placement = placements.get(&component.id)?;
    let (cx, cy) = (
        snap_grid_127(placement.center_mm.0),
        snap_grid_127(placement.center_mm.1),
    );
    let pin_count = part.pins.len();
    let pin_idx = pin_id.0 as usize;

    if pin_count == 2 && is_two_pin_symbol_kind(&part.kind) {
        let reversed = matches!(placement.rotation, Rotation::OneEighty);
        let clockwise = matches!(placement.rotation, Rotation::Ninety);
        let counter_clockwise = matches!(placement.rotation, Rotation::TwoSeventy);

        if clockwise || counter_clockwise {
            let pin0_on_top = !clockwise;
            let top_y = cy - TWOPIN_HALF_W - PIN_LENGTH;
            let bottom_y = cy + TWOPIN_HALF_W + PIN_LENGTH;
            if (pin_idx == 0) == pin0_on_top {
                Some((cx, top_y, 0.0, -1.0))
            } else {
                Some((cx, bottom_y, 0.0, 1.0))
            }
        } else {
            let pin0_on_left = !reversed;
            if (pin_idx == 0) == pin0_on_left {
                Some((cx - TWOPIN_HALF_W - PIN_LENGTH, cy, -1.0, 0.0))
            } else {
                Some((cx + TWOPIN_HALF_W + PIN_LENGTH, cy, 1.0, 0.0))
            }
        }
    } else {
        let sides: Vec<PinSide> = part.pins.iter().map(classify_ic_pin).collect();
        let top_n = sides.iter().filter(|s| **s == PinSide::Top).count();
        let bottom_n = sides.iter().filter(|s| **s == PinSide::Bottom).count();
        let left_n = sides.iter().filter(|s| **s == PinSide::Left).count();
        let right_n = sides.iter().filter(|s| **s == PinSide::Right).count();

        let horiz_max = top_n.max(bottom_n).max(2);
        let vert_max = left_n.max(right_n).max(2);
        let body_w =
            ((horiz_max as f64) * PIN_PITCH + 2.0 * BODY_PIN_PADDING).max(BODY_HALF_WIDTH * 2.0);
        let body_h = ((vert_max as f64) * PIN_PITCH + 2.0 * BODY_PIN_PADDING).max(MIN_BODY_HEIGHT);
        let bx = cx - body_w / 2.0;
        let by = cy - body_h / 2.0;

        let mut top_idx = 0_usize;
        let mut bottom_idx = 0_usize;
        let mut left_idx = 0_usize;
        let mut right_idx = 0_usize;

        let corner_pad = if left_n > 0 || right_n > 0 {
            PIN_PITCH * 2.0
        } else {
            BODY_PIN_PADDING
        };
        for (i, &side) in sides.iter().enumerate() {
            let px;
            let py;
            let dx;
            let dy;
            match side {
                PinSide::Top => {
                    px = bx + corner_pad + PIN_PITCH * (top_idx as f64);
                    py = by - PIN_LENGTH;
                    dx = 0.0;
                    dy = -1.0;
                    top_idx += 1;
                }
                PinSide::Bottom => {
                    px = bx + corner_pad + PIN_PITCH * (bottom_idx as f64);
                    py = by + body_h + PIN_LENGTH;
                    dx = 0.0;
                    dy = 1.0;
                    bottom_idx += 1;
                }
                PinSide::Left => {
                    px = bx - PIN_LENGTH;
                    py = by + BODY_PIN_PADDING + PIN_PITCH * (left_idx as f64);
                    dx = -1.0;
                    dy = 0.0;
                    left_idx += 1;
                }
                PinSide::Right => {
                    px = bx + body_w + PIN_LENGTH;
                    py = by + BODY_PIN_PADDING + PIN_PITCH * (right_idx as f64);
                    dx = 1.0;
                    dy = 0.0;
                    right_idx += 1;
                }
            }
            if i == pin_idx {
                return Some((snap_grid_127(px), snap_grid_127(py), dx, dy));
            }
        }
        None
    }
}

/// Whether a single orthogonal segment passes through any component
/// body (with a 1 mm safety margin).
fn segment_crosses_component(
    p1: (f64, f64),
    p2: (f64, f64),
    board: &Board,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> bool {
    let x_min = p1.0.min(p2.0);
    let x_max = p1.0.max(p2.0);
    let y_min = p1.1.min(p2.1);
    let y_max = p1.1.max(p2.1);

    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let Some(placement) = placements.get(&component.id) else {
            continue;
        };
        let (cx, cy) = placement.center_mm;

        let (bw, bh) = body_size_for_part(part);
        let bx = cx - bw / 2.0;
        let by = cy - bh / 2.0;

        let margin = 1.0;
        let lo_x = bx - margin;
        let hi_x = bx + bw + margin;
        let lo_y = by - margin;
        let hi_y = by + bh + margin;

        if (p1.1 - p2.1).abs() < 0.01 {
            let y = p1.1;
            if y > lo_y && y < hi_y && x_max > lo_x && x_min < hi_x {
                return true;
            }
        } else if (p1.0 - p2.0).abs() < 0.01 {
            let x = p1.0;
            if x > lo_x && x < hi_x && y_max > lo_y && y_min < hi_y {
                return true;
            }
        }
    }
    false
}

fn path_crosses_component(
    pts: &[(f64, f64)],
    board: &Board,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> bool {
    for i in 0..pts.len().saturating_sub(1) {
        let p1 = pts[i];
        let p2 = pts[i + 1];
        if segment_crosses_component(p1, p2, board, placements) {
            return true;
        }
    }
    false
}

/// Whether `point` lies on the axis-aligned segment `(s1, s2)`,
/// allowing a small tolerance for grid-snapping float noise. The
/// segment is assumed orthogonal (both endpoints share one
/// coordinate), matching the invariant on `WirePath.points`.
fn point_on_ortho_segment(point: (f64, f64), s1: (f64, f64), s2: (f64, f64)) -> bool {
    let tol = 0.1;
    let (px, py) = point;
    let (x1, y1) = s1;
    let (x2, y2) = s2;
    let (lo_x, hi_x) = (x1.min(x2), x1.max(x2));
    let (lo_y, hi_y) = (y1.min(y2), y1.max(y2));
    if (y1 - y2).abs() < COORD_EPSILON_MM {
        // horizontal
        (py - y1).abs() <= tol && px >= lo_x - tol && px <= hi_x + tol
    } else if (x1 - x2).abs() < COORD_EPSILON_MM {
        // vertical
        (px - x1).abs() <= tol && py >= lo_y - tol && py <= hi_y + tol
    } else {
        false
    }
}

/// Whether the pin terminal of `(component_id, pin_id)` is an endpoint
/// of `net_id` — i.e. the net is *connected to* that pin, so a wire of
/// that net is allowed to land on it.
fn net_connects_to_pin(
    board: &Board,
    net_id: NetId,
    component_id: ComponentId,
    pin_id: PinId,
) -> bool {
    board.net(net_id).is_some_and(|net| {
        net.endpoints
            .iter()
            .any(|e| e.component == component_id && e.pin == pin_id)
    })
}

/// Whether the single orthogonal segment `(p1, p2)` passes through the
/// pin terminal of a component/pin that `net_id` is **not** connected
/// to (§7.7 gate — wire-through-unrelated-pin DRC). A wire may land on
/// its own net's terminals (that's where it starts and ends) but must
/// never pass over an unrelated component's pin tip.
fn segment_passes_over_unrelated_pin(
    p1: (f64, f64),
    p2: (f64, f64),
    net_id: NetId,
    board: &Board,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> bool {
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        for (idx, _pin) in part.pins.iter().enumerate() {
            let pin_id = PinId(idx as u32);
            if net_connects_to_pin(board, net_id, component.id, pin_id) {
                continue; // this wire may touch its own net's pins
            }
            let Some((tx, ty, _dx, _dy)) = pin_terminal_xy(board, component.id, pin_id, placements)
            else {
                continue;
            };
            if point_on_ortho_segment((tx, ty), p1, p2) {
                return true;
            }
        }
    }
    false
}

/// Whether any segment of `pts` passes through an unrelated pin
/// terminal — the path-level wrapper for
/// [`segment_passes_over_unrelated_pin`].
fn path_passes_over_unrelated_pin(
    pts: &[(f64, f64)],
    net_id: NetId,
    board: &Board,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> bool {
    for i in 0..pts.len().saturating_sub(1) {
        if segment_passes_over_unrelated_pin(pts[i], pts[i + 1], net_id, board, placements) {
            return true;
        }
    }
    false
}

/// A wire-through-unrelated-pin DRC violation (§7.7 gate): a routed
/// wire of `net` passes over the pin terminal of a component/pin the
/// net does **not** connect to.
#[derive(Debug, Clone, PartialEq)]
pub struct PinTerminalCrossing {
    /// The net whose routed wire passes over an unrelated terminal.
    pub net: NetId,
    /// Refdes of the component whose pin terminal is crossed.
    pub refdes: String,
    /// The crossed pin's name (falls back to its pin number).
    pub pin_name: String,
    /// The wire segment (mm) that passes over the terminal.
    pub segment: ((f64, f64), (f64, f64)),
    /// The pin terminal coordinate crossed (mm).
    pub terminal: (f64, f64),
}

/// Scan every routed wire on `layout` against every component pin
/// terminal and report the wires that pass over a terminal belonging
/// to a pin their net is **not** connected to (§7.7 gate,
/// `test_no_wire_crosses_unrelated_pin_terminals`).
///
/// A wire is allowed to land on its own net's endpoints (that's where
/// it begins and ends), so those terminals are excluded; every other
/// terminal the wire runs over is reported as a violation. Returns an
/// empty vec when the sheet is clean.
pub fn drc_wire_crosses_unrelated_pin_terminal(
    board: &Board,
    layout: &Layout,
) -> Vec<PinTerminalCrossing> {
    let placements: HashMap<ComponentId, &ComponentPlacement> =
        layout.components.iter().map(|p| (p.id, p)).collect();

    // Precompute every pin terminal once: (component_id, pin_id) ->
    // (x, y). Pin terminals never move during routing, so this is the
    // full set of coordinates a wire must not cross.
    let mut terminals: Vec<(ComponentId, PinId, String, (f64, f64))> = Vec::new();
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        for (idx, pin) in part.pins.iter().enumerate() {
            let pin_id = PinId(idx as u32);
            let name = if pin.name.is_empty() {
                pin.number.0.clone()
            } else {
                pin.name.clone()
            };
            if let Some((x, y, _dx, _dy)) =
                pin_terminal_xy(board, component.id, pin_id, &placements)
            {
                terminals.push((component.id, pin_id, name, (x, y)));
            }
        }
    }

    let mut violations = Vec::new();
    for wire in &layout.wires {
        for pair in wire.points.windows(2) {
            let (p1, p2) = (pair[0], pair[1]);
            for (cid, pid, name, terminal) in &terminals {
                if net_connects_to_pin(board, wire.net, *cid, *pid) {
                    continue; // wire is allowed to touch its own net's pin
                }
                if point_on_ortho_segment(*terminal, p1, p2) {
                    let refdes = board
                        .component(*cid)
                        .map_or_else(|| format!("?{}", cid.0), |c| c.refdes.clone());
                    violations.push(PinTerminalCrossing {
                        net: wire.net,
                        refdes,
                        pin_name: name.clone(),
                        segment: (p1, p2),
                        terminal: *terminal,
                    });
                }
            }
        }
    }
    violations
}

/// L-shaped route candidates between two pin terminals `(x, y, dx, dy)`,
/// with the pin stubs and a set of detour offsets. Returns the first
/// candidate that doesn't cross any component body *and* doesn't run
/// collinear with an already-routed different net's wire (checked
/// against `grid`, since `SchematicGrid::find_path`'s A* search
/// refuses that but this fallback path otherwise wouldn't) *and*
/// doesn't pass over an unrelated component's pin terminal (§7.7 gate);
/// if no candidate clears both hazards, falls back to the first that
/// at least avoids crossing a component body and any unrelated pin
/// terminal; if every candidate crosses a component body, returns the
/// last candidate (caller decides whether that's acceptable).
#[allow(clippy::too_many_lines)]
fn l_route_points(
    a: (f64, f64, f64, f64),
    b: (f64, f64, f64, f64),
    stub_len: f64,
    board: &Board,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    grid: &SchematicGrid,
    net_id: NetId,
) -> Vec<(f64, f64)> {
    let (ax, ay, adx, ady) = a;
    let (bx, by, bdx, bdy) = b;
    let a_stub = (ax + adx * stub_len, ay + ady * stub_len);
    let b_stub = (bx + bdx * stub_len, by + bdy * stub_len);
    let a_horiz = adx.abs() > 0.5;
    let b_horiz = bdx.abs() > 0.5;

    let mut candidate_paths = Vec::new();

    if a_horiz && b_horiz {
        let opposite = adx * bdx < 0.0;
        if (a_stub.1 - b_stub.1).abs() < 0.01 {
            candidate_paths.push(vec![(ax, ay), (bx, by)]);
        } else if opposite {
            candidate_paths.push(vec![(ax, ay), (a_stub.0, ay), (a_stub.0, by), (bx, by)]);
            candidate_paths.push(vec![(ax, ay), (b_stub.0, ay), (b_stub.0, by), (bx, by)]);
            let mid_x = f64::midpoint(ax, bx);
            candidate_paths.push(vec![(ax, ay), (mid_x, ay), (mid_x, by), (bx, by)]);
        } else {
            candidate_paths.push(vec![
                (ax, ay),
                a_stub,
                (a_stub.0, b_stub.1),
                b_stub,
                (bx, by),
            ]);
            candidate_paths.push(vec![(ax, ay), (b_stub.0, a_stub.1), b_stub, (bx, by)]);
            let mid_x = if adx > 0.0 {
                a_stub.0.max(b_stub.0)
            } else {
                a_stub.0.min(b_stub.0)
            };
            candidate_paths.push(vec![
                (ax, ay),
                a_stub,
                (mid_x, a_stub.1),
                (mid_x, b_stub.1),
                b_stub,
                (bx, by),
            ]);
        }
    } else if !a_horiz && !b_horiz {
        let opposite = ady * bdy < 0.0;
        if (a_stub.0 - b_stub.0).abs() < 0.01 {
            candidate_paths.push(vec![(ax, ay), (bx, by)]);
        } else if opposite {
            candidate_paths.push(vec![(ax, ay), (ax, a_stub.1), (bx, a_stub.1), (bx, by)]);
            candidate_paths.push(vec![(ax, ay), (ax, b_stub.1), (bx, b_stub.1), (bx, by)]);
            let mid_y = f64::midpoint(ay, by);
            candidate_paths.push(vec![(ax, ay), (ax, mid_y), (bx, mid_y), (bx, by)]);
        } else {
            candidate_paths.push(vec![
                (ax, ay),
                a_stub,
                (b_stub.0, a_stub.1),
                b_stub,
                (bx, by),
            ]);
            candidate_paths.push(vec![(ax, ay), (a_stub.0, b_stub.1), b_stub, (bx, by)]);
            let mid_y = if ady > 0.0 {
                a_stub.1.max(b_stub.1)
            } else {
                a_stub.1.min(b_stub.1)
            };
            candidate_paths.push(vec![
                (ax, ay),
                a_stub,
                (a_stub.0, mid_y),
                (b_stub.0, mid_y),
                b_stub,
                (bx, by),
            ]);
        }
    } else if a_horiz {
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (a_stub.0, b_stub.1),
            b_stub,
            (bx, by),
        ]);
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (b_stub.0, a_stub.1),
            b_stub,
            (bx, by),
        ]);
    } else {
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (b_stub.0, a_stub.1),
            b_stub,
            (bx, by),
        ]);
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (a_stub.0, b_stub.1),
            b_stub,
            (bx, by),
        ]);
    }

    // Detours! If any standard candidate has a collision, detours will be evaluated.
    for offset in &[15.24, 25.4] {
        let detour_y = a_stub.1.min(b_stub.1) - offset;
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (a_stub.0, detour_y),
            (b_stub.0, detour_y),
            b_stub,
            (bx, by),
        ]);
        let down_y = a_stub.1.max(b_stub.1) + offset;
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (a_stub.0, down_y),
            (b_stub.0, down_y),
            b_stub,
            (bx, by),
        ]);
        let lft_x = a_stub.0.min(b_stub.0) - offset;
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (lft_x, a_stub.1),
            (lft_x, b_stub.1),
            b_stub,
            (bx, by),
        ]);
        let rgt_x = a_stub.0.max(b_stub.0) + offset;
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (rgt_x, a_stub.1),
            (rgt_x, b_stub.1),
            b_stub,
            (bx, by),
        ]);
    }

    let cleaned_candidates: Vec<Vec<(f64, f64)>> = candidate_paths
        .iter()
        .map(|path| {
            let mut clean_path: Vec<(f64, f64)> = Vec::new();
            for &p in path {
                let snapped = (snap_grid_127(p.0), snap_grid_127(p.1));
                if let Some(last) = clean_path.last() {
                    if (snapped.0 - last.0).abs() < 0.01 && (snapped.1 - last.1).abs() < 0.01 {
                        continue;
                    }
                }
                clean_path.push(snapped);
            }
            clean_path
        })
        .collect();

    // Pass 1: a candidate clearing both hazards (component bodies and
    // other nets' already-routed wires).
    let mut selected_pts = cleaned_candidates
        .iter()
        .find(|clean_path| {
            !path_crosses_component(clean_path, board, placements)
                && !grid.path_overlaps_other_net_wire(clean_path, net_id)
                && !path_passes_over_unrelated_pin(clean_path, net_id, board, placements)
        })
        .cloned();

    // Pass 2: fall back to a candidate that at least clears component
    // bodies, same as the original (pre-wire-overlap-check) behavior —
    // a wire overlap is visually worse than a crossing (crossings are
    // an accepted, costed case; a dead-on overlap reads as one wire),
    // but neither is as bad as drawing straight through a symbol.
    if selected_pts.is_none() {
        selected_pts = cleaned_candidates
            .iter()
            .find(|clean_path| {
                !path_crosses_component(clean_path, board, placements)
                    && !path_passes_over_unrelated_pin(clean_path, net_id, board, placements)
            })
            .cloned();
    }

    let pts = selected_pts.unwrap_or_else(|| {
        let mut clean_path: Vec<(f64, f64)> = Vec::new();
        let default_path = vec![(ax, ay), (bx, by)];
        let path = candidate_paths.last().unwrap_or(&default_path);
        for &p in path {
            let snapped = (snap_grid_127(p.0), snap_grid_127(p.1));
            if let Some(last) = clean_path.last() {
                if (snapped.0 - last.0).abs() < 0.01 && (snapped.1 - last.1).abs() < 0.01 {
                    continue;
                }
            }
            clean_path.push(snapped);
        }
        clean_path
    });

    pts
}

// ----- Hashimoto-Stevens channel routing (§7.5.6 / §7.7.5) ---------------

/// Minimum horizontal track pitch enforced inside channel strips (mm).
/// Matches the 2.54 mm schematic grid so packed tracks land on grid.
pub const CHANNEL_PITCH_MM: f64 = 2.54;

/// A horizontal channel strip: the open sheet gutter between two
/// adjacent rows of components, spanning `[x_left, x_right]` in x and
/// `[y_top, y_bottom]` in y. Nets traverse it on horizontal track
/// levels spaced [`CHANNEL_PITCH_MM`] apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Channel {
    pub x_left: f64,
    pub x_right: f64,
    pub y_top: f64,
    pub y_bottom: f64,
}

impl Channel {
    /// World y of the `level`-th track inside the channel. Tracks start
    /// one pitch below the top edge and step down by [`CHANNEL_PITCH_MM`]
    /// per level, so consecutive levels are exactly one pitch apart —
    /// this is what *guarantees* minimum track pitch. Snapped to the
    /// 1.27 mm grid so tracks land on the schematic grid.
    pub fn track_y(&self, level: usize) -> f64 {
        snap_grid_127(self.y_top + CHANNEL_PITCH_MM * (level as f64) + CHANNEL_PITCH_MM)
    }

    /// Width of the strip in mm.
    pub fn width(&self) -> f64 {
        self.x_right - self.x_left
    }

    /// Height of the strip in mm.
    pub fn height(&self) -> f64 {
        self.y_bottom - self.y_top
    }
}

/// A net's requested horizontal span across a channel, for left-edge
/// packing. `x1 <= x2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelInterval {
    pub net: NetId,
    pub x1: f64,
    pub x2: f64,
}

/// Result of left-edge packing: a net assigned to a horizontal track
/// level inside a channel. `level` indexes [`Channel::track_y`].
#[derive(Debug, Clone, PartialEq)]
pub struct PackedTrack {
    pub net: NetId,
    pub x1: f64,
    pub x2: f64,
    pub level: usize,
}

/// Hashimoto-Stevens left-edge packing of horizontal intervals into a
/// single channel strip (§7.7.5).
///
/// Intervals are sorted by their left edge and each greedily assigned
/// to the lowest-numbered track whose rightmost previously-placed
/// interval ends before this one begins — the classic left-edge
/// algorithm. It minimizes the number of track levels needed and
/// guarantees **zero line overlap**: two intervals sharing a track
/// never overlap in x. Because [`Channel::track_y`] steps down exactly
/// one [`CHANNEL_PITCH_MM`] per level, consecutive tracks are one pitch
/// apart, guaranteeing **minimum track pitch** in the strip.
///
/// Deterministic: ties are broken by input order, which callers keep
/// stable by iterating `board.nets` in order.
pub fn left_edge_pack(intervals: &[ChannelInterval]) -> Vec<PackedTrack> {
    let mut sorted: Vec<&ChannelInterval> = intervals.iter().collect();
    sorted.sort_by(|a, b| {
        a.x1.partial_cmp(&b.x1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x2.partial_cmp(&b.x2).unwrap_or(std::cmp::Ordering::Equal))
    });

    // tracks[i] = rightmost x (x2) currently occupied on level i.
    let mut tracks: Vec<f64> = Vec::new();
    let mut out = Vec::with_capacity(sorted.len());
    for &iv in &sorted {
        let mut level = None;
        for (i, right) in tracks.iter().enumerate() {
            if *right <= iv.x1 {
                level = Some(i);
                break;
            }
        }
        let level = if let Some(l) = level {
            l
        } else {
            tracks.push(-f64::INFINITY);
            tracks.len() - 1
        };
        tracks[level] = iv.x2;
        out.push(PackedTrack {
            net: iv.net,
            x1: iv.x1,
            x2: iv.x2,
            level,
        });
    }
    out
}

/// Minimum height (mm) a gutter between two component rows must have
/// to be treated as a usable horizontal channel — must fit at least two
/// [`CHANNEL_PITCH_MM`] tracks.
const MIN_CHANNEL_HEIGHT_MM: f64 = 5.08;

/// Decompose the sheet's open space into horizontal channel strips
/// between component rows (§7.7.5 "channel decomposition").
///
/// Each placed component contributes a body y-band; overlapping or
/// near-touching bands are merged, and the gaps between consecutive
/// merged bands wide enough to hold a track become channels spanning
/// the placed sheet's full x-extent. Components are cluster-grid
/// packed, so the gutters between rows are full-width horizontal
/// strips — exactly the strips nets can traverse on packed tracks.
///
/// Deterministic: iterates `board.components` in order and returns
/// channels in top-to-bottom order.
pub fn decompose_channels(board: &Board, layout: &Layout) -> Vec<Channel> {
    let placements: HashMap<ComponentId, &ComponentPlacement> =
        layout.components.iter().map(|p| (p.id, p)).collect();

    let mut bands: Vec<(f64, f64)> = Vec::new();
    let mut x_left = f64::INFINITY;
    let mut x_right = f64::NEG_INFINITY;
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let Some(placement) = placements.get(&component.id) else {
            continue;
        };
        let (cx, cy) = placement.center_mm;
        let (bw, bh) = body_size_for_part(part);
        bands.push((cy - bh / 2.0, cy + bh / 2.0));
        x_left = x_left.min(cx - bw / 2.0);
        x_right = x_right.max(cx + bw / 2.0);
    }
    if bands.is_empty() {
        return Vec::new();
    }
    bands.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Merge overlapping / near-touching bands into rows.
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (top, bot) in bands {
        if let Some(last) = merged.last_mut() {
            if top <= last.1 + MIN_CHANNEL_HEIGHT_MM {
                last.1 = last.1.max(bot);
                continue;
            }
        }
        merged.push((top, bot));
    }

    let mut channels = Vec::new();
    for pair in merged.windows(2) {
        let (_, prev_bot) = pair[0];
        let (next_top, _) = pair[1];
        if next_top - prev_bot >= MIN_CHANNEL_HEIGHT_MM {
            channels.push(Channel {
                x_left: snap_grid_127(x_left),
                x_right: snap_grid_127(x_right),
                y_top: snap_grid_127(prev_bot),
                y_bottom: snap_grid_127(next_top),
            });
        }
    }
    channels
}

/// Pick the horizontal channel that a net's endpoints should traverse,
/// if any. A channel qualifies if it lies strictly between the two
/// extreme endpoint y coordinates (i.e. the pins are on opposite rows
/// with a gutter between); among qualifying channels we prefer the one
/// closest to the midpoint of the two extreme y's. Returns `None` for
/// local (same-row) nets, which stay on the A*/L-route path.
fn pick_net_channel(
    board: &Board,
    net: &synth_ir::Net,
    channels: &[Channel],
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> Option<Channel> {
    if net.endpoints.len() < 2 {
        return None;
    }
    let mut ys = Vec::new();
    for ep in &net.endpoints {
        if let Some((_, y, _, _)) = pin_terminal_xy(board, ep.component, ep.pin, placements) {
            ys.push(y);
        }
    }
    if ys.len() < 2 {
        return None;
    }
    let y_min = ys.iter().copied().fold(f64::INFINITY, f64::min);
    let y_max = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mid = f64::midpoint(y_min, y_max);

    let mut best: Option<Channel> = None;
    let mut best_dist = f64::INFINITY;
    for &ch in channels {
        if ch.y_top >= y_min && ch.y_bottom <= y_max {
            let center = f64::midpoint(ch.y_top, ch.y_bottom);
            let dist = (center - mid).abs();
            if dist < best_dist {
                best_dist = dist;
                best = Some(ch);
            }
        }
    }
    best
}

/// Orthogonal channel route between two pin terminals `(x, y, dx, dy)`
/// through `channel` on `level`: stub out of each pin, drop vertically
/// to the packed track, run horizontally along the track, rise to the
/// other pin's stub, stub in. Returns a connected orthogonal polyline
/// in mm (deduplicated; the track is at [`Channel::track_y`]).
fn channel_route_points(
    a: (f64, f64, f64, f64),
    b: (f64, f64, f64, f64),
    channel: Channel,
    level: usize,
    stub_len: f64,
) -> Vec<(f64, f64)> {
    let track_y = channel.track_y(level);
    let (ax, ay, adx, ady) = a;
    let (bx, by, bdx, bdy) = b;
    let a_stub = (ax + adx * stub_len, ay + ady * stub_len);
    let b_stub = (bx + bdx * stub_len, by + bdy * stub_len);

    let raw = vec![
        (ax, ay),
        a_stub,
        (a_stub.0, track_y),
        (b_stub.0, track_y),
        b_stub,
        (bx, by),
    ];
    // Deduplicate consecutive identical points (zero-length hops) and
    // snap to the 1.27 mm grid.
    let mut pts: Vec<(f64, f64)> = Vec::with_capacity(raw.len());
    for &(px, py) in &raw {
        let snapped = (snap_grid_127(px), snap_grid_127(py));
        if let Some(last) = pts.last() {
            if (snapped.0 - last.0).abs() < COORD_EPSILON_MM
                && (snapped.1 - last.1).abs() < COORD_EPSILON_MM
            {
                continue;
            }
        }
        pts.push(snapped);
    }
    pts
}

/// Quantized grid point + owning net id → count of same-net wire
/// vertices touching that point, used to detect junctions where ≥ 3
/// *same-net* segments meet.
type JunctionMap = BTreeMap<((i64, i64), NetId), usize>;

/// Route every endpoint of `net` back to the first endpoint as
/// orthogonal wires. Returns `None` when the net must be truncated to
/// per-endpoint net labels instead — either because an endpoint pair
/// has no clean path (A* failed *and* the L-route fallback would cross
/// a component body) or because an endpoint's terminal cannot be
/// resolved at all (a partless component, or a pin index past the end
/// of the part's pin list). The caller then turns the whole net into
/// labels — the same escape hatch a human reaches for when a wire
/// would tangle (implementation plan §7.7.3).
///
/// The every-net-terminates invariant (the contract future router
/// passes must keep): after [`route_board`] returns, every signal net
/// carries ≥ 1 wire **or** sits in
/// `RouteResult::nets_truncated_to_labels` (which its caller expands
/// into ≥ 2 labels), unless it was already handled as a power or
/// labeled net. A net that ends up neither routed nor labeled has
/// silently vanished from the schematic — so this function never
/// emits partial wires for a net it cannot fully resolve, and
/// [`route_board`] debug-asserts the invariant over its final output.
///
/// Returns one [`WirePath`] per root-to-endpoint route (`points` is a
/// single connected orthogonal polyline). Multi-endpoint nets produce
/// multiple `WirePath`s sharing the root point.
///
/// Junction dots are *not* computed here: they are derived once from
/// the final, post-cleanup wire set in [`route_board`] (see
/// [`junctions_from_wires`]). Deriving them from the surviving wires
/// means truncated or re-routed nets can never leave a ghost dot at a
/// point where their wires no longer run.
fn build_wires_for_net(
    board: &Board,
    net: &synth_ir::Net,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    grid: &mut SchematicGrid,
    channel_assignment: &HashMap<NetId, (Channel, usize)>,
) -> Option<Vec<WirePath>> {
    let mut out = Vec::new();
    if net.endpoints.len() < 2 {
        return Some(out);
    }
    // Resolve every endpoint terminal before emitting any wire: a
    // single unresolvable terminal must truncate the whole net to
    // labels. Emitting around it would strand that endpoint with
    // neither a wire nor a label — silently when it is the root
    // (empty vec counted as "routed") or partially for non-root
    // endpoints (`continue` past the missing leg). Labels need no
    // coordinates, so the escape hatch always applies.
    let mut terminals = Vec::with_capacity(net.endpoints.len());
    for endpoint in &net.endpoints {
        let xy_data = pin_terminal_xy(board, endpoint.component, endpoint.pin, placements)?;
        terminals.push(xy_data);
    }
    let root_xy_data = terminals[0];
    let root_xy = (root_xy_data.0, root_xy_data.1);

    for (_endpoint, &(ox, oy, odx, ody)) in net.endpoints.iter().skip(1).zip(&terminals[1..]) {
        let other_xy_data = (ox, oy, odx, ody);
        let other_xy = (other_xy_data.0, other_xy_data.1);

        // Pin-aware L-route fallback, shared by the A*-rejected path
        // and the no-path case: returns `None` when the best candidate
        // still crosses a component body, overlaps another net's wire,
        // or passes over an unrelated pin terminal — the caller then
        // truncates the net to labels (§7.7 escape hatch).
        let try_fallback = |grid: &mut SchematicGrid| {
            let fallback = l_route_points(
                root_xy_data,
                other_xy_data,
                2.54,
                board,
                placements,
                grid,
                net.id,
            );
            if path_crosses_component(&fallback, board, placements)
                || grid.path_overlaps_other_net_wire(&fallback, net.id)
                || path_passes_over_unrelated_pin(&fallback, net.id, board, placements)
            {
                None
            } else {
                Some(fallback)
            }
        };

        // Try the Hashimoto-Stevens channel router first (§7.5.6 /
        // §7.7.5): if this net was assigned a packed track level in a
        // horizontal channel strip, build the stub-out → track →
        // stub-in polyline and use it when it's clean (no body
        // crossing, no other-net overlap, no unrelated pin terminal).
        // Otherwise fall through to the A* grid router for local
        // connections, and only then the L-route fallback.
        let channel_candidate = channel_assignment
            .get(&net.id)
            .copied()
            .map(|(channel, level)| {
                channel_route_points(root_xy_data, other_xy_data, channel, level, 2.54)
            });
        let channel_ok = channel_candidate.as_deref().is_some_and(|pts| {
            !path_crosses_component(pts, board, placements)
                && !grid.path_overlaps_other_net_wire(pts, net.id)
                && !path_passes_over_unrelated_pin(pts, net.id, board, placements)
        });

        let pts = if channel_ok {
            channel_candidate.expect("channel_ok implies a candidate")
        } else if let Some(pts) = grid.find_path(root_xy, other_xy, net.id) {
            if path_passes_over_unrelated_pin(&pts, net.id, board, placements) {
                // The A* path crossed an unrelated pin terminal (it is
                // not pin-aware). Re-route via the pin-aware L-route
                // fallback; if that also can't clear the terminal,
                // truncate this net to labels.
                try_fallback(grid)?
            } else {
                pts
            }
        } else {
            try_fallback(grid)?
        };
        let pts = straighten_path(pts, board, placements, grid, net.id);

        for j in 0..pts.len().saturating_sub(1) {
            let p1 = pts[j];
            let p2 = pts[j + 1];

            grid.register_wire_segment(p1, p2, net.id);
        }

        out.push(WirePath {
            net: net.id,
            points: simplify_path(pts),
            junctions: Vec::new(),
        });
    }
    Some(out)
}

/// Drop redundant collinear waypoints from a routed path (turn
/// reduction, §7.8.6 Stage C pass 5). `SchematicGrid::find_path`
/// walks cell-by-cell, so a straight run of any length comes back as
/// many single-step points along the same line — visually that's
/// one wire, but the raw path carries a waypoint per grid step. Drop
/// any point that lies strictly between its neighbours on the same
/// axis; this never changes the route's shape, only how many points
/// describe it, so it's always safe to apply.
fn simplify_path(points: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if points.len() < 3 {
        return points;
    }
    let mut out = Vec::with_capacity(points.len());
    out.push(points[0]);
    for i in 1..points.len() - 1 {
        let (px, py) = out[out.len() - 1];
        let (cx, cy) = points[i];
        let (nx, ny) = points[i + 1];
        let vertical_run = (px - cx).abs() < COORD_EPSILON_MM && (cx - nx).abs() < COORD_EPSILON_MM;
        let horizontal_run =
            (py - cy).abs() < COORD_EPSILON_MM && (cy - ny).abs() < COORD_EPSILON_MM;
        if vertical_run || horizontal_run {
            continue;
        }
        out.push(points[i]);
    }
    out.push(points[points.len() - 1]);
    out
}

/// Orthogonal path straightening — Stage C pass 5 "turn reduction"
/// (§7.8.6). `find_path` produces staircase paths: a diagonal hop
/// across the grid comes back as an alternating H/V zigzag with a
/// turn at every step even after collinear simplification. This pass
/// replaces interior sub-spans of such paths with a single L-bend
/// whenever the replacement is provably clean.
///
/// Greedy sweep over waypoints: from the current point, find the
/// *farthest* later waypoint reachable via one orthogonal bend whose
/// first leg continues the current segment's axis (so pin escape
/// stubs keep their facing direction), and splice it in. Every
/// candidate is validated with the same three checks the router uses
/// everywhere else — no component-body crossing, no other-net wire
/// overlap, no unrelated pin terminal — so the pass can never make a
/// route dirtier than what it replaces; when nothing clean is found
/// the original next waypoint is kept and the sweep moves on.
///
/// Deterministic: pure function of (`pts`, `board`, grid state);
/// ties are resolved by taking the farthest valid target first.
fn straighten_path(
    pts: Vec<(f64, f64)>,
    board: &Board,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    grid: &SchematicGrid,
    net_id: NetId,
) -> Vec<(f64, f64)> {
    if pts.len() < 3 {
        return pts;
    }
    let clean = |path: &[(f64, f64)]| -> bool {
        !path_crosses_component(path, board, placements)
            && !grid.path_overlaps_other_net_wire(path, net_id)
            && !path_passes_over_unrelated_pin(path, net_id, board, placements)
    };

    let last = pts.len() - 1;
    let mut out: Vec<(f64, f64)> = vec![pts[0]];
    let mut i = 0usize;
    while i < last {
        let cur = pts[i];
        // Axis of the current outgoing segment — a spliced L must
        // start along it so pin escape stubs keep their direction.
        let next = pts[i + 1];
        let horizontal_exit = (next.1 - cur.1).abs() <= COORD_EPSILON_MM;

        // Farthest-first scan: prefer the longest shortcut so a single
        // bend replaces as many staircase turns as possible.
        let mut best: Option<(usize, (f64, f64))> = None;
        for j in ((i + 2)..=last).rev() {
            let target = pts[j];
            let corner = (
                if horizontal_exit { target.0 } else { cur.0 },
                if horizontal_exit { cur.1 } else { target.1 },
            );
            // Drop degenerate legs (corner coincides with either end).
            let mut deduped: Vec<(f64, f64)> = Vec::with_capacity(3);
            for &p in [&cur, &corner, &target] {
                if deduped.last().is_some_and(|q: &(f64, f64)| {
                    (p.0 - q.0).abs() < 0.01 && (p.1 - q.1).abs() < 0.01
                }) {
                    continue;
                }
                deduped.push(p);
            }
            if deduped.len() >= 2 && clean(&deduped) {
                best = Some((j, corner));
                break;
            }
        }

        if let Some((j, corner)) = best {
            if out
                .last()
                .is_none_or(|q| (corner.0 - q.0).abs() >= 0.01 || (corner.1 - q.1).abs() >= 0.01)
            {
                out.push(corner);
            }
            out.push(pts[j]);
            i = j;
        } else {
            out.push(next);
            i += 1;
        }
    }

    simplify_path(out)
}

/// The output of routing a board's signals over its [`Layout`].
#[derive(Debug, Clone, PartialEq)]
pub struct RouteResult {
    /// One connected orthogonal polyline per root-to-endpoint route.
    /// Populated onto `Layout.wires` by [`route_board`]'s caller.
    /// Never contains a net listed in `nets_truncated_to_labels`.
    pub wires: Vec<WirePath>,
    /// Global junction dots where ≥3 wire segments meet (mm).
    pub junctions: Vec<(f64, f64)>,
    /// Nets truncated to per-endpoint net labels instead of drawn as
    /// wires, for either of two reasons: the net could not be routed
    /// at all (A* failed AND the L-route fallback would cross a
    /// component body), or it routed but crosses more than
    /// `CROSSING_TRUNCATION_THRESHOLD` other-net wire segments (the
    /// "would tangle the sheet" case — §7.7.3). Caller truncates
    /// every net here to net labels; it does not need to distinguish
    /// which reason applied.
    pub nets_truncated_to_labels: Vec<NetId>,
}

/// Route every signal net on `board` over the already-placed `layout`.
///
/// Replicates, in order: building the obstacle grid from component
/// bodies + text extents + net-label extents + power-flag extents,
/// punching pin-approach corridors, routing every net not in
/// `layout.power_net_ids()` / `layout.labeled_net_ids()`, and
/// collecting junctions and unroutable nets. Pure geometry — no
/// KiCad s-expressions or UUIDs are produced here.
///
/// Output is deterministic.
pub fn route_board(board: &Board, layout: &Layout) -> RouteResult {
    let placements: HashMap<ComponentId, &ComponentPlacement> =
        layout.components.iter().map(|p| (p.id, p)).collect();

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for placement in &layout.components {
        let (cx, cy) = placement.center_mm;
        min_x = min_x.min(cx);
        min_y = min_y.min(cy);
        max_x = max_x.max(cx);
        max_y = max_y.max(cy);
    }
    if min_x > max_x {
        min_x = 0.0;
        max_x = 200.0;
        min_y = 0.0;
        max_y = 200.0;
    }

    let mut grid = SchematicGrid::new(min_x, min_y, max_x, max_y);

    // 1. Mark component body and text label (Reference/Value) obstacles.
    for component in &board.components {
        if let Some(part) = component.part.as_ref() {
            if let Some(placement) = placements.get(&component.id) {
                let (cx, cy) = (
                    snap_grid_127(placement.center_mm.0),
                    snap_grid_127(placement.center_mm.1),
                );
                let (_, bh) = body_size_for_part(part);
                let margin = 0.635;

                // Text-inclusive half-width — the exact sizing
                // placement uses (`crate::text_inclusive_half_width`),
                // so the router's obstacle rect reserves the room the
                // sheet actually renders. Sizing the Value label off
                // `part.id` alone underestimated long custom values
                // and let A*/L routes run straight through the text.
                let half_w = text_inclusive_half_width(component, part);

                // Reference text sits ~3.81 mm above top edge; Value text sits ~3.81 mm below bottom edge
                let text_margin_y = 6.35;

                grid.mark_obstacle_rect(
                    cx - half_w - margin,
                    cy - bh / 2.0 - text_margin_y - margin,
                    cx + half_w + margin,
                    cy + bh / 2.0 + text_margin_y + margin,
                );
            }
        }
    }

    // 2. Mark net label (global_label) obstacles.
    for label in &layout.net_labels {
        if let Some((x, y, dx, _dy)) =
            pin_terminal_xy(board, label.component, label.pin, &placements)
        {
            let stub_len = 5.08;
            let label_x = if dx >= -0.1 {
                x + stub_len
            } else {
                x - stub_len
            };
            let label_w = (label.label.len() as f64) * 1.5 + 3.0;
            let label_h = 3.81;
            grid.mark_obstacle_rect(
                label_x - label_w / 2.0,
                y - label_h / 2.0,
                label_x + label_w / 2.0,
                y + label_h / 2.0,
            );
        }
    }

    // 3. Mark power flag symbol obstacles.
    for flag in &layout.power_flags {
        if let Some((x, y, dx, dy)) = pin_terminal_xy(board, flag.component, flag.pin, &placements)
        {
            let stub_len = 2.54;
            let flag_x = x + dx * stub_len;
            let flag_y = y + dy * stub_len;
            grid.mark_obstacle_rect(flag_x - 2.54, flag_y - 2.54, flag_x + 2.54, flag_y + 2.54);
        }
    }

    // 4. Unmark pin terminals so wire endpoints land cleanly on pin
    // tips — and carve the approach corridor straight out of each
    // pin. A wire can only reach the pin tip through the cell just
    // outside it, but for small 2-pin parts the Reference/Value text
    // margins extend *past* the pin tip, leaving the tip walled in
    // by the part's own obstacle rect (A* then fails and the L-route
    // fallback draws across bodies). Walk outward from each tip,
    // unmarking cells until we clear the part's own obstacle —
    // cluster spacing (~17 mm) keeps the walk inside soft text-
    // margin space, never into another body.
    let corridor_steps = (CORRIDOR_MAX_MM / GRID_STEP).round() as i32;
    for component in &board.components {
        if let Some(part) = component.part.as_ref() {
            for (idx, _pin) in part.pins.iter().enumerate() {
                let pin_id = PinId(idx as u32);
                if let Some((px, py, dx, dy)) =
                    pin_terminal_xy(board, component.id, pin_id, &placements)
                {
                    grid.unmark_pos(px, py);
                    for step in 1..=corridor_steps {
                        let cx = px + dx * GRID_STEP * f64::from(step);
                        let cy = py + dy * GRID_STEP * f64::from(step);
                        // Stop once we've exited the blocked zone —
                        // the cell is already free, the corridor is
                        // open, no need to punch further.
                        if !grid.is_blocked(cx, cy) {
                            break;
                        }
                        grid.unmark_pos(cx, cy);
                    }
                }
            }
        }
    }

    // 5. Route signal wires. Junction dots are derived once from the
    // final, post-cleanup wire set (see `junctions_from_wires`) so a
    // net truncated to labels — or re-routed by the cleanup passes —
    // can never leave a ghost dot at a point where its wires no
    // longer run.
    let power_nets = layout.power_net_ids();
    let labeled_nets = layout.labeled_net_ids();

    // 5a. Hashimoto-Stevens channel routing (§7.5.6 / §7.7.5): decompose
    // the sheet into horizontal channel strips between component rows,
    // assign every signal net that spans rows to the gutter between
    // them, and left-edge pack each channel's intervals onto a minimal
    // set of track levels (guaranteeing 2.54 mm pitch and zero line
    // overlap in the strip). `build_wires_for_net` then prefers the
    // packed channel route for those nets, keeping A* for local
    // connections.
    let channels = decompose_channels(board, layout);
    let mut channel_assignment: HashMap<NetId, (Channel, usize)> = HashMap::new();
    if !channels.is_empty() {
        let mut intervals_by_channel: BTreeMap<u32, Vec<ChannelInterval>> = BTreeMap::new();
        for net in &board.nets {
            if power_nets.contains(&net.id) || labeled_nets.contains(&net.id) {
                continue;
            }
            let Some(channel) = pick_net_channel(board, net, &channels, &placements) else {
                continue;
            };
            let mut xs = Vec::new();
            for ep in &net.endpoints {
                if let Some((x, _, _, _)) =
                    pin_terminal_xy(board, ep.component, ep.pin, &placements)
                {
                    xs.push(x);
                }
            }
            if xs.len() < 2 {
                continue;
            }
            let x1 = xs.iter().copied().fold(f64::INFINITY, f64::min);
            let x2 = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if (x2 - x1) < CHANNEL_PITCH_MM {
                continue; // degenerate span, not worth a track
            }
            let Some(ch_idx) = channels.iter().position(|c| *c == channel) else {
                continue;
            };
            intervals_by_channel
                .entry(ch_idx as u32)
                .or_default()
                .push(ChannelInterval {
                    net: net.id,
                    x1,
                    x2,
                });
        }
        for (ch_idx, intervals) in &intervals_by_channel {
            let channel = channels[*ch_idx as usize];
            for track in left_edge_pack(intervals) {
                channel_assignment.insert(track.net, (channel, track.level));
            }
        }
    }

    let mut wires = Vec::new();
    let mut nets_truncated_to_labels: Vec<NetId> = Vec::new();
    for net in &board.nets {
        // Nets that carry net labels or power flags are connected via
        // symbols/labels; suppressing long-distance wires eliminates
        // ugly redundant wire loops across the page.
        if power_nets.contains(&net.id) || labeled_nets.contains(&net.id) {
            continue;
        }
        match build_wires_for_net(board, net, &placements, &mut grid, &channel_assignment) {
            Some(mut net_wires) => wires.append(&mut net_wires),
            None => nets_truncated_to_labels.push(net.id),
        }
    }

    // Nets that DID route but tangle the sheet — crossing more than
    // `CROSSING_TRUNCATION_THRESHOLD` other-net segments — get the
    // same truncate-to-label treatment a human reaches for rather
    // than leaving a rat's nest of crossings (§7.7.3). A single pass
    // over the crossing counts computed from the full initial wire
    // set: nets are flagged and removed together, not iteratively
    // re-checked against each other's removal, to keep this
    // deterministic and bounded (no risk of oscillation/cascading
    // re-routing decisions).
    let high_crossing_nets = nets_exceeding_crossing_threshold(&wires);
    if !high_crossing_nets.is_empty() {
        let flagged: HashSet<NetId> = high_crossing_nets.iter().copied().collect();
        wires.retain(|w| !flagged.contains(&w.net));
        nets_truncated_to_labels.extend(high_crossing_nets);
    }

    // Every-net-terminates invariant (see `build_wires_for_net`): each
    // signal net must carry at least one wire or be truncated to
    // labels. A net that is neither would vanish from the schematic —
    // no wire AND no label — silently deleting connectivity.
    #[cfg(debug_assertions)]
    {
        let routed_nets: HashSet<NetId> = wires.iter().map(|w| w.net).collect();
        let needs_labels: HashSet<NetId> = nets_truncated_to_labels.iter().copied().collect();
        for net in &board.nets {
            if power_nets.contains(&net.id)
                || labeled_nets.contains(&net.id)
                || net.endpoints.len() < 2
            {
                continue;
            }
            debug_assert!(
                routed_nets.contains(&net.id) || needs_labels.contains(&net.id),
                "net {} ended up neither routed nor truncated to labels",
                net.id.0
            );
        }
    }

    // Stage C pass 6 (§7.8.6): align parallel bus runs between the
    // same component pair onto evenly spaced 2.54 mm rails.
    align_bus_rails(board, &placements, &grid, &mut wires);

    // Junction dots from the final wire set: every grid point where
    // three or more segment endpoints meet. Derived *after* all
    // cleanup passes so moved/removed wires can't leave ghost dots.
    let junctions = junctions_from_wires(&wires);

    RouteResult {
        wires,
        junctions,
        nets_truncated_to_labels,
    }
}

/// Junction dots for a final wire set: every grid point where three
/// or more **same-net** segment vertices meet, as world-space mm
/// coordinates.
///
/// Counting is scoped per net — the [`JunctionMap`] key includes the
/// wire's net id and the per-point fold takes the maximum single-net
/// count, never the sum across nets. Perpendicular crossings between
/// different nets are legal (`grid.rs` permits them), so two unrelated
/// nets merely passing through the same lattice point must NOT read as
/// connected: KiCad renders junction dots as electrical connections,
/// so an unconditional dot at a cross-net coincidence would be a
/// designed-in short between unrelated signals. This scoping can only
/// *remove* dots relative to unscoped counting, never add them.
fn junctions_from_wires(wires: &[WirePath]) -> Vec<(f64, f64)> {
    let mut counts: JunctionMap = BTreeMap::new();
    for wire in wires {
        for pair in wire.points.windows(2) {
            for &p in [&pair[0], &pair[1]] {
                *counts
                    .entry((
                        ((p.0 * 100.0).round() as i64, (p.1 * 100.0).round() as i64),
                        wire.net,
                    ))
                    .or_insert(0) += 1;
            }
        }
    }
    // Fold per-net counts back to one entry per coordinate, keeping
    // the strongest single-net count. Iterating the BTreeMap keeps
    // emission ordered by (x, y) for deterministic output.
    let mut by_point: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    for ((point, _net), count) in counts {
        let slot = by_point.entry(point).or_insert(0);
        *slot = (*slot).max(count);
    }
    by_point
        .into_iter()
        .filter(|(_, count)| *count >= 3)
        .map(|((qx, qy), _)| ((qx as f64) / 100.0, (qy as f64) / 100.0))
        .collect()
}

/// Rail pitch for aligned bus runs (Stage C pass 6). Matches the
/// schematic grid so aligned rails stay on-grid.
const BUS_RAIL_PITCH_MM: f64 = 2.54;

/// A Z-shaped bus run whose middle segment can be re-railed.
struct BusRun {
    /// Index into the `wires` slice being realigned.
    wire_idx: usize,
    /// Index of the waypoint that starts the middle segment (`points[mid]`
    /// to `points[mid + 1]` is the long parallel run).
    mid: usize,
    /// Current coordinate of the middle segment on its rail axis
    /// (x for a vertical run, y for a horizontal run).
    mid_coord: f64,
    /// Sort key ordering rails within the group: the smaller of the
    /// two pin coordinates along the entry/exit axis, so adjacent
    /// pins map to adjacent rails and members never cross.
    sort_key: f64,
}

/// Classify a simplified polyline as a Z-run with exactly one long
/// interior segment parallel to the other runs of its group. Returns
/// `(mid_index, mid_coord)` — the middle segment spans
/// `points[mid]..points[mid+1]` — or `None` for any other shape.
fn z_run_mid_segment(points: &[(f64, f64)]) -> Option<(usize, f64)> {
    if points.len() != 4 {
        return None;
    }
    let vertical_mid = (points[1].0 - points[2].0).abs() < COORD_EPSILON_MM;
    let horizontal_mid = (points[1].1 - points[2].1).abs() < COORD_EPSILON_MM;
    if vertical_mid == horizontal_mid {
        return None; // degenerate or diagonal — not an orthogonal Z
    }
    // The middle segment must actually have length on its axis.
    if vertical_mid && (points[1].1 - points[2].1).abs() < COORD_EPSILON_MM {
        return None;
    }
    if horizontal_mid && (points[1].0 - points[2].0).abs() < COORD_EPSILON_MM {
        return None;
    }
    let coord = if vertical_mid {
        points[1].0
    } else {
        points[1].1
    };
    Some((1, coord))
}

/// Whether two segments belonging to different nets *overlap* — i.e.
/// run collinear with any shared span. Proper perpendicular crossings
/// are allowed inside a bus group (the sheet tolerates crossings
/// generally, §7.5.6), but two nets printed on top of each other read
/// as one wire and are never acceptable.
fn segments_conflict(a1: (f64, f64), a2: (f64, f64), b1: (f64, f64), b2: (f64, f64)) -> bool {
    const EPS: f64 = 1e-6;
    let range_overlap = |lo_a: f64, hi_a: f64, lo_b: f64, hi_b: f64| -> bool {
        lo_a <= hi_b + EPS && lo_b <= hi_a + EPS
    };
    let a_horizontal = (a1.1 - a2.1).abs() < EPS;
    let b_horizontal = (b1.1 - b2.1).abs() < EPS;
    match (a_horizontal, b_horizontal) {
        // Horizontal pair: conflict iff collinear and x-spans overlap.
        (true, true) => {
            (a1.1 - b1.1).abs() < EPS
                && range_overlap(
                    a1.0.min(a2.0),
                    a1.0.max(a2.0),
                    b1.0.min(b2.0),
                    b1.0.max(b2.0),
                )
        }
        // Vertical pair: conflict iff collinear and y-spans overlap.
        (false, false) => {
            (a1.0 - b1.0).abs() < EPS
                && range_overlap(
                    a1.1.min(a2.1),
                    a1.1.max(a2.1),
                    b1.1.min(b2.1),
                    b1.1.max(b2.1),
                )
        }
        // Perpendicular segments can never collinearly overlap.
        _ => false,
    }
}

/// Whether any segment pair across the two polylines conflicts.
fn polylines_conflict(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    for ap in a.windows(2) {
        for bp in b.windows(2) {
            if segments_conflict(ap[0], ap[1], bp[0], bp[1]) {
                return true;
            }
        }
    }
    false
}

/// Stage C pass 6 (§7.8.6): same-net-rail alignment for parallel
/// buses. When several nets connect the *same pair of components*
/// (an SPI or memory bus between an MCU and a flash chip, say), each
/// net's route currently staggers its long parallel run by whatever
/// stub length its own id hashed into. This pass collects such runs,
/// orders them by pin position so adjacent pins map to adjacent rails
/// (the `enriver` ordering discipline from §7.7.9 item 4), and moves
/// each middle segment onto an evenly spaced 2.54 mm rail centred on
/// the group's existing corridor.
///
/// Validation is atomic per group: every adjusted polyline must clear
/// component bodies, other nets' registered wires, unrelated pin
/// terminals, and every other *adjusted* member of the group. If any
/// member fails, the whole group keeps its original routing — the
/// pass improves presentation only, never risks correctness.
fn align_bus_rails(
    board: &Board,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    grid: &SchematicGrid,
    wires: &mut [WirePath],
) {
    // Group wire indices by unordered endpoint component pair; only
    // 2-endpoint nets produce single-Z runs worth realigning.
    let mut groups: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for (idx, w) in wires.iter().enumerate() {
        let Some(net) = board.net(w.net) else {
            continue;
        };
        if net.endpoints.len() != 2 {
            continue;
        }
        let mut ids = [net.endpoints[0].component.0, net.endpoints[1].component.0];
        ids.sort_unstable();
        groups.entry((ids[0], ids[1])).or_default().push(idx);
    }

    for (_pair, idxs) in groups {
        if idxs.len() < 2 {
            continue;
        }
        let mut runs: Vec<BusRun> = Vec::new();
        for &idx in &idxs {
            let Some((mid, coord)) = z_run_mid_segment(&wires[idx].points) else {
                continue;
            };
            let pts = &wires[idx].points;
            let vertical_mid = (pts[mid].0 - pts[mid + 1].0).abs() < COORD_EPSILON_MM;
            // Order rails by the smaller pin coordinate along the
            // entry/exit axis so neighbours stay neighbours.
            let sort_key = if vertical_mid {
                pts[0].1.min(pts[3].1)
            } else {
                pts[0].0.min(pts[3].0)
            };
            runs.push(BusRun {
                wire_idx: idx,
                mid,
                mid_coord: coord,
                sort_key,
            });
        }
        if runs.len() < 2 {
            continue;
        }
        runs.sort_by(|a, b| {
            a.sort_key
                .partial_cmp(&b.sort_key)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| wires[a.wire_idx].net.cmp(&wires[b.wire_idx].net))
        });

        let n = runs.len() as f64;
        let mean = runs.iter().map(|r| r.mid_coord).sum::<f64>() / n;

        // Build the adjusted polyline for every run whose rail moves.
        let mut adjusted: Vec<(usize, Vec<(f64, f64)>)> = Vec::with_capacity(runs.len());
        for (rank, run) in runs.iter().enumerate() {
            let target = ((mean + (rank as f64 - (n - 1.0) / 2.0) * BUS_RAIL_PITCH_MM)
                / BUS_RAIL_PITCH_MM)
                .round()
                * BUS_RAIL_PITCH_MM;
            let delta = target - run.mid_coord;
            if delta.abs() < COORD_EPSILON_MM {
                continue;
            }
            let mut pts = wires[run.wire_idx].points.clone();
            let vertical_mid = (pts[run.mid].0 - pts[run.mid + 1].0).abs() < COORD_EPSILON_MM;
            if vertical_mid {
                pts[run.mid].0 += delta;
                pts[run.mid + 1].0 += delta;
            } else {
                pts[run.mid].1 += delta;
                pts[run.mid + 1].1 += delta;
            }
            adjusted.push((run.wire_idx, pts));
        }
        if adjusted.is_empty() {
            continue;
        }

        let mut pairwise_clean = true;
        for i in 0..adjusted.len() {
            for j in (i + 1)..adjusted.len() {
                if polylines_conflict(&adjusted[i].1, &adjusted[j].1) {
                    pairwise_clean = false;
                }
            }
        }
        let valid = pairwise_clean
            && adjusted.iter().all(|(idx, pts)| {
                !path_crosses_component(pts, board, placements)
                    && !grid.path_overlaps_other_net_wire(pts, wires[*idx].net)
                    && !path_passes_over_unrelated_pin(pts, wires[*idx].net, board, placements)
            });
        if !valid {
            continue;
        }

        for (idx, pts) in adjusted {
            wires[idx].points = simplify_path(pts);
        }
    }
}

/// A net crossing more than this many *other*-net wire segments is
/// truncated to labels instead of drawn (§7.7.3).
const CROSSING_TRUNCATION_THRESHOLD: u32 = 2;

/// A wire segment tagged with the net it belongs to, for crossing
/// detection in [`nets_exceeding_crossing_threshold`].
type TaggedSegment = (NetId, (f64, f64), (f64, f64));

/// Net ids whose routed wires cross more than
/// [`CROSSING_TRUNCATION_THRESHOLD`] segments belonging to other
/// nets. Same-net segment pairs are skipped (a wire's own bends and
/// junctions aren't crossings). Deterministic: iterates `wires` in
/// order and returns ids in ascending numeric order regardless of
/// hash/iteration order.
fn nets_exceeding_crossing_threshold(wires: &[WirePath]) -> Vec<NetId> {
    let mut segments: Vec<TaggedSegment> = Vec::new();
    for wire in wires {
        for pair in wire.points.windows(2) {
            segments.push((wire.net, pair[0], pair[1]));
        }
    }

    let mut crossings_by_net: BTreeMap<u32, u32> = BTreeMap::new();
    for i in 0..segments.len() {
        for j in (i + 1)..segments.len() {
            let (net_a, a1, a2) = segments[i];
            let (net_b, b1, b2) = segments[j];
            if net_a == net_b {
                continue;
            }
            if crate::score::segments_cross(a1, a2, b1, b2) {
                *crossings_by_net.entry(net_a.0).or_insert(0) += 1;
                *crossings_by_net.entry(net_b.0).or_insert(0) += 1;
            }
        }
    }

    crossings_by_net
        .into_iter()
        .filter(|&(_, count)| count > CROSSING_TRUNCATION_THRESHOLD)
        .map(|(net_id, _)| NetId(net_id))
        .collect()
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;

    #[test]
    fn simplify_path_drops_collinear_waypoints_on_straight_runs() {
        // A straight horizontal run stepped one grid cell at a time,
        // the way `SchematicGrid::find_path`'s fallback candidates
        // (`l_route_points`) can produce: every intermediate point is
        // redundant except where the direction actually changes.
        let stepped = vec![
            (0.0, 0.0),
            (1.27, 0.0),
            (2.54, 0.0),
            (3.81, 0.0),
            (3.81, 2.54),
            (3.81, 5.08),
        ];
        let simplified = simplify_path(stepped);
        assert_eq!(
            simplified,
            vec![(0.0, 0.0), (3.81, 0.0), (3.81, 5.08)],
            "collinear intermediate points removed, real corners kept"
        );
    }

    #[test]
    fn simplify_path_leaves_a_genuine_zigzag_untouched() {
        let zigzag = vec![(0.0, 0.0), (2.54, 0.0), (2.54, 2.54), (5.08, 2.54)];
        assert_eq!(simplify_path(zigzag.clone()), zigzag);
    }

    #[test]
    fn simplify_path_handles_short_paths() {
        assert_eq!(simplify_path(vec![]), Vec::<(f64, f64)>::new());
        assert_eq!(simplify_path(vec![(1.0, 1.0)]), vec![(1.0, 1.0)]);
        assert_eq!(
            simplify_path(vec![(0.0, 0.0), (1.0, 1.0)]),
            vec![(0.0, 0.0), (1.0, 1.0)]
        );
    }

    fn wire(net: u32, points: &[(f64, f64)]) -> WirePath {
        WirePath {
            net: NetId(net),
            points: points.to_vec(),
            junctions: Vec::new(),
        }
    }

    #[test]
    fn crossing_threshold_flags_a_net_crossing_three_others() {
        // Net 0 is a long horizontal wire; nets 1..4 each cross it
        // once with a short vertical stub — 3 crossings exceeds the
        // threshold of 2, so net 0 (and each of the 3 crossers) gets
        // flagged.
        let wires = vec![
            wire(0, &[(0.0, 5.0), (40.0, 5.0)]),
            wire(1, &[(10.0, 0.0), (10.0, 10.0)]),
            wire(2, &[(20.0, 0.0), (20.0, 10.0)]),
            wire(3, &[(30.0, 0.0), (30.0, 10.0)]),
        ];
        let flagged = nets_exceeding_crossing_threshold(&wires);
        assert_eq!(
            flagged,
            vec![NetId(0)],
            "only the 3x-crossed net exceeds >2"
        );
    }

    #[test]
    fn crossing_threshold_does_not_flag_two_crossings() {
        let wires = vec![
            wire(0, &[(0.0, 5.0), (40.0, 5.0)]),
            wire(1, &[(10.0, 0.0), (10.0, 10.0)]),
            wire(2, &[(20.0, 0.0), (20.0, 10.0)]),
        ];
        assert!(nets_exceeding_crossing_threshold(&wires).is_empty());
    }

    #[test]
    fn crossing_threshold_ignores_same_net_self_crossings() {
        // A net whose own two disjoint routes happen to cross each
        // other (e.g. two endpoint-pairs of a multi-drop net) must
        // not count against itself.
        let wires = vec![
            wire(0, &[(0.0, 5.0), (40.0, 5.0)]),
            wire(0, &[(20.0, 0.0), (20.0, 10.0)]),
        ];
        assert!(nets_exceeding_crossing_threshold(&wires).is_empty());
    }

    // ---- net-scoped junctions ---------------------------------------------

    #[test]
    fn cross_net_lattice_coincidence_emits_no_junction_dot() {
        // Two L-shaped routes of DIFFERENT nets whose corners land on
        // the same lattice point: unscoped counting summed both
        // corners' vertex pairs (2+2 ≥ 3) and emitted a dot — a
        // designed-in short in KiCad. Net-scoped counting sees only
        // 2 vertices per net and emits nothing.
        let wires = vec![
            wire(0, &[(0.0, 0.0), (2.54, 0.0), (2.54, 2.54)]),
            wire(1, &[(2.54, 2.54), (5.08, 2.54), (5.08, 5.08)]),
        ];
        assert!(
            !junctions_from_wires(&wires).contains(&(2.54, 2.54)),
            "cross-net corner coincidence must not produce a dot"
        );
    }

    #[test]
    fn same_net_t_topology_still_emits_a_junction_dot() {
        // Three legs of ONE net meeting at a point keep their
        // legitimate connection dot.
        let point = (5.08_f64, 5.08_f64);
        let wires = vec![
            wire(7, &[(0.0, 5.08), point]),
            wire(7, &[point, (5.08, 10.16)]),
            wire(7, &[point, (10.16, 5.08)]),
        ];
        assert!(
            junctions_from_wires(&wires).contains(&point),
            "same-net 3-way meeting must keep its dot"
        );
    }

    // ---- straighten_path (Stage C pass 5) --------------------------------

    fn empty_board() -> Board {
        Board {
            name: "test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components: Vec::new(),
            nets: Vec::new(),
            diff_pairs: Vec::new(),
            keepouts: Vec::new(),
            source_span: synth_diagnostics::Span::new(0, 0),
        }
    }

    fn no_placements() -> HashMap<ComponentId, &'static ComponentPlacement> {
        HashMap::new()
    }

    #[test]
    fn straighten_path_collapses_a_staircase_into_a_single_bend() {
        let board = empty_board();
        let placements = no_placements();
        let grid = SchematicGrid::new(-10.0, -10.0, 60.0, 60.0);
        // A* staircase: right-down-right-down. Nothing blocks an L.
        let pts = vec![
            (0.0, 0.0),
            (2.54, 0.0),
            (2.54, 2.54),
            (5.08, 2.54),
            (5.08, 5.08),
            (7.62, 5.08),
        ];
        let out = straighten_path(pts, &board, &placements, &grid, NetId(0));
        assert_eq!(
            out,
            vec![(0.0, 0.0), (7.62, 0.0), (7.62, 5.08)],
            "staircase replaced by one horizontal run + one bend down"
        );
    }

    #[test]
    fn straighten_path_keeps_exit_direction_of_the_first_leg() {
        // First leg exits downward (pin stub pointing down): the
        // spliced L must keep its first leg vertical even when the
        // staircase wanders sideways afterwards.
        let board = empty_board();
        let placements = no_placements();
        let grid = SchematicGrid::new(-10.0, -10.0, 60.0, 60.0);
        let pts = vec![
            (0.0, 0.0),
            (0.0, 2.54),
            (2.54, 2.54),
            (2.54, 5.08),
            (10.16, 5.08),
        ];
        let out = straighten_path(pts, &board, &placements, &grid, NetId(0));
        assert_eq!(out[0], (0.0, 0.0));
        assert_eq!(
            out[1],
            (0.0, 5.08),
            "first leg stays on the pin's exit axis (vertical)"
        );
        assert_eq!(out.last(), Some(&(10.16, 5.08)));
    }

    #[test]
    fn straighten_path_never_changes_endpoints() {
        let board = empty_board();
        let placements = no_placements();
        let grid = SchematicGrid::new(-50.0, -50.0, 90.0, 90.0);
        let zigzag = vec![
            (0.0, 0.0),
            (2.54, 0.0),
            (2.54, 2.54),
            (5.08, 2.54),
            (5.08, 5.08),
            (7.62, 5.08),
            (7.62, 7.62),
            (20.32, 7.62),
        ];
        let out = straighten_path(zigzag.clone(), &board, &placements, &grid, NetId(0));
        assert_eq!(out.first(), zigzag.first());
        assert_eq!(out.last(), zigzag.last());
        assert!(out.len() < zigzag.len(), "zigzag lost at least one turn");
    }

    #[test]
    fn straighten_path_output_is_always_orthogonal() {
        let board = empty_board();
        let placements = no_placements();
        let grid = SchematicGrid::new(-50.0, -50.0, 90.0, 90.0);
        let mut staircase = vec![(0.0, 0.0)];
        for k in 1u32..12 {
            let (px, py) = *staircase.last().unwrap();
            let step = 1.27;
            if k % 2 == 1 {
                staircase.push((px + step, py));
            } else {
                staircase.push((px, py + step));
            }
        }
        let out = straighten_path(staircase, &board, &placements, &grid, NetId(0));
        assert!(out.len() >= 2);
        for pair in out.windows(2) {
            let orthogonal = (pair[0].0 - pair[1].0).abs() < COORD_EPSILON_MM
                || (pair[0].1 - pair[1].1).abs() < COORD_EPSILON_MM;
            assert!(orthogonal, "segment {pair:?} is diagonal");
        }
    }

    #[test]
    fn z_run_mid_segment_classifies_z_shapes_and_rejects_others() {
        // Vertical middle: stub right, run down, stub right.
        assert_eq!(
            z_run_mid_segment(&[(0.0, 0.0), (5.0, 0.0), (5.0, 9.0), (11.0, 9.0)]),
            Some((1, 5.0))
        );
        // Horizontal middle: stub down, run right, stub up.
        assert_eq!(
            z_run_mid_segment(&[(0.0, 0.0), (0.0, 5.0), (9.0, 5.0), (9.0, 11.0)]),
            Some((1, 5.0))
        );
        // Not a Z: L-shape has only three points.
        assert_eq!(
            z_run_mid_segment(&[(0.0, 0.0), (5.0, 0.0), (5.0, 9.0)]),
            None
        );
        // Five-point channel-style route — not a simple Z.
        assert_eq!(
            z_run_mid_segment(&[(0.0, 0.0), (2.0, 0.0), (2.0, 5.0), (8.0, 5.0), (8.0, 9.0)]),
            None
        );
        // Zero-length middle is degenerate, not a rail.
        assert_eq!(
            z_run_mid_segment(&[(0.0, 0.0), (5.0, 3.0), (5.0, 3.0), (9.0, 3.0)]),
            None
        );
    }

    #[test]
    fn segments_conflict_flags_collinear_overlap_only() {
        // Collinear horizontal overlap → conflict.
        assert!(segments_conflict(
            (0.0, 5.0),
            (10.0, 5.0),
            (8.0, 5.0),
            (14.0, 5.0)
        ));
        // Parallel but offset in y → no conflict.
        assert!(!segments_conflict(
            (0.0, 5.0),
            (10.0, 5.0),
            (8.0, 7.54),
            (14.0, 7.54)
        ));
        // Proper perpendicular crossing → allowed inside a group.
        assert!(!segments_conflict(
            (0.0, 5.0),
            (10.0, 5.0),
            (4.0, 0.0),
            (4.0, 9.0)
        ));
        // Collinear vertical overlap → conflict.
        assert!(segments_conflict(
            (3.0, 0.0),
            (3.0, 6.0),
            (3.0, 5.0),
            (3.0, 9.0)
        ));
    }

    // ---- bus rail alignment (Stage C pass 6) ------------------------------

    fn bus_board(n_nets: usize) -> Board {
        let mut board = empty_board();
        for id in [0u32, 1] {
            board.components.push(synth_ir::Component {
                id: ComponentId(id),
                refdes: format!("U{}", id + 1),
                kind: "mcu".to_string(),
                part: None,
                value: None,
                placement_hint: None,
                group: None,
                source_span: synth_diagnostics::Span::new(0, 0),
            });
        }
        for idx in 0..n_nets {
            board.nets.push(synth_ir::Net {
                id: NetId(idx as u32),
                name: format!("bus{idx}"),
                endpoints: vec![
                    synth_ir::NetEndpoint {
                        component: ComponentId(0),
                        pin: synth_ir::PinId(idx as u32),
                        source_span: synth_diagnostics::Span::new(0, 0),
                    },
                    synth_ir::NetEndpoint {
                        component: ComponentId(1),
                        pin: synth_ir::PinId(idx as u32),
                        source_span: synth_diagnostics::Span::new(0, 0),
                    },
                ],
            });
        }
        board
    }

    #[test]
    fn align_bus_rails_spreads_parallel_runs_one_pitch_apart() {
        let board = bus_board(3);
        let placements = no_placements();
        let grid = SchematicGrid::new(0.0, 0.0, 80.0, 80.0);
        // Three Z-runs between the same component pair with scattered
        // middle segments — exactly the §7.8.6 pass 6 case.
        let mut wires = vec![
            wire(0, &[(10.0, 20.0), (30.0, 20.0), (30.0, 25.4), (50.0, 25.4)]),
            wire(1, &[(12.0, 24.0), (26.0, 24.0), (26.0, 29.0), (46.0, 29.0)]),
            wire(2, &[(14.0, 28.0), (22.0, 28.0), (22.0, 33.0), (42.0, 33.0)]),
        ];
        align_bus_rails(&board, &placements, &grid, &mut wires);

        let mid_x = |w: &WirePath| w.points[1].0;
        let rails: Vec<f64> = wires.iter().map(mid_x).collect();
        for pair in rails.windows(2) {
            let pitch = (pair[1] - pair[0]).abs();
            assert!(
                (pitch - BUS_RAIL_PITCH_MM).abs() < 1e-6,
                "rails {rails:?} are not one pitch apart"
            );
        }
    }

    #[test]
    fn align_bus_rails_orders_rails_by_pin_position_not_net_id() {
        let board = bus_board(2);
        let placements = no_placements();
        let grid = SchematicGrid::new(0.0, 0.0, 80.0, 80.0);
        // Net 1 enters BELOW net 0; after alignment its rail must sit
        // below net 0's regardless of net id order.
        let mut wires = vec![
            wire(0, &[(10.0, 40.0), (40.0, 40.0), (40.0, 45.4), (60.0, 45.4)]),
            wire(1, &[(12.0, 50.0), (36.0, 50.0), (36.0, 55.0), (58.0, 55.0)]),
        ];
        align_bus_rails(&board, &placements, &grid, &mut wires);
        assert!(
            wires[1].points[1].0 > wires[0].points[1].0,
            "lower-entering net got the lower rail (mids: {:?} vs {:?})",
            wires[0].points[1],
            wires[1].points[1]
        );
    }

    #[test]
    fn align_bus_rails_ignores_non_z_wires_and_singleton_groups() {
        let board = bus_board(2);
        let placements = no_placements();
        let grid = SchematicGrid::new(0.0, 0.0, 80.0, 80.0);
        let original = vec![
            // Straight two-point wire — no rail to move.
            wire(0, &[(10.0, 20.0), (50.0, 20.0)]),
            // Six-point channel-style route — not a simple Z.
            wire(
                1,
                &[
                    (12.0, 24.0),
                    (16.0, 24.0),
                    (16.0, 27.0),
                    (26.0, 27.0),
                    (26.0, 29.0),
                    (46.0, 29.0),
                ],
            ),
        ];
        let mut wires = original.clone();
        align_bus_rails(&board, &placements, &grid, &mut wires);
        assert_eq!(wires, original, "non-Z shapes are left untouched");
    }
}

#[cfg(test)]
mod channel_router_tests {
    use super::*;
    use crate::SheetSize;
    use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin, PinNumber};

    fn two_pin_part() -> Part {
        Part {
            id: PartId("r_generic".to_string()),
            kind: "resistor".to_string(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: None,
            lcsc_pn: None,
            provenance: None,
            pins: vec![
                Pin {
                    name: "p1".to_string(),
                    number: PinNumber("1".to_string()),
                    electrical_type: ElectricalType::Passive,
                    capabilities: Vec::new(),
                    required: false,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
                Pin {
                    name: "p2".to_string(),
                    number: PinNumber("2".to_string()),
                    electrical_type: ElectricalType::Passive,
                    capabilities: Vec::new(),
                    required: false,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
            ],
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
        }
    }

    fn component_with_part(id: ComponentId, refdes: &str) -> synth_ir::Component {
        synth_ir::Component {
            id,
            refdes: refdes.to_string(),
            kind: "resistor".to_string(),
            part: Some(two_pin_part()),
            value: None,
            placement_hint: None,
            group: None,
            source_span: synth_diagnostics::Span::new(0, 0),
        }
    }

    #[test]
    fn left_edge_pack_assigns_non_overlapping_intervals_to_one_track() {
        // Two intervals that don't overlap in x fit on a single level.
        let intervals = vec![
            ChannelInterval {
                net: NetId(0),
                x1: 0.0,
                x2: 10.0,
            },
            ChannelInterval {
                net: NetId(1),
                x1: 12.0,
                x2: 20.0,
            },
        ];
        let packed = left_edge_pack(&intervals);
        assert_eq!(packed.len(), 2);
        assert_eq!(packed[0].level, 0);
        assert_eq!(packed[1].level, 0, "non-overlapping spans share level 0");
    }

    #[test]
    fn left_edge_pack_uses_separate_levels_for_overlapping_spans() {
        // Overlapping spans can never share a track; level 1 is opened.
        let intervals = vec![
            ChannelInterval {
                net: NetId(0),
                x1: 0.0,
                x2: 15.0,
            },
            ChannelInterval {
                net: NetId(1),
                x1: 5.0,
                x2: 20.0,
            },
            ChannelInterval {
                net: NetId(2),
                x1: 8.0,
                x2: 9.0,
            },
        ];
        let packed = left_edge_pack(&intervals);
        let levels: Vec<usize> = packed.iter().map(|t| t.level).collect();
        assert_eq!(levels, vec![0, 1, 2], "three mutually-overlapping spans");
    }

    #[test]
    fn left_edge_pack_never_overlaps_on_a_shared_track() {
        // Property-ish: for any two packed intervals on the same level,
        // their x-spans must not overlap.
        let intervals: Vec<ChannelInterval> = (0..8_u32)
            .map(|i| ChannelInterval {
                net: NetId(i),
                x1: (i as f64) * 3.0,
                x2: (i as f64) * 3.0 + 4.0,
            })
            .collect();
        let packed = left_edge_pack(&intervals);
        for i in 0..packed.len() {
            for j in (i + 1)..packed.len() {
                if packed[i].level == packed[j].level {
                    let (a_lo, a_hi) = (
                        packed[i].x1.min(packed[i].x2),
                        packed[i].x1.max(packed[i].x2),
                    );
                    let (b_lo, b_hi) = (
                        packed[j].x1.min(packed[j].x2),
                        packed[j].x1.max(packed[j].x2),
                    );
                    assert!(
                        a_hi <= b_lo || b_hi <= a_lo,
                        "same-track intervals must not overlap: {:?} vs {:?}",
                        packed[i],
                        packed[j]
                    );
                }
            }
        }
    }

    #[test]
    fn left_edge_pack_is_deterministic() {
        let intervals = vec![
            ChannelInterval {
                net: NetId(0),
                x1: 2.0,
                x2: 10.0,
            },
            ChannelInterval {
                net: NetId(1),
                x1: 0.0,
                x2: 6.0,
            },
            ChannelInterval {
                net: NetId(2),
                x1: 4.0,
                x2: 8.0,
            },
        ];
        assert_eq!(left_edge_pack(&intervals), left_edge_pack(&intervals));
    }

    #[test]
    fn channel_tracks_are_exactly_one_pitch_apart() {
        // Minimum track pitch guarantee: consecutive levels are spaced
        // CHANNEL_PITCH_MM apart and land on the 1.27 mm grid.
        let channel = Channel {
            x_left: 0.0,
            x_right: 100.0,
            y_top: 0.0,
            y_bottom: 20.0,
        };
        for level in 0..4 {
            let y = channel.track_y(level);
            assert!(
                (y / 1.27).fract().abs() < 1e-9,
                "track {level} must land on the 1.27 mm grid, got {y}"
            );
            if level > 0 {
                assert!(
                    (y - channel.track_y(level - 1)).abs() > (CHANNEL_PITCH_MM - 1e-9),
                    "track {level} must be at least one pitch above {prev}",
                    prev = level - 1
                );
            }
        }
    }

    #[test]
    fn channel_route_points_are_orthogonal_and_reach_both_pins() {
        let channel = Channel {
            x_left: 0.0,
            x_right: 100.0,
            y_top: 0.0,
            y_bottom: 20.0,
        };
        // Pin A on the top edge facing down, pin B on the bottom edge
        // facing up — the classic across-a-channel pair. Coordinates
        // are 1.27 mm grid-aligned so snapping leaves endpoints exact.
        let a = (12.7, 2.54, 0.0, 1.0);
        let b = (76.2, 20.32, 0.0, -1.0);
        let pts = channel_route_points(a, b, channel, 0, 2.54);
        assert!(pts.len() >= 2);
        assert_eq!(pts.first().copied(), Some((12.7, 2.54)));
        assert_eq!(pts.last().copied(), Some((76.2, 20.32)));
        for pair in pts.windows(2) {
            let (x1, y1) = pair[0];
            let (x2, y2) = pair[1];
            assert!(
                (x1 - x2).abs() < 1e-6 || (y1 - y2).abs() < 1e-6,
                "channel route must be orthogonal: {pair:?}"
            );
        }
    }

    #[test]
    fn decompose_channels_splits_distinct_rows_into_strips() {
        // Two clusters far apart in y => one horizontal gutter between
        // them becomes a channel. Build a board + layout by hand.
        let board = Board {
            name: "t".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components: vec![
                component_with_part(ComponentId(0), "R1"),
                component_with_part(ComponentId(1), "R2"),
            ],
            nets: Vec::new(),
            diff_pairs: Vec::new(),
            keepouts: Vec::new(),
            source_span: synth_diagnostics::Span::new(0, 0),
        };
        let layout = Layout {
            components: vec![
                ComponentPlacement {
                    id: ComponentId(0),
                    center_mm: (0.0, 0.0),
                    rotation: Rotation::Zero,
                },
                ComponentPlacement {
                    id: ComponentId(1),
                    center_mm: (0.0, 60.0),
                    rotation: Rotation::Zero,
                },
            ],
            wires: Vec::new(),
            junctions: Vec::new(),
            power_flags: Vec::new(),
            net_labels: Vec::new(),
            annotations: Vec::new(),
            sheet_size: SheetSize::A4,
        };
        let channels = decompose_channels(&board, &layout);
        assert!(!channels.is_empty());
        for ch in &channels {
            assert!(
                ch.height() >= MIN_CHANNEL_HEIGHT_MM,
                "channel must be wide enough to hold a track"
            );
            // Channel sits strictly between the two bodies (y=0 and y=60).
            assert!(ch.y_top >= 0.0 && ch.y_bottom <= 60.0);
        }
    }
}

#[cfg(test)]
mod drc_tests {
    use super::*;
    use crate::SheetSize;
    use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin, PinNumber};

    fn two_pin_part() -> Part {
        Part {
            id: PartId("r_generic".to_string()),
            kind: "resistor".to_string(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: None,
            lcsc_pn: None,
            provenance: None,
            pins: vec![
                Pin {
                    name: "p1".to_string(),
                    number: PinNumber("1".to_string()),
                    electrical_type: ElectricalType::Passive,
                    capabilities: Vec::new(),
                    required: false,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
                Pin {
                    name: "p2".to_string(),
                    number: PinNumber("2".to_string()),
                    electrical_type: ElectricalType::Passive,
                    capabilities: Vec::new(),
                    required: false,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
            ],
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
        }
    }

    fn component_with_part(id: ComponentId, refdes: &str) -> synth_ir::Component {
        synth_ir::Component {
            id,
            refdes: refdes.to_string(),
            kind: "resistor".to_string(),
            part: Some(two_pin_part()),
            value: None,
            placement_hint: None,
            group: None,
            source_span: synth_diagnostics::Span::new(0, 0),
        }
    }

    fn placements_ref(
        placements: &HashMap<ComponentId, ComponentPlacement>,
    ) -> HashMap<ComponentId, &ComponentPlacement> {
        placements.iter().map(|(k, v)| (*k, v)).collect()
    }

    fn empty_layout(placements: &HashMap<ComponentId, ComponentPlacement>) -> Layout {
        Layout {
            components: placements.values().copied().collect(),
            wires: Vec::new(),
            junctions: Vec::new(),
            power_flags: Vec::new(),
            net_labels: Vec::new(),
            annotations: Vec::new(),
            sheet_size: SheetSize::A4,
        }
    }

    #[test]
    fn point_on_ortho_segment_hits_horizontal_and_vertical() {
        assert!(point_on_ortho_segment((5.0, 3.0), (0.0, 3.0), (10.0, 3.0)));
        assert!(point_on_ortho_segment((5.0, 3.0), (5.0, 0.0), (5.0, 6.0)));
        assert!(!point_on_ortho_segment((5.0, 4.0), (0.0, 3.0), (10.0, 3.0)));
        assert!(!point_on_ortho_segment(
            (11.0, 3.0),
            (0.0, 3.0),
            (10.0, 3.0)
        ));
    }

    #[test]
    fn drc_reports_a_wire_over_an_unrelated_pin_terminal_and_exempts_its_own() {
        // Two 2-pin resistors: R1 (net0 on p1) and R2 (net1 on p1).
        // A net0 wire drawn straight through R2's p1 terminal must be
        // flagged; the same geometry through R1's p1 (net0's own pin)
        // must not.
        let board = Board {
            name: "t".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components: vec![
                component_with_part(ComponentId(0), "R1"),
                component_with_part(ComponentId(1), "R2"),
            ],
            nets: vec![
                synth_ir::Net {
                    id: NetId(0),
                    name: "n0".to_string(),
                    endpoints: vec![
                        synth_ir::NetEndpoint {
                            component: ComponentId(0),
                            pin: PinId(0),
                            source_span: synth_diagnostics::Span::new(0, 0),
                        },
                        synth_ir::NetEndpoint {
                            component: ComponentId(1),
                            pin: PinId(0),
                            source_span: synth_diagnostics::Span::new(0, 0),
                        },
                    ],
                },
                synth_ir::Net {
                    id: NetId(1),
                    name: "n1".to_string(),
                    endpoints: vec![synth_ir::NetEndpoint {
                        component: ComponentId(1),
                        pin: PinId(1),
                        source_span: synth_diagnostics::Span::new(0, 0),
                    }],
                },
            ],
            diff_pairs: Vec::new(),
            keepouts: Vec::new(),
            source_span: synth_diagnostics::Span::new(0, 0),
        };
        let placements: HashMap<ComponentId, ComponentPlacement> = [
            (
                ComponentId(0),
                ComponentPlacement {
                    id: ComponentId(0),
                    center_mm: (0.0, 0.0),
                    rotation: Rotation::Zero,
                },
            ),
            (
                ComponentId(1),
                ComponentPlacement {
                    id: ComponentId(1),
                    center_mm: (40.0, 0.0),
                    rotation: Rotation::Zero,
                },
            ),
        ]
        .into_iter()
        .collect();

        // R1's p1 terminal (net0's own endpoint).
        let r1p1 = pin_terminal_xy(
            &board,
            ComponentId(0),
            PinId(0),
            &placements_ref(&placements),
        )
        .expect("R1.p1 terminal");
        // R2's p2 terminal (net1's pin, unrelated to net0).
        let r2p2 = pin_terminal_xy(
            &board,
            ComponentId(1),
            PinId(1),
            &placements_ref(&placements),
        )
        .expect("R2.p2 terminal");

        // (a) A net0 wire passing through R2.p2 => violation.
        let mut layout = empty_layout(&placements);
        layout.wires.push(WirePath {
            net: NetId(0),
            points: vec![(r1p1.0, r1p1.1), (r2p2.0, r2p2.1)],
            junctions: Vec::new(),
        });
        let violations = drc_wire_crosses_unrelated_pin_terminal(&board, &layout);
        assert!(
            violations.iter().any(|v| v.refdes == "R2"),
            "expected a violation over R2, got {violations:?}"
        );

        // (b) A net0 wire passing through R1.p1 (its own pin) => clean.
        let mut layout_own = empty_layout(&placements);
        layout_own.wires.push(WirePath {
            net: NetId(0),
            points: vec![(r1p1.0, r1p1.1), (r1p1.0 - 10.0, r1p1.1)],
            junctions: Vec::new(),
        });
        let violations_own = drc_wire_crosses_unrelated_pin_terminal(&board, &layout_own);
        assert!(
            violations_own.is_empty(),
            "own pin must be exempt: {violations_own:?}"
        );
    }
}

#[cfg(test)]
mod net_termination_tests {
    use super::*;
    use crate::SheetSize;
    use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin, PinNumber};

    fn two_pin_part() -> Part {
        Part {
            id: PartId("r_generic".to_string()),
            kind: "resistor".to_string(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: None,
            lcsc_pn: None,
            provenance: None,
            pins: vec![
                Pin {
                    name: "p1".to_string(),
                    number: PinNumber("1".to_string()),
                    electrical_type: ElectricalType::Passive,
                    capabilities: Vec::new(),
                    required: false,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
                Pin {
                    name: "p2".to_string(),
                    number: PinNumber("2".to_string()),
                    electrical_type: ElectricalType::Passive,
                    capabilities: Vec::new(),
                    required: false,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
            ],
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
        }
    }

    /// Two nearby 2-pin resistors (span under the label threshold, so
    /// `classify_net_labels` leaves the net to the router) plus one
    /// signal net touching both. `bad_on` selects which component's
    /// endpoint references a pin index past the end of its part's
    /// pin list — the silent-drop shape from the finding.
    fn board_with_dangling_pin(bad_on: ComponentId) -> Board {
        let bad_pin = |id| if id == bad_on { PinId(5) } else { PinId(0) };
        Board {
            name: "t".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components: vec![
                synth_ir::Component {
                    id: ComponentId(0),
                    refdes: "R1".to_string(),
                    kind: "resistor".to_string(),
                    part: Some(two_pin_part()),
                    value: None,
                    placement_hint: None,
                    group: None,
                    source_span: synth_diagnostics::Span::new(0, 0),
                },
                synth_ir::Component {
                    id: ComponentId(1),
                    refdes: "R2".to_string(),
                    kind: "resistor".to_string(),
                    part: Some(two_pin_part()),
                    value: None,
                    placement_hint: None,
                    group: None,
                    source_span: synth_diagnostics::Span::new(0, 0),
                },
            ],
            nets: vec![synth_ir::Net {
                id: NetId(0),
                name: "n0".to_string(),
                endpoints: vec![
                    synth_ir::NetEndpoint {
                        component: ComponentId(0),
                        pin: bad_pin(ComponentId(0)),
                        source_span: synth_diagnostics::Span::new(0, 0),
                    },
                    synth_ir::NetEndpoint {
                        component: ComponentId(1),
                        pin: bad_pin(ComponentId(1)),
                        source_span: synth_diagnostics::Span::new(0, 0),
                    },
                ],
            }],
            diff_pairs: Vec::new(),
            keepouts: Vec::new(),
            source_span: synth_diagnostics::Span::new(0, 0),
        }
    }

    fn nearby_layout() -> Layout {
        Layout {
            components: vec![
                ComponentPlacement {
                    id: ComponentId(0),
                    center_mm: (50.8, 50.8),
                    rotation: Rotation::Zero,
                },
                ComponentPlacement {
                    id: ComponentId(1),
                    center_mm: (76.2, 50.8),
                    rotation: Rotation::Zero,
                },
            ],
            wires: Vec::new(),
            junctions: Vec::new(),
            power_flags: Vec::new(),
            net_labels: Vec::new(),
            annotations: Vec::new(),
            sheet_size: SheetSize::A4,
        }
    }

    #[test]
    fn unresolvable_non_root_terminal_truncates_net_to_labels_not_partial_wires() {
        // R2's endpoint names pin 5 of a 2-pin part: the old
        // `continue` path drew a partial wire to R1 and left R2 with
        // neither wire nor label. Now the whole net falls back to
        // per-endpoint labels.
        let board = board_with_dangling_pin(ComponentId(1));
        let mut layout = nearby_layout();
        crate::route_and_label(&board, &mut layout);
        assert!(
            layout.wires.iter().all(|w| w.net != NetId(0)),
            "a net with an unresolvable terminal must produce zero wires"
        );
        let labels = layout
            .net_labels
            .iter()
            .filter(|l| l.net == NetId(0))
            .count();
        assert!(labels >= 2, "expected ≥2 escape-hatch labels, got {labels}");
    }

    #[test]
    fn unresolvable_root_terminal_truncates_net_to_labels_not_silent_drop() {
        // R1 is the root endpoint: the old code returned an empty vec
        // and counted the net as routed — no wire AND no label.
        let board = board_with_dangling_pin(ComponentId(0));
        let mut layout = nearby_layout();
        crate::route_and_label(&board, &mut layout);
        assert!(
            layout.wires.iter().all(|w| w.net != NetId(0)),
            "a net with an unresolvable root terminal must produce zero wires"
        );
        let labels = layout
            .net_labels
            .iter()
            .filter(|l| l.net == NetId(0))
            .count();
        assert!(labels >= 2, "expected ≥2 escape-hatch labels, got {labels}");
    }

    #[test]
    fn fully_resolvable_net_still_routes_to_wires() {
        // Control: the same board with all terminals resolvable keeps
        // producing wires — the truncation must only trigger on the
        // broken shapes above.
        let mut board = board_with_dangling_pin(ComponentId(1));
        for ep in &mut board.nets[0].endpoints {
            ep.pin = PinId(0);
        }
        let mut layout = nearby_layout();
        crate::route_and_label(&board, &mut layout);
        assert!(
            layout.wires.iter().any(|w| w.net == NetId(0)),
            "healthy net must still route"
        );
    }
}
