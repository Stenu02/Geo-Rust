//! Geographic computations. Written by hand to avoid a dependency: it's ~15 lines.

use crate::position::Position;

/// Mean Earth radius in kilometers (IUGG).
pub const EARTH_RADIUS_KM: f64 = 6371.0088;

/// Distance in kilometers between two coordinates, Haversine formula.
///
/// Approximates the Earth as a sphere. The error versus an ellipsoidal model
/// (Vincenty) is under 0.5%, entirely irrelevant for the distances involved.
/// To be cited in the report as a deliberate choice.
///
/// Suggested test: Turin (45.0703, 7.6869) - Asti (44.9009, 8.2065) ~ 44 km.
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
        // Turin - Asti, the reference pair from the spec: roughly 44 km.
        let turin = Position::new(45.0703, 7.6869);
        let asti = Position::new(44.9009, 8.2065);
        let d = haversine_km(&turin, &asti);
        assert!((43.0..=46.0).contains(&d), "expected ~44 km, got {d}");
    }

    #[test]
    fn test_haversine_zero_distance() {
        // Same point: distance must be exactly 0.
        let p = Position::new(45.0703, 7.6869);
        assert_eq!(haversine_km(&p, &p), 0.0);
    }

    #[test]
    fn test_haversine_is_symmetric() {
        // Swapping the arguments must not change the distance.
        let turin = Position::new(45.0703, 7.6869);
        let asti = Position::new(44.9009, 8.2065);
        assert!((haversine_km(&turin, &asti) - haversine_km(&asti, &turin)).abs() < 1e-9);
    }
}
