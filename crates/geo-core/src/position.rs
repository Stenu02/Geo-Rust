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

    /// Vero se le due coordinate vanno considerate DIVERSE
    /// E' il confronto che fa scattare il passaggio a IN MOVIMENTO.
    pub fn differs_from(&self, other: &Position) -> bool {
        (self.lat - other.lat).abs() > COORD_EPSILON_DEG
            || (self.lon - other.lon).abs() > COORD_EPSILON_DEG
    }

    /// Validazione: latitudine in [-90, 90], longitudine in [-180, 180].
    /// Da chiamare su ogni posizione che arriva dalla rete
    pub fn is_valid(&self) -> bool {
        todo!("F1 — P1")
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
