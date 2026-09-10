// SPDX-License-Identifier: Apache-2.0

//! Component registry: part definitions for SynthSpec.
//!
//! A *part* is a concrete electronic component identified by a string
//! id (`rp2350`, `atecc608`, `c_generic_0402`). Each part declares its
//! kind (`mcu`, `secure_element`, `capacitor`), its pins with
//! electrical types and semantic capabilities, and optional
//! required-support-circuitry metadata.
//!
//! The format is hand-authored TOML; see `registry/parts/**/*.synth.toml`
//! for the canonical examples. The registry is intentionally
//! clean-slate rather than referencing upstream KiCad libraries —
//! every byte that flows through the Synth compiler must be owned and
//! understood by Synth, per the compiler-centric thesis.
//!
//! Phase 3 scope: seed corpus + loader + structural validation +
//! lookup API used by `synth-validate`. Footprint geometry and
//! schematic symbol drawing are deferred to later phases (the registry
//! today carries only the *logical* model — pin names, numbers, types,
//! capabilities — not the geometric one).

#![forbid(unsafe_code)]

mod capability;
mod kicad_pin_check;
mod loader;
mod part;
mod registry;
pub mod vector_search;

pub use capability::{ElectricalType, PinCapability};
pub use loader::{
    create_part_stub, embedded_registry, load_dir, load_from_strs, load_tiered, load_user_overlay,
    shipped_registry_dir, user_registry_dir, write_embedded_seed, LoadError, LoadResult,
    LoadWarning,
};
pub use part::{
    Lifecycle, Part, PartId, Pin, PinNumber, Provenance, ProvenanceSource, RequiredDecoupling,
};

/// Clean-room EasyEDA → KiCad footprint converter (Phase 15, R15.4).
pub mod easyeda;
pub use easyeda::{
    extract_pins, generate_part_toml, parse_easyeda, to_kicad_mod, EasyEdaComponent,
};
pub use registry::Registry;
pub use vector_search::{VectorSearchIndex, VectorSearchResult};
