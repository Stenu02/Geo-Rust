//! Analisi del movimento di un utente su un periodo.
//!
//! Il calcolo avviene qui ed e' invocato dal server

use crate::interval::StateInterval;
use crate::position::{FixedPosition, UserId};
use crate::timerange::{RangeSpec, TimeRange};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovementReport {
    /// Chi, quale periodo e' stato chiesto e su quali estremi si e' risolto.
    pub user: UserId,
    pub spec: RangeSpec,
    pub range: TimeRange,

    /// Le posizioni registrate nel periodo, in ordine cronologico.
    pub track: Vec<FixedPosition>,

    /// Distanza totale percorsa lungo quelle posizioni, in km.
    pub distance_km: f64,

    /// Distanza / tempo in movimento: la velocita' di marcia.
    pub avg_speed_moving_kmh: f64,

    /// Distanza / (movimento + soste): la velocita' comprensiva delle fermate.
    /// Il tempo da sconnesso non entra in nessuna delle due.
    pub avg_speed_overall_kmh: f64,

    /// Tempo totale in movimento.
    pub moving_time: Duration,

    /// Tempo totale in stato FERMO.
    pub paused_time: Duration,

    /// Tempo totale in stato SCONNESSO.
    pub disconnected_time: Duration,

    /// Numero di soste distinte nel periodo.
    pub pause_count: usize,
}

/// Calcola il report per un utente su un periodo.
/// `intervals` e `positions` riguardano un solo utente e sono gia' filtrati
/// grossolanamente dalla query; il ritaglio fine agli estremi avviene qui.
/// `now` serve a chiudere l'intervallo eventualmente ancora aperto.
pub fn analyze(
    _user: UserId,
    _spec: RangeSpec,
    _range: TimeRange,
    _intervals: &[StateInterval],
    _positions: &[FixedPosition],
    _now: DateTime<Utc>,
) -> MovementReport {
    todo!("F1 — P1")
}

/// Somma delle distanze fra posizioni consecutive, in km.
pub fn total_distance_km(_positions: &[FixedPosition]) -> f64 {
    todo!("F1 — P1")
}
