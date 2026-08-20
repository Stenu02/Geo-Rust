//! I tre periodi su cui si possono chiedere le analisi.


use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Quale periodo si vuole analizzare: e' la domanda.
/// L'amministratore lo sceglie nell'interfaccia
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RangeSpec {
    Today,
    CurrentWeek,
    CurrentMonth,
}

impl RangeSpec {
    pub fn label(&self) -> &'static str {
        match self {
            RangeSpec::Today => "giorno corrente",
            RangeSpec::CurrentWeek => "settimana corrente",
            RangeSpec::CurrentMonth => "mese corrente",
        }
    }
}

/// Un periodo concreto con estremi assoluti.
/// Semiaperto [start, end): altrimenti la mezzanotte apparterrebbe
/// a due giorni e verrebbe contata due volte.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl TimeRange {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { start, end }
    }

    /// Risolve una `RangeSpec` in un periodo concreto, rispetto a un istante di riferimento.
    ///
    /// `now` e' un parametro, non `Utc::now()`: e' cio' che permette di testare
    /// il comportamento a cavallo di mezzanotte, di fine mese e del cambio di ora legale
    /// senza aspettare che accadano.
    pub fn resolve(_spec: RangeSpec, _now: DateTime<Utc>) -> TimeRange {
        todo!("F1 — P1")
    }

    pub fn duration(&self) -> Duration {
        self.end - self.start
    }

    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        at >= self.start && at < self.end
    }
}
