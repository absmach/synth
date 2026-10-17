// SPDX-License-Identifier: Apache-2.0

//! `synth capability list` states what Synth cannot do or cannot verify,
//! in both the human table and the JSON descriptor.

use std::process::Command;

fn capability_list(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_synth"))
        .args(["capability", "list"])
        .args(args)
        .output()
        .expect("run synth capability list");
    assert!(
        output.status.success(),
        "capability list failed: {output:?}"
    );
    String::from_utf8(output.stdout).expect("utf-8 output")
}

#[test]
fn json_and_human_output_name_every_unsupported_and_unverified_entry() {
    let descriptor: serde_json::Value =
        serde_json::from_str(&capability_list(&["--json"])).expect("descriptor is JSON");
    let human = capability_list(&[]);

    for (section, heading) in [("unsupported", "UNSUPPORTED"), ("unverified", "UNVERIFIED")] {
        let entries = descriptor[section].as_array().expect("section is an array");
        assert!(!entries.is_empty(), "`{section}` must not be empty");
        assert!(
            human.contains(heading),
            "human output lacks the {heading} heading"
        );
        for entry in entries {
            let id = entry["id"].as_str().expect("entry has an id");
            assert!(
                human.contains(id),
                "human output lacks {section} entry {id}"
            );
        }
    }
}

#[test]
fn the_native_router_fixed_rf_width_is_disclosed() {
    let descriptor: serde_json::Value =
        serde_json::from_str(&capability_list(&["--json"])).expect("descriptor is JSON");
    let entry = descriptor["unverified"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "rf_net_trace_width")
        .expect("rf_net_trace_width is listed");
    let summary = entry["summary"].as_str().unwrap();
    assert!(
        summary.contains("0.33") && summary.contains("native router"),
        "{summary}"
    );
}

/// The descriptor is what an agent reads before writing a board, and
/// via-in-pad is the capability a dense board's escape turns on. A
/// descriptor that did not disclose where the decision lives would send an
/// agent to the CLI flag, which is the operator's override rather than the
/// design's declaration.
#[test]
fn the_descriptor_states_where_the_via_in_pad_decision_lives() {
    let descriptor: serde_json::Value =
        serde_json::from_str(&capability_list(&["--json"])).expect("descriptor is JSON");
    let processes = &descriptor["geometry_and_constraints"]["fabrication_processes"];

    assert_eq!(
        processes["declared_by"].as_str(),
        Some("requires <process>"),
        "the descriptor must name the statement a board writes"
    );
    assert!(
        processes["processes"]["via_in_pad"].is_string(),
        "via_in_pad must be described, not merely listed"
    );
    assert!(
        processes["profiles_offering_via_in_pad"]
            .as_array()
            .is_some_and(|tiers| tiers.iter().any(|t| t == "jlc-filled-via")),
        "a board cannot reach the capability without a tier that offers it"
    );
    assert!(
        processes["gate"]
            .as_str()
            .is_some_and(|g| g.contains("E-SYNTH-PROCESS-002")),
        "the descriptor must name the diagnostic that refuses an unmet requirement"
    );
    assert!(
        descriptor["geometry_and_constraints"]["routing_constraints"]["requires"]
            .as_array()
            .is_some_and(|v| v.iter().any(|a| a == "via_in_pad")),
        "`requires` belongs in the statement/attribute list an agent reads"
    );
}
