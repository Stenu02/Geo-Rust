//! Chi e' collegato adesso, e come lo si raggiunge.
//!
//! Tiene la mappa dei camionisti connessi, che alimenta la tabella della
//! flotta, e i canali per mandare loro messaggi: uno personale per ciascuno
//! piu' uno condiviso per gli annunci.
//!
//! Qui non serve un attore come per il database: la mappa vive in memoria e
//! ogni operazione dura microsecondi, meno del passaggio su un canale.

use chrono::{DateTime, Utc};
use geo_core::{Position, StateMachine, Transition, UserId, UserState};
use geo_protocol::{AdminReply, FleetEntry, ServerMessage};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};

/// Quanti messaggi restano in attesa nella casella di un camionista.
const MAILBOX: usize = 32;

/// Quanti annunci il canale conserva per chi e' momentaneamente indietro.
///
/// Chi resta piu' indietro perde i piu' vecchi: e' il comportamento voluto per
/// un annuncio, che vale adesso e non fra dieci minuti.
const BROADCAST: usize = 64;

/// Cosa il server sa di un camionista collegato in questo momento.
struct Connessa {
    username: String,
    /// Casella personale: da qui passano i messaggi diretti a questo utente.
    mailbox: mpsc::Sender<ServerMessage>,
    /// Unica fonte di verita' sullo stato: la tabella della flotta lo legge da
    /// qui, non da una copia che prima o poi divergerebbe.
    fsm: StateMachine,
    last_position: Option<Position>,
    last_update: Option<DateTime<Utc>>,
}

/// Il registro dei collegati.
///
/// Clonabile: ogni sessione ne tiene una copia e tutte vedono la stessa mappa.
#[derive(Clone)]
pub struct Registry {
    connessi: Arc<Mutex<HashMap<UserId, Connessa>>>,
    /// Canale verso tutti i camionisti collegati, per gli annunci.
    annunci: broadcast::Sender<ServerMessage>,
    /// Canale verso tutti i coordinatori collegati.
    ///
    /// Serve la direzione opposta: quando un camionista scrive, il messaggio
    /// deve arrivare a chi guarda la console. I coordinatori non stanno nella
    /// mappa `connessi`, che elenca la flotta, quindi hanno un canale a se'.
    verso_admin: broadcast::Sender<AdminReply>,
}

impl Registry {
    pub fn new() -> Self {
        // Il ricevitore restituito da `channel` non serve: ognuno si iscrive
        // per conto suo con `subscribe`.
        let (annunci, _) = broadcast::channel(BROADCAST);
        let (verso_admin, _) = broadcast::channel(BROADCAST);
        Self {
            connessi: Arc::new(Mutex::new(HashMap::new())),
            annunci,
            verso_admin,
        }
    }

    /// Iscrive una sessione del coordinatore ai messaggi in arrivo.
    ///
    /// Non lo inserisce in nessuna mappa: non fa parte della flotta e non ha
    /// uno stato da seguire, gli serve solo poter ascoltare.
    pub fn subscribe_admin(&self) -> broadcast::Receiver<AdminReply> {
        self.verso_admin.subscribe()
    }

    /// Recapita un messaggio a tutti i coordinatori collegati.
    ///
    /// Restituisce quanti lo riceveranno. Zero significa che nessuno sta
    /// guardando: il messaggio va salvato lo stesso sul database.
    pub fn verso_admin(&self, msg: AdminReply) -> usize {
        self.verso_admin.send(msg).unwrap_or(0)
    }

    /// Segna un utente come collegato.
    ///
    /// Restituisce i due canali da cui la sua sessione ricevera': la casella
    /// personale e gli annunci.
    ///
    /// Se lo stesso utente era gia' collegato altrove, la voce precedente viene
    /// sostituita e la vecchia sessione smette di ricevere.
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
            // Nasce sconnessa: e' `on_login` a portarla a fermo e a produrre la
            // transizione che il chiamante scrivera' sul database.
            fsm: StateMachine::new(),
            last_position: None,
            last_update: None,
        };
        self.blocca().insert(user, voce);
        (posta, self.annunci.subscribe())
    }

    /// Toglie un utente dai collegati.
    ///
    /// Va chiamata anche quando la sessione finisce con un errore, altrimenti
    /// la tabella della flotta mostra fantasmi.
    pub fn unregister(&self, user: UserId) {
        self.blocca().remove(&user);
    }

    /// Accesso effettuato: porta la macchina a fermo.
    ///
    /// I metodi che seguono hanno tutti la stessa forma: prendono il lucchetto,
    /// mutano la macchina a stati, restituiscono la transizione e rilasciano.
    /// Non scrivono sul database, perche' dentro un lucchetto non si puo'
    /// aspettare: a salvare e' il chiamante, a lucchetto gia' rilasciato.
    pub fn on_login(&self, user: UserId, now: DateTime<Utc>) -> Option<Transition> {
        self.blocca().get_mut(&user)?.fsm.on_login(now)
    }

    /// Posizione ricevuta: puo' far passare da fermo a in movimento.
    pub fn on_position(
        &self,
        user: UserId,
        now: DateTime<Utc>,
        position: Position,
    ) -> Option<Transition> {
        let mut mappa = self.blocca();
        let voce = mappa.get_mut(&user)?;
        // Questi due servono alla tabella della flotta, la macchina a stati
        // tiene i suoi per conto proprio.
        voce.last_position = Some(position);
        voce.last_update = Some(now);
        voce.fsm.on_position(now, position)
    }

    /// Disconnessione: chiude lo stato corrente e passa a sconnesso.
    pub fn on_disconnect(&self, user: UserId, now: DateTime<Utc>) -> Option<Transition> {
        self.blocca().get_mut(&user)?.fsm.on_disconnect(now)
    }

    /// Rivaluta tutti i collegati e raccoglie le transizioni scattate.
    ///
    /// E' qui che i tre minuti di inattivita' diventano un passaggio a fermo.
    /// Il lucchetto resta preso per tutta la scansione, ma sono microsecondi e
    /// non c'e' nessuna attesa dentro.
    pub fn tick_all(&self, now: DateTime<Utc>) -> Vec<(UserId, Transition)> {
        self.blocca()
            .iter_mut()
            .filter_map(|(user, voce)| voce.fsm.on_tick(now).map(|t| (*user, t)))
            .collect()
    }

    /// Lo stato corrente di un utente collegato.
    ///
    /// Serve a rispondere al client: la verita' sullo stato sta sul server, il
    /// client la mostra e basta.
    pub fn state_of(&self, user: UserId) -> Option<UserState> {
        Some(self.blocca().get(&user)?.fsm.state())
    }

    /// La fotografia della flotta da mandare al coordinatore.
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
        // Ordine stabile: la mappa non ne garantisce uno, e senza questo la
        // tabella si rimescolerebbe a ogni aggiornamento.
        righe.sort_by_key(|r| r.user);
        righe
    }

    /// Recapita un messaggio a un singolo camionista collegato.
    ///
    /// Restituisce `false` se non e' collegato o se la sua casella e' piena.
    /// Non e' un errore: e' l'informazione che serve al chiamante per decidere
    /// se mettere il messaggio in coda per il prossimo accesso.
    pub fn send_to(&self, user: UserId, msg: ServerMessage) -> bool {
        match self.blocca().get(&user) {
            Some(voce) => voce.mailbox.try_send(msg).is_ok(),
            None => false,
        }
    }

    /// Manda un annuncio a tutti i camionisti collegati.
    ///
    /// Restituisce quanti lo riceveranno. Zero non e' un errore: vuol dire che
    /// non c'e' nessuno collegato.
    pub fn annuncia(&self, msg: ServerMessage) -> usize {
        self.annunci.send(msg).unwrap_or(0)
    }

    /// Quanti camionisti sono collegati.
    pub fn count(&self) -> usize {
        self.blocca().len()
    }

    /// Prende il lucchetto sulla mappa.
    ///
    /// E' un mutex normale e non quello di tokio: quello serve solo se il
    /// lucchetto va tenuto attraverso un `.await`, ed e' piu' costoso. Qui ogni
    /// sezione critica e' prendi, tocca la mappa, rilascia.
    ///
    /// Se un altro thread e' andato in panic mentre lo teneva, la mappa
    /// potrebbe essere incoerente: fermarsi e' meglio che proseguire.
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
        // Registrato ma non ancora autenticato: la macchina nasce sconnessa.
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

        // La prima posizione dopo l'accesso non e' un cambio.
        assert!(reg
            .on_position(UserId(1), t0, Position::new(45.0, 7.0))
            .is_none());
        assert_eq!(reg.fleet()[0].state, UserState::Stopped);

        // La seconda, diversa dalla prima, lo e'.
        let t = reg
            .on_position(
                UserId(1),
                t0 + Duration::seconds(30),
                Position::new(45.1, 7.0),
            )
            .expect("transizione");
        assert_eq!(t.to, UserState::Moving);

        // La tabella della flotta riporta stato e ultima posizione aggiornati.
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

        // Prima della soglia non scatta niente.
        assert!(reg.tick_all(t0 + Duration::seconds(60)).is_empty());

        // Dopo la soglia, contata dall'ultimo cambio, si'.
        let scattate = reg.tick_all(t0 + Duration::seconds(30 + 181));
        assert_eq!(scattate.len(), 1);
        assert_eq!(scattate[0].1.to, UserState::Stopped);
        assert_eq!(reg.fleet()[0].state, UserState::Stopped);
    }

    #[test]
    fn eventi_su_un_utente_non_collegato_non_esplodono() {
        // Capita davvero: un tick arriva dopo che la sessione si e' chiusa.
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
        // Inseriti in disordine, devono uscire ordinati per identificativo.
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
