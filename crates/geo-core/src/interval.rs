//! Intervalli di stato: la rappresentazione su cui poggiano tutte le analisi.
//! Si memorizza `(stato, inizio, fine)` invece dei singoli cambi di stato:
//! cosi' sommare le durate e' diretto e ritagliare un periodo e' banale.

use crate::state_machine::UserState;
use crate::timerange::TimeRange;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};


/// Un periodo continuo durante il quale un utente e' rimasto in un certo stato.
/// `end == None` significa intervallo ancora aperto, cioe' lo stato corrente:
/// ogni utente ne ha al piu' uno.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StateInterval {
    pub state: UserState,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
}

impl StateInterval {
    /// Apre un intervallo che inizia a `start` e non ha ancora una fine.
    pub fn open(state: UserState, start: DateTime<Utc>) -> Self {
        Self {
            state,
            start,
            end: None,
        }
    }
    /// Crea un intervallo gia' concluso, con inizio e fine noti.
    pub fn closed(state: UserState, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            state,
            start,
            end: Some(end),
        }
    }
    /// Vero se l'intervallo e' ancora in corso.
    pub fn is_open(&self) -> bool {
        self.end.is_none()
    }

    /// Quanto e' durato l'intervallo.
    /// Se e' ancora aperto lo si chiude virtualmente a `now`.
    pub fn duration(&self, _now: DateTime<Utc>) -> Duration {
        todo!("F1 — P1")
    }


    /// Interseca l'intervallo con un periodo; `None` se non si sovrappongono.
    ///
    /// Serve perche' un intervallo puo' sporgere fuori dal periodo chiesto:
    /// chi e' in movimento dalle 23:40 alle 00:20 contribuisce al giorno
    /// corrente per 20 minuti, non per 40.
    ///
    /// Il risultato e' sempre chiuso: al peggio lo delimita la fine del periodo.
    pub fn clip_to(&self, _range: &TimeRange, _now: DateTime<Utc>) -> Option<StateInterval> {
        todo!("F1 — P1")
    }
}

/// Somma la durata degli intervalli in un dato stato, dentro un periodo.
pub fn total_in(
    _intervals: &[StateInterval],
    _state: UserState,
    _range: &TimeRange,
    _now: DateTime<Utc>,
) -> Duration {
    todo!("F1 — P1")
}
