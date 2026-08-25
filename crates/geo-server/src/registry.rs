//! Chi e' collegato adesso, e come lo si raggiunge.
//!
//! Tiene due cose: la mappa dei camionisti attualmente connessi — che alimenta
//! la tabella flotta dell'amministratore — e i canali per mandare loro messaggi,
//! uno personale per ciascuno piu' uno condiviso per gli annunci a tutti.
//!
//! A differenza dello storage qui NON serve un attore: la mappa vive in memoria
//! e ogni operazione dura microsecondi. Un attore aggiungerebbe due passaggi di
//! canale per un lavoro piu' breve del passaggio stesso.

use chrono::{DateTime, Utc};
use geo_core::{Position, StateMachine, Transition, UserId, UserState};
use geo_protocol::{FleetEntry, ServerMessage};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};

/// Quanti messaggi possono restare in attesa nella casella di un camionista.
const MAILBOX: usize = 32;

/// Quanti annunci il canale broadcast conserva per chi e' momentaneamente
/// indietro. Chi resta piu' indietro di cosi' perde i messaggi piu' vecchi e
/// riceve un errore di ritardo: e' il comportamento voluto per un annuncio,
/// che vale adesso e non fra dieci minuti.
const BROADCAST: usize = 64;

/// Cosa il server sa di un camionista collegato in questo momento.
struct Connessa {
    username: String,
    /// Casella personale: da qui passano i messaggi diretti a questo utente.
    mailbox: mpsc::Sender<ServerMessage>,
    /// La macchina a stati di questo utente. E' l'unica fonte di verita' sul suo
    /// stato: la tabella flotta lo legge da qui, non da una copia a parte che
    /// prima o poi divergerebbe.
    fsm: StateMachine,
    last_position: Option<Position>,
    last_update: Option<DateTime<Utc>>,
}

/// Il registro dei collegati. Clonabile: ogni sessione ne tiene una copia e
/// tutte vedono la stessa mappa.
#[derive(Clone)]
pub struct Registry {
    connessi: Arc<Mutex<HashMap<UserId, Connessa>>>,
    annunci: broadcast::Sender<ServerMessage>,
}

impl Registry {
    pub fn new() -> Self {
        let (annunci, _) = broadcast::channel(BROADCAST);
        Self {
            connessi: Arc::new(Mutex::new(HashMap::new())),
            annunci,
        }
    }

    /// Segna un utente come collegato e restituisce i due canali da cui la sua
    /// sessione ricevera' messaggi: la casella personale e gli annunci.
    ///
    /// Se lo stesso utente era gia' collegato altrove, la voce precedente viene
    /// sostituita: la sua vecchia sessione smettera' di ricevere. E' la scelta
    /// piu' semplice e va documentata; l'alternativa sarebbe rifiutare il secondo
    /// login o consegnare a entrambe.
    pub fn register(
        &self,
        user: UserId,
        username: String,
    ) -> (
        mpsc::Receiver<ServerMessage>,
        broadcast::Receiver<ServerMessage>,
    ) {
        let (mailbox, posta) = mpsc::channel(MAILBOX);
        let voce = Connessa {
            username,
            mailbox,
            // Nasce SCONNESSA: e' `on_login` a portarla a FERMO, e a produrre la
            // transizione che il chiamante scrivera' sul database.
            fsm: StateMachine::new(),
            last_position: None,
            last_update: None,
        };
        self.blocca().insert(user, voce);
        (posta, self.annunci.subscribe())
    }

    /// Toglie un utente dai collegati. Va chiamata anche quando la sessione
    /// termina con un errore, altrimenti la tabella flotta mostra fantasmi.
    pub fn unregister(&self, user: UserId) {
        self.blocca().remove(&user);
    }

    /// Login: porta la macchina a FERMO e restituisce la transizione.
    ///
    /// I metodi che seguono hanno tutti la stessa forma: prendono il lucchetto,
    /// mutano la macchina a stati, restituiscono la transizione e rilasciano.
    /// NON scrivono sul database — non possono, perche' dentro un lucchetto non
    /// si puo' aspettare. E' il chiamante a persistere, a lucchetto rilasciato.
    pub fn on_login(&self, user: UserId, now: DateTime<Utc>) -> Option<Transition> {
        self.blocca().get_mut(&user)?.fsm.on_login(now)
    }

    /// Posizione ricevuta: puo' far scattare FERMO -> IN MOVIMENTO.
    pub fn on_position(
        &self,
        user: UserId,
        now: DateTime<Utc>,
        position: Position,
    ) -> Option<Transition> {
        let mut mappa = self.blocca();
        let voce = mappa.get_mut(&user)?;
        voce.last_position = Some(position);
        voce.last_update = Some(now);
        voce.fsm.on_position(now, position)
    }

    /// Disconnessione: chiude lo stato corrente e passa a SCONNESSO.
    pub fn on_disconnect(&self, user: UserId, now: DateTime<Utc>) -> Option<Transition> {
        self.blocca().get_mut(&user)?.fsm.on_disconnect(now)
    }

    /// Valuta tutti i collegati e raccoglie le transizioni scattate.
    ///
    /// E' qui che i tre minuti di inattivita' diventano un passaggio a FERMO.
    /// Il lucchetto resta preso per tutta l'iterazione, ma sono microsecondi e
    /// non c'e' nessuna attesa dentro: le scritture le fa l'evaluator dopo.
    pub fn tick_all(&self, now: DateTime<Utc>) -> Vec<(UserId, Transition)> {
        self.blocca()
            .iter_mut()
            .filter_map(|(user, voce)| voce.fsm.on_tick(now).map(|t| (*user, t)))
            .collect()
    }

    /// Lo stato corrente di un utente collegato.
    ///
    /// Serve a rispondere `PositionAck`: la verita' sullo stato sta sul server,
    /// il client la mostra e basta.
    pub fn state_of(&self, user: UserId) -> Option<UserState> {
        Some(self.blocca().get(&user)?.fsm.state())
    }

    /// La fotografia della flotta da mandare all'amministratore.
    pub fn fleet(&self) -> Vec<FleetEntry> {
        let mut righe: Vec<FleetEntry> = self
            .blocca()
            .iter()
            .map(|(user, v)| FleetEntry {
                user: *user,
                username: v.username.clone(),
                state: v.fsm.state(),
                last_position: v.last_position,
                last_update: v.last_update,
            })
            .collect();
        // Ordine stabile: senza, la tabella dell'amministratore si rimescola a
        // ogni aggiornamento, perche' HashMap non garantisce un ordine.
        righe.sort_by_key(|r| r.user);
        righe
    }

    /// Recapita un messaggio a un singolo camionista collegato.
    ///
    /// Restituisce `false` se non e' collegato o se la sua casella e' piena.
    /// Non e' un errore del server: e' informazione che il chiamante puo' usare
    /// per decidere se mettere il messaggio in coda per il prossimo login.
    pub fn send_to(&self, user: UserId, msg: ServerMessage) -> bool {
        match self.blocca().get(&user) {
            Some(voce) => voce.mailbox.try_send(msg).is_ok(),
            None => false,
        }
    }

    /// Manda un annuncio a tutti i camionisti collegati.
    ///
    /// Restituisce quanti lo riceveranno. Zero non e' un errore: significa che
    /// in questo momento non c'e' nessuno collegato.
    pub fn annuncia(&self, msg: ServerMessage) -> usize {
        self.annunci.send(msg).unwrap_or(0)
    }

    /// Quanti camionisti sono collegati.
    pub fn count(&self) -> usize {
        self.blocca().len()
    }

    /// Prende il lucchetto sulla mappa.
    ///
    /// E' un `std::sync::Mutex` e non quello di tokio: quello di tokio serve solo
    /// se il lucchetto va tenuto attraverso un `.await`, ed e' piu' costoso. Qui
    /// ogni sezione critica e' "prendi, tocca la mappa, rilascia" — nessuna
    /// attesa dentro. La regola da non violare mai e' proprio quella.
    ///
    /// `unwrap` sul lock avvelenato: se un altro thread e' andato in panic mentre
    /// teneva il lucchetto, la mappa potrebbe essere incoerente e proseguire
    /// sarebbe peggio che fermarsi.
    fn blocca(&self) -> std::sync::MutexGuard<'_, HashMap<UserId, Connessa>> {
        self.connessi
            .lock()
            .expect("registro avvelenato da un panic")
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn un_utente_registrato_compare_nella_flotta() {
        let reg = Registry::new();
        let (_posta, _annunci) = reg.register(UserId(1), "camion01".into());

        let flotta = reg.fleet();
        assert_eq!(flotta.len(), 1);
        assert_eq!(flotta[0].username, "camion01");
        // Registrato ma non ancora autenticato: la macchina nasce SCONNESSA,
        // e' `on_login` a portarla a FERMO.
        assert_eq!(flotta[0].state, UserState::Disconnected);
        assert_eq!(flotta[0].last_position, None);
    }

    #[test]
    fn dopo_unregister_la_flotta_e_vuota() {
        let reg = Registry::new();
        let _ = reg.register(UserId(1), "camion01".into());
        reg.unregister(UserId(1));
        assert!(reg.fleet().is_empty());
        assert_eq!(reg.count(), 0);
    }

    #[test]
    fn il_login_porta_a_fermo() {
        let reg = Registry::new();
        let _ = reg.register(UserId(1), "camion01".into());
        // Prima del login la macchina e' sconnessa.
        assert_eq!(reg.fleet()[0].state, UserState::Disconnected);

        let t = reg.on_login(UserId(1), Utc::now()).expect("transizione");
        assert_eq!(t.to, UserState::Stopped);
        assert_eq!(reg.fleet()[0].state, UserState::Stopped);
    }

    #[test]
    fn il_primo_cambio_di_coordinata_avvia_il_movimento() {
        let reg = Registry::new();
        let _ = reg.register(UserId(1), "camion01".into());
        let t0 = Utc::now();
        reg.on_login(UserId(1), t0);

        // La prima posizione non e' un "cambio".
        assert!(reg
            .on_position(UserId(1), t0, Position::new(45.0, 7.0))
            .is_none());
        assert_eq!(reg.fleet()[0].state, UserState::Stopped);

        // La seconda, diversa, lo e'.
        let t = reg
            .on_position(
                UserId(1),
                t0 + Duration::seconds(30),
                Position::new(45.1, 7.0),
            )
            .expect("transizione");
        assert_eq!(t.to, UserState::Moving);

        let riga = &reg.fleet()[0];
        assert_eq!(riga.state, UserState::Moving);
        assert_eq!(riga.last_position, Some(Position::new(45.1, 7.0)));
    }

    #[test]
    fn dopo_tre_minuti_fermi_si_torna_a_fermo() {
        let reg = Registry::new();
        let _ = reg.register(UserId(1), "camion01".into());
        let t0 = Utc::now();
        reg.on_login(UserId(1), t0);
        reg.on_position(UserId(1), t0, Position::new(45.0, 7.0));
        reg.on_position(
            UserId(1),
            t0 + Duration::seconds(30),
            Position::new(45.1, 7.0),
        );
        assert_eq!(reg.fleet()[0].state, UserState::Moving);

        // Un tick troppo presto non fa nulla.
        assert!(reg.tick_all(t0 + Duration::seconds(60)).is_empty());

        // Uno dopo la soglia si'.
        let scattate = reg.tick_all(t0 + Duration::seconds(30 + 181));
        assert_eq!(scattate.len(), 1);
        assert_eq!(scattate[0].1.to, UserState::Stopped);
        assert_eq!(reg.fleet()[0].state, UserState::Stopped);
    }

    #[test]
    fn eventi_su_un_utente_non_collegato_non_esplodono() {
        // Puo' capitare: un tick arriva dopo che la sessione si e' chiusa.
        let reg = Registry::new();
        assert!(reg.on_login(UserId(99), Utc::now()).is_none());
        assert!(reg
            .on_position(UserId(99), Utc::now(), Position::new(45.0, 7.0))
            .is_none());
        assert!(reg.on_disconnect(UserId(99), Utc::now()).is_none());
        assert!(reg.tick_all(Utc::now()).is_empty());
    }

    #[test]
    fn la_flotta_e_ordinata_in_modo_stabile() {
        let reg = Registry::new();
        for id in [3, 1, 2] {
            let _ = reg.register(UserId(id), format!("camion{id:02}"));
        }
        let id: Vec<i64> = reg.fleet().iter().map(|r| r.user.0).collect();
        assert_eq!(id, vec![1, 2, 3]);
    }

    #[test]
    fn il_registro_e_condiviso_fra_le_copie() {
        // Ogni sessione tiene un clone: devono vedere tutte la stessa mappa.
        let reg = Registry::new();
        let copia = reg.clone();
        let _ = reg.register(UserId(1), "camion01".into());
        assert_eq!(copia.count(), 1);
        copia.unregister(UserId(1));
        assert_eq!(reg.count(), 0);
    }
}
