//! Gestione delle interrogazioni dell'amministratore (R12a).
//!
//! Trasforma un `AdminMessage::Query` in un `MovementReport` gia' pronto.
//!
//! IL CALCOLO AVVIENE QUI, non nel client amministratore: il professore e' stato
//! esplicito ("Devi rendere disponibile una applicazione lato server che fa
//! interrogazioni"). `geo-admin` riceve un report finito e si limita a mostrarlo.
//!
//! Divisione delle responsabilita':
//! - la lettura dal DB (`storage::positions_for` / `intervals_for`) e' dell'attore
//!   storage.
//! - risoluzione del range in session.rs (chiamante)
//! - AdminHandler si occupa di chiamare `analyze` e di restituire il report generato.
//!
//! Il flusso completo e' implementato in `session::admin_loop`, ramo `Query`
//!
//! Le due letture di storage filtrano solo grossolanamente sul periodo (posizioni
//! dentro `[start, end)`, intervalli che si sovrappongono al range); il troncamento
//! fine agli estremi lo fa poi `analyze` tramite `StateInterval::clip_to`.

use chrono::{DateTime, Utc};
use geo_core::analytics::analyze;
use geo_core::position::FixedPosition;
use geo_core::{MovementReport, RangeSpec, StateInterval, TimeRange, UserId};

pub struct AdminHandler;

impl AdminHandler {
    /// Costruisce il report per una interrogazione, dal periodo gia' risolto e da
    /// intervalli e posizioni dell'utente gia' letti dal DB (vedi il flusso sopra).
    ///
    /// `range` arriva dal chiamante, che lo ha gia' usato per le letture di storage:
    /// non viene ricalcolato qui. `now` serve ad `analyze` per chiudere virtualmente
    /// gli intervalli ancora aperti.
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
    use geo_core::{Position, Timestamped, UserState};
    use geo_core::geo_math::haversine_km;

    #[test]
    fn test_admin_query_roundtrip() {
        // Una interrogazione completa dagli input al report: risolve il periodo, poi analizza.
        let now = Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap();
        let range = TimeRange::resolve(RangeSpec::Today, now);
        // Ancoriamo i dati dentro il giorno risolto, relativi al suo inizio.
        let at = |h: i64| range.start + Duration::hours(h);

        let pos1 =  Position::new(45.0, 7.0);
        let pos2 =  Position::new(45.5, 7.0);

        let intervals = [
            StateInterval::closed(UserState::Moving, at(2), at(3)),
            StateInterval::closed(UserState::Stopped, at(3), at(4)),
        ];
        let positions = [
            Timestamped::new(at(2), Position::new(45.0, 7.0)),
            Timestamped::new(at(3), Position::new(45.5, 7.0)),
        ];

        let distance= haversine_km(&pos1, &pos2);

        let report = AdminHandler::build_report(
            UserId(1), RangeSpec::Today, range, &intervals, &positions, now,
        );

        // I metadati della query sono riportati e il periodo e' quello risolto.
        assert_eq!(report.user, UserId(1));
        assert_eq!(report.spec, RangeSpec::Today);
        assert_eq!(report.range, range);

        // check sui tempi nei diversi stati
        assert_eq!(report.moving_time, Duration::hours(1));
        assert_eq!(report.paused_time, Duration::hours(1));
        assert_eq!(report.disconnected_time, Duration::hours(0));
        assert_eq!(report.pause_count, 1);

        //check sulle posizioni e la distanza percorsa
        assert_eq!(report.track.len(), 2);
        assert!(report.distance_km > 0.0);
        assert_eq!(report.distance_km, distance);

        // check sulla distanza complessiva e su quella di marcia
        assert!((report.avg_speed_overall_kmh - report.avg_speed_moving_kmh / 2.0).abs() < 1e-9);
    }
}
