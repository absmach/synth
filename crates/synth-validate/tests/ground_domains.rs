// SPDX-License-Identifier: Apache-2.0

//! `ground_domain` blocks (absmach/synth#464): `E-SYNTH-POWER-016` for a
//! declaration that does not hold together, `E-SYNTH-POWER-017` for a signal
//! that crosses the split with no declared stitching path.
//!
//! The parts are defined here, so the tests do not depend on registry data.

use synth_diagnostics::Diagnostic;

const CHIP: &str = r#"
id = "t_chip"
kind = "ic"

[[pins]]
name = "gnd"
number = "1"
electrical_type = "power_input"

[[pins]]
name = "io"
number = "2"
electrical_type = "bidirectional"

[[pins]]
name = "io2"
number = "3"
electrical_type = "bidirectional"
"#;

const RESISTOR: &str = r#"
id = "t_res"
kind = "resistor"

[[pins]]
name = "p1"
number = "1"
electrical_type = "passive"

[[pins]]
name = "p2"
number = "2"
electrical_type = "passive"
"#;

const CAPACITOR: &str = r#"
id = "t_cap"
kind = "capacitor"

[[pins]]
name = "p1"
number = "1"
electrical_type = "passive"

[[pins]]
name = "p2"
number = "2"
electrical_type = "passive"
"#;

/// U1 on AGND and U2 on DGND share `SCLK`; R1 is the 0 ohm join and C1 a
/// stitching capacitor between the grounds. `domains` is spliced in.
fn run(domains: &str) -> Vec<Diagnostic> {
    let registry = synth_registry::load_from_strs(&[
        ("t_chip.synth.toml", CHIP),
        ("t_res.synth.toml", RESISTOR),
        ("t_cap.synth.toml", CAPACITOR),
    ])
    .expect("test parts must load");
    let src = format!(
        r#"board "t" {{
  layers 2
  component U1: ic "t_chip"
  component U2: ic "t_chip"
  component R1: resistor "t_res" value "0R"
  component C1: capacitor "t_cap" value "1nF"
  net "AGND" {{ U1.gnd R1.p1 C1.p1 }}
  net "DGND" {{ U2.gnd R1.p2 C1.p2 }}
  net "SCLK" {{ U1.io U2.io }}
  net "LOCAL" {{ U1.io2 }}
{domains}
}}"#
    );
    let filename = "ground_domains.synth".to_string();
    let parse = synth_parser::parse(&src, filename.clone());
    assert!(
        parse
            .diagnostics
            .iter()
            .all(|d| !d.code.starts_with("E-SYNTH-PARSE")),
        "{:?}",
        parse.diagnostics
    );
    let ast = parse.ast.as_ref().expect("source must parse");
    let lowered = synth_ir::lower(ast, &registry, &filename);
    let board = lowered.board.as_ref().expect("board must lower");
    synth_validate::run_erc(board, &filename)
        .into_iter()
        .filter(|d| matches!(d.code.as_str(), "E-SYNTH-POWER-016" | "E-SYNTH-POWER-017"))
        .collect()
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn a_board_without_ground_domains_is_not_checked() {
    assert!(run("").is_empty());
}

#[test]
fn joined_domains_with_a_stitched_crossing_are_clean() {
    let d = run(
        "  ground_domain AGND { join R1 crossing SCLK stitch C1 }\n  ground_domain DGND { join R1 }",
    );
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn a_crossing_over_the_join_is_clean() {
    let d = run(
        "  ground_domain AGND { join R1 crossing SCLK stitch join }\n  ground_domain DGND { join R1 }",
    );
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn an_undeclared_crossing_is_a_warning() {
    let d = run("  ground_domain AGND { join R1 }\n  ground_domain DGND { join R1 }");
    assert_eq!(codes(&d), vec!["E-SYNTH-POWER-017"], "{d:?}");
    assert!(
        d[0].found.as_deref().is_some_and(|f| f.contains("SCLK")),
        "{d:?}"
    );
}

#[test]
fn declared_domains_with_no_join_are_unjoined() {
    let d = run("  ground_domain AGND { crossing SCLK stitch C1 }\n  ground_domain DGND { }");
    assert_eq!(
        codes(&d),
        vec!["E-SYNTH-POWER-016", "E-SYNTH-POWER-016"],
        "{d:?}"
    );
}

#[test]
fn a_join_that_does_not_bridge_two_grounds_is_an_error() {
    let d = run("  ground_domain AGND { join U1 crossing SCLK stitch join }");
    assert!(codes(&d).contains(&"E-SYNTH-POWER-016"), "{d:?}");
}

#[test]
fn a_capacitor_between_the_grounds_is_not_a_join() {
    // C1 touches both grounds, but a capacitor is a stitch, not a DC join.
    let d = run("  ground_domain AGND { join C1 }\n  ground_domain DGND { join C1 }");
    assert!(
        d.iter()
            .any(|d| d.code == "E-SYNTH-POWER-016"
                && d.title.contains("not a net tie or a 0 ohm link")),
        "{d:?}"
    );
}

#[test]
fn a_second_join_is_a_parse_error() {
    let parse = synth_parser::parse(
        r#"board "t" { layers 2 ground_domain AGND { join R1 join R2 } }"#,
        "t.synth".to_string(),
    );
    assert!(
        parse
            .diagnostics
            .iter()
            .any(|d| d.code == "E-SYNTH-PARSE-048" && d.title.contains("one `join`")),
        "{:?}",
        parse.diagnostics
    );
}

#[test]
fn a_domain_on_a_missing_or_non_ground_net_is_an_error() {
    assert_eq!(
        codes(&run("  ground_domain NOPE { }")),
        vec!["E-SYNTH-POWER-016"]
    );
    assert_eq!(
        codes(&run("  ground_domain SCLK { }")),
        vec!["E-SYNTH-POWER-016"]
    );
}

#[test]
fn a_stitch_that_is_not_between_the_grounds_is_an_error() {
    let d = run(
        "  ground_domain AGND { join R1 crossing SCLK stitch U2 }\n  ground_domain DGND { join R1 }",
    );
    assert!(codes(&d).contains(&"E-SYNTH-POWER-016"), "{d:?}");
}

#[test]
fn the_block_lowers_into_the_board() {
    let registry = synth_registry::load_from_strs(&[
        ("t_chip.synth.toml", CHIP),
        ("t_res.synth.toml", RESISTOR),
        ("t_cap.synth.toml", CAPACITOR),
    ])
    .expect("test parts must load");
    let src = r#"board "t" {
  layers 2
  component U1: ic "t_chip"
  component R1: resistor "t_res" value "0R"
  net "AGND" { U1.gnd R1.p1 }
  ground_domain "AGND" { join R1 crossing SCLK stitch join crossing SDO stitch C9 }
}"#;
    let parse = synth_parser::parse(src, "t.synth".to_string());
    let ast = parse.ast.as_ref().expect("parses");
    let lowered = synth_ir::lower(ast, &registry, "t.synth");
    let board = lowered.board.as_ref().expect("lowers");
    let [domain] = board.ground_domains.as_slice() else {
        panic!("one domain: {:?}", board.ground_domains);
    };
    assert_eq!(domain.net, "AGND");
    assert_eq!(domain.join.as_deref(), Some("R1"));
    assert_eq!(domain.crossings.len(), 2);
    assert_eq!(domain.crossings[0].stitch, synth_ir::GroundStitch::Join);
    assert_eq!(
        domain.crossings[1].stitch,
        synth_ir::GroundStitch::Part("C9".to_string())
    );
}

#[test]
fn a_malformed_block_is_a_parse_error() {
    let parse = synth_parser::parse(
        r#"board "t" { layers 2 ground_domain AGND { crossing SCLK join } }"#,
        "t.synth".to_string(),
    );
    assert!(
        parse
            .diagnostics
            .iter()
            .any(|d| d.code == "E-SYNTH-PARSE-048"),
        "{:?}",
        parse.diagnostics
    );
}
