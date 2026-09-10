// SPDX-License-Identifier: Apache-2.0

//! Copper-to-component clearance — checks routed segments
//! against component courtyards and pads using continuous
//! geometry recomputed directly from `Board` + `Placement`,
//! independent of `synth_route::grid::build_grid`.
//!
//! `synth_route::drc::check` already re-verifies the router's
//! output, but it does so by rebuilding the *same* pitch/cell
//! grid the router searched over — a bug in that grid's
//! obstacle-expansion math would be invisible to both the
//! router and that check, since they share it. This module
//! recomputes courtyard/pad rectangles straight from
//! `pcb_courtyard_for_part` / `kicad_footprint_loader::pads`
//! (the same public geometry primitives, never the grid) and
//! measures exact rectangle-to-rectangle clearance against the
//! manufacturer profile. Deliberate duplication of the
//! extraction logic in `synth_route::grid::build_grid`, per
//! this crate's stated principle in `lib.rs`: "a DRC engine
//! that shares state with the producer can't catch the
//! producer's bugs."
//!
//! Prior-art note (2026-07-21 scan of pcbflow/cuflow): pcbflow's
//! `Board.check()` finds clearance violations by growing the
//! merged copper polygon (via a `shapely` buffer) until shapes
//! touch — a general-polygon technique needed because its trace
//! geometry isn't restricted to axis-aligned rectangles. Synth's
//! V1 router only emits axis-aligned segments (plan §10.6) and
//! placement only rotates components in 90° steps, so every
//! shape here — courtyard, pad, and the swept copper of a trace
//! — is an axis-aligned rectangle. That makes a closed-form
//! rectangle-gap distance both exact-in-practice and cheaper
//! than a general polygon buffer, while preserving the same
//! "independent geometric verification" property pcbflow's
//! check exists for.

use std::collections::HashMap;

use synth_geometry::{mm_to_nm, nm_to_mm, Point, Rect, Rotation};
use synth_ir::{Board, ComponentId, NetId};
use synth_layout::kicad_footprint_loader;
use synth_place::Placement;
use synth_route::{Routing, Segment};

use crate::profile::ManufacturerProfile;
use crate::Violation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObstacleLayer {
    Single(synth_geometry::Layer),
    All,
}

impl ObstacleLayer {
    pub fn matches(self, l: synth_geometry::Layer) -> bool {
        match self {
            Self::Single(target) => target == l,
            Self::All => true,
        }
    }
}

/// A keep-out rectangle recomputed from placement geometry.
/// `net = None` for a courtyard (every net must clear it — a
/// trace has no business inside a component body it isn't
/// pinned to). `net = Some(n)` for a single pad (only nets
/// other than `n` must clear it; `n`'s own trace is expected to
/// land there).
struct Obstacle {
    rect: Rect,
    net: Option<NetId>,
    component_nets: Vec<NetId>,
    layer: ObstacleLayer,
    component_id: Option<ComponentId>,
}

/// Copper-to-component clearance — every routed segment must
/// keep the profile's clearance from every component courtyard
/// (unless the net connects to a pad on that component) and
/// from every pad belonging to a *different* net. Code:
/// `E-SYNTH-DRC-007`.
#[must_use]
pub fn check_copper_to_component_clearance(
    board: &Board,
    placement: &Placement,
    routing: &Routing,
    profile: &ManufacturerProfile,
) -> Vec<Violation> {
    let clearance = profile.min_copper_clearance_nm;
    let obstacles = collect_obstacles(board, placement);
    let mut violations = Vec::new();
    for s in &routing.segments {
        let capsule = segment_capsule(s);
        for obstacle in &obstacles {
            if !obstacle.layer.matches(s.layer) {
                continue;
            }
            if obstacle.net == Some(s.net) || obstacle.component_nets.contains(&s.net) {
                continue;
            }
            let d = rect_gap_nm(&capsule, &obstacle.rect);
            if d < clearance {
                violations.push(Violation {
                    code: "E-SYNTH-DRC-007".to_string(),
                    message: format!(
                        "net {} trace is {:.3} mm from {} (< {:.3} mm required)",
                        s.net.0,
                        nm_to_mm(d),
                        obstacle.net.map_or_else(
                            || "a component courtyard".to_string(),
                            |n| format!("net {}'s pad", n.0)
                        ),
                        nm_to_mm(clearance),
                    ),
                    witness: vec![s.start, s.end],
                    nets: match obstacle.net {
                        Some(n) => vec![s.net, n],
                        None => vec![s.net],
                    },
                    components: obstacle
                        .component_id
                        .map(|c| c.0.to_string())
                        .into_iter()
                        .collect(),
                    pos_mm: Some((nm_to_mm(s.start.x_nm), nm_to_mm(s.start.y_nm))),
                    suggested_override: None,
                });
            }
        }
    }
    violations
}

/// Short-detection — every routed segment's copper rectangle must not
/// overlap a pad belonging to a *different* net. Code:
/// `E-SYNTH-DRC-008`. A trace that lands on (or through) a foreign
/// pad is a hard short, not merely a clearance violation, and would
/// be fabbed as one.
#[must_use]
pub fn check_shorts(board: &Board, placement: &Placement, routing: &Routing) -> Vec<Violation> {
    let obstacles = collect_obstacles(board, placement);
    let mut violations = Vec::new();
    for s in &routing.segments {
        let capsule = segment_capsule(s);
        for obstacle in &obstacles {
            if !obstacle.layer.matches(s.layer) {
                continue;
            }
            // Same net connection is the intended pad-to-trace join.
            if obstacle.net == Some(s.net) {
                continue;
            }
            // Only copper pads (net != None) short; courtyards
            // (net None) are covered by the clearance rule.
            let Some(other_net) = obstacle.net else {
                continue;
            };
            if capsule.intersects(&obstacle.rect) {
                violations.push(Violation {
                    code: "E-SYNTH-DRC-008".to_string(),
                    message: format!(
                        "net {} trace short-circuits into net {}'s pad",
                        s.net.0, other_net.0
                    ),
                    witness: vec![s.start, s.end],
                    nets: vec![s.net, other_net],
                    components: Vec::new(),
                    pos_mm: Some((nm_to_mm(s.start.x_nm), nm_to_mm(s.start.y_nm))),
                    suggested_override: None,
                });
            }
        }
    }
    violations
}

/// Map every `(component, pin number)` to its net, adding the
/// USB-C shield / CC-mirror pins so connector shells inherit the
/// net of the pad they are metallurgically bonded to.
fn build_pad_net_lookup(board: &Board) -> HashMap<(ComponentId, String), NetId> {
    let mut pad_net_lookup: HashMap<(ComponentId, String), NetId> = HashMap::new();
    for net in &board.nets {
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
            pad_net_lookup.insert((component.id, pin.number.0.clone()), net.id);
        }
    }

    let mut mirror_additions = Vec::new();
    for ((comp, pin), net_id) in &pad_net_lookup {
        if pin == "A1" || pin == "B12" {
            mirror_additions.push(((*comp, "A12".to_string()), *net_id));
            mirror_additions.push(((*comp, "B1".to_string()), *net_id));
            mirror_additions.push(((*comp, "B12".to_string()), *net_id));
            mirror_additions.push(((*comp, "SH".to_string()), *net_id));
            mirror_additions.push(((*comp, "SH1".to_string()), *net_id));
            mirror_additions.push(((*comp, "SH2".to_string()), *net_id));
            mirror_additions.push(((*comp, "SH3".to_string()), *net_id));
            mirror_additions.push(((*comp, "SH4".to_string()), *net_id));
        } else if pin == "A4" || pin == "B9" {
            mirror_additions.push(((*comp, "A9".to_string()), *net_id));
            mirror_additions.push(((*comp, "B4".to_string()), *net_id));
            mirror_additions.push(((*comp, "B9".to_string()), *net_id));
        }
    }
    for (k, v) in mirror_additions {
        pad_net_lookup.entry(k).or_insert(v);
    }
    pad_net_lookup
}

fn collect_obstacles(board: &Board, placement: &Placement) -> Vec<Obstacle> {
    let pad_net_lookup = build_pad_net_lookup(board);

    let mut obstacles = Vec::new();
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let Some(comp_placement) = placement.components.iter().find(|p| p.id == component.id)
        else {
            continue;
        };

        let comp_nets: Vec<NetId> = pad_net_lookup
            .iter()
            .filter(|((cid, _), _)| *cid == component.id)
            .map(|(_, &nid)| nid)
            .collect();

        let ((court_cx_mm, court_cy_mm), (court_w_mm, court_h_mm)) =
            synth_layout::pcb_courtyard_geometry_for_part(part);
        let half_w_nm = mm_to_nm(court_w_mm) / 2;
        let half_h_nm = mm_to_nm(court_h_mm) / 2;
        let (rotated_half_w, rotated_half_h) = match comp_placement.rotation {
            Rotation::Zero | Rotation::OneEighty => (half_w_nm, half_h_nm),
            Rotation::Ninety | Rotation::TwoSeventy => (half_h_nm, half_w_nm),
        };
        let (rot_cx, rot_cy) = comp_placement
            .rotation
            .rotate_offset(mm_to_nm(court_cx_mm), mm_to_nm(court_cy_mm));
        let court_center = comp_placement.center;
        obstacles.push(Obstacle {
            rect: Rect::from_center_half_extents(court_center, rotated_half_w, rotated_half_h),
            net: None,
            component_nets: comp_nets,
            layer: ObstacleLayer::Single(comp_placement.layer),
            component_id: Some(component.id),
        });

        let Some(lib_id) = part.kicad_footprint.as_deref() else {
            continue;
        };
        let Some(pads) = kicad_footprint_loader::pads(lib_id) else {
            continue;
        };
        for pad in pads {
            let (pad_w_mm, pad_h_mm) = pad.size_mm;
            // KiCad's file convention via the shared helper, so
            // DRC inspects pads exactly where pcbnew renders them.
            let (rot_x, rot_y) = comp_placement
                .rotation
                .rotate_offset(mm_to_nm(pad.center_mm.0), mm_to_nm(pad.center_mm.1));
            let (rot_w_nm, rot_h_nm) = if comp_placement.rotation.swaps_extents() {
                (mm_to_nm(pad_h_mm), mm_to_nm(pad_w_mm))
            } else {
                (mm_to_nm(pad_w_mm), mm_to_nm(pad_h_mm))
            };
            let pad_centre = Point::new(
                comp_placement.center.x_nm - rot_cx + rot_x,
                comp_placement.center.y_nm - rot_cy + rot_y,
            );
            let net = pad_net_lookup
                .get(&(component.id, pad.number.clone()))
                .copied();
            let pad_layer = match pad.copper_layers {
                kicad_footprint_loader::PadCopperLayers::Both
                | kicad_footprint_loader::PadCopperLayers::None => ObstacleLayer::All,
                kicad_footprint_loader::PadCopperLayers::Front => {
                    ObstacleLayer::Single(comp_placement.layer)
                }
                kicad_footprint_loader::PadCopperLayers::Back => match comp_placement.layer {
                    synth_geometry::Layer::Top => {
                        ObstacleLayer::Single(synth_geometry::Layer::Bottom)
                    }
                    _ => ObstacleLayer::Single(synth_geometry::Layer::Top),
                },
            };
            obstacles.push(Obstacle {
                rect: Rect::from_center_half_extents(pad_centre, rot_w_nm / 2, rot_h_nm / 2),
                net,
                component_nets: Vec::new(),
                layer: pad_layer,
                component_id: Some(component.id),
            });
        }
    }
    obstacles
}

/// The copper rectangle a segment actually occupies: its
/// axis-aligned bounding box expanded by half the trace width.
/// Exact because the V1 router only emits axis-aligned segments
/// (plan §10.6).
fn segment_capsule(s: &Segment) -> Rect {
    let half_w = s.width_nm / 2;
    let (min_x, max_x) = sort_pair(s.start.x_nm, s.end.x_nm);
    let (min_y, max_y) = sort_pair(s.start.y_nm, s.end.y_nm);
    Rect::new(
        Point::new(min_x - half_w, min_y - half_w),
        Point::new(max_x + half_w, max_y + half_w),
    )
}

fn sort_pair(a: i64, b: i64) -> (i64, i64) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Minimum gap between two axis-aligned rectangles. Manhattan
/// (`dx + dy`), not Euclidean — same conservative convention as
/// `rules::check_min_drill_to_copper`: exact whenever the
/// rectangles overlap on one axis (the common case, since every
/// shape here is axis-aligned), a safe overestimate only at a
/// pure diagonal corner approach.
fn rect_gap_nm(a: &Rect, b: &Rect) -> i64 {
    let dx = (a.min.x_nm - b.max.x_nm)
        .max(b.min.x_nm - a.max.x_nm)
        .max(0);
    let dy = (a.min.y_nm - b.max.y_nm)
        .max(b.min.y_nm - a.max.y_nm)
        .max(0);
    dx + dy
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_geometry::{mm_to_nm, Layer};
    use synth_ir::Board;

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

    fn empty_routing() -> Routing {
        Routing {
            segments: Vec::new(),
            vias: Vec::new(),
            diff_pair_reports: Vec::new(),
            unrouted_nets: Vec::new(),
            cells_expanded: 0,
        }
    }

    fn jlc() -> ManufacturerProfile {
        ManufacturerProfile::jlc_standard()
    }

    #[test]
    fn rect_gap_zero_when_overlapping() {
        let a = Rect::new(Point::new(0, 0), Point::new(10, 10));
        let b = Rect::new(Point::new(5, 5), Point::new(15, 15));
        assert_eq!(rect_gap_nm(&a, &b), 0);
    }

    #[test]
    fn rect_gap_matches_axis_aligned_separation() {
        let a = Rect::new(Point::new(0, 0), Point::new(10, 10));
        let b = Rect::new(Point::new(20, 0), Point::new(30, 10));
        assert_eq!(rect_gap_nm(&a, &b), 10);
    }

    #[test]
    fn flags_trace_running_through_a_component_courtyard() {
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let comp = placement
            .components
            .first()
            .expect("at least one component");
        let mut r = empty_routing();
        // A net id that owns no pin on this design, so no pad
        // exemption applies; the segment sits dead-centre on
        // the first component's placed position.
        r.segments.push(Segment {
            net: NetId(999_999),
            layer: Layer::Top,
            start: comp.center,
            end: Point::new(comp.center.x_nm + mm_to_nm(0.01), comp.center.y_nm),
            width_nm: mm_to_nm(0.15),
        });
        let violations = check_copper_to_component_clearance(&board, &placement, &r, &jlc());
        assert!(
            !violations.is_empty(),
            "a trace through a component body must be flagged"
        );
        assert!(violations.iter().all(|v| v.code == "E-SYNTH-DRC-007"));
    }

    #[test]
    fn does_not_flag_a_trace_far_from_every_component() {
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let mut r = empty_routing();
        r.segments.push(Segment {
            net: NetId(999_999),
            layer: Layer::Top,
            start: Point::new(mm_to_nm(500.0), mm_to_nm(500.0)),
            end: Point::new(mm_to_nm(501.0), mm_to_nm(500.0)),
            width_nm: mm_to_nm(0.15),
        });
        assert!(check_copper_to_component_clearance(&board, &placement, &r, &jlc()).is_empty());
    }
}
