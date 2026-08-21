//! State intervals: the representation all the analytics rest on.
//!
//! WHY INTERVALS AND NOT EVENTS: storing `(state, start, end)` instead of
//! single events makes the R10 sums direct and makes the R11 clipping trivial.
//! With events alone every query would have to reconstruct durations by hand,
//! and that is where the errors come from.

use std::cmp::{max, min};
use crate::state_machine::UserState;
use crate::timerange::TimeRange;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// A continuous period during which a user stayed in a given state.
///
/// `end == None` means "still-open interval" (the current state).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StateInterval {
    pub state: UserState,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
}


impl StateInterval {
    pub fn open(state: UserState, start: DateTime<Utc>) -> Self {
        Self { state, start, end: None }
    }

    pub fn closed(state: UserState, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { state, start, end: Some(end) }
    }

    pub fn is_open(&self) -> bool {
        self.end.is_none()
    }

    /// The end used for computations: the real end, or `now` for a still-open interval.
    fn effective_end(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        self.end.unwrap_or(now)
    }

    /// Interval duration. For a still-open interval `now` is needed to close it
    /// virtually: here too time comes in as a parameter, never from `Utc::now()`.
    pub fn duration(&self, now: DateTime<Utc>) -> Duration {
        self.effective_end(now) - self.start
    }

    /// Intersects the interval with a period, returning `None` if they do not overlap.
    ///
    /// IT IS THE CRITICAL POINT OF R11. Example: a user is moving from 23:40 to 00:20.
    /// Without clipping, the "current day" analysis also counts the 20 minutes that
    /// belong to the previous day.
    ///
    /// ```text
    /// period:       [--------- current day ---------]
    /// case A:              [---- movement ----]           -> all of it
    /// case B:  [---movement---]                           -> only the part inside
    /// case C:                            [---movement---] -> only the part inside
    /// case D:  [------------- movement -------------]     -> the whole period
    /// case E: [--mov--]                                   -> None
    /// ```
    pub fn clip_to(&self, range: &TimeRange, now: DateTime<Utc>) -> Option<StateInterval> {
        let clipped_start = max(self.start, range.start);
        let clipped_end = min(self.effective_end(now), range.end);

        if clipped_start < clipped_end {
            return Some(Self::closed(self.state, clipped_start, clipped_end));
        }
        None
    }
}

/// Sums the duration of the intervals in a given state, within a period.
///
/// CORRECTNESS CHECK to use as a test: for any period,
/// `total_in(Moving) + total_in(Stopped) + total_in(Disconnected)` must give
/// EXACTLY the duration of the period (or the time since the user's first connection).
/// If it does not add up, there is a gap in the intervals or an error in the clipping.
pub fn total_in(
    intervals: &[StateInterval],
    state: UserState,
    range: &TimeRange,
    now: DateTime<Utc>,
) -> Duration {
    let mut total_duration = Duration::zero();
    for interval in intervals {
        if interval.state == state {
            if let Some(clipped) = interval.clip_to(range, now) {
                total_duration+= clipped.duration(now);
            }
        }
    }
    total_duration
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// A UTC instant on a fixed reference day (2026-08-20). Clipping is timezone
    /// agnostic, so raw UTC is enough here.
    fn utc(h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 20, h, mi, 0).unwrap()
    }

    fn moving(start: DateTime<Utc>, end: DateTime<Utc>) -> StateInterval {
        StateInterval::closed(UserState::Moving, start, end)
    }

    // The analysis period used by the clip cases: [10:00, 20:00).
    fn day_range() -> TimeRange {
        TimeRange::new(utc(10, 0), utc(20, 0))
    }

    #[test]
    fn test_clip_case_a_inside() {
        let iv = moving(utc(12, 0), utc(15, 0));
        let c = iv.clip_to(&day_range(), utc(23, 0)).unwrap();
        assert_eq!((c.start, c.end), (utc(12, 0), Some(utc(15, 0))));
    }

    #[test]
    fn test_clip_case_b_starts_before() {
        let iv = moving(utc(8, 0), utc(12, 0));
        let c = iv.clip_to(&day_range(), utc(23, 0)).unwrap();
        assert_eq!((c.start, c.end), (utc(10, 0), Some(utc(12, 0))));
    }

    #[test]
    fn test_clip_case_c_ends_after() {
        let iv = moving(utc(18, 0), utc(22, 0));
        let c = iv.clip_to(&day_range(), utc(23, 0)).unwrap();
        assert_eq!((c.start, c.end), (utc(18, 0), Some(utc(20, 0))));
    }

    #[test]
    fn test_clip_case_d_covers_period() {
        let iv = moving(utc(8, 0), utc(23, 0));
        let c = iv.clip_to(&day_range(), utc(23, 0)).unwrap();
        assert_eq!((c.start, c.end), (utc(10, 0), Some(utc(20, 0))));
    }

    #[test]
    fn test_clip_case_e_no_overlap() {
        assert!(moving(utc(21, 0), utc(22, 0)).clip_to(&day_range(), utc(23, 0)).is_none());
        // Touching the boundary exactly is NOT an overlap (half-open, strict `<`).
        assert!(moving(utc(6, 0), utc(10, 0)).clip_to(&day_range(), utc(23, 0)).is_none());
    }

    #[test]
    fn test_interval_clipping_at_midnight() {
        // The doc example: moving 23:40 -> 00:20; the "current day" (2026-08-20)
        // must count only the 20 minutes after midnight.
        let range = TimeRange::new(utc(0, 0), Utc.with_ymd_and_hms(2026, 8, 21, 0, 0, 0).unwrap());
        let iv = moving(
            Utc.with_ymd_and_hms(2026, 8, 19, 23, 40, 0).unwrap(),
            utc(0, 20),
        );
        let c = iv.clip_to(&range, utc(12, 0)).unwrap();
        assert_eq!(c.start, utc(0, 0));
        assert_eq!(c.duration(utc(12, 0)), Duration::minutes(20));
    }

    #[test]
    fn test_clip_open_interval_uses_now() {
        // Open interval starting at 15:00, still open: clips at `now`.
        let iv = StateInterval::open(UserState::Moving, utc(15, 0));
        let c = iv.clip_to(&day_range(), utc(18, 0)).unwrap();
        assert_eq!((c.start, c.end), (utc(15, 0), Some(utc(18, 0))));
    }

    #[test]
    fn test_duration_open_vs_closed() {
        assert_eq!(moving(utc(12, 0), utc(15, 0)).duration(utc(23, 0)), Duration::hours(3));
        let open = StateInterval::open(UserState::Moving, utc(12, 0));
        assert_eq!(open.duration(utc(14, 0)), Duration::hours(2));
    }

    #[test]
    fn test_total_in_balances() {
        // Contiguous intervals partitioning the timeline; over [10:00, 20:00) the
        // three states must sum to exactly the period duration (the balance check).
        let now = utc(23, 0);
        let range = day_range();
        let intervals = [
            StateInterval::closed(UserState::Disconnected, utc(9, 0), utc(10, 30)),
            StateInterval::closed(UserState::Stopped, utc(10, 30), utc(12, 0)),
            StateInterval::closed(UserState::Moving, utc(12, 0), utc(18, 0)),
            StateInterval::closed(UserState::Stopped, utc(18, 0), utc(21, 0)),
        ];

        let moving_t = total_in(&intervals, UserState::Moving, &range, now);
        let stopped_t = total_in(&intervals, UserState::Stopped, &range, now);
        let disc_t = total_in(&intervals, UserState::Disconnected, &range, now);

        assert_eq!(moving_t, Duration::hours(6));
        assert_eq!(stopped_t, Duration::minutes(90) + Duration::hours(2));
        assert_eq!(disc_t, Duration::minutes(30));
        // The invariant that catches gaps and off-by-one clipping errors:
        assert_eq!(moving_t + stopped_t + disc_t, range.duration());
    }
}
