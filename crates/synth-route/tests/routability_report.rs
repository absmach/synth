//! Routability report across the example designs.
//!
//! Prints placement HPWL next to the router's own cost (wall time, A* cells
//! expanded, unrouted nets, track length, vias) so a placement change can be
//! judged by how much easier it makes routing, not by eyeballing.
//!
//! Ignored by default because it is a diagnostic, not a gate. Run it in
//! release mode; a debug-build route takes minutes:
//!
//! `cargo test --release -p synth-route --test routability_report -- --ignored --nocapture`

use std::path::Path;
use std::time::Instant;

fn load_board(path: &str) -> synth_ir::Board {
    let source = std::fs::read_to_string(path).expect("read fixture");
    let file = path.to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("parse");
    let registry_dir = Path::new("../..").join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry");
    let loader = synth_ir::FsImportLoader {
        root: std::path::PathBuf::from("../.."),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    lowered.board.expect("board")
}

#[test]
#[ignore = "diagnostic report, not a gate"]
fn routability_report() {
    let examples = [
        "../../examples/sensor_logger/sensor_logger.synth",
        "../../examples/env_logger/env_logger.synth",
        "../../fixtures/designs/secure_tracker.synth",
    ];
    for path in examples {
        if !Path::new(path).exists() {
            continue;
        }
        let board = load_board(path);
        let started = Instant::now();
        let placement = match synth_place::place(&board) {
            Ok(p) => p,
            Err(e) => {
                println!("--- {path}\n  PLACE FAILED {e:?}");
                continue;
            }
        };
        let place_s = started.elapsed().as_secs_f64();
        let score = synth_place::score::score_placement(&board, &placement);
        let started = Instant::now();
        let routing = synth_route::route(&board, &placement);
        let route_s = started.elapsed().as_secs_f64();
        let track_mm: f64 = routing
            .segments
            .iter()
            .map(|s| {
                let dx = synth_geometry::nm_to_mm(s.end.x_nm - s.start.x_nm);
                let dy = synth_geometry::nm_to_mm(s.end.y_nm - s.start.y_nm);
                dx.hypot(dy)
            })
            .sum();
        println!("--- {path}");
        println!(
            "  place {place_s:.2}s | route {route_s:.2}s | cells {} | unrouted {} | hpwl {:.1} mm | track {track_mm:.1} mm | vias {}",
            routing.cells_expanded,
            routing.unrouted_nets.len(),
            synth_geometry::nm_to_mm(score.hpwl_total_nm),
            routing.vias.len()
        );
        for u in &routing.unrouted_nets {
            println!("  UNROUTED: {}", u.net_name);
        }
        if routing.pad_escape_rejections > 0 {
            println!(
                "  pad-escape rejections: {} paths found by A* then discarded as too close to a foreign pad",
                routing.pad_escape_rejections
            );
        }
    }
}

/// Deterministic routability gate for the normal (debug) CI run.
///
/// The full `unrouted_nets_do_not_regress` gate below needs release mode and
/// minutes, so it lives in the dedicated release workflow. This smaller check
/// runs on every commit in seconds and catches the same class of regression —
/// a placement change that increases unrouted nets — on fixed small designs.
/// Raise a baseline deliberately, never as a side effect of a change that was
/// not measured against this.
#[test]
fn small_designs_do_not_regress_unrouted_nets() {
    // (design, max unrouted nets tolerated)
    let expected = [
        ("../../fixtures/designs/three_components.synth", 0_usize),
        ("../../fixtures/designs/four_components.synth", 0_usize),
        ("../../fixtures/designs/decoupling_heavy.synth", 0_usize),
    ];
    let mut regressions = Vec::new();
    for (path, tolerated) in expected {
        if !Path::new(path).exists() {
            continue;
        }
        let board = load_board(path);
        let Ok(placement) = synth_place::place(&board) else {
            regressions.push(format!("{path}: placement failed"));
            continue;
        };
        let routing = synth_route::route(&board, &placement);
        let unrouted = routing.unrouted_nets.len();
        if unrouted > tolerated {
            regressions.push(format!(
                "{path}: {unrouted} unrouted, tolerates {tolerated} ({:?})",
                routing
                    .unrouted_nets
                    .iter()
                    .map(|net| net.net_name.as_str())
                    .collect::<Vec<_>>()
            ));
        }
    }
    assert!(
        regressions.is_empty(),
        "routability regressed:\n  {}",
        regressions.join("\n  ")
    );
}

/// Unrouted nets must not regress.
///
/// This is the gate that the placement work of phases 2 and 4 failed, and
/// it is here so the next placement change cannot fail it the same way.
/// Distance metrics — HPWL, member-to-anchor distance — all improved while
/// routability got worse, which is exactly the case a distance metric
/// cannot catch.
///
/// It is `#[ignore]`d in the normal test run and executed by the dedicated
/// `routability-gate` release job in `.github/workflows/ci.yml`, which every
/// PR runs:
///
/// `cargo test --release -p synth-route --test routability_report unrouted_nets_do_not_regress -- --ignored`
///
/// The smaller, non-ignored `small_designs_do_not_regress_unrouted_nets`
/// above guards the same property on every debug-mode CI run.
///
/// The recorded figures are the pre-placement-change baseline. Raise them
/// deliberately, never as a side effect of a change that was not measured
/// against this.
#[test]
#[ignore = "release-mode gate; run by the routability-gate CI job"]
fn unrouted_nets_do_not_regress() {
    // (design, max unrouted nets tolerated)
    let expected = [
        ("../../examples/sensor_logger/sensor_logger.synth", 2_usize),
        ("../../examples/env_logger/env_logger.synth", 3_usize),
        ("../../fixtures/designs/secure_tracker.synth", 3_usize),
    ];
    let mut regressions = Vec::new();
    for (path, tolerated) in expected {
        if !Path::new(path).exists() {
            continue;
        }
        let board = load_board(path);
        let Ok(placement) = synth_place::place(&board) else {
            regressions.push(format!("{path}: placement failed"));
            continue;
        };
        let routing = synth_route::route(&board, &placement);
        let unrouted = routing.unrouted_nets.len();
        println!("--- {path}: {unrouted} unrouted (tolerated {tolerated})");
        for net in &routing.unrouted_nets {
            println!("    {}", net.net_name);
        }
        if unrouted > tolerated {
            regressions.push(format!(
                "{path}: {unrouted} unrouted, tolerates {tolerated}"
            ));
        }
    }
    assert!(
        regressions.is_empty(),
        "routability regressed:\n  {}",
        regressions.join("\n  ")
    );
}

/// Why a net failed to route.
/// Unrouted nets are the only placement-quality signal that cannot be
/// inferred from a distance metric, so this prints what each failing net
/// connects and which functional cluster each end belongs to — the shape
/// of the failure is usually obvious once the endpoints are named.
#[test]
#[ignore = "diagnostic report, not a gate"]
#[allow(clippy::cast_precision_loss, clippy::needless_range_loop)]
fn unrouted_net_endpoints() {
    for path in [
        "../../examples/sensor_logger/sensor_logger.synth",
        "../../fixtures/designs/secure_tracker.synth",
    ] {
        if !Path::new(path).exists() {
            continue;
        }
        let board = load_board(path);
        let Ok(placement) = synth_place::place(&board) else {
            continue;
        };
        let routing = synth_route::route(&board, &placement);
        println!("--- {path}: {} unrouted", routing.unrouted_nets.len());
        let by_id: std::collections::HashMap<_, _> =
            placement.components.iter().map(|p| (p.id, p)).collect();

        // Which cluster each component belongs to, so a failing net can be
        // read against the motif it belongs to.
        let clusters = synth_ir::recognize_clusters(&board);
        let cluster_of: std::collections::HashMap<u32, String> = clusters
            .iter()
            .flat_map(|cluster| {
                let label = cluster.display_name(&board);
                let anchor = cluster.anchor.0;
                let members: Vec<u32> = cluster.members.iter().map(|m| m.component.0).collect();
                std::iter::once((anchor, label.clone()))
                    .chain(members.into_iter().map(move |m| (m, label.clone())))
            })
            .collect();

        for unrouted in &routing.unrouted_nets {
            let Some(net) = board.net(unrouted.net) else {
                continue;
            };
            let ends: Vec<String> = net
                .endpoints
                .iter()
                .map(|endpoint| {
                    let component = board.component(endpoint.component);
                    let refdes = component.map_or_else(
                        || format!("#{}", endpoint.component.0),
                        |c| c.refdes.clone(),
                    );
                    let cluster = cluster_of
                        .get(&endpoint.component.0)
                        .cloned()
                        .unwrap_or_else(|| "-".to_string());
                    format!("{refdes}[{cluster}]")
                })
                .collect();
            println!("    {}: {}", unrouted.net_name, ends.join(" -> "));

            // Geometry of the failure: how far apart the ends are, and
            // whether anything sits between them.
            if net.endpoints.len() == 2 {
                let (a, b) = (&net.endpoints[0], &net.endpoints[1]);
                let (Some(pa), Some(pb)) = (by_id.get(&a.component), by_id.get(&b.component))
                else {
                    continue;
                };
                let dx = (pa.center.x_nm - pb.center.x_nm) as f64 / 1_000_000.0;
                let dy = (pa.center.y_nm - pb.center.y_nm) as f64 / 1_000_000.0;
                let mut between = 0_usize;
                for placed in &placement.components {
                    if net.endpoints.iter().any(|e| e.component == placed.id) {
                        continue;
                    }
                    let on_segment = {
                        let (x1, y1) = (pa.center.x_nm as f64, pa.center.y_nm as f64);
                        let (x2, y2) = (pb.center.x_nm as f64, pb.center.y_nm as f64);
                        let (px, py) = (placed.center.x_nm as f64, placed.center.y_nm as f64);
                        let cross = (x2 - x1) * (py - y1) - (y2 - y1) * (px - x1);
                        let length = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
                        cross.abs() / length.max(1.0) < 2_000_000.0
                    };
                    if on_segment {
                        between += 1;
                    }
                }
                println!(
                    "        {:.1} mm apart, {between} components between them",
                    (dx * dx + dy * dy).sqrt()
                );
            }
        }
    }
}
