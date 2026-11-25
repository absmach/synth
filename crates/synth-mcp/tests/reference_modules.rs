// SPDX-License-Identifier: Apache-2.0

//! The reference-module tools: search, inspect, instantiate, and the
//! deviation listing a validate of the result carries.

use serde_json::json;

fn registry() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/parts")
}

fn call(name: &str, args: &serde_json::Value) -> serde_json::Value {
    synth_mcp::call_tool(name, args, Some(&registry())).expect("tool call succeeds")
}

#[test]
fn the_module_tools_are_listed() {
    let names: Vec<String> = synth_mcp::list_tools()
        .into_iter()
        .map(|t| t.name)
        .collect();
    for tool in [
        "synth_search_modules",
        "synth_get_module",
        "synth_instantiate_module",
    ] {
        assert!(names.iter().any(|n| n == tool), "{tool} missing");
    }
}

#[test]
fn search_and_get_return_provenance() {
    let found = call("synth_search_modules", &json!({ "query": "ldo" }));
    assert_eq!(found["modules"][0]["id"], "ldo_ams1117_3v3", "{found}");
    let module = call(
        "synth_get_module",
        &json!({ "module_id": "ldo_ams1117_3v3" }),
    );
    assert!(
        module["provenance"]["source"]
            .as_str()
            .unwrap()
            .contains("AMS1117")
    );
    let fixed: Vec<&str> = module["params"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["fixed"] == true)
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(fixed, vec!["c_out"]);
    assert!(
        module["source"]
            .as_str()
            .unwrap()
            .contains("module \"LdoAms1117\"")
    );
}

#[test]
fn an_instantiated_module_validates_and_lists_its_deviation() {
    let design = r#"board "b" {
  layers 2
  component J1: connector "header_1x4"
  power "VIN" 5v { J1.p1 }
  power "+3V3" 3.3v { J1.p2 }
  net "GND" { J1.p3 }
  no_connect J1.p4
}
"#;
    let placed = call(
        "synth_instantiate_module",
        &json!({
            "module_id": "ldo_ams1117_3v3",
            "label": "LDO1",
            "source": design,
            "bindings": { "vin": "VIN", "vout": "+3V3", "gnd": "GND" },
            "params": { "c_out": "47uF" },
            "deviations": { "c_out": "low-ESR polymer part qualified for this LDO" }
        }),
    );
    let source = placed["source"].as_str().unwrap();
    let report = call("synth_validate", &json!({ "source": source }));
    let diagnostics = report["diagnostics"].as_array().expect("diagnostics");
    let listing = diagnostics
        .iter()
        .find(|d| d["code"] == "E-SYNTH-MODULE-009")
        .unwrap_or_else(|| panic!("{report:#}"));
    assert!(
        listing["found"]
            .as_str()
            .unwrap()
            .contains("low-ESR polymer"),
        "{listing}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|d| d["severity"] != "error" && d["severity"] != "fatal"),
        "{report:#}"
    );

    // The review packet lists the same deviation on its own. It renders the
    // sheet, which needs kicad-cli.
    if which("kicad-cli") {
        let review = call(
            "synth_review_schematic",
            &json!({ "source": source, "width_px": 400, "inline": false }),
        );
        let deviations = review["module_deviations"].as_array().expect("deviations");
        assert_eq!(deviations.len(), 1, "{review:#}");
        assert_eq!(deviations[0]["instance"], "LDO1");
        assert_eq!(deviations[0]["param"], "c_out");
    }
}

fn which(binary: &str) -> bool {
    std::process::Command::new(binary)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn a_fixed_override_without_a_reason_is_refused() {
    let err = synth_mcp::call_tool(
        "synth_instantiate_module",
        &json!({
            "module_id": "ldo_ams1117_3v3",
            "label": "LDO1",
            "source": "board \"b\" {\n}\n",
            "bindings": { "vin": "VIN", "vout": "+3V3", "gnd": "GND" },
            "params": { "c_out": "47uF" }
        }),
        Some(&registry()),
    )
    .expect_err("refused");
    assert!(err.contains("fixed by the reference"), "{err}");
}
