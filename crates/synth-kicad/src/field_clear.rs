// SPDX-License-Identifier: Apache-2.0

//! Pick where a symbol's Reference or Value text goes so it does not land on
//! a wire, another body or a caption (absmach/synth#203).
//!
//! The exporter already has a preferred side for each field
//! (`choose_field_sides`). That rule only looks at the symbol's own pins, so
//! this module checks candidates against what is around the symbol, in the
//! caller's order, and takes the first that overlaps nothing. A board where
//! nothing is in the way exports exactly as before.

/// An axis-aligned rectangle in sheet millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    fn overlap_area(&self, other: &Rect) -> f64 {
        let w = self.x1.min(other.x1) - self.x0.max(other.x0);
        let h = self.y1.min(other.y1) - self.y0.max(other.y0);
        if w > 0.0 && h > 0.0 {
            w * h
        } else {
            0.0
        }
    }
}

/// The first of `candidates` whose text overlaps no obstacle; when every one
/// does, the one that overlaps least, the earlier winning a tie. `text_rect`
/// gives the rectangle the text covers at a candidate.
///
/// # Panics
///
/// When `candidates` is empty.
pub(crate) fn pick_clear<'a, C: Copy>(
    candidates: &[C],
    text_rect: &dyn Fn(C) -> Rect,
    obstacles: &(impl Iterator<Item = &'a Rect> + Clone),
) -> C {
    let cost = |candidate: C| -> f64 {
        let rect = text_rect(candidate);
        obstacles.clone().map(|o| rect.overlap_area(o)).sum()
    };
    let mut best = (candidates[0], cost(candidates[0]));
    for &candidate in &candidates[1..] {
        if best.1 <= 0.0 {
            break;
        }
        let c = cost(candidate);
        if c < best.1 {
            best = (candidate, c);
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // Candidates are plain labels here; "near" sits at x 0 to 4, "far" at x
    // 10 to 14 and "away" at x 20 to 24, each 1 mm tall.
    fn text_rect(candidate: &'static str) -> Rect {
        let x0 = match candidate {
            "near" => 0.0,
            "far" => 10.0,
            _ => 20.0,
        };
        Rect {
            x0,
            y0: 0.0,
            x1: x0 + 4.0,
            y1: 1.0,
        }
    }

    const CANDIDATES: [&str; 3] = ["near", "far", "away"];

    fn block(x0: f64, x1: f64) -> Rect {
        Rect {
            x0,
            y0: 0.0,
            x1,
            y1: 1.0,
        }
    }

    #[test]
    fn the_first_candidate_stays_when_nothing_is_in_the_way() {
        assert_eq!(pick_clear(&CANDIDATES, &text_rect, &[].iter()), "near");
    }

    #[test]
    fn the_first_candidate_stays_when_an_obstacle_is_elsewhere() {
        let elsewhere = Rect {
            x0: 50.0,
            y0: 50.0,
            x1: 60.0,
            y1: 60.0,
        };
        assert_eq!(
            pick_clear(&CANDIDATES, &text_rect, &[elsewhere].iter()),
            "near"
        );
    }

    #[test]
    fn an_obstacle_on_the_first_candidate_moves_to_the_next_clear_one() {
        assert_eq!(
            pick_clear(&CANDIDATES, &text_rect, &[block(1.0, 3.0)].iter()),
            "far"
        );
    }

    #[test]
    fn a_thin_obstacle_across_the_text_still_moves_it() {
        let wire = Rect {
            x0: 2.0,
            y0: -5.0,
            x1: 2.3,
            y1: 5.0,
        };
        assert_eq!(pick_clear(&CANDIDATES, &text_rect, &[wire].iter()), "far");
    }

    #[test]
    fn with_everything_blocked_the_least_overlap_wins() {
        // The overlaps are 4, 2 and 3 mm wide.
        let got = pick_clear(
            &CANDIDATES,
            &text_rect,
            &[block(0.0, 4.0), block(10.0, 12.0), block(20.0, 23.0)].iter(),
        );
        assert_eq!(got, "far");
    }

    #[test]
    fn a_tie_goes_to_the_earlier_candidate() {
        let got = pick_clear(
            &CANDIDATES,
            &text_rect,
            &[block(0.0, 1.0), block(10.0, 11.0), block(20.0, 21.0)].iter(),
        );
        assert_eq!(got, "near");
    }
}
