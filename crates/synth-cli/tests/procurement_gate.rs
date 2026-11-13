use std::{fs, process::Command};

#[test]
fn a_hand_assembled_connector_can_be_locked_with_a_disposition() {
    let directory = std::env::temp_dir().join(format!(
        "synth-procurement-disposition-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let input = directory.join("input.synth");
    fs::write(
        &input,
        r#"board "hand" {
  layers 2
  component J1: connector "jst_ph_2pin"
  component R1: resistor "r_generic_0603" value "10k"
  connect J1.p1 -> R1.p1
  connect J1.p2 -> R1.p2
}"#,
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_synth"))
        .arg("procurement-lock")
        .arg(&input)
        .args([
            "--disposition",
            "J1=hand-assembly",
            "--disposition",
            "R1=hand-assembly",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "the lock was refused: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let lock: serde_json::Value = serde_json::from_slice(&result.stdout).expect("a lock");
    let part = lock["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["refdes"] == "J1")
        .expect("J1 in the lock");
    assert_eq!(part["disposition"], "hand_assembly");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn invalid_designs_never_replace_a_procurement_lock() {
    for source in [
        "board \"empty\" { layers 2 }",
        "board \"broken\" { component U1: mcu \"not_a_registry_part\" }",
        "board \"broken\" { layers invalid }",
        "import \"missing.synth\"\nboard \"broken\" { layers 2 }",
    ] {
        let directory = std::env::temp_dir().join(format!(
            "synth-procurement-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let input = directory.join("input.synth");
        let output = directory.join("procurement.json");
        fs::write(&input, source).unwrap();
        fs::write(&output, "previous reviewed lock").unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_synth"))
            .arg("procurement-lock")
            .arg(&input)
            .arg("--out")
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            !result.status.success(),
            "invalid source was locked: {source}"
        );
        assert_eq!(
            fs::read_to_string(&output).unwrap(),
            "previous reviewed lock"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
