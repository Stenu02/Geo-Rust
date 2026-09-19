//! Eventi esposti da geo-net verso chi lo usa
//! Nasconde i dettagli di rete: chi riceve un Event non sa nulla di TCP o framing

use chrono::{DateTime, Utc};
use geo_core::UserState;
use geo_protocol::ErrorCode;

#[derive(Debug, Clone)]
pub enum Event {
    /// Handshake completato (TCP stabilito, Hello/Welcome scambiati)
    Connected,
    /// Login accettato dal server
    LoggedIn { username: String },
    /// Connessione persa (chiusura remota, errore di rete, timeout heartbeat)
    Disconnected { reason: String },
    /// Conferma di ricezione posizione, con lo stato risultante
    PositionAck { state: UserState },
    /// Messaggio di chat ricevuto dal server
    ChatReceived {
        text: String,
        broadcast: bool,
        at: DateTime<Utc>,
    },
    /// Errore riportato dal server
    ServerError { code: ErrorCode, detail: String },
}
