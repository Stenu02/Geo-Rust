//! Analisi del movimento di un utente su un periodo.
//!
//! Il calcolo avviene qui ed è invocato dal server: `geo-admin` riceve un
//! `MovementReport` già pronto e si limita a mostrarlo.
//! Requisiti Funzionali rispettati: RF8, RF9, RF10

use crate::geo_math::haversine_km;
use crate::interval::{total_in, StateInterval};
use crate::position::{FixedPosition, UserId};
use crate::timerange::{RangeSpec, TimeRange};
use crate::UserState;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

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
    /// Tempo totale nello stato SCONNESSO. Riportato a parte: non è una pausa.
    /// Serve anche alla verifica di quadratura (vedi `interval::total_in`).
    pub disconnected_time: Duration,

    /// Numero di soste distinte nel periodo.
    pub pause_count: usize,
}

/// Calcola il report per un utente su un periodo.
///
/// Precondizioni attese: `intervals` e `positions` riguardano un solo utente e sono
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
    let track: Vec<FixedPosition> = positions
        .iter()
        .copied()
        .filter(|p| range.contains(p.at))
        .collect();
    let distance_km = total_distance_km(&track);

    let moving_time = total_in(intervals, UserState::Moving, &range, now);
    let paused_time = total_in(intervals, UserState::Stopped, &range, now);
    let disconnected_time = total_in(intervals, UserState::Disconnected, &range, now);

    let pause_count = intervals
        .iter()
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

/// Distanza totale di una polilinea, espressa in chilometri.
///
///Utilizzo `fold` al posto di`sum()`poiché su un iteratore vuoto
/// (meno di due posizioni) `sum()` per gli `f64` restituisce `-0.0`, che nel
/// resoconto verrebbe mostrato come "-0.00 km".
pub fn total_distance_km(positions: &[FixedPosition]) -> f64 {
    positions
        .windows(2)
        .filter(|w| w[1].at - w[0].at <= Duration::seconds(crate::MAX_TRACKING_GAP_SECS))
        .map(|w| haversine_km(&w[0].value, &w[1].value))
        .fold(0.0, |totale, tratta| totale + tratta)
}

pub fn avg_speed(distance_km: f64, time: Duration) -> f64 {
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
        // Tre punti lungo una via di Torino, a 30 s l'uno dall'altro: il tragitto
        // è la somma delle due tratte.
        let t0 = utc(10, 0);
        let track = [
            fp(t0, 45.0618513, 7.6797161),
            fp(t0 + Duration::seconds(30), 45.0601242, 7.6851330),
            fp(t0 + Duration::seconds(60), 45.0577891, 7.6923447),
        ];
        let attesa = haversine_km(&track[0].value, &track[1].value)
            + haversine_km(&track[1].value, &track[2].value);
        assert!((total_distance_km(&track) - attesa).abs() < 1e-9);
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
        let positions = [fp(utc(11, 0), 45.0, 7.0), fp(utc(11, 0) + Duration::seconds(30), 45.001, 7.0),];
        let r = analyze(
            UserId(1),
            RangeSpec::Today,
            range,
            &intervals,
            &positions,
            utc(23, 0),
        );

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
        let intervals = [StateInterval::closed(
            UserState::Stopped,
            utc(11, 0),
            utc(13, 0),
        )];
        let positions = [fp(utc(11, 0), 45.0, 7.0)];
        let r = analyze(
            UserId(1),
            RangeSpec::Today,
            range,
            &intervals,
            &positions,
            utc(23, 0),
        );
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
        let r = analyze(
            UserId(1),
            RangeSpec::Today,
            range,
            &[],
            &positions,
            utc(23, 0),
        );
        assert_eq!(r.track.len(), 2);
        assert_eq!(r.track[0].at, utc(11, 0));
    }

    #[test]
    fn test_report_senza_posizioni_non_ha_zeri_negativi() {
        // In IEEE 754 -0.0 == 0.0 e' vero: per questo un assert_eq! con 0.0 non si
        // accorge dello zero negativo, che pero' a schermo diventa "-0.00".
        assert!(total_distance_km(&[]).is_sign_positive());
        assert!(total_distance_km(&[fp(utc(10, 0), 45.0, 7.0)]).is_sign_positive());

        // Un'ora di movimento registrata, ma nessuna posizione nel periodo.
        let range = TimeRange::new(utc(10, 0), utc(20, 0));
        let intervals = [StateInterval::closed(UserState::Moving, utc(11, 0), utc(12, 0))];
        let r = analyze(UserId(1), RangeSpec::Today, range, &intervals, &[], utc(23, 0));
        assert_eq!(format!("{:.2}", r.distance_km), "0.00");
        assert_eq!(format!("{:.2}", r.avg_speed_moving_kmh), "0.00");
        assert_eq!(format!("{:.2}", r.avg_speed_overall_kmh), "0.00");
    }

    #[test]
    fn test_tratta_con_un_buco_nel_tracciamento_non_conta() {
        // Due punti a 30 s: la tratta conta. Poi un'ora senza posizioni (client
        // spento, server riavviato...): non sappiamo che strada abbia fatto, quindi
        // il salto fino al punto successivo non entra nel tragitto.
        let track = [
            fp(utc(10, 0), 45.0, 7.0),
            fp(utc(10, 0) + Duration::seconds(30), 45.001, 7.0),
            fp(utc(11, 0), 45.5, 7.0),
        ];
        let prima_tratta = haversine_km(&track[0].value, &track[1].value);
        assert!((total_distance_km(&track) - prima_tratta).abs() < 1e-9);
    }
}
