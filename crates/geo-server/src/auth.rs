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

// ---------------------------------------------------------------------------
// Regole per un nuovo account
// ---------------------------------------------------------------------------

/// Lunghezza minima della password.
///
/// Vale sia per il camionista che si registra dalla rete sia per
/// l'amministratore creato da riga di comando: due percorsi che scrivono nella
/// stessa tabella non possono applicare criteri diversi.
pub const PASSWORD_MINIMA: usize = 8;

/// Lunghezza massima della password. Argon2 accetta qualsiasi lunghezza, ma
/// nessuno digita piu' di cosi' e un input enorme e' solo CPU sprecata.
pub const PASSWORD_MASSIMA: usize = 128;

/// Lunghezza massima dell'username. Il database non impone limiti: senza questo
/// un client potrebbe far scrivere righe da megabyte.
pub const USERNAME_MASSIMO: usize = 32;

/// Controlla che un username sia accettabile.
pub fn valida_username(username: &str) -> Result<(), String> {
    if username.is_empty() {
        return Err("l'username non puo' essere vuoto".into());
    }
    if username.chars().count() > USERNAME_MASSIMO {
        return Err(format!(
            "l'username non puo' superare i {USERNAME_MASSIMO} caratteri"
        ));
    }
    // Uno spazio in testa o in coda produce due nomi identici a schermo ma
    // distinti per l'indice UNIQUE: due account che sembrano lo stesso.
    if username.chars().any(char::is_whitespace) {
        return Err("l'username non puo' contenere spazi".into());
    }
    Ok(())
}

/// Controlla che una password sia accettabile.
pub fn valida_password(password: &str) -> Result<(), String> {
    let quanti = password.chars().count();
    if quanti < PASSWORD_MINIMA {
        return Err(format!(
            "la password deve essere di almeno {PASSWORD_MINIMA} caratteri"
        ));
    }
    if quanti > PASSWORD_MASSIMA {
        return Err(format!(
            "la password non puo' superare i {PASSWORD_MASSIMA} caratteri"
        ));
    }
    Ok(())
}

/// Entrambi i controlli insieme.
///
/// Va chiamata PRIMA di calcolare l'hash: Argon2 e' volutamente costoso, e una
/// registrazione destinata a essere rifiutata non deve farlo pagare al server.
pub fn valida_credenziali(username: &str, password: &str) -> Result<(), String> {
    valida_username(username)?;
    valida_password(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_credenziali_valide_passano() {
        assert!(valida_credenziali("camion01", "segretissima").is_ok());
    }

    #[test]
    fn l_username_vuoto_o_con_spazi_e_rifiutato() {
        assert!(valida_username("").is_err());
        assert!(valida_username(" camion01").is_err());
        assert!(valida_username("camion 01").is_err());
        assert!(valida_username(&"a".repeat(USERNAME_MASSIMO + 1)).is_err());
    }

    #[test]
    fn la_password_corta_e_rifiutata() {
        assert!(valida_password(&"a".repeat(PASSWORD_MINIMA - 1)).is_err());
        assert!(valida_password(&"a".repeat(PASSWORD_MINIMA)).is_ok());
        assert!(valida_password(&"a".repeat(PASSWORD_MASSIMA + 1)).is_err());
    }

    #[test]
    fn la_lunghezza_si_conta_in_caratteri_non_in_byte() {
        // Sette caratteri accentati fanno quattordici byte: contando i byte
        // questa password passerebbe il minimo di otto.
        assert!(valida_password("àèìòùàè").is_err());
    }
}
