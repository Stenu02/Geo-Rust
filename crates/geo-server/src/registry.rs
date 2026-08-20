//! Registro delle sessioni connesse e instradamento dei messaggi (R13).
//!
//! - broadcast::Sender  -> messaggi a tutti i camionisti connessi
//! - mpsc::Sender per utente -> messaggi diretti a uno solo
//!
//! ATTACCO: P2, fase F4.

pub struct Registry;
