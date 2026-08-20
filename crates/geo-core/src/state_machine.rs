//! Macchina a stati dell'utente: i tre stati e le transizioni fra loro.
 //! Il ritorno a FERMO si misura con un timer sull'ultimo cambio di coordinata
use crate::position::Position;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UserState {
    /// Non connesso.
    Disconnected,
    /// Connesso, coordinate ferme.
    Stopped,
    /// Connesso, coordinate in variazione.
    Moving,
}

impl UserState {
    /// Vero solo per Stopped
    pub fn counts_as_pause(&self) -> bool {
        matches!(self, UserState::Stopped)
    }
}

/// Un cambio di stato avvenuto a un certo istante.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    pub from: UserState,
    pub to: UserState,
    pub at: DateTime<Utc>,
}

/// Stato di un singolo utente. Una istanza per utente connesso, tenuta dal server.
#[derive(Debug, Clone)]
// I campi sono letti solo dalle implementazioni di F1. L'allow sparisce
// non appena i `todo!()` vengono sostituiti dal codice vero.
#[allow(dead_code)]
pub struct StateMachine {
    state: UserState,
    last_position: Option<Position>,
    /// Istante dell'ultimo cambiamento EFFETTIVO di coordinata.
    last_change_at: Option<DateTime<Utc>>,
}

impl StateMachine {
    /// Costruisce una macchina nello stato SCONNESSO.
    pub fn new() -> Self {
        Self {
            state: UserState::Disconnected,
            last_position: None,
            last_change_at: None,
        }
    }

    pub fn state(&self) -> UserState {
        self.state
    }

    pub fn last_position(&self) -> Option<Position> {
        self.last_position
    }

    /// Login: SCONNESSO -> FERMO.
    ///
    /// Lo stato iniziale e' FERMO, non IN MOVIMENTO: si passa a IN MOVIMENTO solo
    /// al primo cambio effettivo di coordinata.
    pub fn on_login(&mut self, _now: DateTime<Utc>) -> Option<Transition> {
        todo!("F1 — P1")
    }

    /// Posizione ricevuta dal client (R4).
    ///
    /// Casi da gestire:
    /// - PRIMA posizione dopo il login: non e' un "cambio"
    /// - coordinata diversa dalla precedente: FERMO -> IN MOVIMENTO, e in ogni caso
    ///   aggiorna `last_change_at`
    /// - coordinata uguale: nessuna transizione qui (ci pensa `on_tick`)
    pub fn on_position(&mut self, _now: DateTime<Utc>, _position: Position) -> Option<Transition> {
        todo!("F1 — P1")
    }

    /// Controllo periodico: se e' IN MOVIMENTO da troppo tempo senza cambi,
    /// passa a FERMO.
    ///  /// L'istante della transizione e' `ultimo cambio + soglia`, non `now`:
    /// il tick e' grossolano, e usare `now` allungherebbe il movimento di
    /// quanto e' rado il controllo.
    pub fn on_tick(&mut self, _now: DateTime<Utc>) -> Option<Transition> {
        todo!("F1 — P1")
    }

    /// Disconnessione da qualsiasi stato.
    ///
    /// Alla riconnessione il timer dei 3 minuti riparte: scelta da documentare nel report.
    pub fn on_disconnect(&mut self, _now: DateTime<Utc>) -> Option<Transition> {
        todo!("F1 — P1")
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}
