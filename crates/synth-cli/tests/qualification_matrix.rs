// SPDX-License-Identifier: Apache-2.0

//! The Class-B board rows are in the qualification matrix and point at the
//! checked-in board and its evidence (#66, #67).

use std::path::Path;

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

#[test]
fn the_usb_uart_and_buck_boards_are_matrix_rows() {
    let root = root();
    let text = std::fs::read_to_string(root.join("qualification/matrix.toml")).expect("matrix");
    let matrix: toml::Value = toml::from_str(&text).expect("matrix TOML");
    let boards = matrix["board"].as_array().expect("[[board]] rows");
    for id in ["usb-uart", "buck"] {
        let row = boards
            .iter()
            .find(|row| row["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("{id} is a matrix row"));
        assert_eq!(row["class"].as_str(), Some("B"), "{id}");
        let source = row["path"].as_str().expect("path");
        assert!(
            root.join(source).is_file(),
            "{id}: matrix source `{source}` exists"
        );
        assert!(
            root.join(format!("qualification/class-b/{id}")).is_dir(),
            "{id}: evidence directory exists"
        );
        assert!(
            row["rationale"].as_str().is_some_and(|r| !r.is_empty()),
            "{id}: rationale"
        );
    }
}
