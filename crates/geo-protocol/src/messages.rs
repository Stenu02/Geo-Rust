//! I messaggi che viaggiano sul filo.


use chrono::{DateTime, Utc};
use geo_core::{MovementReport, Position, RangeSpec, UserId, UserState};
use serde::{Deserialize, Serialize};

/// Cambia a ogni modifica incompatibile dei messaggi. Il server rifiuta le
/// versioni diverse dalla propria, cosi' l'errore emerge subito all'apertura
/// invece che a byte gia' interpretati male.
pub const PROTOCOL_VERSION: u16 = 1;

/// Ruolo dichiarato all'apertura della connessione
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
    Register {
        username: String,
        password: String,
    },
    Login {
        username: String,
        password: String,
    },
    /// Inviato ogni 30 secondi
    PositionUpdate {
        at: DateTime<Utc>,
        position: Position,
    },
    /// Il camionista scrive all'amministratore
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
    /// Messaggio scritto a mano dall'amministratore
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
    /// Stato corrente della flotta
    ListFleet,
    /// Il calcolo avviene sul server
    Query {
        user: UserId,
        spec: RangeSpec,
    },
    /// Messaggio a un singolo camionista
    SendDirect {
        to: UserId,
        text: String,
    },
    /// Messaggio a tutti i camionisti connessi
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
    /// La risposta di un camionista
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
