//! Registrazione e autenticazione (R1, R2).
//!
//! Le password non vanno mai memorizzate in chiaro: Argon2 con salt per utente.
//! Non e' richiesto esplicitamente dalla specifica, ma e' una di quelle cose che
//! all'orale distinguono chi ha capito da chi ha fatto i compiti.
//!
//! ATTACCO: P2, fase F2.

pub fn hash_password(_plain: &str) -> anyhow::Result<String> {
    todo!("F2 — P2")
}

pub fn verify_password(_plain: &str, _phc: &str) -> bool {
    todo!("F2 — P2")
}
