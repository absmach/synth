// SPDX-License-Identifier: Apache-2.0

//! Keeps net label flags off each other and off other parts.
//!
//! A flag hangs off a stub at its pin and runs away from it. Left at the
//! default spot, the flags of two pins whose stubs point at each other, or of
//! a pull-up that stands in front of an IC's flag column, are drawn box over
//! box, and a flag runs across another part's pin numbers. Each label, in
//! order, takes the spot among [`LabelSpot`]'s moves, the other side of a pin
//! above or below its part or a rise to another row, where it covers the least
//! of the flags and parts already placed.
//!
//! A wire that touches another wire or pin joins their nets in KiCad, so a spot
//! where one would is never taken, and wires get no tolerance. Two flags may
//! touch: flags on a 2.54 mm pin pitch are 0.11 mm taller than the pitch.
//!
//! Only [`Layout::net_labels`] are planned. A cross-sheet stub is a
//! hierarchical label made when a sheet is split, after this pass, and it keeps
//! the default spot.

use std::collections::{HashMap, HashSet};

use synth_ir::{Board, ComponentId, PinId};

use crate::route::{
    FLAG_CHAR_MM, FLAG_H_MM, FLAG_PAD_MM, LABEL_STUB_MM, LabelPlan, net_label_plan, pin_terminal_xy,
};
use crate::{ComponentPlacement, LabelSpot, Layout, NetLabel, Rotation, body_size_for_part};

type Rect = (f64, f64, f64, f64);

/// Half the width of a drawn wire, in mm.
const WIRE_HALF_MM: f64 = 0.15;
/// Flags that overlap by no more than this only touch.
const FLAG_TOUCH_MM: f64 = 0.2;
/// Room a pin's number takes beyond its terminal, in mm.
const PIN_NUMBER_MM: f64 = 1.27;
/// Rises tried, in grid steps. One step puts a flag on its neighbour's row.
const LIFTS: [i8; 5] = [0, 2, -2, 4, -4];

/// The boxes a label covers: its flag and the wire that reaches it.
struct Pieces {
    flag: Rect,
    wire: Vec<Rect>,
}

impl Pieces {
    /// Covers nothing.
    fn none() -> Self {
        Self {
            flag: (0.0, 0.0, 0.0, 0.0),
            wire: Vec::new(),
        }
    }
}

/// Give `layout.net_labels[from..]` the spot that covers least, with
/// `net_labels[..from]` already placed. A label with no spot whose wire is
/// clear keeps the default.
pub(crate) fn clear_net_labels(board: &Board, layout: &mut Layout, from: usize) {
    let placements: HashMap<ComponentId, &ComponentPlacement> =
        layout.components.iter().map(|p| (p.id, p)).collect();
    let parts = part_boxes(board, &placements);
    let place = |label: &NetLabel, spot| {
        net_label_plan(board, label.component, label.pin, &placements, spot)
            .map(|plan| pieces(&plan, flag_width(&label.label)))
    };
    // A label placed ahead of the rest must leave their stubs alone, or the
    // last pin of a column finds its way blocked and sits on a wire. So must
    // it leave the way out of a pin without a label: a wire from it, or a
    // label added after routing, runs there.
    let defaults: Vec<Pieces> = layout
        .net_labels
        .iter()
        .map(|label| place(label, LabelSpot::default()).unwrap_or_else(Pieces::none))
        .collect();
    let labelled: HashSet<(ComponentId, PinId)> = layout
        .net_labels
        .iter()
        .map(|l| (l.component, l.pin))
        .collect();
    let bare = bare_pin_stubs(board, &placements, &labelled);
    let mut taken: Vec<Pieces> = Vec::new();
    let mut spots = Vec::with_capacity(layout.net_labels.len());
    for (index, label) in layout.net_labels.iter().enumerate() {
        let spot = if index < from {
            label.spot
        } else {
            let ahead: Vec<&Pieces> = bare.iter().chain(&defaults[index + 1..]).collect();
            let mut best: Option<(f64, LabelSpot)> = None;
            for spot in candidates() {
                let Some(mine) = place(label, spot) else {
                    continue;
                };
                let Some(cost) = overlap_cost(&mine, label.component, &parts, &taken, &ahead)
                else {
                    continue;
                };
                if best.is_none_or(|(least, _)| cost < least) {
                    best = Some((cost, spot));
                }
                if cost == 0.0 {
                    break;
                }
            }
            best.map_or_else(LabelSpot::default, |(_, spot)| spot)
        };
        if let Some(placed) = place(label, spot) {
            taken.push(placed);
        }
        spots.push(spot);
    }
    for (label, spot) in layout.net_labels.iter_mut().zip(spots) {
        label.spot = spot;
    }
}

/// The spots to try, nearest to the default first.
fn candidates() -> impl Iterator<Item = LabelSpot> {
    LIFTS.into_iter().flat_map(|lift| {
        [false, true]
            .into_iter()
            .map(move |flipped| LabelSpot { lift, flipped })
    })
}

/// How much `mine`, the label of a pin on `owner`, is drawn over: the area its
/// flag shares with parts and flags, plus the area of a flag for each wire
/// that runs through a flag or a flag over a wire. `None` when a wire of it
/// would touch a part other than its own or another wire: a touch joins nets,
/// and no spot is worth that. A label `ahead` is only where it would sit by
/// default; it moves when its turn comes, so only its wire is kept clear.
fn overlap_cost(
    mine: &Pieces,
    owner: ComponentId,
    parts: &[(ComponentId, Rect)],
    taken: &[Pieces],
    ahead: &[&Pieces],
) -> Option<f64> {
    let others = || taken.iter().chain(ahead.iter().copied());
    let on_part = parts
        .iter()
        .any(|(id, part)| *id != owner && mine.wire.iter().any(|w| overlap(*part, *w, 0.0)));
    let wires_meet = others().any(|other| {
        other
            .wire
            .iter()
            .any(|o| mine.wire.iter().any(|w| overlap(*o, *w, 0.0)))
    });
    if on_part || wires_meet {
        return None;
    }
    let flags = parts
        .iter()
        .map(|(_, part)| *part)
        .chain(others().map(|other| other.flag));
    let on_flags: f64 = flags
        .map(|rect| overlap_area(rect, mine.flag, FLAG_TOUCH_MM))
        .sum();
    let crossings = others()
        .filter(|other| {
            other.wire.iter().any(|o| overlap(*o, mine.flag, 0.0))
                || mine.wire.iter().any(|w| overlap(other.flag, *w, 0.0))
        })
        .count();
    Some(on_flags + crossings as f64 * FLAG_H_MM * FLAG_H_MM)
}

fn flag_width(text: &str) -> f64 {
    text.chars().count() as f64 * FLAG_CHAR_MM + FLAG_PAD_MM
}

fn pieces(plan: &LabelPlan, width: f64) -> Pieces {
    let (x, y) = plan.anchor;
    let (x0, x1) = if plan.faces_right {
        (x, x + width)
    } else {
        (x - width, x)
    };
    let wire = plan
        .wire
        .windows(2)
        .map(|seg| {
            (
                seg[0].0.min(seg[1].0) - WIRE_HALF_MM,
                seg[0].1.min(seg[1].1) - WIRE_HALF_MM,
                seg[0].0.max(seg[1].0) + WIRE_HALF_MM,
                seg[0].1.max(seg[1].1) + WIRE_HALF_MM,
            )
        })
        .collect();
    Pieces {
        flag: (x0, y - FLAG_H_MM / 2.0, x1, y + FLAG_H_MM / 2.0),
        wire,
    }
}

/// Whether the boxes overlap by more than `tolerance` in both directions.
fn overlap(a: Rect, b: Rect, tolerance: f64) -> bool {
    a.2.min(b.2) - a.0.max(b.0) > tolerance && a.3.min(b.3) - a.1.max(b.1) > tolerance
}

/// The area two boxes share, 0 when they overlap by no more than `tolerance`.
fn overlap_area(a: Rect, b: Rect, tolerance: f64) -> f64 {
    if overlap(a, b, tolerance) {
        (a.2.min(b.2) - a.0.max(b.0)) * (a.3.min(b.3) - a.1.max(b.1))
    } else {
        0.0
    }
}

/// The way out of every pin that has no label: a default stub long.
fn bare_pin_stubs(
    board: &Board,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
    labelled: &HashSet<(ComponentId, PinId)>,
) -> Vec<Pieces> {
    let mut out = Vec::new();
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        for index in 0..part.pins.len() {
            // u32 cast is bounded: the index comes from the part's own pin list.
            let pin = PinId(index as u32);
            if labelled.contains(&(component.id, pin)) {
                continue;
            }
            let Some((x, y, dx, dy)) = pin_terminal_xy(board, component.id, pin, placements) else {
                continue;
            };
            let (ex, ey) = (x + dx * LABEL_STUB_MM, y + dy * LABEL_STUB_MM);
            out.push(Pieces {
                wire: vec![(
                    x.min(ex) - WIRE_HALF_MM,
                    y.min(ey) - WIRE_HALF_MM,
                    x.max(ex) + WIRE_HALF_MM,
                    y.max(ey) + WIRE_HALF_MM,
                )],
                ..Pieces::none()
            });
        }
    }
    out
}

/// The box round each part's body and the pin numbers at its terminals.
fn part_boxes(
    board: &Board,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> Vec<(ComponentId, Rect)> {
    let mut out = Vec::new();
    for component in &board.components {
        let (Some(part), Some(placement)) =
            (component.part.as_ref(), placements.get(&component.id))
        else {
            continue;
        };
        let (w, h) = body_size_for_part(part);
        let (w, h) = match placement.rotation {
            Rotation::Ninety | Rotation::TwoSeventy => (h, w),
            _ => (w, h),
        };
        let (cx, cy) = placement.center_mm;
        let mut rect = (cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0);
        for index in 0..part.pins.len() {
            // u32 cast is bounded: the index comes from the part's own pin list.
            let pin = PinId(index as u32);
            if let Some((x, y, ..)) = pin_terminal_xy(board, component.id, pin, placements) {
                rect = (rect.0.min(x), rect.1.min(y), rect.2.max(x), rect.3.max(y));
            }
        }
        out.push((
            component.id,
            (
                rect.0 - PIN_NUMBER_MM,
                rect.1 - PIN_NUMBER_MM,
                rect.2 + PIN_NUMBER_MM,
                rect.3 + PIN_NUMBER_MM,
            ),
        ));
    }
    out
}
