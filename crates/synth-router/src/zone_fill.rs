// SPDX-License-Identifier: Apache-2.0

//! Deliver the routed board with its pour zones filled.
//!
//! A router writes zones as outlines only, so KiCad opens the board with
//! every plane unfilled and shows each pad that joins through one as an
//! unrouted connection until the user presses B. The refill that
//! [`crate::drc::run_detailed`] performs in memory is saved here, and only
//! its `filled_polygon` copper is copied into the router's board.
//!
//! KiCad's own saved file is deliberately not delivered. It rewrites the
//! net table and drops the board-level design settings the exporter relies
//! on, so a board that validated clean would open with different rules.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;

use crate::drc::{first_line, DRC_BUDGET, DRC_OUTPUT_CAP};
use crate::pcb_read::PcbBoard;

/// Fill the pour zones of the board at `board_path`, in place.
///
/// A board with no fill-enabled zone is left untouched. Any fills the board
/// already carries are discarded and recomputed, since they may be partial
/// or predate the final routing. On any failure the board is also left
/// untouched and the reason is returned: an unfilled board is a nuisance, a
/// lost routed board is not.
///
/// # Errors
/// Returns why the board could not be filled and was left as it was.
pub fn refill_zones(board_path: &Path) -> Result<(), String> {
    match filled_text(board_path)? {
        Some(filled) => install(board_path, &filled),
        None => Ok(()),
    }
}

/// The board at `board_path` with its pour zones filled, without writing it.
///
/// `None` for a board with no fill-enabled zone. Fails for the reasons
/// [`refill_zones`] does, and leaves the file as it was in every case.
///
/// # Errors
/// Returns why the board could not be filled.
pub fn filled_text(board_path: &Path) -> Result<Option<String>, String> {
    let text = std::fs::read_to_string(board_path)
        .map_err(|e| format!("could not read {}: {e}", board_path.display()))?;
    PcbBoard::parse(&text).map_err(|e| e.to_string())?;
    let pours = forms(&text, "zone", 1)
        .iter()
        .any(|range| pours_copper(&text[range.clone()]));
    if !pours {
        return Ok(None);
    }
    if let Some((_, detail)) =
        synth_drc::kicad_cli::version_rejection(synth_drc::kicad_cli::version().as_deref())
    {
        return Err(detail);
    }

    // KiCad saves over its input, so it works on a copy.
    let scratch = synth_drc::kicad_cli::ScratchFile::reserve("synth_zone_fill_board", "kicad_pcb");
    let report = synth_drc::kicad_cli::ScratchFile::reserve("synth_zone_fill_report", "json");
    std::fs::copy(board_path, scratch.path())
        .map_err(|e| format!("could not stage the board for kicad-cli: {e}"))?;
    let args: Vec<String> = [
        "pcb",
        "drc",
        "--refill-zones",
        "--save-board",
        "--format",
        "json",
        "--output",
    ]
    .iter()
    .map(ToString::to_string)
    .chain([report.arg(), scratch.arg()])
    .collect();
    let cli = synth_drc::kicad_cli::binary();
    let invocation = crate::process::run(Path::new(&cli), &args, DRC_BUDGET, DRC_OUTPUT_CAP)
        .map_err(|e| e.detail)?;
    if !invocation.succeeded() {
        return Err(format!(
            "kicad-cli pcb drc --save-board did not complete: {}",
            first_line(&invocation.combined_output())
        ));
    }

    let saved = std::fs::read_to_string(scratch.path())
        .map_err(|e| format!("kicad-cli saved no board: {e}"))?;
    let filled = graft_fills(&text, &saved)?;
    PcbBoard::parse(&filled).map_err(|e| format!("the filled board does not parse: {e}"))?;
    Ok(Some(filled))
}

/// Replace `board_path` with `contents` through a staged file, which is
/// removed if the replacement fails.
fn install(board_path: &Path, contents: &str) -> Result<(), String> {
    let staged = board_path.with_extension("zonefill.tmp");
    std::fs::write(&staged, contents)
        .and_then(|()| std::fs::rename(&staged, board_path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&staged);
            format!("could not install the filled board: {e}")
        })
}

/// Copy each zone's `filled_polygon` forms from `saved` into the zone of
/// `text` that has the same uuid, replacing any fills `text` already had and
/// adding nothing else to it.
///
/// All or nothing: every zone of `text` that pours copper must come back
/// filled, and a save that fills fewer than that is an error. A half-filled board
/// shows some planes connected and others not, which is harder to notice
/// than an unfilled one.
fn graft_fills(text: &str, saved: &str) -> Result<String, String> {
    let text = strip_fills(text);
    let text = text.as_str();
    let mut fills: HashMap<&str, Vec<&str>> = HashMap::new();
    for range in forms(saved, "zone", 1) {
        let zone = &saved[range];
        if let Some(uuid) = uuid_of(zone) {
            let polygons = forms(zone, "filled_polygon", 1);
            fills.insert(uuid, polygons.into_iter().map(|r| &zone[r]).collect());
        }
    }

    let zones = forms(text, "zone", 1);
    let expected = zones
        .iter()
        .filter(|range| pours_copper(&text[(*range).clone()]))
        .count();
    let mut seen = HashSet::new();
    if let Some(uuid) = zones
        .iter()
        .filter_map(|range| uuid_of(&text[range.clone()]))
        .find(|uuid| !seen.insert(*uuid))
    {
        return Err(format!(
            "zone uuid {uuid} appears twice in the routed board"
        ));
    }

    let mut out = String::with_capacity(text.len() * 2);
    let mut cursor = 0;
    let mut grafted = 0;
    for range in zones {
        let Some(polygons) = uuid_of(&text[range.clone()])
            .and_then(|uuid| fills.get(uuid))
            .filter(|polygons| !polygons.is_empty())
        else {
            continue;
        };
        // Insert before the zone's closing paren, ahead of the whitespace
        // that already precedes it, so only the new forms differ.
        let close = range.end - 1;
        let body_end = cursor.max(text[..close].trim_end().len());
        out.push_str(&text[cursor..body_end]);
        for polygon in polygons {
            out.push_str("\n\t\t");
            out.push_str(polygon);
        }
        out.push_str(&text[body_end..close]);
        cursor = close;
        grafted += 1;
    }
    if grafted < expected {
        return Err(format!(
            "kicad-cli filled {grafted} of {expected} zones; the board is left unfilled"
        ));
    }
    out.push_str(&text[cursor..]);
    Ok(out)
}

/// `text` without the `filled_polygon` forms inside its zones, and without
/// the whitespace that led up to each.
fn strip_fills(text: &str) -> String {
    let mut stale = Vec::new();
    for zone in forms(text, "zone", 1) {
        for polygon in forms(&text[zone.clone()], "filled_polygon", 1) {
            stale.push(zone.start + polygon.start..zone.start + polygon.end);
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for range in stale {
        let from = cursor.max(text[..range.start].trim_end().len());
        out.push_str(&text[cursor..from]);
        cursor = range.end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Whether KiCad will give this zone a `filled_polygon`: an explicit
/// `(fill yes ...)` on a zone that is not a keepout. Rule areas are saved
/// with a bare `(fill ...)` and are never filled.
fn pours_copper(zone: &str) -> bool {
    !zone.contains("(keepout")
        && forms(zone, "fill", 1).first().is_some_and(|range| {
            zone[range.start + "(fill".len()..]
                .trim_start()
                .starts_with("yes")
        })
}

fn uuid_of(zone: &str) -> Option<&str> {
    let rest = &zone[zone.find("(uuid \"")? + "(uuid \"".len()..];
    Some(&rest[..rest.find('"')?])
}

/// Byte range of every `(name ...)` form that opens at nesting `depth`.
fn forms(text: &str, name: &str, depth: usize) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let (mut level, mut start) = (0usize, None);
    let (mut in_string, mut escaped) = (false, false);
    for (i, byte) in text.bytes().enumerate() {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'(' => {
                let head = &text[i + 1..];
                let named = head.starts_with(name)
                    && head[name.len()..].starts_with(|c: char| c.is_whitespace() || c == ')');
                if level == depth && named {
                    start = Some(i);
                }
                level += 1;
            }
            b')' => {
                level = level.saturating_sub(1);
                if level == depth {
                    if let Some(open) = start.take() {
                        found.push(open..i + 1);
                    }
                }
            }
            _ => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTED: &str = r#"(kicad_pcb
	(net 1 "A)")
	(zone
		(net 1)
		(name "Zoné ) 地")
		(uuid "z-1")
		(fill yes)
		(polygon (pts (xy 0 0) (xy 9 0) (xy 9 9)))
	)
	(zone
		(net 1)
		(uuid "z-2")
		(keepout (tracks not_allowed))
		(fill yes)
	)
)
"#;

    const SAVED: &str = r#"(kicad_pcb
	(zone
		(uuid "z-1")
		(fill yes (island_removal_mode 0))
		(filled_polygon (layer "F.Cu") (pts (xy 1 1) (xy 2 1) (xy 2 2)))
		(filled_polygon (layer "F.Cu") (pts (xy 3 3) (xy 4 3) (xy 4 4)))
	)
	(zone (uuid "z-2") (fill yes))
)
"#;

    #[test]
    fn fills_are_copied_into_the_matching_zone_and_nothing_else_changes() {
        let merged = graft_fills(ROUTED, SAVED).expect("grafts");
        assert_eq!(merged.matches("(filled_polygon").count(), 2);
        assert!(merged.contains(r#"(net 1 "A)")"#), "net table kept");
        assert!(
            !merged.contains("island_removal_mode"),
            "KiCad's rewrite is not copied"
        );
        let zones = forms(&merged, "zone", 1);
        assert_eq!(zones.len(), 2);
        let first = &merged[zones[0].clone()];
        assert_eq!(forms(first, "filled_polygon", 1).len(), 2);
        assert!(forms(&merged[zones[1].clone()], "filled_polygon", 1).is_empty());
        assert!(PcbBoard::parse(&merged).is_ok());

        // Removing the grafted forms gives back the routed board exactly,
        // with no stray whitespace left behind.
        let mut stripped = merged.clone();
        for range in forms(first, "filled_polygon", 1).into_iter().rev() {
            let at = zones[0].start + range.start;
            let end = zones[0].start + range.end;
            let from = stripped[..at].trim_end().len();
            stripped.replace_range(from..end, "");
        }
        assert_eq!(stripped, ROUTED);
    }

    #[test]
    fn a_stale_fill_already_in_the_zone_is_replaced_not_kept_beside_the_new_one() {
        let stale = ROUTED.replace(
            "(fill yes)\n\t\t(polygon",
            "(fill yes)\n\t\t(filled_polygon (layer \"F.Cu\") (pts (xy 7 7) (xy 8 7) (xy 8 8)))\n\t\t(polygon",
        );
        assert!(
            stale.contains("(xy 7 7)"),
            "the stale fill was not injected"
        );
        assert_eq!(strip_fills(&stale), ROUTED, "only the fill is stripped");
        let merged = graft_fills(&stale, SAVED).expect("grafts");
        assert_eq!(merged.matches("(filled_polygon").count(), 2, "{merged}");
        assert!(!merged.contains("(xy 7 7)"), "stale copper survived");
        assert!(merged.contains("(xy 1 1)"), "fresh copper missing");
    }

    #[test]
    fn a_save_with_no_fill_is_an_error_rather_than_an_unchanged_success() {
        let unfilled = r#"(kicad_pcb (zone (uuid "z-1") (fill yes)))"#;
        assert!(graft_fills(ROUTED, unfilled).is_err());
    }

    #[test]
    fn a_save_that_fills_only_some_zones_is_rejected() {
        let both_pour = ROUTED.replace("(keepout (tracks not_allowed))\n\t\t", "");
        let err = graft_fills(&both_pour, SAVED).expect_err("z-2 has no fill");
        assert!(err.contains("1 of 2"), "{err}");
    }

    #[test]
    fn a_keepout_zone_does_not_stop_the_ground_pour_being_filled() {
        let routed = r#"(kicad_pcb
	(zone (net 0) (uuid "k-1") (keepout (tracks not_allowed)) (fill (thermal_gap 0.5)))
	(zone
		(net 1)
		(uuid "g-1")
		(fill
			yes
			(thermal_gap 0.2)
		)
	)
)
"#;
        let saved = r#"(kicad_pcb
	(zone (uuid "k-1") (keepout (tracks not_allowed)) (fill (thermal_gap 0.5)))
	(zone (uuid "g-1") (fill yes) (filled_polygon (layer "F.Cu") (pts (xy 1 1) (xy 2 1) (xy 2 2))))
)
"#;
        let merged = graft_fills(routed, saved).expect("the pour is filled");
        assert_eq!(merged.matches("(filled_polygon").count(), 1);
        assert!(!pours_copper(&routed[forms(routed, "zone", 1)[0].clone()]));
    }

    #[test]
    fn a_failed_install_removes_the_staged_file_and_keeps_the_board() {
        let dir = std::env::temp_dir().join(format!("synth_zone_install_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A directory where the board should be makes the final rename fail.
        let board = dir.join("demo.kicad_pcb");
        std::fs::create_dir_all(&board).expect("dir");
        let err = install(&board, "(kicad_pcb)").expect_err("rename onto a directory fails");
        assert!(err.contains("could not install"), "{err}");
        assert!(!dir.join("demo.zonefill.tmp").exists());
        assert!(board.is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_duplicate_zone_uuid_in_the_routed_board_is_rejected() {
        let twice = ROUTED.replace("z-2", "z-1");
        let err = graft_fills(&twice, SAVED).expect_err("ambiguous");
        assert!(err.contains("z-1"), "{err}");
    }
}
