// SPDX-License-Identifier: Apache-2.0

//! Topological bus bundle routing engine (`crates/synth-route/src/bus.rs`).
//!
//! Inspired by `enriver` / `pcbflow` algorithms:
//! Groups parallel signal buses (data/address lines, high-speed SPI/SDRAM lines)
//! into uniform-pitch parallel trace trunks with `shuffle` via-field reordering.

use std::collections::BTreeMap;
use synth_ir::{Board, NetId};

/// A multi-net parallel bus bundle definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusBundle {
    pub name: String,
    pub net_ids: Vec<NetId>,
}

/// Identify multi-net parallel bus bundles in `board`.
///
/// Matches nets by common prefix patterns (e.g. `d0`..`d7`, `addr0`..`addr15`, `spi_*`).
pub fn identify_bus_bundles(board: &Board) -> Vec<BusBundle> {
    let mut groups: BTreeMap<String, Vec<NetId>> = BTreeMap::new();

    for net in &board.nets {
        if net.endpoints.len() < 2 {
            continue;
        }
        let lower = net.name.to_ascii_lowercase();

        let prefix = if lower.starts_with("spi_") {
            "spi"
        } else if lower.starts_with("i2c_") {
            "i2c"
        } else if lower.starts_with("sdram_") {
            "sdram"
        } else if lower.starts_with('d') && lower[1..].chars().all(|c| c.is_ascii_digit()) {
            "data_bus"
        } else if lower.starts_with("addr") && lower[4..].chars().all(|c| c.is_ascii_digit()) {
            "address_bus"
        } else {
            continue;
        };

        groups.entry(prefix.to_string()).or_default().push(net.id);
    }

    groups
        .into_iter()
        .filter(|(_, nets)| nets.len() >= 2)
        .map(|(name, net_ids)| BusBundle { name, net_ids })
        .collect()
}

/// Calculate parallel channel offsets for a bus bundle of `count` nets with pitch `pitch_nm`.
pub fn bus_channel_offsets(count: usize, pitch_nm: i64) -> Vec<i64> {
    let total_width = (count.saturating_sub(1)) as i64 * pitch_nm;
    let start_offset = -total_width / 2;
    (0..count)
        .map(|i| start_offset + (i as i64 * pitch_nm))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bus_channel_offsets_symmetry() {
        let offsets = bus_channel_offsets(4, 400_000);
        assert_eq!(offsets.len(), 4);
        assert_eq!(offsets[0], -600_000);
        assert_eq!(offsets[1], -200_000);
        assert_eq!(offsets[2], 200_000);
        assert_eq!(offsets[3], 600_000);
    }
}
