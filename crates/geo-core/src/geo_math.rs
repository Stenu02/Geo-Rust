//! Calcoli geografici. Scritti a mano per evitare una dipendenza: sono ~15 righe.

use crate::position::Position;

/// Raggio medio terrestre in chilometri (IUGG).
pub const EARTH_RADIUS_KM: f64 = 6371.0088;

/// Distanza in chilometri fra due coordinate, formula di Haversine.
///
/// Approssima la Terra a una sfera. L'errore rispetto a un modello ellissoidico
/// (Vincenty) è sotto lo 0.5%, del tutto irrilevante per le distanze in gioco.
pub fn haversine_km(a: &Position, b: &Position) -> f64 {
    let delta_lat = (a.lat.to_radians() - b.lat.to_radians()).abs();
    let delta_lon = (a.lon.to_radians() - b.lon.to_radians()).abs();

    let h = (delta_lat / 2.0).sin().powi(2)
        + a.lat.to_radians().cos() * b.lat.to_radians().cos() * (delta_lon / 2.0).sin().powi(2);

    let c = 2.0 * h.sqrt().asin();
    EARTH_RADIUS_KM * c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haversine_known_distance() {
        // Torino - Asti, la coppia di riferimento dista circa 44 km.
        let turin = Position::new(45.0703, 7.6869);
        let asti = Position::new(44.9009, 8.2065);
        let d = haversine_km(&turin, &asti);
        assert!((43.0..=46.0).contains(&d), "atteso ~44 km, ottenuto {d}");
    }

    #[test]
    fn test_haversine_zero_distance() {
        // In caso di stesso punto la distanza deve essere esattamente 0.
        let p = Position::new(45.0703, 7.6869);
        assert_eq!(haversine_km(&p, &p), 0.0);
    }

    #[test]
    fn test_haversine_is_symmetric() {
        // Scambiare gli argomenti non deve cambiare la distanza.
        let turin = Position::new(45.0703, 7.6869);
        let asti = Position::new(44.9009, 8.2065);
        assert!((haversine_km(&turin, &asti) - haversine_km(&asti, &turin)).abs() < 1e-9);
    }
}
