//! I messaggi che viaggiano sul filo.
//!
//! QUESTO FILE VA SCRITTO COMPLETO NELLA FASE COMUNE, e poi cambiato il meno possibile.
//! Motivo: aggiungere una variante a un enum di dominio non disturba nessuno, ma
//! aggiungere un campo a un messaggio rompe tre crate e richiede una riunione.
//! Se dovete modificarlo, avvisate gli altri tre.

use chrono::{DateTime, Utc};
use geo_core::{MovementReport, Position, RangeSpec, UserId, UserState};
use serde::{Deserialize, Serialize};

/// Incrementare a ogni modifica incompatibile. Il server rifiuta versioni diverse:
/// meglio un errore chiaro all'handshake che byte interpretati male a meta' demo.
pub const PROTOCOL_VERSION: u16 = 1;

/// Ruolo dichiarato all'apertura della connessione (R12: due entita' distinte).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Trucker,
    Admin,
}

/// Primo frame di ogni connessione, in entrambe le direzioni d'uso.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_version: u16,
    pub role: Role,
}

// ---------------------------------------------------------------------------
// Camionista -> Server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// R1
    Register {
        username: String,
        password: String,
    },
    /// R2
    Login {
        username: String,
        password: String,
    },
    /// R4 — inviato ogni 30 secondi
    PositionUpdate {
        at: DateTime<Utc>,
        position: Position,
    },
    /// R14 — il camionista scrive all'amministratore
    Chat {
        text: String,
    },
    /// Mantiene viva la connessione e permette di rilevare i client spariti
    Heartbeat,
    Logout,
}

// ---------------------------------------------------------------------------
// Server -> Camionista
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Welcome {
        protocol_version: u16,
    },
    RegisterOk {
        user: UserId,
    },
    LoginOk {
        user: UserId,
        username: String,
    },
    /// Conferma di ricezione posizione, con lo stato risultante: permette al client
    /// di mostrare il proprio stato senza ricalcolarlo (la verita' sta sul server).
    PositionAck {
        state: UserState,
    },
    /// R13 — messaggio scritto a mano dall'amministratore
    Chat {
        text: String,
        broadcast: bool,
        at: DateTime<Utc>,
    },
    Pong,
    Error {
        code: ErrorCode,
        detail: String,
    },
}

// ---------------------------------------------------------------------------
// Amministratore -> Server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdminMessage {
    Login {
        username: String,
        password: String,
    },
    /// Stato corrente della flotta (visualizzazione real-time: opzionale ma utile)
    ListFleet,
    /// R8, R9, R10, R11, R12 — il calcolo avviene sul server
    Query {
        user: UserId,
        spec: RangeSpec,
    },
    /// R13 — messaggio a un singolo camionista
    SendDirect {
        to: UserId,
        text: String,
    },
    /// R13 — messaggio a tutti i camionisti CONNESSI
    SendBroadcast {
        text: String,
    },
    Heartbeat,
    Logout,
}

// ---------------------------------------------------------------------------
// Server -> Amministratore
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdminReply {
    Welcome {
        protocol_version: u16,
    },
    LoginOk {
        user: UserId,
        username: String,
    },
    Fleet {
        entries: Vec<FleetEntry>,
    },
    /// Il report arriva gia' calcolato: l'admin non computa nulla.
    QueryResult(Box<MovementReport>),
    /// R14 — la risposta di un camionista
    Chat {
        from: UserId,
        username: String,
        text: String,
        at: DateTime<Utc>,
    },
    /// Notifica di cambio stato, per aggiornare la tabella senza rifare ListFleet
    StateChanged {
        user: UserId,
        state: UserState,
        at: DateTime<Utc>,
    },
    Pong,
    Error {
        code: ErrorCode,
        detail: String,
    },
}

/// Una riga della tabella flotta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetEntry {
    pub user: UserId,
    pub username: String,
    pub state: UserState,
    pub last_position: Option<Position>,
    pub last_update: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Errori
// ---------------------------------------------------------------------------

/// Codici distinti dal testo: il client puo' reagire in modo diverso a ciascuno
/// senza dover fare pattern matching su stringhe in italiano.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    ProtocolVersionMismatch,
    UsernameTaken,
    InvalidCredentials,
    NotAuthenticated,
    NotAuthorized,
    UnknownUser,
    InvalidPosition,
    RateLimited,
    Internal,
}
