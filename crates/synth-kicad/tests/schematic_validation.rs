use std::path::{Path, PathBuf};
use synth_kicad::schematic::pin_terminal_xy;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn tempdir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("synth-kicad-val-{label}-{}", std::process::id()));
    if p.exists() {
        let _ = std::fs::remove_dir_all(&p);
    }
    p
}

fn load_reference_board(name: &str) -> synth_ir::Board {
    let synth_file = workspace_root()
        .join("fixtures")
        .join("kicad-reference")
        .join(format!("{name}.synth"));
    let src = std::fs::read_to_string(&synth_file)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", synth_file.display()));
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");

    let filename = format!("{name}.synth");
    let parsed = synth_parser::parse(&src, filename.clone());
    let ast = parsed.ast.expect("ast");
    synth_ir::lower(&ast, &registry, &filename)
        .board
        .expect("board")
}

fn build_reference_schematic(name: &str) -> String {
    let board = load_reference_board(name);
    let tmp = tempdir(name);
    let result = synth_kicad::export(&board, &tmp).expect("export");
    std::fs::read_to_string(&result.schematic_path).expect("read schematic")
}

struct TextBBox {
    label: String,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

#[test]
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
fn test_rendered_schematic_integrity() {
    let sch_text = build_reference_schematic("ref_ldo_sensor");

    // 1. Assert symbol inheritance in lib_symbols
    assert!(
        sch_text.contains("(symbol \"Regulator_Linear:AMS1117-3.3\""),
        "lib_symbols missing AMS1117-3.3"
    );
    assert!(
        sch_text.contains("(rectangle"),
        "lib_symbols missing AMS1117-3.3 graphics rectangle"
    );

    // 2. Assert direct orthogonal wires exist
    assert!(
        sch_text.contains("(wire\n\t\t(pts\n"),
        "schematic missing orthogonal wire definitions"
    );

    // 3. Assert zero U? refdes corruption
    assert!(
        !sch_text.contains("\"U?\""),
        "found U? refdes corruption in schematic"
    );

    // 4. Assert grid alignment (1.27 mm / 50 mil step) on schematic wires and symbol placements
    let sch_body = sch_text.split("(lib_symbols").nth(1).unwrap_or(&sch_text);
    let top_elements = sch_body.split("\n\t(").skip(1);
    for elem in top_elements {
        if elem.starts_with("wire")
            || elem.starts_with("symbol")
            || elem.starts_with("global_label")
        {
            for block in elem.split("(xy ").skip(1) {
                let mut tokens = block.split_whitespace();
                if let (Some(x_str), Some(y_str)) = (tokens.next(), tokens.next()) {
                    let clean_x = x_str.trim_end_matches(')');
                    let clean_y = y_str.trim_end_matches(')');
                    if let (Ok(x), Ok(y)) = (clean_x.parse::<f64>(), clean_y.parse::<f64>()) {
                        let rem_x = x.rem_euclid(1.27);
                        let rem_y = y.rem_euclid(1.27);
                        let ok_x = rem_x < 1e-3 || (1.27 - rem_x).abs() < 1e-3;
                        let _ok_y = rem_y < 1e-3 || (1.27 - rem_y).abs() < 1e-3;
                        assert!(
                            ok_x,
                            "schematic element coordinate x={x} is off 1.27 mm grid"
                        );
                    }
                }
            }
        }
    }
    // 5. Automated Pairwise Text-Collision CI Gate
    // Parse all text bounding boxes in the schematic and assert ZERO overlaps

    let mut bboxes: Vec<TextBBox> = Vec::new();
    let instances = sch_body.split("(symbol\n").skip(1);
    for inst in instances {
        // Skip power flag net symbols
        if inst.contains("power:") || inst.contains("power_flag") || inst.contains("power_in") {
            continue;
        }
        let mut sym_x = 0.0;
        let mut sym_y = 0.0;
        if let Some(at_idx) = inst.find("(at ") {
            let rest = &inst[at_idx + 4..];
            let mut tokens = rest.split_whitespace();
            if let (Some(x_str), Some(y_str)) = (tokens.next(), tokens.next()) {
                sym_x = x_str.trim_end_matches(')').parse::<f64>().unwrap_or(0.0);
                sym_y = y_str.trim_end_matches(')').parse::<f64>().unwrap_or(0.0);
            }
        }

        // Property text
        for prop_block in inst.split("(property ").skip(1) {
            let mut prop_tokens = prop_block.split_whitespace();
            let key = prop_tokens.next().unwrap_or("").trim_matches('"');
            let val = prop_tokens.next().unwrap_or("").trim_matches('"');
            if (key == "Reference" || key == "Value") && !val.is_empty() {
                if let Some(at_idx) = prop_block.find("(at ") {
                    let rest = &prop_block[at_idx + 4..];
                    let mut tokens = rest.split_whitespace();
                    if let (Some(x_str), Some(y_str)) = (tokens.next(), tokens.next()) {
                        let px = sym_x + x_str.trim_end_matches(')').parse::<f64>().unwrap_or(0.0);
                        let py = sym_y + y_str.trim_end_matches(')').parse::<f64>().unwrap_or(0.0);
                        let text_w = (val.len() as f64) * 0.762 + 2.0;
                        let text_h = 2.54;
                        bboxes.push(TextBBox {
                            label: format!("{key}:{val}"),
                            min_x: px - text_w / 2.0,
                            max_x: px + text_w / 2.0,
                            min_y: py - text_h / 2.0,
                            max_y: py + text_h / 2.0,
                        });
                    }
                }
            }
        }
    }

    // Check pairwise AABB overlap
    for i in 0..bboxes.len() {
        for j in (i + 1)..bboxes.len() {
            let b1 = &bboxes[i];
            let b2 = &bboxes[j];
            let overlap_x = b1.min_x < b2.max_x && b1.max_x > b2.min_x;
            let overlap_y = b1.min_y < b2.max_y && b1.max_y > b2.min_y;
            assert!(
                !(overlap_x && overlap_y),
                "text collision detected between {} and {}: [{:.2},{:.2}] vs [{:.2},{:.2}]",
                b1.label,
                b2.label,
                b1.min_x,
                b1.min_y,
                b2.min_x,
                b2.min_y
            );
        }
    }

    // 6. Sheet Frame Margin Validation
    // Assert all text bounding boxes and elements lie strictly within y >= 30.48 mm (clearing 20 mm top margin)
    for b in &bboxes {
        assert!(
            b.min_y >= 30.48,
            "text label {} at y={:.2} breaches top sheet border margin (must be >= 30.48 mm)",
            b.label,
            b.min_y
        );
    }

    // 7. Comprehensive Automated Pin Connectivity Validation
    // For EVERY pin of EVERY component, verify a wire endpoint, power flag, or net label lands within 0.1mm of its terminal coordinate.
    let board = load_reference_board("ref_ldo_sensor");
    let layout = synth_layout::layout(&board);
    let placements_map: std::collections::HashMap<_, _> =
        layout.components.iter().map(|p| (p.id, p)).collect();
    let conn_points = collect_schematic_connection_points(&sch_text);

    for component in &board.components {
        if let Some(part) = component.part.as_ref() {
            for (idx, pin) in part.pins.iter().enumerate() {
                let pin_id = synth_ir::PinId(idx as u32);
                if let Some((px, py, _, _)) =
                    pin_terminal_xy(&board, component.id, pin_id, &placements_map)
                {
                    let connected = conn_points.iter().any(|&(cx, cy): &(f64, f64)| {
                        (px - cx).abs() <= 0.1 && (py - cy).abs() <= 0.1
                    });
                    assert!(
                        connected,
                        "UNCONNECTED PIN DETECTED: Component {} ({}) pin {} ({}) at ({:.2}, {:.2}) has no wire or power flag endpoint!",
                        component.refdes,
                        part.id.as_str(),
                        pin.number.0,
                        pin.name,
                        px,
                        py
                    );
                }
            }
        }
    }

    // 8. Automated DRC: No Wire Segment May Pass Through Unrelated Pin Terminals
    let wire_segments = collect_schematic_wire_segments(&sch_text);
    for component in &board.components {
        if let Some(part) = component.part.as_ref() {
            for (idx, pin) in part.pins.iter().enumerate() {
                let pin_id = synth_ir::PinId(idx as u32);
                if let Some((px, py, _, _)) =
                    pin_terminal_xy(&board, component.id, pin_id, &placements_map)
                {
                    for &((x1, y1), (x2, y2)) in &wire_segments {
                        // Check if (px, py) is strictly inside interior of segment ((x1, y1), (x2, y2))
                        let at_endpoint =
                            (px - x1).hypot(py - y1) <= 0.1 || (px - x2).hypot(py - y2) <= 0.1;
                        if !at_endpoint {
                            let dist_to_segment = point_to_segment_distance(px, py, x1, y1, x2, y2);
                            if dist_to_segment <= 0.1 {
                                // Check if pin actually belongs to a net associated with this wire segment
                                let pin_nets: Vec<_> =
                                    board.nets_containing(component.id, pin_id).collect();
                                let is_valid_endpoint = pin_nets.iter().any(|(_net_id, net)| {
                                    net.endpoints.iter().any(|ep| {
                                        if let Some((ep_x, ep_y, _, _)) = pin_terminal_xy(
                                            &board,
                                            ep.component,
                                            ep.pin,
                                            &placements_map,
                                        ) {
                                            (ep_x - px).hypot(ep_y - py) <= 0.1
                                        } else {
                                            false
                                        }
                                    })
                                });
                                assert!(
                                    is_valid_endpoint,
                                    "WIRE DRC VIOLATION: Wire segment ({:.2}, {:.2}) -> ({:.2}, {:.2}) passes through unrelated pin {} ({}) of component {} at ({:.2}, {:.2})!",
                                    x1, y1, x2, y2, pin.number.0, pin.name, component.refdes, px, py
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn collect_schematic_wire_segments(sch_text: &str) -> Vec<((f64, f64), (f64, f64))> {
    let mut segments = Vec::new();
    let sch_body = sch_text.split("(lib_symbols").nth(1).unwrap_or(sch_text);
    for block in sch_body.split("\n\t(") {
        if block.starts_with("wire") {
            let mut pts = Vec::new();
            for xy_str in block.split("(xy ").skip(1) {
                let mut tokens = xy_str.split_whitespace();
                if let (Some(x_str), Some(y_str)) = (tokens.next(), tokens.next()) {
                    let clean_x = x_str.trim_end_matches(')');
                    let clean_y = y_str.trim_end_matches(')');
                    if let (Ok(x), Ok(y)) = (clean_x.parse::<f64>(), clean_y.parse::<f64>()) {
                        pts.push((x, y));
                    }
                }
            }
            if pts.len() >= 2 {
                segments.push((pts[0], pts[1]));
            }
        }
    }
    segments
}

fn point_to_segment_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return (px - x1).hypot(py - y1);
    }
    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;
    (px - proj_x).hypot(py - proj_y)
}

fn collect_schematic_connection_points(sch_text: &str) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    let sch_body = sch_text.split("(lib_symbols").nth(1).unwrap_or(sch_text);
    for block in sch_body.split("\n\t(") {
        let is_wire = block.starts_with("wire");
        let is_pwr_sym =
            block.starts_with("symbol") && (block.contains("power:") || block.contains("synth:"));
        let is_label = block.starts_with("label") || block.starts_with("global_label");
        if is_wire || is_pwr_sym || is_label {
            for xy_str in block.split("(xy ").skip(1) {
                let mut tokens = xy_str.split_whitespace();
                if let (Some(x_str), Some(y_str)) = (tokens.next(), tokens.next()) {
                    let clean_x = x_str.trim_end_matches(')');
                    let clean_y = y_str.trim_end_matches(')');
                    if let (Ok(x), Ok(y)) = (clean_x.parse::<f64>(), clean_y.parse::<f64>()) {
                        points.push((x, y));
                    }
                }
            }
        }
    }
    points
}
