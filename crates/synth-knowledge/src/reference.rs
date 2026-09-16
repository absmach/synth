// SPDX-License-Identifier: Apache-2.0

//! Verified reference circuits: per-part typical-application data.
//!
//! The knowledge graph (`circuits.toml`) encodes *generic* support
//! circuits (every switch needs debounce, every LED needs current
//! limiting). This module encodes *part-specific* ones: the AMS1117
//! wants 10 µF bulk on `vin`/`vout`, the STM32F103C8 wants 100 nF on
//! each supply pin plus a 10k/100nF reset network. Each entry carries
//! provenance (`source`) so an agent or reviewer can trace every
//! value back to a datasheet or app note.
//!
//! Entries live in `knowledge/reference/*.toml` (one file per part,
//! each holding a `[[circuit]]` array) and are embedded into the
//! binary with `include_dir!`, so agents querying through MCP work
//! without a workspace checkout. V1 is a read catalog
//! ([`ReferenceLib::get`]); diffing a board against its parts'
//! reference circuits is the follow-up.

use serde::Deserialize;

/// One support component inside a reference circuit: a role (not a
/// refdes), the component kind and value, placement guidance, and
/// the two nets (IC pin first, rail second) it spans.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SupportRef {
    pub designator: String,
    pub kind: String,
    pub value: String,
    #[serde(default)]
    pub placement: String,
    pub connect: [String; 2],
}

/// A part-specific typical-application circuit.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReferenceCircuit {
    pub part_id: String,
    pub title: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub support: Vec<SupportRef>,
}

/// The parsed reference-circuit catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceLib {
    circuits: Vec<ReferenceCircuit>,
}

impl ReferenceLib {
    fn from_toml_str(toml_src: &str) -> Result<Vec<ReferenceCircuit>, toml::de::Error> {
        #[derive(Deserialize)]
        struct File {
            #[serde(rename = "circuit")]
            circuits: Vec<ReferenceCircuit>,
        }
        Ok(toml::from_str::<File>(toml_src)?.circuits)
    }

    /// The seed catalog compiled into the binary.
    pub fn embedded() -> Self {
        static EMBEDDED: std::sync::OnceLock<ReferenceLib> = std::sync::OnceLock::new();
        EMBEDDED
            .get_or_init(|| {
                let dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/knowledge/reference");
                let mut circuits = Vec::new();
                for file in dir.files() {
                    let is_toml = file.path().extension().is_some_and(|ext| ext == "toml");
                    if !is_toml {
                        continue;
                    }
                    let src = std::str::from_utf8(file.contents())
                        .unwrap_or_else(|_| panic!("reference {:?} must be UTF-8", file.path()));
                    let mut parsed = Self::from_toml_str(src).unwrap_or_else(|e| {
                        panic!("reference {:?} must always parse: {e}", file.path())
                    });
                    circuits.append(&mut parsed);
                }
                assert!(
                    !circuits.is_empty(),
                    "seed reference catalog must not be empty"
                );
                Self { circuits }
            })
            .clone()
    }

    /// The reference circuit for a registry part id, if catalogued.
    pub fn get(&self, part_id: &str) -> Option<&ReferenceCircuit> {
        self.circuits.iter().find(|c| c.part_id == part_id)
    }

    /// Every catalogued part id, in file order.
    pub fn part_ids(&self) -> Vec<&str> {
        self.circuits.iter().map(|c| c.part_id.as_str()).collect()
    }

    pub fn len(&self) -> usize {
        self.circuits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.circuits.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_loads_seed_circuits() {
        let lib = ReferenceLib::embedded();
        assert!(lib.len() >= 2, "ams1117 + stm32f103c8 seeds");
        assert!(lib.get("ams1117_3v3").is_some());
        assert!(lib.get("stm32f103c8").is_some());
        assert!(lib.get("no_such_part").is_none());
    }

    #[test]
    fn ams1117_reference_carries_bulk_values() {
        let lib = ReferenceLib::embedded();
        let circuit = lib.get("ams1117_3v3").cloned().unwrap();
        assert!(!circuit.source.is_empty(), "provenance required");
        assert!(circuit.support.len() >= 2);
        let values: Vec<&str> = circuit.support.iter().map(|s| s.value.as_str()).collect();
        assert!(values.contains(&"10u"), "{values:?}");
    }

    #[test]
    fn every_support_entry_is_complete() {
        let lib = ReferenceLib::embedded();
        for circuit in &lib.circuits {
            assert!(!circuit.part_id.is_empty());
            assert!(!circuit.title.is_empty());
            for s in &circuit.support {
                assert!(!s.designator.is_empty(), "{circuit:?}");
                assert!(!s.kind.is_empty(), "{circuit:?}");
                assert!(!s.value.is_empty(), "{circuit:?}");
                assert!(
                    !s.connect[0].is_empty() && !s.connect[1].is_empty(),
                    "{circuit:?}"
                );
            }
        }
    }

    #[test]
    fn part_ids_lists_seed_parts() {
        let lib = ReferenceLib::embedded();
        let ids = lib.part_ids();
        assert!(ids.contains(&"ams1117_3v3"));
        assert!(ids.contains(&"stm32f103c8"));
    }
}
