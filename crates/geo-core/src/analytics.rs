//! Analisi del movimento di un utente su un periodo.
//!
//! Il calcolo avviene qui ed è invocato dal server: `geo-admin` riceve un
//! `MovementReport` già pronto e si limita a mostrarlo.

use crate::interval::{total_in, StateInterval};
use crate::position::{FixedPosition, UserId};
use crate::timerange::{RangeSpec, TimeRange};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use crate::geo_math::haversine_km;
use crate::UserState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovementReport {
    /// Chi, quale periodo è stato chiesto e su quali estremi si è risolto.
    pub user: UserId,
    pub spec: RangeSpec,
    pub range: TimeRange,

    /// La polilinea: le posizioni registrate nel periodo, in ordine cronologico.
    pub track: Vec<FixedPosition>,
    /// La distanza totale percorsa, in chilometri (somma delle tratte, Haversine).
    pub distance_km: f64,

    // --- velocita' media ---
    /// Distanza / durata del movimento. La velocità "di marcia".
    pub avg_speed_moving_kmh: f64,
    /// Distanza / (movimento + pause). La velocità "complessiva", soste incluse.
    pub avg_speed_overall_kmh: f64,

    // --- durate ---
    /// Tempo totale nello stato IN MOVIMENTO.
    pub moving_time: Duration,
    /// Tempo totale nello stato FERMO.
    pub paused_time: Duration,
    /// Tempo totale nello stato SCONNESSO. Riportato a parte: NON è una pausa.
    /// Serve anche alla verifica di quadratura (vedi `interval::total_in`).
    pub disconnected_time: Duration,

    /// Numero di soste distinte nel periodo.
    pub pause_count: usize,
}

/// Calcola il report per un utente su un periodo.
///
/// Precondizioni attese: `intervals` e `positions` riguardano UN SOLO utente e sono
/// gia' filtrati grossolanamente sul periodo dalla query SQL. Il troncamento rispetto
/// agli estremi avviene qui, tramite `StateInterval::clip_to`.
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

/// Distanza totale di una polilinea, in chilometri.
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
        // Torino -> Asti -> Torino: due tratte di ~44 km ciascuna.
        let track = [
            fp(utc(10, 0), 45.0703, 7.6869),
            fp(utc(11, 0), 44.9009, 8.2065),
            fp(utc(12, 0), 45.0703, 7.6869),
        ];
        let d = total_distance_km(&track);
        assert!((d - 88.0).abs() < 2.0, "atteso ~88 km, ottenuto {d}");
    }

    #[test]
    fn test_total_distance_km_needs_two_points() {
        assert_eq!(total_distance_km(&[]), 0.0);
        assert_eq!(total_distance_km(&[fp(utc(10, 0), 45.0, 7.0)]), 0.0);
    }

    #[test]
    fn test_avg_speed_both_definitions() {
        // 1h di movimento + 1h di pausa: la velocita' complessiva deve essere
        // esattamente la meta' di quella di marcia.
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
        // marcia = distanza / 1h; complessiva = distanza / 2h
        assert!((r.avg_speed_moving_kmh - r.distance_km).abs() < 1e-9);
        assert!((r.avg_speed_overall_kmh - r.distance_km / 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_avg_speed_zero_when_not_moving() {
        // Nessun movimento: la guardia restituisce 0.0, mai NaN da una divisione per zero.
        let range = TimeRange::new(utc(10, 0), utc(20, 0));
        let intervals = [StateInterval::closed(UserState::Stopped, utc(11, 0), utc(13, 0))];
        let positions = [fp(utc(11, 0), 45.0, 7.0)];
        let r = analyze(UserId(1), RangeSpec::Today, range, &intervals, &positions, utc(23, 0));
        assert_eq!(r.avg_speed_moving_kmh, 0.0);
        assert!(!r.avg_speed_moving_kmh.is_nan());
    }

    #[test]
    fn test_analyze_clips_track_to_range() {
        // Una posizione prima del range e' esclusa dalla track e dalla distanza.
        let range = TimeRange::new(utc(10, 0), utc(20, 0));
        let positions = [
            fp(utc(9, 0), 45.0, 7.0), // prima del range -> esclusa
            fp(utc(11, 0), 45.0, 7.0),
            fp(utc(12, 0), 45.5, 7.0),
        ];
        let r = analyze(UserId(1), RangeSpec::Today, range, &[], &positions, utc(23, 0));
        assert_eq!(r.track.len(), 2);
        assert_eq!(r.track[0].at, utc(11, 0));
    }
}
