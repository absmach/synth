// SPDX-License-Identifier: Apache-2.0

//! Automatic footprint resolution: find a real `.kicad_mod` for a part that
//! names one that does not exist.
//!
//! # Why this exists
//!
//! Registry parts carry a `kicad_footprint` like
//! `Package_DFN_QFN:QFN-73-1EP_7x7mm_P0.5mm_EP4.5x4.5mm`. That name looks
//! plausible and is load-bearing, but nothing ever checked it against the
//! installed KiCad libraries. When the name is wrong the part lowers,
//! wires, and ERCs cleanly; only `E-SYNTH-EXPORT-001` notices, at the very
//! end, and the fix ("go find the right one") is left to a human with a file
//! browser.
//!
//! In practice the right footprint is usually *already installed*. The part
//! author either mistyped a name or modelled a package from memory. Both
//! real cases this was written for are a near-miss on a name that KiCad
//! ships:
//!
//! ```text
//! QFN-73-1EP_7x7mm_P0.5mm_EP4.5x4.5mm -> Nordic_AQFN-73-1EP_7x7mm_P0.5mm
//! Johanson_2450AT43B100               -> Johanson_2450AT43F0100
//! ```
//!
//! So the first resolution stage costs no network and draws on KiCad's
//! vetted library, which is the safest footprint source available. Only when
//! that finds nothing should anything reach for the network (see
//! `synth part import-lcsc`).
//!
//! # What this never does
//!
//! It never invents pin numbers. A ball-grid package's designators
//! (`A22`, `AC13`) cannot be derived from a part's `[[pins]]` list without
//! the datasheet, and a plausible-but-wrong pin number produces a board that
//! routes and passes every check while being unbuildable. Resolver output
//! therefore only ever repairs the *footprint reference*; pin/pad agreement
//! stays a blocking diagnostic (`E-SYNTH-PIN-001`) for a human or agent
//! with the datasheet in hand.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;

/// A scored match between a part and an installed footprint.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// `Library:FootprintName`, the form `kicad_footprint` takes.
    pub lib_id: String,
    /// 0.0–1.0. Only candidates at or above [`AUTO_APPLY_MIN_SCORE`] are
    /// applied without asking.
    pub score: f64,
    /// Human-readable justification, surfaced in the diagnostic and the CLI
    /// so a human can audit why this footprint was chosen.
    pub reason: String,
    /// Fraction of the part's declared pins that exist as pads here.
    /// `1.0` means every declared pin lands on a real pad.
    pub pin_coverage: f64,
}

/// A match confident enough to apply without human review.
///
/// Deliberately conservative, and a floor is not sufficient on its own —
/// see [`AUTO_APPLY_MIN_MARGIN`]. A wrong footprint silently misplaces every
/// part of that type, so the resolver only acts when the evidence is both
/// strong and unambiguous. Below either bound the candidate is still
/// *reported*, just not applied.
pub const AUTO_APPLY_MIN_SCORE: f64 = 0.55;

/// How far ahead of the runner-up the winner must be.
///
/// A mistyped part number can leave two plausible readings close together
/// (`2450AT43B100` is one edit from `2450AT43F0100` and three from
/// `2450AT18x100`). When the top two are within noise, picking one is a
/// coin flip dressed up as a lookup, so the resolver declines and reports
/// both. A correct match is usually far ahead: the 73-pin 0.5 mm Nordic
/// part beats the 32- and 48-pin parts of the same family by a wide margin.
pub const AUTO_APPLY_MIN_MARGIN: f64 = 0.08;

/// Every footprint installed in the search path, as `(library, name)`.
#[derive(Debug, Clone, Default)]
pub struct FootprintIndex {
    /// Sorted for deterministic ordering — a resolver whose output order
    /// depends on directory iteration is not reproducible.
    pub entries: Vec<(String, String)>,
}

impl FootprintIndex {
    /// Scan the search path once and cache it.
    ///
    /// Returns an empty index when no KiCad footprint directory is
    /// installed; every caller treats that as "cannot help", never as
    /// "no match found", so the distinction matters to the caller.
    pub fn load() -> Self {
        static INDEX: OnceLock<FootprintIndex> = OnceLock::new();
        INDEX.get_or_init(Self::scan).clone()
    }

    fn scan() -> Self {
        let mut entries = Vec::new();
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Some(user) = user_footprint_dir() {
            roots.push(user);
        }
        for candidate in [
            std::env::var("KICAD_FOOTPRINT_DIR").ok().map(PathBuf::from),
            Some(PathBuf::from("/usr/share/kicad/footprints")),
            Some(PathBuf::from(
                "/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints",
            )),
        ]
        .into_iter()
        .flatten()
        {
            if candidate.is_dir() && !roots.contains(&candidate) {
                roots.push(candidate);
            }
        }

        for root in roots {
            let Ok(libs) = std::fs::read_dir(&root) else {
                continue;
            };
            for lib in libs.flatten() {
                let path = lib.path();
                if path.extension().and_then(|e| e.to_str()) != Some("pretty") {
                    continue;
                }
                let Some(lib_name) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Ok(footprints) = std::fs::read_dir(&path) else {
                    continue;
                };
                for fp in footprints.flatten() {
                    if fp.path().extension().and_then(|e| e.to_str()) != Some("kicad_mod") {
                        continue;
                    }
                    if let Some(name) = fp.path().file_stem().and_then(|s| s.to_str()) {
                        entries.push((lib_name.to_string(), name.to_string()));
                    }
                }
            }
        }
        entries.sort();
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn user_footprint_dir() -> Option<PathBuf> {
    std::env::var("SYNTH_USER_FOOTPRINT_DIR")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// The footprint name within a `Library:Name` reference, or the whole string
/// when it carries no library.
///
/// The library half of a reference is noise for this comparison:
/// `Package_DFN_QFN` contributes the tokens `package` and `dfn`, which match
/// every QFN in the library and therefore carry no information about which
/// part is meant.
fn footprint_name(reference: &str) -> &str {
    reference
        .rsplit_once(':')
        .map_or(reference, |(_, name)| name)
}

/// Split an identifier into comparable tokens.
///
/// `Nordic_AQFN-73-1EP_7x7mm_P0.5mm` → `["nordic", "aqfn", "73", "1EP",
/// "7x7mm", "P0.5mm"]`.
///
/// `.` is deliberately *not* a separator. Decimals are the load-bearing part
/// of a package name: `P0.5mm` and `P0.65mm` are different pitches, and
/// `EP4.5x4.5mm` and `EP4.65x4.65mm` are different exposed pads. Splitting
/// on `.` shatters both into fragments (`p0`,`5mm`) that then match across
/// incompatible packages, which is how a 32-pin part once outscored the
/// 73-pin part it was being asked to replace.
fn tokenize(s: &str) -> Vec<String> {
    s.split(['_', '-', ' ', '(', ')'])
        .filter(|t| !t.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Strip the manufacturer prefix and packaging-class qualifiers so the
/// package geometry dominates the comparison.
///
/// `Nordic_AQFN-73-1EP_7x7mm_P0.5mm` and `QFN-73-1EP_7x7mm_P0.5mm_EP4.5x4.5mm`
/// describe the same 7x7/0.5 mm body; their difference is manufacturer and
/// one extra exposed-pad size, neither of which changes the land pattern.
fn package_core(s: &str) -> String {
    let mut out = String::new();
    for token in tokenize(s) {
        // Known manufacturer names, and generic package classes, are not
        // part of the geometry. Anything unrecognised is kept so an unusual
        // part number still contributes evidence.
        const MANUFACTURERS: &[&str] = &[
            "nordic",
            "texas",
            "infineon",
            "nxp",
            "microchip",
            "st",
            "stmicro",
            "onsemi",
            "johanson",
            "murata",
            "tdk",
            "avx",
            "yageo",
            "vishay",
            "panasonic",
            "rohm",
            "renesas",
            "mps",
            "allegro",
            "aosmd",
            "amphenol",
            "molex",
            "jst",
            "phoenix",
            "wurth",
            "bourns",
            "omron",
            "artinchip",
            "mini",
            "circuits",
            "mic",
            "micro",
            "labs",
        ];
        // `aqfn` is a class variant that never introduces a pin count, so it
        // is listed here but not in `PACKAGE_CLASSES`.
        const EXTRA_CLASSES: &[&str] = &["aqfn"];
        if MANUFACTURERS.contains(&token.as_str())
            || PACKAGE_CLASSES.contains(&token.as_str())
            || EXTRA_CLASSES.contains(&token.as_str())
        {
            continue;
        }
        out.push_str(&token);
    }
    out
}

/// Normalized Levenshtein similarity, 0.0–1.0.
fn edit_similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let dist = prev[b.len()];
    1.0 - (dist as f64 / a.len().max(b.len()) as f64)
}

/// Score one (query, candidate) name pair in 0.0–1.0.
///
/// Two independent signals, combined:
/// - token overlap, which rewards sharing discrete facts (7x7mm, P0.5mm);
/// - similarity of the concatenated package core, which catches
///   single-character slips inside one token (2450AT43**B**100 vs
///   2450AT43**F**0100) where token overlap sees two different tokens.
///
/// Package-class prefixes that introduce a pin count, e.g. the `QFN`
/// of `QFN-73`.
const PACKAGE_CLASSES: &[&str] = &[
    "qfn", "lqfn", "vqfn", "tqfn", "pqfn", "sqfn", "dqfn", "drqfn", "dfn", "bga", "lga", "csp",
    "soic", "tssop", "msop", "sot", "sod", "mlpq", "rgz", "qfp", "tqfp", "vqfp",
];

/// The pin count a name declares, as in the `73` of `QFN-73`.
///
/// This is the one fact that must be *exactly* right, and it is the fact the
/// blended score is worst at. Every QFN in a library shares the same body
/// size and exposed-pad convention, so a 32-pin part and a 73-pin part of
/// the same family score alike on every other token and on the concatenated
/// core string — which is how `QFN-32-1EP_7x7mm_P0.65mm` once outranked the
/// `AQFN-73` it was being asked to replace. A pin-count disagreement is
/// therefore a hard disqualification, not a penalty.
///
/// Returns `None` when the name carries no pin count, which is the normal
/// case for a manufacturer part number (`2450AT43F0100`): those are opaque
/// blobs, and edit distance is the right tool for them.
fn pin_count(name: &str) -> Option<String> {
    let tokens = tokenize(footprint_name(name));
    for (i, token) in tokens.iter().enumerate() {
        for class in PACKAGE_CLASSES {
            // Joined form: `QFN73`.
            if let Some(rest) = token.strip_prefix(class) {
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                if !digits.is_empty() {
                    return Some(digits);
                }
            }
            // Split form. Tokenizing on `-`/`_` separates the class from its
            // count — `QFN-73` becomes `["qfn", "73"]` — so the count is the
            // *next* token, not the tail of this one.
            if token == class {
                if let Some(next) = tokens.get(i + 1) {
                    if !next.is_empty() && next.chars().all(|c| c.is_ascii_digit()) {
                        return Some(next.clone());
                    }
                }
            }
        }
    }
    None
}

fn name_score(query: &str, candidate: &str) -> (f64, String) {
    let qn = footprint_name(query);
    let cn = footprint_name(candidate);
    let qt = tokenize(qn);
    let ct = tokenize(cn);
    let cset: BTreeSet<&str> = ct.iter().map(String::as_str).collect();

    let mut hits = Vec::new();
    for token in &qt {
        // Exact token, or one containing the other: the query's package
        // class (`QFN`) is stripped from both sides before this, but a
        // class-qualified candidate (`AQFN`) still contains it.
        if cset.contains(token.as_str()) || ct.iter().any(|c| c.contains(token.as_str())) {
            hits.push(token.clone());
        }
    }
    let token_score = if qt.is_empty() {
        0.0
    } else {
        hits.len() as f64 / qt.len() as f64
    };

    let qc = package_core(qn);
    let cc = package_core(cn);
    let core_score = edit_similarity(&qc, &cc);

    // Tokens are the stronger signal: they encode the discrete facts (pin
    // count, body size, pitch) that decide whether two packages are the
    // same. The core string catches a single-character slip inside one
    // token, where token overlap sees two different tokens.
    let score = (0.65 * token_score + 0.35 * core_score).clamp(0.0, 1.0);
    let reason = format!(
        "{}/{} shared name facts ({}); package-core similarity {:.2}",
        hits.len(),
        qt.len(),
        if hits.is_empty() {
            "none".to_string()
        } else {
            hits.join(", ")
        },
        core_score,
    );
    (score, reason)
}

/// Rank installed footprints against a part.
///
/// Returns at most `limit` candidates above `min_score`, best first. The
/// ordering is total and deterministic (score, then `lib_id`), so repeated
/// runs on the same machine agree.
pub fn candidates(
    index: &FootprintIndex,
    query: &str,
    part_pins: &[String],
    limit: usize,
    min_score: f64,
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = index
        .entries
        .iter()
        .filter_map(|(lib, name)| {
            let lib_id = format!("{lib}:{name}");
            // Pin count must agree exactly when both sides declare one. This
            // is a gate rather than a term in the score, because a same-family
            // package of the wrong pin count is otherwise indistinguishable
            // by name.
            match (pin_count(query), pin_count(&lib_id)) {
                (Some(q), Some(c)) if q != c => return None,
                _ => {}
            }
            let (score, reason) = name_score(query, &lib_id);
            if score < min_score {
                return None;
            }
            // Pad lookup is a filesystem read per candidate, so it only runs
            // for names that already scored well enough to be reported.
            let coverage = pin_coverage(&lib_id, part_pins);
            let mut reason = reason;
            if !part_pins.is_empty() {
                use std::fmt::Write as _;
                let _ = write!(
                    reason,
                    "; {:.0}% of declared pins are pads here",
                    coverage * 100.0
                );
            }
            // A footprint that carries every declared pin is the strongest
            // possible corroboration, so it outranks an equally-named one
            // that does not.
            let score = (score + 0.15 * coverage).clamp(0.0, 1.0);
            Some(Candidate {
                lib_id,
                score,
                reason,
                pin_coverage: coverage,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.lib_id.cmp(&b.lib_id))
    });
    out.truncate(limit);
    out
}

fn pin_coverage(lib_id: &str, part_pins: &[String]) -> f64 {
    if part_pins.is_empty() {
        return 0.0;
    }
    let Some(pads) = crate::kicad_footprint_loader::pads(lib_id) else {
        return 0.0;
    };
    let numbers: BTreeSet<&str> = pads.iter().map(|p| p.number.as_str()).collect();
    let found = part_pins
        .iter()
        .filter(|n| numbers.contains(n.as_str()))
        .count();
    found as f64 / part_pins.len() as f64
}

/// Resolve one part to a footprint, if a confident *and* unambiguous match
/// exists.
///
/// Returns `None` when there is no confident match — which is different
/// from "no match at all", and the caller must not conflate them. Callers
/// that want the near-misses use [`candidates`] directly.
///
/// Two conditions, both required:
/// - the winner scores at least [`AUTO_APPLY_MIN_SCORE`];
/// - the winner leads the runner-up by at least [`AUTO_APPLY_MIN_MARGIN`].
///
/// The margin is what makes this safe to run unattended. A part number with
/// one wrong character can sit close to two real parts, and choosing the
/// higher of two near-identical scores is a guess; declining and reporting
/// both is a better outcome than a board full of the wrong antenna.
pub fn resolve_one(
    index: &FootprintIndex,
    footprint_ref: &str,
    part_pins: &[String],
) -> Option<Candidate> {
    // Only try to repair a reference that does not already work. A part
    // pointing at a real footprint is not this function's business.
    if crate::kicad_footprint_loader::pads(footprint_ref).is_some() {
        return None;
    }
    let mut ranked =
        candidates(index, footprint_ref, part_pins, 2, AUTO_APPLY_MIN_SCORE).into_iter();
    let top = ranked.next()?;
    if top.score < AUTO_APPLY_MIN_SCORE {
        return None;
    }
    if let Some(second) = ranked.next() {
        if top.score - second.score < AUTO_APPLY_MIN_MARGIN {
            return None;
        }
    }
    Some(top)
}

/// Search path roots, for the CLI to report where it looked.
pub fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(user) = user_footprint_dir() {
        roots.push(user);
    }
    for candidate in [
        std::env::var("KICAD_FOOTPRINT_DIR").ok().map(PathBuf::from),
        Some(PathBuf::from("/usr/share/kicad/footprints")),
        Some(PathBuf::from(
            "/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints",
        )),
    ]
    .into_iter()
    .flatten()
    {
        if candidate.is_dir() && !roots.contains(&candidate) {
            roots.push(candidate);
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx(names: &[&str]) -> FootprintIndex {
        FootprintIndex {
            entries: names
                .iter()
                .map(|n| {
                    let n = (*n).to_string();
                    let (lib, rest) = n.split_once(':').expect("lib:name");
                    (lib.to_string(), rest.to_string())
                })
                .collect(),
        }
    }

    /// The real miss from a board that shipped fictional footprint names.
    #[test]
    fn finds_the_nordic_aqfn_for_a_fabricated_qfn73_name() {
        let index = idx(&[
            "Package_DFN_QFN:Nordic_AQFN-73-1EP_7x7mm_P0.5mm",
            "Package_DFN_QFN:QFN-32-1EP_7x7mm_P0.65mm_EP4.65x4.65mm",
            "Package_DFN_QFN:Infineon_MLPQ-48-1EP_7x7mm_P0.5mm_EP5.15x5.15mm",
        ]);
        let got = candidates(
            &index,
            "Package_DFN_QFN:QFN-73-1EP_7x7mm_P0.5mm_EP4.5x4.5mm",
            &[],
            1,
            0.0,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].lib_id, "Package_DFN_QFN:Nordic_AQFN-73-1EP_7x7mm_P0.5mm",
            "the 73-pin 0.5 mm Nordic part must win over the 32- and 48-pin ones"
        );
    }

    /// A one-character slip inside a part number: tokens differ, so only
    /// core-string similarity can see it.
    #[test]
    fn finds_the_johanson_antenna_for_a_mistyped_part_number() {
        let index = idx(&[
            "RF_Antenna:Johanson_2450AT43F0100",
            "RF_Antenna:Johanson_2450AT18x100",
            "RF_Antenna:Molex_47948-0001_2.4Ghz",
        ]);
        let got = candidates(&index, "RF_Antenna:Johanson_2450AT43B100", &[], 1, 0.0);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].lib_id, "RF_Antenna:Johanson_2450AT43F0100");
    }

    #[test]
    fn a_different_package_in_the_same_family_does_not_match() {
        let index = idx(&["Package_DFN_QFN:QFN-32-1EP_7x7mm_P0.65mm_EP4.65x4.65mm"]);
        let got = candidates(
            &index,
            "Package_DFN_QFN:QFN-73-1EP_7x7mm_P0.5mm_EP4.5x4.5mm",
            &[],
            5,
            0.0,
        );
        // It shares 7x7mm but differs in pin count and pitch; it may appear
        // with a low score, but never above the auto-apply threshold.
        assert!(
            got.iter().all(|c| c.score < AUTO_APPLY_MIN_SCORE),
            "a wrong package must never clear the apply threshold: {got:?}"
        );
    }

    #[test]
    fn an_unrelated_query_scores_below_the_threshold() {
        let index = idx(&[
            "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm",
            "Resistor_SMD:R_0603_1608Metric",
        ]);
        let got = candidates(&index, "Package_DFN_QFN:Imaginary-99X", &[], 5, 0.0);
        assert!(
            got.iter().all(|c| c.score < AUTO_APPLY_MIN_SCORE),
            "nonsense query must not auto-apply: {got:?}"
        );
    }

    #[test]
    fn a_footprint_carrying_every_declared_pin_outranks_an_equally_named_one() {
        // A real installed footprint, so the pad lookup that drives coverage
        // actually runs.
        let index = idx(&["Resistor_SMD:R_0603_1608Metric"]);
        let got = candidates(
            &index,
            "Resistor_SMD:R_0603_1608Metric",
            &["1".to_string(), "2".to_string()],
            1,
            0.0,
        );
        assert!((got[0].pin_coverage - 1.0).abs() < 1e-9, "{got:?}");
    }

    #[test]
    fn a_clear_winner_is_applied() {
        let index = idx(&[
            "Package_DFN_QFN:Nordic_AQFN-73-1EP_7x7mm_P0.5mm",
            "Package_DFN_QFN:QFN-32-1EP_7x7mm_P0.65mm_EP4.65x4.65mm",
            "Package_DFN_QFN:Infineon_MLPQ-48-1EP_7x7mm_P0.5mm_EP5.15x5.15mm",
        ]);
        let got = resolve_one(
            &index,
            "Package_DFN_QFN:QFN-73-1EP_7x7mm_P0.5mm_EP4.5x4.5mm",
            &[],
        )
        .expect("a clear winner must be applied");
        assert_eq!(
            got.lib_id,
            "Package_DFN_QFN:Nordic_AQFN-73-1EP_7x7mm_P0.5mm"
        );
    }

    /// Two plausible readings of the same mistyped part number: the resolver
    /// must decline rather than pick one, because either choice silently
    /// misplaces the part.
    #[test]
    fn an_ambiguous_match_is_declined_not_guessed() {
        let index = idx(&[
            "RF_Antenna:Johanson_2450AT43F0100",
            "RF_Antenna:Johanson_2450AT18x100",
        ]);
        assert!(
            resolve_one(&index, "RF_Antenna:Johanson_2450AT43B100", &[]).is_none(),
            "two near-identical candidates must not be auto-applied"
        );
        // ...but both are still offered, so a human can pick.
        let all = candidates(&index, "RF_Antenna:Johanson_2450AT43B100", &[], 2, 0.0);
        assert_eq!(all.len(), 2, "{all:?}");
    }

    #[test]
    fn resolve_one_is_a_no_op_for_a_footprint_that_already_resolves() {
        let index = idx(&["Resistor_SMD:R_0603_1608Metric"]);
        assert!(
            resolve_one(&index, "Resistor_SMD:R_0603_1608Metric", &[]).is_none(),
            "a working reference must not be 'repaired'"
        );
    }

    #[test]
    fn resolve_one_reports_no_match_rather_than_a_weak_one() {
        let index = idx(&["Package_SO:SOIC-8_3.9x4.9mm_P1.27mm"]);
        assert!(
            resolve_one(&index, "Package_DFN_QFN:Imaginary-99X", &[]).is_none(),
            "a weak candidate must be reported, never applied"
        );
    }

    #[test]
    fn ordering_is_deterministic_and_stable() {
        let index = idx(&["L:C", "L:A", "L:B"]);
        let first = candidates(&index, "L", &[], 3, 0.0);
        let second = candidates(&index, "L", &[], 3, 0.0);
        assert_eq!(first, second);
        // Ties break on lib_id, so the order cannot depend on readdir order.
        let ids: Vec<&str> = first.iter().map(|c| c.lib_id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn package_core_drops_manufacturer_and_package_class() {
        assert_eq!(
            package_core("Nordic_AQFN-73-1EP_7x7mm_P0.5mm"),
            package_core("QFN-73-1EP_7x7mm_P0.5mm"),
            "manufacturer and package class must not drive the geometry match"
        );
    }
}
