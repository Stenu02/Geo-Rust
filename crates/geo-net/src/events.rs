//! eventi esposti da geo-net verso chi lo usa
//! nasconde i dettagli di rete: chi riceve un Event non sa nulla di TCP o framing

use chrono::{DateTime, Utc};
use geo_core::UserState;
use geo_protocol::ErrorCode;

#[derive(Debug, Clone)]
pub enum Event {
    /// handshake completato (TCP stabilito, Hello/Welcome scambiati)
    Connected,
    /// login accettato dal server
    LoggedIn { username: String },
    /// connessione persa (chiusura remota, errore di rete, timeout heartbeat)
    Disconnected { reason: String },
    /// conferma di ricezione posizione, con lo stato risultante
    PositionAck { state: UserState },
    /// messaggio di chat ricevuto dal server
    ChatReceived { text: String, broadcast: bool, at: DateTime<Utc> },
    /// errore riportato dal server
    ServerError { code: ErrorCode, detail: String },
}