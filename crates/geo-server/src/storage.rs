//! Persistenza (R15). Attore proprietario dell'unica connessione SQLite.
//!
//! PERCHE' UN ATTORE: rusqlite e' SINCRONO. Chiamarlo da un task tokio blocca il
//! thread del runtime e con esso tutti gli altri task assegnati a quel thread.
//! Un solo task possiede la connessione e riceve comandi via mpsc; chi vuole
//! leggere o scrivere allega un canale oneshot per la risposta.
//!
//!   [sessione 1] --+
//!   [sessione 2] --+--> mpsc --> [storage actor] --> SQLite
//!   [evaluator]  --+                   |
//!                                      +--> oneshot --> risposta
//!
//! ATTACCO: P2, fase F2. E' molto probabile che ve lo chiedano all'orale.

use geo_core::UserId;
use rusqlite::Connection;
use std::path::Path;
use tokio::sync::{mpsc, oneshot};

/// Quanti comandi possono restare in coda prima che chi invia debba aspettare.
/// Un limite serve: senza, una raffica di scritture farebbe crescere la coda
/// senza freno mentre l'attore resta indietro.
const CODA: usize = 64;

/// Cosa l'attore sa fare. Ogni variante porta con se' il canale su cui
/// rispondere: chi manda il comando aspetta su quel canale.
enum Command {
    CreateUser {
        username: String,
        password_hash: String,
        role: String,
        reply: oneshot::Sender<anyhow::Result<UserId>>,
    },
    FindUser {
        username: String,
        /// Some(id, hash) se esiste, None se non c'e'.
        reply: oneshot::Sender<anyhow::Result<Option<(UserId, String)>>>,
    },
}

/// Da qui si parla con il database. E' clonabile: ogni sessione ne tiene
/// una copia, e tutte finiscono nella stessa coda.
#[derive(Clone)]
pub struct StorageHandle {
    tx: mpsc::Sender<Command>,
}

impl StorageHandle {
    /// Apre il database, applica lo schema e avvia l'attore.
    ///
    /// L'apertura e lo schema avvengono qui, sul thread del chiamante: e'
    /// un'operazione bloccante ma accade una volta sola all'avvio, prima che
    /// il server accetti connessioni.
    pub fn spawn(db_path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(db_path)?;

        // Lo schema e' incorporato nel binario (lib.rs) e usa CREATE TABLE
        // IF NOT EXISTS: applicarlo a ogni avvio e' innocuo su un database
        // gia' popolato, ed evita un passo di installazione separato.
        conn.execute_batch(crate::SCHEMA)?;

        let (tx, mut rx) = mpsc::channel::<Command>(CODA);

        // spawn_blocking, NON spawn: dentro si chiama rusqlite, che blocca.
        // Su un task normale bloccherebbe un thread del runtime e con esso
        // ogni altro task assegnato a quel thread.
        tokio::task::spawn_blocking(move || {
            // blocking_recv e non recv().await: qui non siamo in un contesto
            // asincrono, siamo su un thread del pool bloccante.
            while let Some(cmd) = rx.blocking_recv() {
                esegui(&conn, cmd);
            }
            // Si esce quando tutti i mittenti sono stati distrutti, cioe'
            // quando il server si spegne. La connessione si chiude qui.
            tracing::info!("attore storage terminato");
        });

        Ok(Self { tx })
    }

    /// Registra un nuovo utente. Fallisce se lo username e' gia' preso.
    pub async fn create_user(
        &self,
        username: &str,
        password_hash: &str,
        role: &str,
    ) -> anyhow::Result<UserId> {
        let (reply, rx) = oneshot::channel();
        self.invia(Command::CreateUser {
            username: username.to_string(),
            password_hash: password_hash.to_string(),
            role: role.to_string(),
            reply,
        })
        .await?;
        rx.await?
    }

    /// Cerca un utente per nome. `None` se non esiste.
    pub async fn find_user(&self, username: &str) -> anyhow::Result<Option<(UserId, String)>> {
        let (reply, rx) = oneshot::channel();
        self.invia(Command::FindUser {
            username: username.to_string(),
            reply,
        })
        .await?;
        rx.await?
    }

    /// Mette un comando in coda.
    ///
    /// Fallisce solo se l'attore non c'e' piu': succede allo spegnimento, ed e'
    /// meglio un errore leggibile che un unwrap che fa cadere una sessione
    /// senza spiegazione.
    async fn invia(&self, cmd: Command) -> anyhow::Result<()> {
        self.tx
            .send(cmd)
            .await
            .map_err(|_| anyhow::anyhow!("attore storage non piu' attivo"))
    }
}

/// Esegue un comando. Gira sul thread bloccante, quindi puo' chiamare
/// rusqlite liberamente.
///
/// Se chi ha chiesto se n'e' andato nel frattempo, il send sul oneshot
/// fallisce: non e' un errore, e' una sessione che ha chiuso. Si ignora.
fn esegui(conn: &Connection, cmd: Command) {
    match cmd {
        Command::CreateUser {
            username,
            password_hash,
            role,
            reply,
        } => {
            let esito = crea_utente(conn, &username, &password_hash, &role);
            let _ = reply.send(esito);
        }
        Command::FindUser { username, reply } => {
            let _ = reply.send(trova_utente(conn, &username));
        }
    }
}

/// Inserisce un utente e restituisce l'id assegnato da SQLite.
///
/// Il rifiuto dei nomi gia' presi non e' scritto qui: lo impone l'indice UNIQUE
/// su `username` nello schema. Il vincolo sta in un posto solo, e vale anche per
/// due registrazioni simultanee — cosa che un controllo "esiste gia'?" seguito
/// da un inserimento non garantirebbe.
fn crea_utente(
    conn: &Connection,
    username: &str,
    password_hash: &str,
    role: &str,
) -> anyhow::Result<UserId> {
    let esito = conn.execute(
        "INSERT INTO users (username, password_hash, role, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        (
            username,
            password_hash,
            role,
            chrono::Utc::now().to_rfc3339(),
        ),
    );

    match esito {
        Ok(_) => Ok(UserId(conn.last_insert_rowid())),
        // Tradotto in un messaggio comprensibile: il chiamante lo rigira al
        // client come "username gia' in uso".
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            anyhow::bail!("username gia' in uso")
        }
        Err(e) => Err(e.into()),
    }
}

/// Cerca un utente e ne restituisce id e hash della password.
fn trova_utente(conn: &Connection, username: &str) -> anyhow::Result<Option<(UserId, String)>> {
    let esito = conn.query_row(
        "SELECT id, password_hash FROM users WHERE username = ?1",
        [username],
        |riga| Ok((UserId(riga.get(0)?), riga.get::<_, String>(1)?)),
    );

    match esito {
        Ok(v) => Ok(Some(v)),
        // "Non esiste" non e' un errore: e' una risposta valida alla domanda.
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
