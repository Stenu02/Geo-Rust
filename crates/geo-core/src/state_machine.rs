//! Macchina a stati dell'utente.
//!
//! Si parte sconnessi. L'accesso porta a fermo; il primo cambio di coordinata
//! porta in movimento; tre minuti senza cambiamenti riportano a fermo. La
//! disconnessione riporta a sconnesso da qualsiasi stato.

use crate::position::Position;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::{stop_threshold};

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
    /// Vero solo per `Stopped`. Usato dalle analisi per la "durata delle pause".
    pub fn counts_as_pause(&self) -> bool {
        matches!(self, UserState::Stopped)
    }

    /// Il campo salvato nella colonna `state` di `state_intervals` a db.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            UserState::Disconnected => "disconnected",
            UserState::Stopped => "stopped",
            UserState::Moving => "moving",
        }
    }

    /// Interpreta il token scritto da `as_db_str`; `None` per qualsiasi altra cosa
    /// (una riga corrotta del DB non deve generare panic).
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "disconnected" => Some(UserState::Disconnected),
            "stopped" => Some(UserState::Stopped),
            "moving" => Some(UserState::Moving),
            _ => None
        }
    }
}

/// Rappresenta un cambio di stato avvenuto a un certo istante.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    pub from: UserState,
    pub to: UserState,
    pub at: DateTime<Utc>,
}

/// Rappresenta lo stato di un singolo utente. 
/// Viene creata una istanza per utente connesso, tenuta dal server.
#[derive(Debug, Clone)]
pub struct StateMachine {
    state: UserState,
    last_position: Option<Position>,
    /// Istante dell'ultimo cambiamento effettivo di coordinata.
    last_change_at: Option<DateTime<Utc>>,
}

impl StateMachine {
    /// Alla creazione la macchina è nello stato SCONNESSO.
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

    /// Imposta il nuovo stato e restituisce la transizione corrispondente.
    ///
    /// Si occupa di centralizzare la manutenzione dei campi in caso di cambio di stato.
    /// Viene usato nei quattro metodi `on_*`, i quali passano due parametri:
    /// (lo stato in cui passare, l'istante in cui si verifica il cambiamento)
    /// Il parametro `at` è passato esplicitamente perche' non è sempre `now`: `on_tick` chiude
    /// il movimento a `last_change_at + soglia`.
    fn transition_to(&mut self, to: UserState, at: DateTime<Utc>) -> Transition {
        let from = self.state;
        self.state = to;
        Transition { from, to, at }
    }

    /// Login: SCONNESSO -> FERMO.
    ///
    /// Lo stato iniziale e' FERMO, non IN MOVIMENTO: si passa a IN MOVIMENTO solo
    /// al primo cambio effettivo di coordinata.
    pub fn on_login(&mut self, now: DateTime<Utc>) -> Option<Transition> {
        if self.state != UserState::Disconnected {
            return None;
        }
        self.last_position = None;
        self.last_change_at = None;
        Some(self.transition_to(UserState::Stopped, now))
    }

    /// Posizione ricevuta dal client.
    ///
    /// Casi da gestire:
    /// - prima posizione dopo il login: non è un "cambio", non fa scattare il movimento
    /// - coordinata diversa dalla precedente: FERMO -> IN MOVIMENTO, e in ogni caso
    ///   aggiorna `last_change_at`
    /// - coordinata uguale: nessuna transizione qui (il compito è delegato a `on_tick`)
    pub fn on_position(
        &mut self,
        now: DateTime<Utc>,
        position: Position,
    ) -> Option<Transition> {
        match self.state {
            UserState::Stopped => {
                if self.last_position.is_none() {
                    self.last_position = Some(position);
                    None
                }
                else if self.last_position.unwrap().differs_from(&position) {
                    self.last_position = Some(position);
                    self.last_change_at = Some(now);
                    Some(self.transition_to(UserState::Moving, now))
                }
                else {
                    None
                }
            }
            UserState::Moving => {
                if let Some(prev) =self.last_position{
                    if prev.differs_from(&position) {
                        self.last_position = Some(position);
                        self.last_change_at = Some(now);
                    }
                }
                None
            }
            _ => None
        }
    }

    /// Valutazione periodica. Chiamata dal server a intervalli regolari.
    ///
    /// Se lo stato e' IN MOVIMENTO e sono passati almeno `STOP_THRESHOLD_SECS`
    /// dall'ultimo cambio di coordinata, transita a FERMO.
    ///
    /// L'istante della transizione deve essere `last_change_at + soglia`, non `now`:
    /// altrimenti la durata del movimento risulta piu' lunga del dovuto di quanto è
    /// fine il tick, e i totali delle durate non tornano.
    pub fn on_tick(&mut self, now: DateTime<Utc>) -> Option<Transition> {
        if self.state != UserState::Moving {
            return None;
        }
        if let Some(last_change_at) = self.last_change_at {
            let delta = now - last_change_at;
            if delta >= stop_threshold() {
                return Some(self.transition_to(UserState::Stopped, last_change_at + stop_threshold() ))
            }
        }
        None
    }

    /// Disconnessione da qualsiasi stato.
    ///
    /// Alla riconnessione il timer dei 3 minuti riparte.
    pub fn on_disconnect(&mut self, now: DateTime<Utc>) -> Option<Transition> {
        if self.state == UserState::Disconnected {
            return None
        }
        self.last_position = None;
        self.last_change_at = None;
        Some(self.transition_to(UserState::Disconnected, now))
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stop_threshold;
    use chrono::{Duration, TimeZone};


    fn t(offset_secs: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap() + Duration::seconds(offset_secs)
    }

    #[test]
    fn test_user_state_db_string_roundtrip() {
        for s in [UserState::Disconnected, UserState::Stopped, UserState::Moving] {
            assert_eq!(UserState::from_db_str(s.as_db_str()), Some(s));
        }
        assert_eq!(UserState::from_db_str("garbage"), None);
        // I token esatti devono combaciare con schema.sql.
        assert_eq!(UserState::Moving.as_db_str(), "moving");
    }

    #[test]
    fn test_state_transitions() {
        let mut sm = StateMachine::new();
        assert_eq!(sm.state(), UserState::Disconnected);

        // login: SCONNESSO -> FERMO
        let tr = sm.on_login(t(0)).unwrap();
        assert_eq!((tr.from, tr.to), (UserState::Disconnected, UserState::Stopped));

        // primo fix: non e' un cambio, resta FERMO
        assert!(sm.on_position(t(30), Position::new(45.0, 7.0)).is_none());
        assert_eq!(sm.state(), UserState::Stopped);

        // cambio coordinata: FERMO -> IN MOVIMENTO
        let tr = sm.on_position(t(60), Position::new(45.001, 7.0)).unwrap();
        assert_eq!((tr.from, tr.to), (UserState::Stopped, UserState::Moving));

        // 3 min senza cambiare: IN MOVIMENTO -> FERMO
        let tr = sm.on_tick(t(60) + stop_threshold() + Duration::seconds(5)).unwrap();
        assert_eq!((tr.from, tr.to), (UserState::Moving, UserState::Stopped));

        // disconnessione: FERMO -> SCONNESSO, mantenendo il `from` reale
        let tr = sm.on_disconnect(t(400)).unwrap();
        assert_eq!((tr.from, tr.to), (UserState::Stopped, UserState::Disconnected));
        assert_eq!(sm.state(), UserState::Disconnected);
    }

    #[test]
    fn test_first_coord_change_starts_movement() {
        let mut sm = StateMachine::new();
        sm.on_login(t(0));

        // La prima posizione dopo il login non e' un "cambio"
        assert!(sm.on_position(t(30), Position::new(45.0, 7.0)).is_none());
        assert_eq!(sm.state(), UserState::Stopped);

        // stessa coordinata di nuovo: ancora nessun movimento
        assert!(sm.on_position(t(60), Position::new(45.0, 7.0)).is_none());
        assert_eq!(sm.state(), UserState::Stopped);

        // primo cambio effettivo: ora si muove
        let tr = sm.on_position(t(90), Position::new(45.002, 7.0)).unwrap();
        assert_eq!(tr.to, UserState::Moving);
    }

    #[test]
    fn test_three_minutes_triggers_stopped() {
        let mut sm = StateMachine::new();
        sm.on_login(t(0));
        sm.on_position(t(30), Position::new(45.0, 7.0)); // primo fix
        sm.on_position(t(60), Position::new(45.001, 7.0)); // -> IN MOVIMENTO, ultimo cambio a t(60)

        // un secondo prima della soglia: nessuna transizione
        assert!(sm
            .on_tick(t(60) + stop_threshold() - Duration::seconds(1))
            .is_none());
        assert_eq!(sm.state(), UserState::Moving);

        // esattamente alla soglia: transizione a FERMO
        let tr = sm.on_tick(t(60) + stop_threshold()).unwrap();
        assert_eq!(tr.to, UserState::Stopped);
        // l'istante e' last_change_at + soglia, non `now`
        assert_eq!(tr.at, t(60) + stop_threshold());
    }

    #[test]
    fn test_continued_movement_delays_stop() {
        // Protegge il refresh del timer di sosta: in MOVIMENTO ogni cambio reale riavvia il conteggio.
        let mut sm = StateMachine::new();
        sm.on_login(t(0));
        sm.on_position(t(30), Position::new(45.0, 7.0)); // primo fix
        sm.on_position(t(60), Position::new(45.001, 7.0)); // -> IN MOVIMENTO, ultimo cambio a t(60)

        // ancora in movimento a t(120): rinfresca il timer, nessuna transizione
        assert!(sm.on_position(t(120), Position::new(45.002, 7.0)).is_none());
        assert_eq!(sm.state(), UserState::Moving);

        // 3 min dopo il primo cambio ma solo 2 dopo l'ultimo: ancora IN MOVIMENTO
        assert!(sm.on_tick(t(60) + stop_threshold()).is_none());
        assert_eq!(sm.state(), UserState::Moving);

        // 3 min dopo l'ultimo cambio: ora si ferma, a last_change_at + soglia
        let tr = sm.on_tick(t(120) + stop_threshold()).unwrap();
        assert_eq!(tr.to, UserState::Stopped);
        assert_eq!(tr.at, t(120) + stop_threshold());
    }
}
