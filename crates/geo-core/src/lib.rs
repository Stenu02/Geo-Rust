//! GeoRuggine domain: what the data means and which rules govern it.
//!
//! BOUNDARY RULE (holds for every file in this crate):
//! no function in here reads the clock, opens a socket or touches the filesystem.
//! Time is ALWAYS passed in as a parameter.
//!
//! Practical consequence: a four-hour trip can be simulated in two milliseconds,
//! so the 3-minute rule (R7) is testable without waiting 3 minutes.
//!
//! Requirements covered: R5, R6, R7, R8, R9, R10, R11.

pub mod analytics;
pub mod geo_math;
pub mod interval;
pub mod position;
pub mod state_machine;
pub mod timerange;

pub use analytics::MovementReport;
pub use interval::StateInterval;
pub use position::{Position, Timestamped, UserId};
pub use state_machine::{StateMachine, Transition, UserState};
pub use timerange::{RangeSpec, TimeRange};

use chrono::Duration;

/// Position transmission interval (R4). Value fixed by the specification.
pub const POSITION_INTERVAL_SECS: i64 = 30;

/// Time of unchanged coordinates after which MOVING -> STOPPED (R7).
pub const STOP_THRESHOLD_SECS: i64 = 180;

/// Tolerance when comparing two coordinates.
///
/// Comparing `f64` with `==` is fragile. 1e-6 degrees is roughly 11 cm:
/// fine enough not to mask real movement, coarse enough to absorb
/// representation noise. To be mentioned in the report (R6).
pub const COORD_EPSILON_DEG: f64 = 1e-6;

/// Company time zone. "Current day / week / month" (R11) are LOCAL concepts:
/// `TimeRange::resolve` finds the local midnight / week start / month start in this
/// zone and stores the boundaries back as UTC.
///
/// Fixed explicitly instead of reading the machine's zone via `chrono::Local`: this
/// keeps `resolve` a pure function of `now` (the crate's boundary rule), so the R11
/// tests are deterministic on any host, CI included, and the "day" is the company's
/// day rather than wherever the server happens to run. Also gives correct CET/CEST
/// DST handling. See docs/decisioni.md.
pub const COMPANY_TZ: chrono_tz::Tz = chrono_tz::Europe::Rome;

pub fn position_interval() -> Duration {
    Duration::seconds(POSITION_INTERVAL_SECS)
}

pub fn stop_threshold() -> Duration {
    Duration::seconds(STOP_THRESHOLD_SECS)
}
