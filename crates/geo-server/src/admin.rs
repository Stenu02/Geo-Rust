//! Costruzione del resoconto richiesto dal coordinatore.
//!
//! Il calcolo avviene qui, sul server: la console riceve un resoconto finito e
//! si limita a mostrarlo.
//!
//! Il lavoro e' diviso in tre: `session` risolve il periodo e chiede i dati,
//! `storage` legge dal database, questo modulo li passa ad `analyze`.

use chrono::{DateTime, Utc};
use geo_core::analytics::analyze;
use geo_core::position::FixedPosition;
use geo_core::{MovementReport, RangeSpec, StateInterval, TimeRange, UserId};

pub struct AdminHandler;

impl AdminHandler {
    /// Costruisce il resoconto da intervalli e posizioni gia' letti dal database.
    ///
    /// `range` arriva dal chiamante, che lo ha gia' usato per le letture: non si
    /// ricalcola. `now` serve a chiudere gli intervalli ancora aperti.
    ///
    /// Le letture filtrano il periodo alla grossa; il taglio preciso agli estremi
    /// lo fa `analyze`.
    pub fn build_report(
        user: UserId,
        spec: RangeSpec,
        range: TimeRange,
        intervals: &[StateInterval],
        positions: &[FixedPosition],
        now: DateTime<Utc>,
    ) -> MovementReport {
        analyze(user, spec, range, intervals, positions, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use geo_core::geo_math::haversine_km;
    use geo_core::{Position, Timestamped, UserState};

    /// Un'interrogazione completa: dal periodo risolto fino al resoconto.
    #[test]
    fn test_admin_query_roundtrip() {
        let now = Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap();
        let range = TimeRange::resolve(RangeSpec::Today, now);

        // Tutti i dati di prova sono ancorati all'inizio del giorno risolto.
        let at = |h: i64| range.start + Duration::hours(h);

        let pos1 = Position::new(45.0, 7.0);
        let pos2 = Position::new(45.5, 7.0);

        // Un'ora in movimento seguita da un'ora di sosta.
        let intervals = [
            StateInterval::closed(UserState::Moving, at(2), at(3)),
            StateInterval::closed(UserState::Stopped, at(3), at(4)),
        ];
        let positions = [
            Timestamped::new(at(2), Position::new(45.0, 7.0)),
            Timestamped::new(at(2) + Duration::seconds(30), Position::new(45.5, 7.0)),
        ];

        // Distanza attesa, calcolata a parte per confrontarla con il resoconto.
        let distance = haversine_km(&pos1, &pos2);

        let report = AdminHandler::build_report(
            UserId(1),
            RangeSpec::Today,
            range,
            &intervals,
            &positions,
            now,
        );

        // Il resoconto riporta utente e periodo della richiesta.
        assert_eq!(report.user, UserId(1));
        assert_eq!(report.spec, RangeSpec::Today);
        assert_eq!(report.range, range);

        // Tempi nei tre stati e numero di soste.
        assert_eq!(report.moving_time, Duration::hours(1));
        assert_eq!(report.paused_time, Duration::hours(1));
        assert_eq!(report.disconnected_time, Duration::hours(0));
        assert_eq!(report.pause_count, 1);

        // Le due posizioni entrano nel tragitto e la distanza coincide.
        assert_eq!(report.track.len(), 2);
        assert!(report.distance_km > 0.0);
        assert_eq!(report.distance_km, distance);

        // Un'ora ferma su due: la media complessiva e' meta' di quella in marcia.
        assert!((report.avg_speed_overall_kmh - report.avg_speed_moving_kmh / 2.0).abs() < 1e-9);
    }
}
