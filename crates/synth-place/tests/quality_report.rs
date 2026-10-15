//! Placement quality report across the example designs.
//!
//! Prints objective metrics so a placement change can be compared rather than
//! eyeballed: HPWL, decoupling distance, connector edge distance, board area,
//! and the structural visual-review findings.
//!
//! Ignored by default because it is a diagnostic, not a gate.

use std::path::Path;

fn load_board(path: &str) -> synth_ir::Board {
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

/// Centre distance between each bound cluster member and its anchor, for the
/// roles the board places beside their anchor.
///
/// This is the number pad-aware placement moves. `score_placement`'s own
/// decap metric cannot be used for that: it pairs every capacitor on a
/// merged rail with every IC on it, so it measures pairs no placement
/// decision ever made — on a board with one 3V3 net feeding five parts, the
/// "worst decoupling distance" is a cap against a part it does not decouple,
/// and improving real bindings barely moves it.
///
/// Centre distance rather than pad distance, because that is what placement
/// actually decides; on a large anchor the pad and the centre are far apart
/// even when a member sits right against the pad.
#[test]
#[ignore = "diagnostic report, not a gate"]
#[allow(clippy::cast_precision_loss)]
fn bound_member_pad_distances() {
    for path in [
        "../../examples/sensor_logger/sensor_logger.synth",
        "../../examples/env_logger/env_logger.synth",
        "../../fixtures/designs/secure_tracker.synth",
    ] {
        if !Path::new(path).exists() {
            continue;
        }
        let board = load_board(path);
        let Ok(placement) = synth_place::place(&board) else {
            continue;
        };
        let by_id: std::collections::HashMap<_, _> = placement
            .components
            .iter()
            .map(|p| (p.id, p.center))
            .collect();
        let mut rows: Vec<(f64, String, String)> = Vec::new();
        for cluster in synth_ir::recognize_clusters(&board) {
            if !matches!(
                cluster.kind,
                synth_ir::ClusterKind::IcBlock
                    | synth_ir::ClusterKind::Crystal
                    | synth_ir::ClusterKind::LdoBlock
                    | synth_ir::ClusterKind::UsbEsd
            ) {
                continue;
            }
            let Some(anchor_center) = by_id.get(&cluster.anchor) else {
                continue;
            };
            let anchor_refdes = board
                .component(cluster.anchor)
                .map_or_else(|| "?".to_string(), |c| c.refdes.clone());
            for member in &cluster.members {
                if !matches!(
                    member.role,
                    synth_ir::MemberRole::DecouplingCap
                        | synth_ir::MemberRole::RailCap
                        | synth_ir::MemberRole::LoadCap
                        | synth_ir::MemberRole::EsdDiode
                ) {
                    continue;
                }
                let Some(member_center) = by_id.get(&member.component) else {
                    continue;
                };
                // Centre-to-centre in millimetres, which is what the placement
                // decides; pad offsets would need the rotation the exporter
                // uses, and the centre is the quantity the packer moves.
                #[allow(clippy::cast_precision_loss)]
                let dx =
                    (anchor_center.x_nm - member_center.x_nm).unsigned_abs() as f64 / 1_000_000.0;
                #[allow(clippy::cast_precision_loss)]
                let dy =
                    (anchor_center.y_nm - member_center.y_nm).unsigned_abs() as f64 / 1_000_000.0;
                let member_refdes = board
                    .component(member.component)
                    .map_or_else(|| "?".to_string(), |c| c.refdes.clone());
                rows.push((dx + dy, anchor_refdes.clone(), member_refdes));
            }
        }
        rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let total: f64 = rows.iter().map(|r| r.0).sum();
        let max = rows.first().map_or(0.0, |r| r.0);
        let mean = if rows.is_empty() {
            0.0
        } else {
            total / rows.len() as f64
        };
        println!(
            "--- {path}: bound members {} | mean {mean:.2} mm | max {max:.2} mm",
            rows.len()
        );
        for (dist, anchor, member) in rows.iter().take(6) {
            println!("      {dist:6.2} mm  {member} .. {anchor}");
        }
    }
}

/// Differential-pair geometry: how far apart the two ends are, and whether
/// the far end's pair pads escape toward the near end rather than away from
/// it.
#[test]
#[ignore = "diagnostic report, not a gate"]
#[allow(clippy::cast_precision_loss)]
fn diff_pair_geometry() {
    for path in [
        "../../examples/placement_and_diff_pair/placement_and_diff_pair.synth",
        "../../examples/sensor_logger/sensor_logger.synth",
        "../../examples/env_logger/env_logger.synth",
    ] {
        if !Path::new(path).exists() {
            continue;
        }
        let board = load_board(path);
        let Ok(placement) = synth_place::place(&board) else {
            continue;
        };
        let pairs = synth_ir::resolve_pairs(&board);
        if pairs.is_empty() {
            continue;
        }
        let by_id: std::collections::HashMap<_, _> =
            placement.components.iter().map(|p| (p.id, *p)).collect();
        println!("--- {path}: {} pairs", pairs.len());
        for connection in synth_ir::pair_connections(&board, &pairs) {
            let near = connection.pair.positive_pin.0;
            let Some(near_place) = by_id.get(&near) else {
                continue;
            };
            let Some(far_place) = by_id.get(&connection.far_end) else {
                continue;
            };
            let dx = (near_place.center.x_nm - far_place.center.x_nm) as f64 / 1_000_000.0;
            let dy = (near_place.center.y_nm - far_place.center.y_nm) as f64 / 1_000_000.0;
            let near_refdes = board
                .component(near)
                .map_or_else(|| "?".to_string(), |c| c.refdes.clone());
            let far_refdes = board
                .component(connection.far_end)
                .map_or_else(|| "?".to_string(), |c| c.refdes.clone());
            println!(
                "    {near_refdes} -> {far_refdes}: {:.2} mm apart | near rot={:?} far rot={:?}",
                (dx * dx + dy * dy).sqrt(),
                near_place.rotation,
                far_place.rotation
            );
        }
    }
}

#[test]
#[ignore = "diagnostic report, not a gate"]
fn placement_quality_report() {
    let examples = [
        "../../examples/sensor_logger/sensor_logger.synth",
        "../../examples/env_logger/env_logger.synth",
        "../../examples/placement_and_diff_pair/placement_and_diff_pair.synth",
        "../../fixtures/designs/secure_tracker.synth",
    ];
    for path in examples {
        if !Path::new(path).exists() {
            continue;
        }
        let board = load_board(path);
        let placement = match synth_place::place(&board) {
            Ok(p) => p,
            Err(e) => {
                println!("--- {path}\n  PLACE FAILED {e:?}");
                continue;
            }
        };
        let desc = synth_place::describe_placement(&board, &placement);
        let score = synth_place::score::score_placement(&board, &placement);
        println!("--- {path}");
        println!(
            "  outline {:.1}x{:.1} mm | components {} | visual_score {} | requires_revision {}",
            desc.board_size_mm[0],
            desc.board_size_mm[1],
            placement.components.len(),
            desc.visual_review.score,
            desc.visual_review.requires_revision
        );
        println!(
            "  hpwl {:.1} mm | max_decap {:.2} mm | avg_decap {:.2} mm | top_rail_ratio {:.2} | min_connector_edge {:.2} mm",
            synth_geometry::nm_to_mm(score.hpwl_total_nm),
            score.max_decoupling_distance_mm,
            score.avg_decoupling_distance_mm,
            score.top_rail_passive_ratio,
            score.edge_connector_boundary_dist_mm
        );
        println!(
            "  passes_human_quality_gate={}",
            score.passes_human_quality_gate
        );
        for f in &desc.visual_review.findings {
            println!("  FINDING: {f}");
        }
        for w in &desc.functional_warnings {
            println!("  FUNC: {w}");
        }
        for d in &desc.dense_regions {
            println!("  DENSE: {} {:.2}", d.region, d.density);
        }
    }
}
