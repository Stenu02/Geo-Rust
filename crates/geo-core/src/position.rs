//! Geographic coordinates and shared vocabulary.

use crate::COORD_EPSILON_DEG;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// User identifier. A newtype instead of a bare `i64`: it prevents accidentally
/// passing a message id where a user id is expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UserId(pub i64);

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// User role (R12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Trucker,
    Admin,
}

/// A geographic coordinate in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub lat: f64,
    pub lon: f64,
}

impl Position {
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    /// True if the two coordinates should be considered DIFFERENT (R6).
    ///
    /// Do not use `==`: see COORD_EPSILON_DEG.
    pub fn differs_from(&self, other: &Position) -> bool {
        (self.lat - other.lat).abs() > COORD_EPSILON_DEG
            || (self.lon - other.lon).abs() > COORD_EPSILON_DEG
    }

    /// Validation: latitude in [-90, 90], longitude in [-180, 180].
    pub fn is_valid(&self) -> bool {
        if self.lat < -90.0 || self.lon < -180.0
        || self.lat > 90.0 || self.lon > 180.0{
            false
        } else {
            true
        }

    }
}

/// A value together with the instant it refers to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Timestamped<T> {
    pub at: DateTime<Utc>,
    pub value: T,
}

impl<T> Timestamped<T> {
    pub fn new(at: DateTime<Utc>, value: T) -> Self {
        Self { at, value }
    }
}

/// Convenience alias: a position with its instant.
pub type FixedPosition = Timestamped<Position>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_accepts_normal_point() {
        assert!(Position::new(45.0703, 7.6869).is_valid());
    }

    #[test]
    fn test_is_valid_accepts_boundaries() {
        // The range is inclusive at both ends.
        assert!(Position::new(90.0, 180.0).is_valid());
        assert!(Position::new(-90.0, -180.0).is_valid());
    }

    #[test]
    fn test_is_valid_rejects_out_of_range() {
        assert!(!Position::new(90.1, 0.0).is_valid()); // latitude too high
        assert!(!Position::new(-90.1, 0.0).is_valid()); // latitude too low
        assert!(!Position::new(0.0, 180.1).is_valid()); // longitude too high
        assert!(!Position::new(0.0, -180.1).is_valid()); // longitude too low
    }

    #[test]
    fn test_differs_from_uses_epsilon() {
        let p = Position::new(45.0, 7.0);
        // A change below epsilon (~11 cm) is not a difference (R6).
        assert!(!p.differs_from(&Position::new(45.0 + 1e-9, 7.0)));
        // A clearly larger change is.
        assert!(p.differs_from(&Position::new(45.001, 7.0)));
    }
}
