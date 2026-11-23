// SPDX-License-Identifier: Apache-2.0

//! Bounding a list of findings in a tool result.
//!
//! A routed 77-part board produced 93 `E-SYNTH-SCHEM-013` findings and a
//! 104,000-character export result, which overflowed the agent's tool-result
//! limit and was never read. A result carries a bounded, fair sample of the
//! findings, the count of every code, and how many were left out; the whole list
//! is written to a file the caller can read in parts.

use std::collections::{BTreeMap, VecDeque};

use serde_json::Value;
use synth_diagnostics::{Diagnostic, EntityRef};

/// Findings of one code kept in a result.
pub const PER_CODE: usize = 5;

/// Findings of all codes kept in a result.
pub const MAX_TOTAL: usize = 40;

/// Serialized bytes of the findings kept in a result. A diagnostic can run to a
/// thousand characters, so a count alone does not bound the payload.
pub const MAX_BYTES: usize = 24_000;

/// A bounded sample of findings.
#[derive(Debug, Clone, PartialEq)]
pub struct Capped {
    /// The findings kept, in the order the rules emitted them.
    pub kept: Vec<Value>,
    /// How many findings there are of each code, whatever was kept.
    pub counts: BTreeMap<String, usize>,
    /// How many findings were left out of `kept`.
    pub omitted: usize,
}

/// What a finding is about: the entity it names, or nothing. Findings are spread
/// across these so that a code reported on many parts, or on several sheets, is
/// sampled across them and not from the first alone.
fn subject(diagnostic: &Diagnostic) -> String {
    match &diagnostic.primary_entity {
        Some(EntityRef::Component { id }) => format!("component {id}"),
        Some(EntityRef::Pin { component, pin }) => format!("pin {component}.{pin}"),
        Some(EntityRef::Net { name }) => format!("net {name}"),
        Some(EntityRef::Constraint { id }) => format!("constraint {id}"),
        Some(EntityRef::Module { name }) => format!("module {name}"),
        None => String::new(),
    }
}

/// A sample of `diagnostics`: at most `per_code` of each code, `max_total` in
/// all and `max_bytes` serialized. Within a code the sample takes one finding
/// from each subject in turn; across codes it takes one from each code in turn,
/// so no code is left out while another has a second finding. One pass over the
/// list builds the counts and the groups.
#[must_use]
pub fn cap(
    diagnostics: &[Diagnostic],
    per_code: usize,
    max_total: usize,
    max_bytes: usize,
) -> Capped {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    // code -> subject -> indices, in emission order.
    let mut groups: BTreeMap<&str, BTreeMap<String, VecDeque<usize>>> = BTreeMap::new();
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        *counts.entry(diagnostic.code.clone()).or_insert(0) += 1;
        groups
            .entry(diagnostic.code.as_str())
            .or_default()
            .entry(subject(diagnostic))
            .or_default()
            .push_back(index);
    }

    // Each code's own order: one per subject in turn, up to `per_code`.
    let mut by_code: Vec<VecDeque<usize>> = Vec::with_capacity(groups.len());
    for subjects in groups.values_mut() {
        let mut order = VecDeque::new();
        while order.len() < per_code && subjects.values().any(|queue| !queue.is_empty()) {
            for queue in subjects.values_mut() {
                if order.len() < per_code
                    && let Some(index) = queue.pop_front()
                {
                    order.push_back(index);
                }
            }
        }
        by_code.push(order);
    }

    // Across codes: one from each in turn, within the total and the bytes.
    let mut chosen: Vec<(usize, Value)> = Vec::new();
    let mut bytes = 0_usize;
    'rounds: loop {
        let mut took = false;
        for order in &mut by_code {
            let Some(index) = order.pop_front() else {
                continue;
            };
            let value = serde_json::json!(&diagnostics[index]);
            let size = value.to_string().len();
            if chosen.len() >= max_total || (bytes + size > max_bytes && !chosen.is_empty()) {
                break 'rounds;
            }
            bytes += size;
            chosen.push((index, value));
            took = true;
        }
        if !took {
            break;
        }
    }
    chosen.sort_by_key(|(index, _)| *index);
    let kept: Vec<Value> = chosen.into_iter().map(|(_, value)| value).collect();
    let omitted = diagnostics.len() - kept.len();
    Capped {
        kept,
        counts,
        omitted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_diagnostics::{DiagnosticBuilder, Severity};

    fn finding(code: &str, component: Option<&str>) -> Diagnostic {
        let builder = DiagnosticBuilder::new(code, Severity::Warning, "t");
        match component {
            Some(id) => builder
                .primary_entity(EntityRef::Component { id: id.to_string() })
                .build(),
            None => builder.build(),
        }
    }

    #[test]
    fn each_code_is_capped_and_every_code_is_counted() {
        let mut found: Vec<_> = (0..8).map(|_| finding("E-SYNTH-SCHEM-013", None)).collect();
        found.insert(2, finding("E-SYNTH-SCHEM-001", None));
        let capped = cap(&found, 5, 100, 1_000_000);
        assert_eq!(capped.kept.len(), 6, "five of -013 and the one -001");
        assert_eq!(
            capped.kept[2]["code"], "E-SYNTH-SCHEM-001",
            "emission order"
        );
        assert_eq!(capped.counts["E-SYNTH-SCHEM-013"], 8);
        assert_eq!(capped.counts["E-SYNTH-SCHEM-001"], 1);
        assert_eq!(capped.omitted, 3);
    }

    #[test]
    fn a_complete_list_reports_nothing_omitted() {
        let found = vec![finding("E-SYNTH-SCHEM-013", None); 3];
        let capped = cap(&found, 5, 100, 1_000_000);
        assert_eq!(capped.kept.len(), 3);
        assert_eq!(capped.omitted, 0);
    }

    #[test]
    fn a_code_is_sampled_across_the_parts_it_names_not_from_the_first() {
        // Nine findings on U1 and then one each on U2 and U3: the first five in
        // emission order would all be U1's.
        let mut found: Vec<_> = (0..9)
            .map(|_| finding("E-SYNTH-SCHEM-013", Some("U1")))
            .collect();
        found.push(finding("E-SYNTH-SCHEM-013", Some("U2")));
        found.push(finding("E-SYNTH-SCHEM-013", Some("U3")));
        let capped = cap(&found, 5, 100, 1_000_000);
        let subjects: Vec<String> = capped
            .kept
            .iter()
            .map(|v| v["primary_entity"]["id"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(subjects.len(), 5);
        assert!(subjects.contains(&"U2".to_string()), "{subjects:?}");
        assert!(subjects.contains(&"U3".to_string()), "{subjects:?}");
    }

    #[test]
    fn the_total_is_bounded_and_no_code_is_starved_by_another() {
        // Twenty codes, ten findings each: the total cap of 12 takes one from
        // twelve different codes before a second from any.
        let found: Vec<_> = (0..20)
            .flat_map(|c| (0..10).map(move |_| finding(&format!("E-SYNTH-SCHEM-{c:03}"), None)))
            .collect();
        let capped = cap(&found, 5, 12, 1_000_000);
        assert_eq!(capped.kept.len(), 12);
        let codes: std::collections::BTreeSet<_> = capped
            .kept
            .iter()
            .map(|v| v["code"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(codes.len(), 12, "one finding from each of twelve codes");
        assert_eq!(capped.counts.len(), 20, "every code is still counted");
        assert_eq!(capped.omitted, 200 - 12);
    }

    #[test]
    fn the_serialized_size_is_bounded() {
        let long = "x".repeat(900);
        let found: Vec<_> = (0..30)
            .map(|i| {
                DiagnosticBuilder::new(format!("E-SYNTH-SCHEM-{i:03}"), Severity::Warning, "t")
                    .message(long.clone())
                    .build()
            })
            .collect();
        let capped = cap(&found, 5, 100, 5_000);
        let size: usize = capped.kept.iter().map(|v| v.to_string().len()).sum();
        assert!(size <= 5_000, "{size} bytes");
        assert!(!capped.kept.is_empty(), "something is always returned");
        assert_eq!(capped.omitted, 30 - capped.kept.len());
    }
}
