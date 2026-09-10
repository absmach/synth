// SPDX-License-Identifier: Apache-2.0
//! Schematic sub-circuit motif recognition (§7.8.4 of synth_implementation_plan.md).
//!
//! Each `Pattern` impl is one recognition pass over the board. Passes
//! run in a fixed order (see `build_clusters` in `lib.rs`); each pass
//! claims the components it matches into the shared `claimed` set so
//! later passes never steal them. Order is significant and must not
//! change: LED indicator -> USB+ESD -> LDO block -> I2C bus -> crystal
//! -> IC block -> divider -> singleton.

use std::collections::HashSet;
use synth_ir::{Board, ComponentId};

use crate::Cluster;

pub(crate) trait Pattern {
    /// Find and claim every instance of this pattern in `board`,
    /// inserting matched component ids into `claimed`. Must not
    /// examine or claim components already present in `claimed`.
    fn recognize(board: &Board, claimed: &mut HashSet<ComponentId>) -> Vec<Cluster>;
}

pub(crate) mod crystal;
pub(crate) mod divider;
pub(crate) mod i2c_bus;
pub(crate) mod ic_block;
pub(crate) mod ldo_block;
pub(crate) mod led_indicator;
pub(crate) mod singleton;
pub(crate) mod usb_esd;
