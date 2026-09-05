//! Hashing e verifica delle password.
//!
//! Le password non vengono mai memorizzate in chiaro: si salva un hash Argon2
//! con salt diverso per ogni utente, cosi' due persone con la stessa password
//! hanno comunque record distinti nel database.

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;

/// Calcola l'hash di una password, con un salt casuale nuovo.
///
/// Restituisce una stringa PHC che contiene tutto il necessario per verificarla:
/// algoritmo, parametri, salt e hash. Il salt non va memorizzato a parte.
///
/// Il salt e' diverso per ogni utente: senza, due persone con la stessa password
/// avrebbero lo stesso hash, e un attaccante che ne rompe uno li rompe entrambi.
pub fn hash_password(plain: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let phc = Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        // password_hash::Error non implementa std::error::Error, quindi non si
        // puo' usare `?` direttamente: va convertito a mano.
        .map_err(|e| anyhow::anyhow!("hashing fallito: {e}"))?
        .to_string();
    Ok(phc)
}

/// Verifica una password contro la stringa PHC memorizzata.
///
/// Restituisce `false` anche se il PHC e' malformato: un record corrotto nel
/// database non deve far passare nessuno, e non deve nemmeno far cadere la sessione.
pub fn verify_password(plain: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(atteso) => Argon2::default()
            .verify_password(plain.as_bytes(), &atteso)
            .is_ok(),
        Err(_) => false,
    }
}

/// Versione asincrona di `hash_password`.
///
/// L'hashing costa una trentina di millisecondi di CPU pura: chiamarlo da un task
/// bloccherebbe un thread del runtime, e con esso tutte le altre sessioni assegnate
/// a quel thread. Stesso motivo per cui l'attore storage gira su spawn_blocking.
pub async fn hash_password_async(plain: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || hash_password(&plain)).await?
}

/// Versione asincrona di `verify_password`.
pub async fn verify_password_async(plain: String, phc: String) -> bool {
    tokio::task::spawn_blocking(move || verify_password(&plain, &phc))
        .await
        .unwrap_or(false)
}
