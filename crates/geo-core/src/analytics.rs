//! Movement analysis (R8, R9, R10, R11).
//!
//! The computation happens here, and it is invoked by the SERVER: the professor was
//! explicit ("You must provide a server-side application that runs the queries").
//! `geo-admin` receives a ready-made `MovementReport` and just displays it.

use crate::interval::{total_in, StateInterval};
use crate::position::{FixedPosition, UserId};
use crate::timerange::{RangeSpec, TimeRange};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use crate::geo_math::haversine_km;
use crate::UserState;

/// The result of a query. It travels over the wire to the administrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovementReport {
    pub user: UserId,
    pub spec: RangeSpec,
    pub range: TimeRange,

    /// The polyline: the positions recorded in the period, in chronological order.
    pub track: Vec<FixedPosition>,
    /// The total distance traveled, in kilometers (sum of the legs, Haversine).
    pub distance_km: f64,

    // --- R9: average speed ---
    /// distance / movement duration. The "driving" speed.
    pub avg_speed_moving_kmh: f64,
    /// distance / (movement + pauses). The "overall" speed, rests included.
    pub avg_speed_overall_kmh: f64,

    // --- R10: durations ---
    /// Total time in the MOVING state.
    pub moving_time: Duration,
    /// Total time in the STOPPED state.
    pub paused_time: Duration,
    /// Total time in the DISCONNECTED state. Reported separately: it is NOT a pause.
    /// Also used by the balance check (see `interval::total_in`).
    pub disconnected_time: Duration,

    /// Number of distinct pauses in the period. Not required, but it is the information
    /// a real administrator would want next to the total duration.
    pub pause_count: usize,
}

/// Computes the report for a user over a period.
///
/// Expected preconditions: `intervals` and `positions` concern A SINGLE user and are
/// already coarsely filtered on the period by the SQL query. The fine clipping at the
/// endpoints happens here, via `StateInterval::clip_to`.
pub fn analyze(
    user: UserId,
    spec: RangeSpec,
    range: TimeRange,
    intervals: &[StateInterval],
    positions: &[FixedPosition],
    now: DateTime<Utc>,
) -> MovementReport {
    let track: Vec<FixedPosition> = positions.iter().copied()
        .filter(|p| range.contains(p.at)).collect();
    let distance_km = total_distance_km(&track);

    let moving_time = total_in(intervals, UserState::Moving, &range, now);
    let paused_time = total_in(intervals, UserState::Stopped, &range, now);
    let disconnected_time = total_in(intervals, UserState::Disconnected, &range, now);

    let pause_count = intervals.iter()
        .filter(|i| i.state == UserState::Stopped && i.clip_to(&range, now).is_some())
        .count();

    let avg_speed_moving_kmh = avg_speed(distance_km, moving_time);
    let avg_speed_overall_kmh = avg_speed(distance_km, moving_time + paused_time);

    MovementReport {
        user,
        spec,
        range,
        track,
        distance_km,
        avg_speed_moving_kmh,
        avg_speed_overall_kmh,
        moving_time,
        paused_time,
        disconnected_time,
        pause_count,
    }
}

/// Total distance of a polyline, in kilometers.
pub fn total_distance_km(positions: &[FixedPosition]) -> f64 {
    positions.windows(2).map(|w| haversine_km(&w[0].value, &w[1].value)).sum()
}

pub fn avg_speed(distance_km: f64, time:Duration) -> f64 {
    let hours = time.num_seconds() as f64 / 3600.0;
    if hours > 0.0 {
        distance_km / hours
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::{Position, Timestamped};
    use chrono::TimeZone;

    fn utc(h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 20, h, mi, 0).unwrap()
    }

    fn fp(at: DateTime<Utc>, lat: f64, lon: f64) -> FixedPosition {
        Timestamped::new(at, Position::new(lat, lon))
    }

    #[test]
    fn test_total_distance_km_sums_legs() {
        // Turin -> Asti -> Turin: two legs of ~44 km each.
        let track = [
            fp(utc(10, 0), 45.0703, 7.6869),
            fp(utc(11, 0), 44.9009, 8.2065),
            fp(utc(12, 0), 45.0703, 7.6869),
        ];
        let d = total_distance_km(&track);
        assert!((d - 88.0).abs() < 2.0, "expected ~88 km, got {d}");
    }

    #[test]
    fn test_total_distance_km_needs_two_points() {
        assert_eq!(total_distance_km(&[]), 0.0);
        assert_eq!(total_distance_km(&[fp(utc(10, 0), 45.0, 7.0)]), 0.0);
    }

    #[test]
    fn test_avg_speed_both_definitions() {
        // 1h moving + 1h paused: the overall speed must be exactly half the driving one.
        let range = TimeRange::new(utc(10, 0), utc(20, 0));
        let intervals = [
            StateInterval::closed(UserState::Moving, utc(11, 0), utc(12, 0)),
            StateInterval::closed(UserState::Stopped, utc(12, 0), utc(13, 0)),
        ];
        let positions = [fp(utc(11, 0), 45.0, 7.0), fp(utc(12, 0), 45.5, 7.0)];
        let r = analyze(UserId(1), RangeSpec::Today, range, &intervals, &positions, utc(23, 0));

        assert_eq!(r.moving_time, Duration::hours(1));
        assert_eq!(r.paused_time, Duration::hours(1));
        assert!(r.distance_km > 0.0);
        // moving = distance / 1h; overall = distance / 2h
        assert!((r.avg_speed_moving_kmh - r.distance_km).abs() < 1e-9);
        assert!((r.avg_speed_overall_kmh - r.distance_km / 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_avg_speed_zero_when_not_moving() {
        // No movement: the guard returns 0.0, never NaN from a divide-by-zero.
        let range = TimeRange::new(utc(10, 0), utc(20, 0));
        let intervals = [StateInterval::closed(UserState::Stopped, utc(11, 0), utc(13, 0))];
        let positions = [fp(utc(11, 0), 45.0, 7.0)];
        let r = analyze(UserId(1), RangeSpec::Today, range, &intervals, &positions, utc(23, 0));
        assert_eq!(r.avg_speed_moving_kmh, 0.0);
        assert!(!r.avg_speed_moving_kmh.is_nan());
    }

    #[test]
    fn test_analyze_clips_track_to_range() {
        // A position before the range is excluded from both the track and the distance.
        let range = TimeRange::new(utc(10, 0), utc(20, 0));
        let positions = [
            fp(utc(9, 0), 45.0, 7.0), // before range -> excluded
            fp(utc(11, 0), 45.0, 7.0),
            fp(utc(12, 0), 45.5, 7.0),
        ];
        let r = analyze(UserId(1), RangeSpec::Today, range, &[], &positions, utc(23, 0));
        assert_eq!(r.track.len(), 2);
        assert_eq!(r.track[0].at, utc(11, 0));
    }
}
