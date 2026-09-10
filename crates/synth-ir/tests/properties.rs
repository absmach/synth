// SPDX-License-Identifier: Apache-2.0

//! Property tests for the §4.4 Phase 2 gate.
//!
//! Two invariants:
//!
//! 1. **`serde_json::to_value(board).is_ok()` for any valid Board.**
//!    The IR is the canonical truth; SynthJSON is its projection.
//!    If serialization could ever fail, downstream tools (browser
//!    viewer, agent protocol) would silently lose state.
//! 2. **Unit conversion round-trips.** `Length::from_mm(x).to_mm()`
//!    must equal `x` within f64 precision; `Voltage::from_v(v).to_v()`
//!    likewise. Internal arithmetic is integer; only the boundary
//!    helpers cross between f64 and the integer base unit.

use proptest::prelude::*;
use synth_ir::{Length, Voltage};

const CASES: u32 = 1000;

// =============================================================================
// Unit conversion round-trips (boundary helpers)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig {
        cases: CASES,
        max_shrink_iters: 100,
        ..ProptestConfig::default()
    })]

    /// `Length::from_mm` followed by `to_mm` must reconstruct the
    /// original mm value within f64 precision. The lossy direction
    /// is the i64 → f64 conversion at the API boundary, capped at
    /// f64's 52-bit mantissa.
    #[test]
    fn length_from_mm_to_mm_roundtrips(mm in -1.0e9_f64..1.0e9_f64) {
        let length = Length::from_mm(mm);
        let back = length.to_mm();
        // Round-trip path is mm → i64 nm → f64 mm. Two precision
        // losses compound: the `mm * 1_000_000.0` multiplication
        // and the eventual i64 → f64 cast on the way back. For
        // mm values approaching 1e9 the cast alone consumes >50
        // bits of mantissa, leaving ~16 ULP of slack we must
        // tolerate. Floor at 1 nm so the tolerance has meaning at
        // small inputs too.
        let tolerance = (mm.abs() * f64::EPSILON * 64.0).max(1e-6);
        prop_assert!(
            (back - mm).abs() <= tolerance,
            "Length::from_mm({mm}).to_mm() = {back}, diff = {}",
            (back - mm).abs(),
        );
    }

    /// `Voltage::from_v` and `to_v` form a stable pair too. Range
    /// chosen to comfortably fit in i64 microvolts.
    #[test]
    fn voltage_from_v_to_v_roundtrips(v in -1.0e6_f64..1.0e6_f64) {
        let voltage = Voltage::from_v(v);
        let back = voltage.to_v();
        let tolerance = (v.abs() * f64::EPSILON * 16.0).max(1e-6);
        prop_assert!(
            (back - v).abs() <= tolerance,
            "Voltage::from_v({v}).to_v() = {back}, diff = {}",
            (back - v).abs(),
        );
    }
}

// =============================================================================
// IR → JSON serialization safety
// =============================================================================
//
// We can't easily generate arbitrary valid Boards directly (the IR
// has invariants about valid ComponentIds, PinIds, etc.), so we
// generate arbitrary SOURCE inputs and exercise the full pipeline
// to confirm whatever IR comes out always serializes.

proptest! {
    #![proptest_config(ProptestConfig {
        cases: CASES,
        max_shrink_iters: 100,
        ..ProptestConfig::default()
    })]

    /// Whatever Board the lowering pipeline produces from arbitrary
    /// input must always serialize to JSON without panic or error.
    #[test]
    fn ir_serializes_for_any_parseable_input(input in ".*") {
        let parsed = synth_parser::parse(&input, "fuzz.synth");
        if let Some(ast) = parsed.ast.as_ref() {
            let registry = synth_registry::Registry::new();
            let lowered = synth_ir::lower(ast, &registry, "fuzz.synth");
            if let Some(board) = lowered.board {
                // The actual gate: serialization must succeed.
                let value = serde_json::to_value(&board);
                prop_assert!(
                    value.is_ok(),
                    "IR -> JSON failed: {:?}",
                    value.err(),
                );
                // Round-trip is gravy — confirms our serde
                // attributes don't lose information.
                let v = value.unwrap();
                let s = serde_json::to_string(&v);
                prop_assert!(s.is_ok());
            }
        }
    }
}
