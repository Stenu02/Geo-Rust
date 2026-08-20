//! Calcoli geografici. Scritti a mano per evitare una dipendenza: sono ~15 righe.

use crate::position::Position;

/// Raggio medio terrestre in chilometri .
pub const EARTH_RADIUS_KM: f64 = 6371.0088;

/// Distanza in chilometri fra due coordinate, formula di Haversine.

pub fn haversine_km(_a: &Position, _b: &Position) -> f64 {
    todo!("F1 — P1")
}
