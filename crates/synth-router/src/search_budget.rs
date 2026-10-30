// SPDX-License-Identifier: Apache-2.0

//! A wall-clock budget for a placement search that routes several candidates.
//!
//! Each candidate is a full routing run with its own timeout, and the export
//! then routes the winner once more. A loop that only checks the clock between
//! runs can still hold a call for the sum of every timeout: 900 s of runs, then
//! a run started at 899 s that is given 1200 s, then 1200 s more for the final
//! route. The budget caps each run at what is left, keeps back the time the
//! final route is likely to need, and says whether it cut the search short.

use std::time::{Duration, Instant};

/// Seconds a placement search may spend when the caller names none.
pub const DEFAULT_BUDGET_SECS: u64 = 900;

/// The shortest routing run worth starting, in seconds. A run given less than
/// this cannot finish, so it is not started.
pub const MIN_RUN_SECS: u64 = 30;

/// The time held back for the final route before any candidate run starts, in
/// seconds, so the board can still be delivered inside the budget. A budget
/// shorter than this is all final route: no candidate is run.
pub const MIN_FINAL_SECS: u64 = 60;

/// The wall-clock budget of one placement search.
#[derive(Debug, Clone)]
pub struct SearchBudget {
    started: Instant,
    total: Duration,
    min_run: Duration,
    final_floor: Duration,
    longest_run: Duration,
}

impl SearchBudget {
    /// A budget of `total_secs` seconds starting now; zero means the default,
    /// as a zero timeout means "no override" everywhere else.
    #[must_use]
    pub fn start(total_secs: u64) -> Self {
        let secs = if total_secs == 0 {
            DEFAULT_BUDGET_SECS
        } else {
            total_secs
        };
        Self::with_floors(
            Duration::from_secs(secs),
            Duration::from_secs(MIN_RUN_SECS),
            Duration::from_secs(MIN_FINAL_SECS),
        )
    }

    /// A budget with an explicit shortest run and no standing share for the final
    /// route beyond the longest run seen, for tests that cannot wait seconds.
    #[must_use]
    pub fn with_limits(total: Duration, min_run: Duration) -> Self {
        Self::with_floors(total, min_run, Duration::ZERO)
    }

    /// A budget with an explicit shortest run and final-route share.
    #[must_use]
    pub fn with_floors(total: Duration, min_run: Duration, final_floor: Duration) -> Self {
        Self {
            started: Instant::now(),
            total,
            min_run,
            final_floor,
            longest_run: Duration::ZERO,
        }
    }

    /// The whole budget.
    #[must_use]
    pub fn total(&self) -> Duration {
        self.total
    }

    /// How long the search has been running.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// What is left of the budget.
    #[must_use]
    pub fn remaining(&self) -> Duration {
        self.total.saturating_sub(self.elapsed())
    }

    /// The clock for a run granted `limit`, started now.
    #[must_use]
    pub fn run_clock(&self, limit: Duration) -> RunClock {
        RunClock::start(limit, self.min_run)
    }

    /// Records how long a run took, so the time the final route will need is
    /// kept back from the next one.
    pub fn note_run(&mut self, took: Duration) {
        self.longest_run = self.longest_run.max(took);
    }

    /// What the final route is kept: the longest run seen, or the standing share
    /// ([`MIN_FINAL_SECS`] for a real budget), but never more than the whole
    /// budget.
    fn reserve(&self) -> Duration {
        self.longest_run.max(self.final_floor).min(self.total)
    }

    /// The wall-clock limit for the next candidate run: `requested`, or what is
    /// left after the final route's share, whichever is less. `None` when that
    /// limit is shorter than the minimum run, whether it is the budget that is
    /// short or the router timeout the caller asked for: a run given less than
    /// [`MIN_RUN_SECS`] cannot finish, so none is started.
    #[must_use]
    pub fn candidate_limit(&self, requested: Duration) -> Option<Duration> {
        let available = self.remaining().saturating_sub(self.reserve());
        let limit = requested.min(available);
        (limit >= self.min_run).then_some(limit)
    }

    /// The wall-clock limit for the final route: `requested`, or what is left,
    /// whichever is less, so the whole call stays inside the budget. The share
    /// held back from the candidates is what makes that enough to deliver the
    /// board. `None` when less than a second is left: the route is not started
    /// and the caller reports the search as out of budget.
    #[must_use]
    pub fn final_limit(&self, requested: Duration) -> Option<Duration> {
        let left = self.remaining();
        (left >= Duration::from_secs(1)).then(|| requested.min(left))
    }
}

/// The clock of one run that is granted a wall-clock limit: it starts when the
/// limit is granted, before the run's export and other preparation, and the
/// router is given what is left of the limit when it starts, so preparation time
/// plus routing time cannot exceed the limit. A run left with less than the
/// minimum run is not started.
#[derive(Debug, Clone, Copy)]
pub struct RunClock {
    started: Instant,
    limit: Duration,
    min_run: Duration,
}

impl RunClock {
    /// Starts the clock for a run granted `limit`, which the router needs at
    /// least `min_run` of.
    #[must_use]
    pub fn start(limit: Duration, min_run: Duration) -> Self {
        Self {
            started: Instant::now(),
            limit,
            min_run,
        }
    }

    /// What is left of the limit.
    #[must_use]
    pub fn left(&self) -> Duration {
        self.limit.saturating_sub(self.started.elapsed())
    }

    /// The router's wall-clock limit in whole seconds: what is left of the run's
    /// limit. `None` when that is less than the minimum run, so the router is not
    /// started: a run given less than [`MIN_RUN_SECS`] cannot finish, and starting
    /// it would take time the budget no longer has.
    #[must_use]
    pub fn router_timeout_secs(&self) -> Option<u64> {
        let left = self.left();
        (left >= self.min_run).then_some(left.as_secs())
    }
}

/// How one candidate's evaluation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attempt<C> {
    /// The candidate was routed.
    Routed(Evaluated<C>),
    /// The routing engine is unavailable, which ends the search.
    EngineUnavailable,
    /// Preparing the candidate used the time its run was granted, so the router
    /// was not started and the candidate is skipped.
    NoTime,
}

impl<C> Attempt<C> {
    /// Maps what a routed attempt carries, leaving the other two as they are.
    pub fn map<D>(self, f: impl FnOnce(C) -> D) -> Attempt<D> {
        match self {
            Self::Routed(evaluated) => Attempt::Routed(Evaluated {
                cost: f(evaluated.cost),
                order: evaluated.order,
            }),
            Self::EngineUnavailable => Attempt::EngineUnavailable,
            Self::NoTime => Attempt::NoTime,
        }
    }
}

/// One candidate's result: its cost (lower is better) and the routing order the
/// router recommends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluated<C> {
    pub cost: C,
    pub order: Vec<String>,
}

/// What a search over candidates found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search<T, C> {
    /// The cheapest candidate routed, with its cost and routing order, if any.
    pub best: Option<(C, T, Vec<String>)>,
    /// How many candidates were routed.
    pub routed: usize,
    /// How many there were.
    pub total: usize,
    /// Whether the budget stopped the search before every candidate was routed.
    pub cut_short: bool,
}

/// Routes `candidates` in order, each under the budget, and keeps the cheapest.
/// A tie keeps the earlier candidate, so the choice is deterministic.
///
/// `evaluate` routes one candidate under the clock it is given, which started
/// before the candidate's export. It answers with an [`Attempt`]: an unavailable
/// engine ends the search with `Ok(None)`, and the caller keeps its own
/// placement; a candidate whose preparation used up its time is skipped and the
/// search goes on, flagged as cut short.
///
/// # Errors
/// Whatever `evaluate` returns.
pub fn route_candidates<T: Clone, C: Ord, E>(
    candidates: &[T],
    budget: &mut SearchBudget,
    requested: Duration,
    mut evaluate: impl FnMut(&T, usize, RunClock) -> Result<Attempt<C>, E>,
) -> Result<Option<Search<T, C>>, E> {
    let mut best: Option<(C, T, Vec<String>)> = None;
    let mut routed = 0;
    let mut cut_short = false;
    for (index, candidate) in candidates.iter().enumerate() {
        let Some(limit) = budget.candidate_limit(requested) else {
            cut_short = true;
            break;
        };
        let began = Instant::now();
        let evaluated = evaluate(candidate, index, budget.run_clock(limit))?;
        budget.note_run(began.elapsed());
        let Evaluated { cost, order } = match evaluated {
            Attempt::Routed(evaluated) => evaluated,
            Attempt::EngineUnavailable => return Ok(None),
            Attempt::NoTime => {
                cut_short = true;
                continue;
            }
        };
        routed += 1;
        if best
            .as_ref()
            .is_none_or(|(previous, _, _)| cost < *previous)
        {
            best = Some((cost, candidate.clone(), order));
        }
    }
    Ok(Some(Search {
        best,
        routed,
        total: candidates.len(),
        cut_short,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn zero_means_the_default_budget() {
        assert_eq!(SearchBudget::start(0).total(), Duration::from_secs(900));
        assert_eq!(SearchBudget::start(120).total(), Duration::from_secs(120));
    }

    #[test]
    fn a_run_is_capped_at_what_is_left_not_at_the_routers_full_timeout() {
        let budget = SearchBudget::with_limits(Duration::from_secs(900), MS(10));
        let limit = budget
            .candidate_limit(Duration::from_secs(1200))
            .expect("room");
        assert!(limit <= Duration::from_secs(900), "{limit:?}");
        let small = budget
            .candidate_limit(Duration::from_secs(60))
            .expect("room");
        assert_eq!(
            small,
            Duration::from_secs(60),
            "never raised above the request"
        );
    }

    /// The router gets what preparation left of the run's limit, not the whole of
    /// it, and is not started at all when less than the minimum run is left.
    #[test]
    fn preparation_time_and_routing_time_together_stay_within_the_limit() {
        let (limit, min_run) = (Duration::from_secs(100), Duration::from_secs(30));
        let after = |secs: u64| RunClock {
            started: Instant::now()
                .checked_sub(Duration::from_secs(secs))
                .expect("a clock that has run for a while"),
            limit,
            min_run,
        };
        // A fresh clock gives the router nearly the whole limit.
        assert!(RunClock::start(limit, min_run).router_timeout_secs() >= Some(99));
        // After 40 s of preparation the router gets what is left, so the two add up
        // to no more than the limit.
        let router = after(40).router_timeout_secs().expect("time is left");
        assert!(
            router <= 60,
            "the router got {router} s after 40 s of preparation"
        );
        assert!(Duration::from_secs(40 + router) <= limit);
        // With less than the minimum run left, or nothing, the router is not started.
        assert_eq!(
            after(80).router_timeout_secs(),
            None,
            "20 s is under the minimum run"
        );
        assert_eq!(
            after(100).router_timeout_secs(),
            None,
            "preparation took it all"
        );
        assert_eq!(
            after(500).router_timeout_secs(),
            None,
            "and more than all of it"
        );
    }

    /// A candidate whose preparation used up its time is skipped, not routed with
    /// a second or two, and the search goes on with the next one.
    #[test]
    fn a_candidate_whose_preparation_used_its_time_is_skipped_and_the_search_goes_on() {
        let mut budget =
            SearchBudget::with_limits(Duration::from_secs(900), Duration::from_secs(10));
        let search = route_candidates::<_, u32, ()>(
            &["slow", "fast"],
            &mut budget,
            Duration::from_secs(60),
            |&name, _, _| {
                Ok(if name == "slow" {
                    Attempt::NoTime
                } else {
                    Attempt::Routed(Evaluated {
                        cost: 1,
                        order: Vec::new(),
                    })
                })
            },
        )
        .expect("no error")
        .expect("an engine");
        assert_eq!(
            search.routed, 1,
            "only the candidate that was routed counts"
        );
        assert!(
            search.cut_short,
            "a skipped candidate is a truncated search"
        );
        assert_eq!(search.best.expect("the other one").1, "fast");
    }

    /// The minimum run applies to the limit a candidate would be given, not only
    /// to what the budget has left: a router timeout shorter than the minimum
    /// starts no candidate run, however much budget there is.
    #[test]
    fn a_router_timeout_below_the_minimum_run_starts_no_candidate() {
        let budget = SearchBudget::with_floors(
            Duration::from_secs(900),
            Duration::from_secs(MIN_RUN_SECS),
            Duration::from_secs(MIN_FINAL_SECS),
        );
        assert_eq!(budget.candidate_limit(Duration::from_secs(10)), None);
        assert_eq!(
            budget.candidate_limit(Duration::from_secs(MIN_RUN_SECS)),
            Some(Duration::from_secs(MIN_RUN_SECS)),
            "the minimum itself is allowed"
        );
        assert!(budget.candidate_limit(Duration::from_secs(120)).is_some());
    }

    #[test]
    fn no_run_starts_when_too_little_is_left() {
        let budget = SearchBudget::with_limits(MS(5), Duration::from_secs(1));
        assert_eq!(budget.candidate_limit(Duration::from_secs(10)), None);
    }

    #[test]
    fn the_final_routes_share_is_kept_back_from_the_candidates() {
        let mut budget =
            SearchBudget::with_limits(Duration::from_secs(100), Duration::from_secs(1));
        budget.note_run(Duration::from_secs(70));
        // 100 s left, 70 s kept for the final route: a candidate gets 30 s.
        let limit = budget
            .candidate_limit(Duration::from_secs(1200))
            .expect("room");
        assert!(limit <= Duration::from_secs(30), "{limit:?}");
        // And once the reserve is more than is left, nothing starts.
        budget.note_run(Duration::from_secs(120));
        assert_eq!(budget.candidate_limit(Duration::from_secs(1200)), None);
    }

    #[test]
    fn the_final_route_gets_only_what_is_left_and_is_skipped_when_nothing_is() {
        let budget = SearchBudget::with_limits(Duration::from_secs(900), MS(10));
        let limit = budget.final_limit(Duration::from_secs(1200)).expect("room");
        assert!(limit <= Duration::from_secs(900), "{limit:?}");
        let spent = SearchBudget::with_limits(Duration::ZERO, MS(10));
        assert_eq!(
            spent.final_limit(Duration::from_secs(1200)),
            None,
            "with the budget gone the route is not started"
        );
    }

    /// The configured budget bounds the whole call: the candidate runs leave the
    /// final route's share, and a budget shorter than that runs no candidate.
    #[test]
    fn candidate_runs_and_the_final_route_together_fit_the_budget() {
        let short = SearchBudget::start(30);
        assert_eq!(
            short.candidate_limit(Duration::from_secs(1200)),
            None,
            "a 30 s budget is all final route"
        );
        let final_only = short.final_limit(Duration::from_secs(1200)).expect("room");
        assert!(final_only <= Duration::from_secs(30), "{final_only:?}");

        let long = SearchBudget::start(900);
        let run = long
            .candidate_limit(Duration::from_secs(1200))
            .expect("room");
        let fin = long.final_limit(Duration::from_secs(1200)).expect("room");
        // The longest a run can last, then what the final route is then left.
        assert!(run + Duration::from_secs(MIN_FINAL_SECS) <= Duration::from_secs(900));
        assert!(fin <= Duration::from_secs(900));
    }

    /// Runs that each take 80 ms against a 200 ms budget: the search stops
    /// starting runs once the budget is spent, and returns the best found.
    #[test]
    fn the_search_stops_starting_runs_when_the_budget_is_spent_and_keeps_the_best() {
        let mut budget = SearchBudget::with_limits(MS(200), MS(10));
        let mut limits = Vec::new();
        let candidates = [0_u8, 1, 2, 3, 4, 5];
        let costs = [9_u32, 4, 7, 1, 1, 1];
        let search = route_candidates::<_, _, ()>(
            &candidates,
            &mut budget,
            Duration::from_secs(1200),
            |&candidate, index, clock| {
                limits.push(clock.left());
                std::thread::sleep(MS(80));
                Ok(Attempt::Routed(Evaluated {
                    cost: costs[index],
                    order: vec![format!("net{candidate}")],
                }))
            },
        )
        .expect("no error")
        .expect("an engine");
        assert!(search.cut_short, "the budget ended the search");
        assert_eq!(search.total, 6);
        assert!(
            (2..=3).contains(&search.routed),
            "200 ms of 80 ms runs fits two or three: {}",
            search.routed
        );
        let (cost, candidate, order) = search.best.expect("the best so far is kept");
        assert_eq!((cost, candidate), (4, 1), "the cheapest of those routed");
        assert_eq!(order, ["net1"]);
        assert!(
            limits.windows(2).all(|pair| pair[1] <= pair[0]),
            "each run is given no more than the one before: {limits:?}"
        );
        assert!(
            limits[0] <= MS(200),
            "the first run is capped at the budget"
        );
    }

    #[test]
    fn a_search_that_fits_its_budget_routes_every_candidate_and_ties_keep_the_earlier() {
        let mut budget = SearchBudget::with_limits(Duration::from_secs(60), MS(10));
        let search = route_candidates::<_, _, ()>(
            &["a", "b", "c"],
            &mut budget,
            Duration::from_secs(10),
            |_, index, _| {
                Ok(Attempt::Routed(Evaluated {
                    cost: [2_u32, 1, 1][index],
                    order: Vec::new(),
                }))
            },
        )
        .expect("no error")
        .expect("an engine");
        assert!(!search.cut_short);
        assert_eq!(search.routed, 3);
        assert_eq!(
            search.best.expect("best").1,
            "b",
            "the earlier of two equal costs"
        );
    }

    #[test]
    fn an_unavailable_engine_ends_the_search_and_keeps_the_callers_placement() {
        let mut budget = SearchBudget::with_limits(Duration::from_secs(60), MS(10));
        let search = route_candidates::<_, u32, ()>(
            &["a", "b"],
            &mut budget,
            Duration::from_secs(10),
            |_, _, _| Ok(Attempt::EngineUnavailable),
        )
        .expect("no error");
        assert!(search.is_none());
    }

    #[test]
    fn a_budget_too_small_for_one_run_routes_nothing_and_says_so() {
        let mut budget = SearchBudget::with_limits(MS(5), Duration::from_secs(1));
        let search = route_candidates::<_, u32, ()>(
            &["a", "b"],
            &mut budget,
            Duration::from_secs(10),
            |_, _, _| panic!("no run may start"),
        )
        .expect("no error")
        .expect("an engine");
        assert!(search.cut_short);
        assert_eq!(search.routed, 0);
        assert!(search.best.is_none());
    }
}
