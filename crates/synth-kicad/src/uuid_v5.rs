// SPDX-License-Identifier: Apache-2.0

//! Deterministic UUID derivation for KiCad entity identity.
//!
//! KiCad assigns a UUID to every persistent entity (sheets, symbol
//! instances, wires). On re-export of unchanged IR we want the same
//! UUIDs to come back so version-control diffs only show real
//! semantic changes.
//!
//! The strategy: a single per-project *namespace* UUID derived from
//! the board name; every entity's UUID is then `uuid::v5(namespace,
//! "<kind>:<name>")`. UUID v5 is SHA-1-based and deterministic for a
//! given (namespace, name) pair.

use uuid::Uuid;

/// Root namespace from which the per-project namespace is derived.
/// Hard-coded random v4 UUID; treat as a constant for backwards
/// compatibility of generated files.
const SYNTH_ROOT_NAMESPACE: Uuid = Uuid::from_bytes([
    0x5b, 0xb2, 0x3d, 0x5e, 0x4f, 0x9c, 0x4f, 0x47, 0x9a, 0xc1, 0xea, 0x9c, 0x4f, 0x2b, 0x4a, 0x7e,
]);

/// Per-project namespace derived from the board name. Pass the
/// returned UUID into [`derive_entity_uuid`] for every entity in the
/// export.
pub fn project_namespace(board_name: &str) -> Uuid {
    Uuid::new_v5(&SYNTH_ROOT_NAMESPACE, board_name.as_bytes())
}

/// Derive a UUID for one entity. `kind` is a short label such as
/// `"sheet"`, `"symbol"`, `"wire"`, `"lib_symbol"`. `name` is a
/// stable identifier within that kind — for symbols it's the
/// refdes; for wires, the net name plus endpoint indices; for
/// sheets, `"root"`.
pub fn derive_entity_uuid(project: &Uuid, kind: &str, name: &str) -> Uuid {
    let mut buf = String::with_capacity(kind.len() + 1 + name.len());
    buf.push_str(kind);
    buf.push(':');
    buf.push_str(name);
    Uuid::new_v5(project, buf.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_produce_same_uuid() {
        let p = project_namespace("hello");
        let a = derive_entity_uuid(&p, "symbol", "U1");
        let b = derive_entity_uuid(&p, "symbol", "U1");
        assert_eq!(a, b);
    }

    #[test]
    fn different_kinds_diverge() {
        let p = project_namespace("hello");
        let a = derive_entity_uuid(&p, "symbol", "U1");
        let b = derive_entity_uuid(&p, "wire", "U1");
        assert_ne!(a, b);
    }

    #[test]
    fn different_projects_diverge() {
        let p1 = project_namespace("hello");
        let p2 = project_namespace("world");
        let a = derive_entity_uuid(&p1, "symbol", "U1");
        let b = derive_entity_uuid(&p2, "symbol", "U1");
        assert_ne!(a, b);
    }

    #[test]
    fn root_namespace_is_stable() {
        // Pin the root namespace string so changes are caught by review.
        assert_eq!(
            SYNTH_ROOT_NAMESPACE.to_string(),
            "5bb23d5e-4f9c-4f47-9ac1-ea9c4f2b4a7e",
        );
    }
}
