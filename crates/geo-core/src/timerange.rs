//! Programmable analysis periods (R11).
//!
//! DECISIONS TO DOCUMENT IN THE REPORT:
//! - local time zone (not UTC): "current day" is a local concept.
//!   Instants stay in UTC internally, the conversion happens only here.
//! - ISO week: Monday to Sunday
//! - month: calendar month
//! - watch out for daylight saving time: on switch days a "day" lasts 23 or 25 hours

use chrono::{DateTime, Duration, Utc, Datelike, Months, TimeZone, NaiveDate};
use serde::{Deserialize, Serialize};
use crate::COMPANY_TZ;

/// Which period is to be analyzed. It is what the administrator selects
/// in the interface and what travels through the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RangeSpec {
    Today,
    CurrentWeek,
    CurrentMonth,
}

impl RangeSpec {
    pub fn label(&self) -> &'static str {
        match self {
            RangeSpec::Today => "current day",
            RangeSpec::CurrentWeek => "current week",
            RangeSpec::CurrentMonth => "current month",
        }
    }
}

/// A concrete period, with absolute endpoints. Half-open: `[start, end)`.
///
/// Half-open and not closed because otherwise the midnight instant
/// would belong to two days and be counted twice.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

fn local_midnight_to_utc(date: NaiveDate) -> DateTime<Utc> {
    let naive = date.and_hms_opt(0, 0, 0).unwrap();
    COMPANY_TZ
        .from_local_datetime(&naive)
        .single()  // midnight is unambiguous in Europe/Rome
        .expect("local midnight always exists in Europe/Rome")
        .with_timezone(&Utc)
}

impl TimeRange {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { start, end }
    }

    /// Resolves a `RangeSpec` into a concrete period, relative to a reference instant.
    ///
    /// `now` is a parameter, not `Utc::now()`: it is what makes it possible to test
    /// the behavior across midnight, month-end and the daylight-saving switch
    /// without waiting for them to happen.
    pub fn resolve(spec: RangeSpec, now: DateTime<Utc>) -> TimeRange {
        match spec {
            RangeSpec::Today => {
                let today = now.with_timezone(&COMPANY_TZ).date_naive();
                let start = local_midnight_to_utc(today);
                let end = local_midnight_to_utc(today.succ_opt().unwrap());
                TimeRange::new(start, end)
            }
            RangeSpec::CurrentWeek => {
                let today = now.with_timezone(&COMPANY_TZ).date_naive();
                let dfm = today.weekday().num_days_from_monday();
                let monday = today - Duration::days(dfm as i64);
                let next_monday = monday + Duration::days(7);

                let start = local_midnight_to_utc(monday);
                let end = local_midnight_to_utc(next_monday);
                TimeRange::new(start, end)
            }
            RangeSpec::CurrentMonth => {
                let today = now.with_timezone(&COMPANY_TZ).date_naive();
                let first = today.with_day(1).unwrap();
                let next_month_first = first.checked_add_months(Months::new(1)).unwrap();

                let start = local_midnight_to_utc(first);
                let end = local_midnight_to_utc(next_month_first);
                TimeRange::new(start, end)
            }
        }
    }

    pub fn duration(&self) -> Duration {
        self.end - self.start
    }

    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        at >= self.start && at < self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a UTC instant from a wall-clock time in the company zone.
    fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        COMPANY_TZ
            .with_ymd_and_hms(y, mo, d, h, mi, s)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn test_today_boundaries() {
        // Thursday, an ordinary 24-hour day.
        let now = local(2026, 8, 20, 15, 30, 0);
        let r = TimeRange::resolve(RangeSpec::Today, now);
        assert_eq!(r.start, local(2026, 8, 20, 0, 0, 0));
        assert_eq!(r.end, local(2026, 8, 21, 0, 0, 0));
        assert_eq!(r.duration(), Duration::hours(24));
    }

    #[test]
    fn test_current_week_starts_monday() {
        // Thursday 2026-08-20 -> [Mon 17, next Mon 24).
        let now = local(2026, 8, 20, 9, 0, 0);
        let r = TimeRange::resolve(RangeSpec::CurrentWeek, now);
        assert_eq!(r.start, local(2026, 8, 17, 0, 0, 0));
        assert_eq!(r.end, local(2026, 8, 24, 0, 0, 0));
        assert_eq!(r.duration(), Duration::days(7));
    }

    #[test]
    fn test_current_month_boundaries() {
        let now = local(2026, 8, 20, 9, 0, 0);
        let r = TimeRange::resolve(RangeSpec::CurrentMonth, now);
        assert_eq!(r.start, local(2026, 8, 1, 0, 0, 0));
        assert_eq!(r.end, local(2026, 9, 1, 0, 0, 0));
    }

    #[test]
    fn test_current_month_december_rollover() {
        // Why checked_add_months: December must roll into next January.
        let now = local(2026, 12, 15, 9, 0, 0);
        let r = TimeRange::resolve(RangeSpec::CurrentMonth, now);
        assert_eq!(r.start, local(2026, 12, 1, 0, 0, 0));
        assert_eq!(r.end, local(2027, 1, 1, 0, 0, 0));
    }

    #[test]
    fn test_today_dst_spring_forward_is_23h() {
        // Last Sunday of March 2026: clocks jump 02:00 -> 03:00, the day is 23h.
        let now = local(2026, 3, 29, 12, 0, 0);
        let r = TimeRange::resolve(RangeSpec::Today, now);
        assert_eq!(r.duration(), Duration::hours(23));
    }

    #[test]
    fn test_today_dst_fall_back_is_25h() {
        // Last Sunday of October 2026: clocks fall 03:00 -> 02:00, the day is 25h.
        let now = local(2026, 10, 25, 12, 0, 0);
        let r = TimeRange::resolve(RangeSpec::Today, now);
        assert_eq!(r.duration(), Duration::hours(25));
    }
}
