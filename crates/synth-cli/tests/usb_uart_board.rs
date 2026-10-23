// SPDX-License-Identifier: Apache-2.0

//! The SYN-66 USB-UART board carries four mounting holes, one in each corner
//! of the placed outline.

use std::process::Command;

fn synth_json(subcommand: &str) -> serde_json::Value {
    let source = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../qualification/class-b/usb-uart/usb_uart.synth"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_synth"))
        .args([subcommand, source])
        .output()
        .expect("run synth");
    assert!(output.status.success(), "{subcommand} failed: {output:?}");
    serde_json::from_slice(&output.stdout).expect("JSON output")
}

#[test]
fn mounting_holes_sit_one_per_corner_of_the_outline() {
    let ir = synth_json("dump-ir");
    let hole_ids: Vec<i64> = ir["components"]
        .as_array()
        .expect("components")
        .iter()
        .filter(|c| c["kind"] == "mounting_hole")
        .map(|c| c["id"].as_i64().expect("id"))
        .collect();
    assert_eq!(hole_ids.len(), 4, "expected four mounting holes");

    let placement = synth_json("place");
    let max_x = placement["board_outline"]["max"]["x_nm"].as_i64().unwrap();
    let max_y = placement["board_outline"]["max"]["y_nm"].as_i64().unwrap();
    let mut corners: Vec<(bool, bool)> = hole_ids
        .iter()
        .map(|id| {
            let c = placement["components"]
                .as_array()
                .expect("placed components")
                .iter()
                .find(|c| c["id"].as_i64() == Some(*id))
                .expect("hole is placed");
            let (x, y) = (
                c["center"]["x_nm"].as_i64().unwrap(),
                c["center"]["y_nm"].as_i64().unwrap(),
            );
            assert!((0..=max_x).contains(&x) && (0..=max_y).contains(&y));
            (x < max_x / 2, y < max_y / 2)
        })
        .collect();
    corners.sort_unstable();
    corners.dedup();
    assert_eq!(corners.len(), 4, "holes must cover all four quadrants");
}
