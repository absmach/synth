// SPDX-License-Identifier: Apache-2.0

//! Per-stage placement diagnostics for the export placement path.
//!
//! `SYNTH_PLACE_STAGE_TRACE=1` writes `stage_trace.tsv` in the working
//! directory. Any other non-empty value is the trace path. Off by default, and
//! the writer only reads the placement: it never moves a part or changes which
//! candidate wins.

use std::io::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use synth_geometry::nm_to_mm;
use synth_ir::Board;

use crate::{Placement, judge};

const ENV: &str = "SYNTH_PLACE_STAGE_TRACE";

/// Whether the stage trace is on.
pub(crate) fn enabled() -> bool {
    std::env::var_os(ENV).is_some_and(|value| !value.is_empty() && value != "0")
}

fn path() -> PathBuf {
    match std::env::var_os(ENV).and_then(|value| value.into_string().ok()) {
        Some(value) if value != "1" => PathBuf::from(value),
        _ => PathBuf::from("stage_trace.tsv"),
    }
}

/// Append a stage's duration without the judge columns.
///
/// The full row calls the judge, which is itself a placement measurement; a
/// pass that calls the judge many times would be measured twice. Writes
/// `<trace>.times.tsv` next to the stage trace.
pub(crate) fn record_time(stage: &str, elapsed: Duration) {
    if !enabled() {
        return;
    }
    let path = path().with_extension("times.tsv");
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    if file.metadata().map_or(0, |meta| meta.len()) == 0 {
        let _ = file.write_all(b"stage\tstage_s\n");
    }
    let row = format!("{stage}\t{:.3}\n", elapsed.as_secs_f64());
    let _ = file.write_all(row.as_bytes());
}

/// Append one judge row for the placement as it stands after `stage`.
pub(crate) fn record(stage: &str, board: &Board, placement: &Placement, elapsed: Duration) {
    if !enabled() {
        return;
    }
    let report = judge(board, placement);
    let metric = |key: &str| {
        report
            .metrics
            .iter()
            .find(|metric| metric.key == key)
            .map(|metric| (metric.measured, metric.failures))
    };
    let number = |value: Option<(f64, usize)>, digits: usize| match value {
        Some((measured, _)) => format!("{measured:.digits$}"),
        None => "-".to_string(),
    };
    let failures = |key: &str| metric(key).map_or_else(|| "-".to_string(), |(_, n)| n.to_string());
    let outline = placement.board_outline;
    let row = format!(
        "{stage}\t{:.3}\t{:.3}\t{:.3}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\n",
        nm_to_mm(outline.width_nm()) * nm_to_mm(outline.height_nm()),
        nm_to_mm(outline.width_nm()),
        nm_to_mm(outline.height_nm()),
        number(metric("usb_offset"), 4),
        number(metric("docking"), 4),
        failures("docking"),
        number(metric("alignment"), 4),
        number(metric("ring"), 4),
        failures("group_gap"),
        failures("overlaps"),
        number(metric("hole_corner"), 4),
        elapsed.as_secs_f64(),
    );
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path())
    else {
        return;
    };
    if file.metadata().map_or(0, |meta| meta.len()) == 0 {
        let _ = file.write_all(
            b"stage\toutline_area\twidth_mm\theight_mm\tusb_offset_mm\tdock\tdock_failures\talignment\tring\tgroup_gap_failures\toverlaps\thole_corner_mm\tstage_s\n",
        );
    }
    let _ = file.write_all(row.as_bytes());
}
