// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use crate::part::{Part, PartId};

/// A loaded, validated set of part definitions, keyed by [`PartId`].
#[derive(Debug, Clone, Default)]
pub struct Registry {
    parts: HashMap<PartId, Part>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, part: Part) {
        self.parts.insert(part.id.clone(), part);
    }

    pub fn lookup(&self, id: &str) -> Option<&Part> {
        self.parts.get(&PartId(id.to_string()))
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    pub fn ids(&self) -> impl Iterator<Item = &PartId> {
        self.parts.keys()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&PartId, &Part)> {
        self.parts.iter()
    }
}
