// SPDX-License-Identifier: Apache-2.0

//! A record of how a board actually routed, and what to try next time.
//!
//! Every routing attempt produces a run record that is thrown away once it
//! has been read. Kept instead, those records are the raw material for a
//! retry that starts smarter: the same board hash, routed before, knows which
//! nets were hardest and how many vias and how much copper each engine used.
//! This is the Dataset 6 hook the route tools already advertise, given a
//! concrete shape.
//!
//! Records are append-only JSON Lines in one file. Nothing here is read
//! during a run unless a caller asks for a suggestion, so the file can be
//! written continuously and pruned freely.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::result::RouteReport;

/// File name the records are appended to inside the configured directory.
pub const OUTCOMES_FILE: &str = "routing-outcomes.jsonl";

/// One routing attempt, reduced to what a later run can learn from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeRecord {
    /// SHA-256 of the board that was routed. Two attempts are comparable
    /// exactly when this matches, which is why it is the key.
    pub board_hash: String,
    pub engine: String,
    pub state: String,
    pub connected_nets: usize,
    pub open_nets: usize,
    pub segments: usize,
    pub vias: usize,
    pub wire_length_mm: f64,
    /// The order this attempt recommends for a retry, hardest first.
    #[serde(default)]
    pub recommended_routing_order: Vec<String>,
    /// Unix seconds, for pruning and for preferring recent attempts.
    pub recorded_at_unix: u64,
}

impl OutcomeRecord {
    /// Reduce a run report to a record.
    #[must_use]
    pub fn from_report(report: &RouteReport) -> Self {
        let connectivity = report.validation.as_ref().map(|v| &v.connectivity);
        Self {
            board_hash: report.provenance.input_hash.clone(),
            engine: report.engine.as_str().to_string(),
            state: report.state.as_str().to_string(),
            connected_nets: connectivity
                .map_or(report.statistics.connected_nets, |c| c.connected_nets),
            open_nets: connectivity.map_or(report.statistics.open_nets, |c| c.open_nets),
            segments: report.statistics.segments,
            vias: report.statistics.vias,
            wire_length_mm: report.statistics.wire_length_mm,
            recommended_routing_order: report.recommended_routing_order.clone(),
            recorded_at_unix: unix_seconds(),
        }
    }

    /// How well this attempt did, for choosing between attempts on one board.
    ///
    /// Fewer open nets is better; ties break on fewer vias then shorter
    /// copper. `open_nets` is the whole point of the dataset, so it leads.
    #[allow(clippy::cast_sign_loss)] // wire_length_mm is a non-negative length
    fn score(&self) -> (usize, usize, u64) {
        (
            self.open_nets,
            self.vias,
            self.wire_length_mm.round().max(0.0) as u64,
        )
    }
}

/// Append one record to `<dir>/routing-outcomes.jsonl`, creating the
/// directory and file if needed.
///
/// # Errors
/// Any I/O failure creating the directory or appending the line.
pub fn append(dir: &Path, record: &OutcomeRecord) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(OUTCOMES_FILE);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(record).unwrap_or_else(|_| "{}".to_string());
    writeln!(file, "{line}")
}

/// The routing order that did best on this board before, if any.
///
/// Reads every record for `board_hash`, keeps the attempt with the fewest
/// open nets, and returns the order *it* recommended — the advice a retry
/// should start from. `None` when the board has not been routed before, or
/// when the best attempt already completed (there is nothing left to
/// reserve routes for, so an empty order is not worth returning).
#[must_use]
pub fn best_known_order(dir: &Path, board_hash: &str) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(dir.join(OUTCOMES_FILE)).ok()?;
    let mut best: Option<OutcomeRecord> = None;
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<OutcomeRecord>(line) else {
            continue;
        };
        if record.board_hash != board_hash {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|current| record.score() < current.score())
        {
            best = Some(record);
        }
    }
    best.map(|record| record.recommended_routing_order)
        .filter(|order| !order.is_empty())
}

/// The records directory a run should use, from the environment.
///
/// `SYNTH_ROUTING_OUTCOMES_DIR` enables logging; unset means the run keeps
/// nothing, so the default is no side effects.
#[must_use]
pub fn configured_dir() -> Option<PathBuf> {
    dir_from(std::env::var_os("SYNTH_ROUTING_OUTCOMES_DIR"))
}

/// The records directory named by a `SYNTH_ROUTING_OUTCOMES_DIR` value.
fn dir_from(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(hash: &str, engine: &str, open: usize, vias: usize, order: &[&str]) -> OutcomeRecord {
        OutcomeRecord {
            board_hash: hash.to_string(),
            engine: engine.to_string(),
            state: "validation_failed".to_string(),
            connected_nets: 11,
            open_nets: open,
            segments: 100,
            vias,
            wire_length_mm: 200.0,
            recommended_routing_order: order.iter().map(|s| (*s).to_string()).collect(),
            recorded_at_unix: 1,
        }
    }

    fn scratch(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("synth-outcomes-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_best_prior_attempt_on_the_same_board_supplies_the_order() {
        let dir = scratch("best");
        append(
            &dir,
            &record("boardA", "freerouting", 5, 30, &["net_1", "net_4"]),
        )
        .unwrap();
        append(
            &dir,
            &record("boardA", "kicad-routing-tools", 1, 12, &["net_9"]),
        )
        .unwrap();
        append(&dir, &record("boardB", "freerouting", 0, 5, &[])).unwrap();

        assert_eq!(
            best_known_order(&dir, "boardA"),
            Some(vec!["net_9".to_string()]),
            "the attempt with the fewest open nets wins"
        );
        // A different board is not confused with this one.
        assert_eq!(best_known_order(&dir, "boardB"), None);
        // An unknown board has no advice.
        assert_eq!(best_known_order(&dir, "boardC"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_completed_board_offers_no_retry_order() {
        let dir = scratch("complete");
        let mut completed = record("board", "kicad-routing-tools", 0, 4, &[]);
        completed.state = "routed".to_string();
        append(&dir, &completed).unwrap();
        assert_eq!(best_known_order(&dir, "board"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unset_environment_means_no_logging() {
        // The default must be side-effect free; a run only writes a dataset
        // when it was asked to.
        assert!(dir_from(None).is_none());
        assert!(dir_from(Some("".into())).is_none());
        assert_eq!(
            dir_from(Some("/tmp/outcomes".into())),
            Some(PathBuf::from("/tmp/outcomes"))
        );
    }
}
