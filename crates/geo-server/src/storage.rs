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

use chrono::{DateTime, Utc};
use geo_core::position::FixedPosition;
use geo_core::{Position, StateInterval, TimeRange, Timestamped, UserId, UserState};
use rusqlite::Connection;
use std::path::Path;
use tokio::sync::{mpsc, oneshot};

/// Errore distinguibile dagli altri: permette a chi chiama di rispondere
/// "username gia' in uso" invece di un errore generico, senza dover confrontare
/// stringhe di messaggio.
#[derive(Debug, thiserror::Error)]
#[error("username gia' in uso")]
pub struct UsernameTaken;

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
    PositionsFor {
        user: UserId,
        range: TimeRange,
        reply: oneshot::Sender<anyhow::Result<Vec<FixedPosition>>>,
    },
    IntervalsFor {
        user: UserId,
        range: TimeRange,
        reply: oneshot::Sender<anyhow::Result<Vec<StateInterval>>>,
    },
    /// Salva una posizione ricevuta da un camionista.
    SavePosition {
        user: UserId,
        at: DateTime<Utc>,
        position: Position,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    /// Chiude l'intervallo di stato aperto e ne apre uno nuovo.
    ApplyTransition {
        user: UserId,
        to: UserState,
        at: DateTime<Utc>,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    /// Salva un messaggio di chat nello storico.
    SaveMessage {
        from_user: Option<UserId>,
        to_user: Option<UserId>,
        body: String,
        at: DateTime<Utc>,
        delivered: bool,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    /// Prende i messaggi diretti a un utente che non gli sono mai arrivati,
    /// e li segna come consegnati.
    TakePending {
        user: UserId,
        reply: oneshot::Sender<anyhow::Result<Vec<(String, DateTime<Utc>)>>>,
    },
    /// Chiude gli intervalli rimasti aperti da un'esecuzione precedente.
    CloseDangling {
        at: DateTime<Utc>,
        reply: oneshot::Sender<anyhow::Result<usize>>,
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

    /// Restituisce un vettore di FixedPositions, necessario per l'analisi e la creazione del report di movimento
    pub async fn positions_for(
        &self,
        user: UserId,
        range: TimeRange,
    ) -> anyhow::Result<Vec<FixedPosition>> {
        let (reply, rx) = oneshot::channel();
        self.invia(Command::PositionsFor { user, range, reply })
            .await?;
        rx.await?
    }

    /// Restituisce un vettore di StateInterval, necessario per l'analisi e la creazione del report di movimento
    pub async fn intervals_for(
        &self,
        user: UserId,
        range: TimeRange,
    ) -> anyhow::Result<Vec<StateInterval>> {
        let (reply, rx) = oneshot::channel();
        self.invia(Command::IntervalsFor { user, range, reply })
            .await?;
        rx.await?
    }

    /// Salva una posizione nello storico.
    pub async fn save_position(
        &self,
        user: UserId,
        at: DateTime<Utc>,
        position: Position,
    ) -> anyhow::Result<()> {
        let (reply, rx) = oneshot::channel();
        self.invia(Command::SavePosition {
            user,
            at,
            position,
            reply,
        })
        .await?;
        rx.await?
    }

    /// Registra un cambio di stato: chiude l'intervallo aperto a `at` e ne apre
    /// uno nuovo nello stato `to`, sempre a `at`.
    pub async fn apply_transition(
        &self,
        user: UserId,
        to: UserState,
        at: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let (reply, rx) = oneshot::channel();
        self.invia(Command::ApplyTransition {
            user,
            to,
            at,
            reply,
        })
        .await?;
        rx.await?
    }

    /// Salva un messaggio di chat.
    ///
    /// `from_user` a None significa "dall'amministratore", `to_user` a None
    /// significa "a tutta la flotta": e' la convenzione dello schema.
    /// `delivered` dice se il destinatario lo ha gia' ricevuto in tempo reale.
    pub async fn save_message(
        &self,
        from_user: Option<UserId>,
        to_user: Option<UserId>,
        body: &str,
        at: DateTime<Utc>,
        delivered: bool,
    ) -> anyhow::Result<()> {
        let (reply, rx) = oneshot::channel();
        self.invia(Command::SaveMessage {
            from_user,
            to_user,
            body: body.to_string(),
            at,
            delivered,
            reply,
        })
        .await?;
        rx.await?
    }

    /// Ritira i messaggi arretrati di un utente e li segna come consegnati.
    ///
    /// Si chiama al login: sono i messaggi che gli sono stati scritti mentre
    /// non era collegato. Il ritiro e la marcatura avvengono insieme, cosi'
    /// non gli arrivano due volte al login successivo.
    pub async fn take_pending(&self, user: UserId) -> anyhow::Result<Vec<(String, DateTime<Utc>)>> {
        let (reply, rx) = oneshot::channel();
        self.invia(Command::TakePending { user, reply }).await?;
        rx.await?
    }

    /// Chiude gli intervalli lasciati aperti da un'esecuzione precedente e
    /// restituisce quanti ne ha chiusi. Da chiamare all'avvio.
    pub async fn close_dangling(&self, at: DateTime<Utc>) -> anyhow::Result<usize> {
        let (reply, rx) = oneshot::channel();
        self.invia(Command::CloseDangling { at, reply }).await?;
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
        Command::PositionsFor { user, range, reply } => {
            let _ = reply.send(posizioni_per(conn, user, range));
        }
        Command::IntervalsFor { user, range, reply } => {
            let _ = reply.send(intervalli_per(conn, user, range));
        }
        Command::SavePosition {
            user,
            at,
            position,
            reply,
        } => {
            let _ = reply.send(salva_posizione(conn, user, at, position));
        }
        Command::ApplyTransition {
            user,
            to,
            at,
            reply,
        } => {
            let _ = reply.send(applica_transizione(conn, user, to, at));
        }
        Command::SaveMessage {
            from_user,
            to_user,
            body,
            at,
            delivered,
            reply,
        } => {
            let _ = reply.send(salva_messaggio(
                conn, from_user, to_user, &body, at, delivered,
            ));
        }
        Command::TakePending { user, reply } => {
            let _ = reply.send(ritira_arretrati(conn, user));
        }
        Command::CloseDangling { at, reply } => {
            let _ = reply.send(chiudi_intervalli_pendenti(conn, at));
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
            anyhow::bail!(UsernameTaken)
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

/// Restituisce le posizioni assunte dall'utente all'interno del range specificato
fn posizioni_per(
    conn: &Connection,
    user: UserId,
    range: TimeRange,
) -> anyhow::Result<Vec<FixedPosition>> {
    let mut statement = conn.prepare(
        "SELECT at, lat, lon FROM positions
         WHERE user_id = ?1 AND at >= ?2 AND at < ?3
         ORDER BY at",
    )?;
    let rows = statement.query_map(
        (user.0, range.start.to_rfc3339(), range.end.to_rfc3339()),
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
            ))
        },
    )?;
    let mut output = Vec::new();
    for row in rows {
        let (at, lat, lon) = row?;
        let at = DateTime::parse_from_rfc3339(&at)?.with_timezone(&Utc);
        output.push(Timestamped::new(at, Position::new(lat, lon)))
    }
    Ok(output)
}

/// Restituisce gli intervalli di tempo registrati dall'utente nel range specificato.
fn intervalli_per(
    conn: &Connection,
    user: UserId,
    range: TimeRange,
) -> anyhow::Result<Vec<StateInterval>> {
    let mut statement = conn.prepare(
        "SELECT state, start_at, end_at FROM state_intervals
         WHERE user_id = ?1 AND start_at < ?3
         AND (end_at IS NULL OR end_at >= ?2)
         ORDER BY start_at",
    )?;
    let rows = statement.query_map(
        (user.0, range.start.to_rfc3339(), range.end.to_rfc3339()),
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    let mut output = Vec::new();
    for row in rows {
        let (state, start_at, end_at) = row?;
        let state = UserState::from_db_str(&state)
            .ok_or_else(|| anyhow::anyhow!("stato non riconosciuto nel DB: {state}"))?;
        let start = DateTime::parse_from_rfc3339(&start_at)?.with_timezone(&Utc);
        let end = end_at
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|d| d.with_timezone(&Utc)))
            .transpose()?;
        output.push(StateInterval { state, start, end });
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 20, h, 0, 0).unwrap()
    }

    /// Un DB in memoria con lo schema applicato e un utente (id 1).
    fn db_con_utente() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, created_at)
             VALUES (1, 'camion', 'x', 'trucker', ?1)",
            [utc(0).to_rfc3339()],
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_posizioni_per_filtra_sul_range() {
        let conn = db_con_utente();
        for (at, lat, lon) in [
            (utc(11), 45.0, 7.0),
            (utc(12), 45.5, 7.0),
            (utc(23), 46.0, 7.0),
        ] {
            conn.execute(
                "INSERT INTO positions (user_id, at, lat, lon) VALUES (1, ?1, ?2, ?3)",
                (at.to_rfc3339(), lat, lon),
            )
            .unwrap();
        }

        let range = TimeRange::new(utc(10), utc(20)); // [10:00, 20:00)
        let pos = posizioni_per(&conn, UserId(1), range).unwrap();

        // La posizione delle 23:00 e' fuori dal range.
        assert_eq!(pos.len(), 2);
        assert_eq!(pos[0].at, utc(11));
        assert_eq!(pos[1].at, utc(12));
    }

    #[test]
    fn test_intervalli_per_include_sovrapposti_e_aperti() {
        let conn = db_con_utente();
        // Chiuso dentro il range.
        conn.execute(
            "INSERT INTO state_intervals (user_id, state, start_at, end_at)
             VALUES (1, 'moving', ?1, ?2)",
            (utc(11).to_rfc3339(), utc(12).to_rfc3339()),
        )
        .unwrap();
        // Aperto (end NULL): si sovrappone al range.
        conn.execute(
            "INSERT INTO state_intervals (user_id, state, start_at, end_at)
             VALUES (1, 'stopped', ?1, NULL)",
            [utc(12).to_rfc3339()],
        )
        .unwrap();
        // Interamente prima del range: deve essere escluso.
        conn.execute(
            "INSERT INTO state_intervals (user_id, state, start_at, end_at)
             VALUES (1, 'moving', ?1, ?2)",
            (utc(1).to_rfc3339(), utc(2).to_rfc3339()),
        )
        .unwrap();

        let range = TimeRange::new(utc(10), utc(20));
        let iv = intervalli_per(&conn, UserId(1), range).unwrap();

        assert_eq!(iv.len(), 2);
        assert_eq!(iv[0].state, UserState::Moving);
        assert_eq!(iv[1].state, UserState::Stopped);
        assert_eq!(iv[1].end, None); // l'intervallo aperto resta aperto
    }
}

/// Inserisce una posizione nello storico.
///
/// Gli istanti vanno in ISO 8601 UTC, lo stesso formato che `posizioni_per` si
/// aspetta di rileggere: le due funzioni vanno tenute d'accordo.
fn salva_posizione(
    conn: &Connection,
    user: UserId,
    at: DateTime<Utc>,
    position: Position,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO positions (user_id, at, lat, lon) VALUES (?1, ?2, ?3, ?4)",
        (user.0, at.to_rfc3339(), position.lat, position.lon),
    )?;
    Ok(())
}

/// Chiude l'intervallo aperto dell'utente e ne apre uno nuovo, entrambi a `at`.
///
/// Le due istruzioni stanno in una TRANSAZIONE perche' devono valere insieme:
/// se passasse solo la chiusura, l'utente resterebbe senza intervallo aperto e
/// il suo stato successivo diventerebbe irrecuperabile; se passasse solo
/// l'apertura, l'indice unico dello schema rifiuterebbe il secondo aperto.
///
/// Chiudere zero intervalli e' normale: succede al primo login di un utente,
/// che non ne ha ancora nessuno.
fn applica_transizione(
    conn: &Connection,
    user: UserId,
    to: UserState,
    at: DateTime<Utc>,
) -> anyhow::Result<()> {
    let quando = at.to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE state_intervals SET end_at = ?2
         WHERE user_id = ?1 AND end_at IS NULL",
        (user.0, &quando),
    )?;
    tx.execute(
        "INSERT INTO state_intervals (user_id, state, start_at, end_at)
         VALUES (?1, ?2, ?3, NULL)",
        (user.0, to.as_db_str(), &quando),
    )?;
    tx.commit()?;
    Ok(())
}

/// Chiude tutti gli intervalli rimasti aperti, e restituisce quanti erano.
///
/// Serve dopo un arresto anomalo: se il server muore mentre dieci camionisti
/// sono collegati, restano dieci intervalli senza fine, e ogni analisi
/// successiva li conterebbe fino a "adesso". Si chiudono all'istante di avvio:
/// non sappiamo quando il server sia morto davvero, e questa e' la scelta
/// conservativa piu' semplice da spiegare.
fn chiudi_intervalli_pendenti(conn: &Connection, at: DateTime<Utc>) -> anyhow::Result<usize> {
    let n = conn.execute(
        "UPDATE state_intervals SET end_at = ?1 WHERE end_at IS NULL",
        [at.to_rfc3339()],
    )?;
    Ok(n)
}

/// Inserisce un messaggio nello storico della chat.
fn salva_messaggio(
    conn: &Connection,
    from_user: Option<UserId>,
    to_user: Option<UserId>,
    body: &str,
    at: DateTime<Utc>,
    delivered: bool,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO messages (from_user_id, to_user_id, body, sent_at, delivered)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        (
            from_user.map(|u| u.0),
            to_user.map(|u| u.0),
            body,
            at.to_rfc3339(),
            delivered as i64,
        ),
    )?;
    Ok(())
}

/// Legge i messaggi arretrati di un utente e li marca come consegnati.
///
/// Le due operazioni stanno in una TRANSAZIONE: se la marcatura fallisse dopo
/// la lettura, al login successivo l'utente riceverebbe di nuovo gli stessi
/// messaggi. Meglio non consegnarli affatto che consegnarli due volte.
///
/// I broadcast sono esclusi: `to_user_id IS NULL` significa "a tutti quelli
/// collegati in quel momento", e chi era assente non li deve recuperare.
fn ritira_arretrati(
    conn: &Connection,
    user: UserId,
) -> anyhow::Result<Vec<(String, DateTime<Utc>)>> {
    let tx = conn.unchecked_transaction()?;

    let arretrati = {
        let mut statement = tx.prepare(
            "SELECT body, sent_at FROM messages
             WHERE to_user_id = ?1 AND delivered = 0
             ORDER BY sent_at",
        )?;
        let righe = statement.query_map([user.0], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;

        let mut out = Vec::new();
        for riga in righe {
            let (body, sent_at) = riga?;
            let at = DateTime::parse_from_rfc3339(&sent_at)?.with_timezone(&Utc);
            out.push((body, at));
        }
        out
    };

    tx.execute(
        "UPDATE messages SET delivered = 1 WHERE to_user_id = ?1 AND delivered = 0",
        [user.0],
    )?;
    tx.commit()?;

    Ok(arretrati)
}
