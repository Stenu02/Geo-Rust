//! Coordinate geografiche e vocabolario condiviso.

use crate::COORD_EPSILON_DEG;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Identificatore di un utente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UserId(pub i64);

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Ruolo dell'utente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Trucker,
    Admin,
}

/// Una coordinata geografica in gradi decimali.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub lat: f64,
    pub lon: f64,
}

impl Position {
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    /// Vero se le due coordinate vanno considerate DIVERSE.
    ///
    /// Non usare `==`: vedi COORD_EPSILON_DEG.
    pub fn differs_from(&self, other: &Position) -> bool {
        (self.lat - other.lat).abs() > COORD_EPSILON_DEG
            || (self.lon - other.lon).abs() > COORD_EPSILON_DEG
    }

    /// Validazione: latitudine in [-90, 90], longitudine in [-180, 180].
    pub fn is_valid(&self) -> bool {
        if self.lat < -90.0 || self.lon < -180.0
        || self.lat > 90.0 || self.lon > 180.0
        || self.lat.is_nan() || self.lon.is_nan() {
            false
        } else {
            true
        }

    }
}

/// Un valore con l'istante a cui si riferisce.
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

/// Alias di comodo: una posizione con il suo istante.
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
        // Il range e' inclusivo a entrambi gli estremi.
        assert!(Position::new(90.0, 180.0).is_valid());
        assert!(Position::new(-90.0, -180.0).is_valid());
    }

    #[test]
    fn test_is_valid_rejects_out_of_range() {
        assert!(!Position::new(90.1, 0.0).is_valid()); // latitudine troppo alta
        assert!(!Position::new(-90.1, 0.0).is_valid()); // latitudine troppo bassa
        assert!(!Position::new(0.0, 180.1).is_valid()); // longitudine troppo alta
        assert!(!Position::new(0.0, -180.1).is_valid()); // longitudine troppo bassa
        assert!(!Position::new(f64::NAN, 7.0).is_valid()); //latitudine non valida
        assert!(!Position::new(45.0, f64::NAN).is_valid()); //longitudine non valida
    }

    #[test]
    fn test_differs_from_uses_epsilon() {
        let p = Position::new(45.0, 7.0);
        // Un cambiamento sotto l'epsilon (~11 cm) non e' una differenza.
        assert!(!p.differs_from(&Position::new(45.0 + 1e-9, 7.0)));
        // Uno chiaramente piu' grande si'.
        assert!(p.differs_from(&Position::new(45.001, 7.0)));
    }
}
