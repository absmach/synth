// SPDX-License-Identifier: Apache-2.0
//! Pass 8: every still-unclaimed component becomes a singleton.

use std::collections::HashSet;
use synth_ir::{Board, ComponentId};

use super::Pattern;
use crate::{Cluster, ClusterKind};

pub(crate) struct Singleton;

impl Pattern for Singleton {
    fn recognize(board: &Board, claimed: &mut HashSet<ComponentId>) -> Vec<Cluster> {
        let mut clusters: Vec<Cluster> = Vec::new();
        for component in &board.components {
            if claimed.contains(&component.id) {
                continue;
            }
            clusters.push(Cluster {
                kind: ClusterKind::Singleton,
                anchor: component.id,
                anchor_vertical: false,
                members: Vec::new(),
            });
            claimed.insert(component.id);
        }
        clusters
    }
}
