//! Password: calcolo dell'impronta, verifica e regole per un nuovo account.
//!
//! Le password non vengono mai memorizzate: si salva un'impronta Argon2 con un
//! sale diverso per ogni utente.

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;

/// Calcola l'impronta di una password con un sale casuale nuovo.
///
/// Restituisce una stringa PHC che contiene tutto il necessario per verificarla:
/// algoritmo, parametri, sale e impronta. Il sale non va conservato a parte.
pub fn hash_password(plain: &str) -> anyhow::Result<String> {
    // Sale nuovo a ogni chiamata: senza, due utenti con la stessa password
    // avrebbero la stessa impronta, e romperne una le romperebbe entrambe.
    let salt = SaltString::generate(&mut OsRng);
    let phc = Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        // L'errore di argon2 non implementa std::error::Error, quindi `?` da
        // solo non basta: va convertito a mano.
        .map_err(|e| anyhow::anyhow!("hashing fallito: {e}"))?
        .to_string();
    Ok(phc)
}

/// Verifica una password contro la stringa PHC conservata nel database.
pub fn verify_password(plain: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(atteso) => Argon2::default()
            .verify_password(plain.as_bytes(), &atteso)
            .is_ok(),
        // PHC illeggibile: si nega l'accesso senza far cadere la sessione.
        Err(_) => false,
    }
}

/// Versione asincrona di `hash_password`.
///
/// Il calcolo costa una trentina di millisecondi di CPU pura: dentro un task
/// bloccherebbe un thread del runtime e con esso le altre sessioni che gli sono
/// assegnate, quindi si sposta sul pool dei lavori bloccanti.
pub async fn hash_password_async(plain: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || hash_password(&plain)).await?
}

/// Versione asincrona di `verify_password`.
pub async fn verify_password_async(plain: String, phc: String) -> bool {
    tokio::task::spawn_blocking(move || verify_password(&plain, &phc))
        .await
        // Se il task fallisce si nega l'accesso, non si concede.
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Regole per un nuovo account
// ---------------------------------------------------------------------------

/// Lunghezza minima della password, uguale per camionisti e amministratori.
pub const PASSWORD_MINIMA: usize = 8;

/// Tetto alla password: Argon2 accetta qualsiasi lunghezza, ma un input enorme
/// e' solo CPU sprecata.
pub const PASSWORD_MASSIMA: usize = 128;

/// Tetto al nome utente: il database non ne impone, e senza questo un client
/// potrebbe far scrivere righe da megabyte.
pub const USERNAME_MASSIMO: usize = 32;

/// Controlla che un nome utente sia accettabile.
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
    // Si contano i caratteri e non i byte: una lettera accentata ne occupa due.
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
/// Va chiamata prima di calcolare l'impronta: Argon2 e' volutamente costoso, e
/// una registrazione da rifiutare non deve farlo pagare al server.
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
        // Spazio in testa e spazio in mezzo, entrambi da rifiutare.
        assert!(valida_username(" camion01").is_err());
        assert!(valida_username("camion 01").is_err());
        assert!(valida_username(&"a".repeat(USERNAME_MASSIMO + 1)).is_err());
    }

    #[test]
    fn la_password_corta_e_rifiutata() {
        // Un carattere sotto il minimo, il minimo esatto, uno sopra il tetto.
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
