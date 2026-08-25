//! Ogni connessione accettata dal server diventa qui un task a se' stante:
//! `handle` fa l'handshake, poi smista verso `trucker_loop` o `admin_loop` a
//! seconda del ruolo dichiarato. I due cicli leggono un messaggio alla volta e
//! rispondono, appoggiandosi a `storage` per il database e a `registry` per
//! sapere e aggiornare chi e' collegato — non fanno mai questo lavoro da soli.

use crate::auth;
use crate::registry::Registry;
use crate::storage::{StorageHandle, UsernameTaken};
use crate::stub;
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use geo_core::{TimeRange, UserId, UserState};
use geo_protocol::{
    decode, encode, frame_codec, AdminMessage, AdminReply, ClientMessage, ErrorCode, Hello, Role,
    ServerMessage, PROTOCOL_VERSION,
};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio_util::sync::CancellationToken;


type Wire = Framed<TcpStream, LengthDelimitedCodec>;

/// Quanto si attende il primo frame prima di chiudere.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

// Gestisce una singola connessione fino alla sua chiusura.
// Funzione chiamata in lib, per ogni connessione accettata
pub async fn handle(
    stream: TcpStream,
    shutdown: CancellationToken,
    storage: StorageHandle,
    registry: Registry,
) -> anyhow::Result<()> {

    let peer = stream.peer_addr()?;
    let mut wire = Framed::new(stream, frame_codec());

    //Chiamata alla funzione handshake
    let Some(hello) = handshake(&mut wire).await? else {
        return Ok(());
    };

    tracing::info!(%peer, role = ?hello.role, "handshake completato");

    // Chi si e' autenticato su questa connessione. Vive qui e non dentro il
    // ciclo perche' serve anche DOPO, per la chiusura ordinata.
    let mut sessione: Option<(UserId, String)> = None;

    // Il ruolo dichiarato nell'handshake decide come vanno letti tutti i frame
    // successivi: gli stessi byte sono un ClientMessage o un AdminMessage.
    let esito = match hello.role {
        Role::Trucker => {
            trucker_loop(&mut wire, &shutdown, &storage, &registry, &mut sessione).await
        }
        Role::Admin => admin_loop(&mut wire, &shutdown, &storage, &registry).await,
    };


    if let Some((user, username)) = sessione {
        let ora = Utc::now();
        if let Some(t) = registry.on_disconnect(user, ora) {
            if let Err(e) = storage.apply_transition(user, t.to, t.at).await {
                tracing::error!(%user, error = %e, "chiusura intervallo fallita");
            }
        }
        registry.unregister(user);
        tracing::info!(%peer, %username, "camionista scollegato");
    }

    tracing::info!(%peer, "sessione chiusa");
    esito
}


async fn handshake(wire: &mut Wire) -> anyhow::Result<Option<Hello>> {
    let frame = match tokio::time::timeout(HANDSHAKE_TIMEOUT, wire.next()).await {
        Err(_) => {
            tracing::warn!("handshake non completato entro il timeout");
            return Ok(None);
        }
        Ok(None) => return Ok(None),
        Ok(Some(frame)) => frame?,
    };

    let hello: Hello = decode(&frame[..])?;

    // Controllo sulla struttura Hello
    if hello.protocol_version != PROTOCOL_VERSION {
        tracing::warn!(
            richiesta = hello.protocol_version,
            nostra = PROTOCOL_VERSION,
            "versione di protocollo incompatibile"
        );
        let detail = format!(
            "il server parla la versione {PROTOCOL_VERSION}, il client la {}",
            hello.protocol_version
        );
        reply_error(wire, hello.role, ErrorCode::ProtocolVersionMismatch, detail).await?;
        return Ok(None);
    }

    // Messaggio di Welcome in base al ruolo
    match hello.role {
        Role::Trucker => {
            send(
                wire,
                &ServerMessage::Welcome {
                    protocol_version: PROTOCOL_VERSION,
                },
            )
            .await?
        }
        Role::Admin => {
            send(
                wire,
                &AdminReply::Welcome {
                    protocol_version: PROTOCOL_VERSION,
                },
            )
            .await?
        }
    }

    Ok(Some(hello))
}

/// Hash contro cui verificare quando l'utente non esiste.
///
/// Senza, rispondere a un username inesistente costerebbe zero millisecondi
/// contro i ~15 di una verifica vera, e chi cronometra le risposte scoprirebbe
/// quali utenti esistono. Misurato: 19 ms contro 0.
const PHC_FITTIZIO: &str = "$argon2id$v=19$m=19456,t=2,p=1$mPPAx9/sJgdFwPpOmZkZzQ$RW7w1/7mKTOGp6CFI7okMaRciTuid7mzwPmOJuhtQRI";

/// Dialogo con un camionista, fino alla disconnessione o alla chiusura del server.
async fn trucker_loop(
    wire: &mut Wire,
    shutdown: &CancellationToken,
    storage: &StorageHandle,
    registry: &Registry,
    autenticato: &mut Option<(UserId, String)>,
) -> anyhow::Result<()> {
    loop {
        // Il frame si estrae qui
        let frame = tokio::select! {
            _ = shutdown.cancelled() => break,
            frame = wire.next() => match frame {
                // Nessun frame: il client ha chiuso la connessione.
                None => break,
                Some(frame) => frame?,
            },
        };

        match decode::<ClientMessage>(&frame[..])? {
            //Registrazione
            ClientMessage::Register { username, password } => {
                let phc = auth::hash_password_async(password).await?;
                match storage.create_user(&username, &phc, "trucker").await {
                    Ok(user) => {
                        tracing::info!(%username, %user, "utente registrato");
                        send(wire, &ServerMessage::RegisterOk { user }).await?;
                    }
                    Err(e) if e.downcast_ref::<UsernameTaken>().is_some() => {
                        send(
                            wire,
                            &ServerMessage::Error {
                                code: ErrorCode::UsernameTaken,
                                detail: "username gia' in uso".into(),
                            },
                        )
                        .await?;
                    }
                    Err(e) => {
                        tracing::error!(%username, error = %e, "registrazione fallita");
                        send(
                            wire,
                            &ServerMessage::Error {
                                code: ErrorCode::Internal,
                                detail: "registrazione non riuscita".into(),
                            },
                        )
                        .await?;
                    }
                }
            }
            //Login
            ClientMessage::Login { username, password } => {
                //Già autenticato
                if autenticato.is_some() {
                    send(
                        wire,
                        &ServerMessage::Error {
                            code: ErrorCode::NotAuthorized,
                            detail: "sessione gia' autenticata".into(),
                        },
                    )
                    .await?;
                    continue;
                }

                let (utente, phc) = match storage.find_user(&username).await? {
                    Some((u, p)) => (Some(u), p),
                    None => (None, PHC_FITTIZIO.to_string()),
                };
                let corretta = auth::verify_password_async(password, phc).await;

                match (utente, corretta) {
                    (Some(user), true) => {
                        tracing::info!(%username, %user, "login effettuato");
                        let (_posta, _annunci) = registry.register(user, username.clone());

                        // SCONNESSO -> FERMO. La transizione va scritta subito:
                        // e' l'apertura del primo intervallo di questa sessione,
                        // e senza di essa le analisi avrebbero un buco.
                        let ora = Utc::now();
                        if let Some(t) = registry.on_login(user, ora) {
                            storage.apply_transition(user, t.to, t.at).await?;
                        }

                        *autenticato = Some((user, username.clone()));
                        send(wire, &ServerMessage::LoginOk { user, username }).await?;
                    }
                    // Stesso identico messaggio nei due casi: non si rivela se sia
                    // l'utente a non esistere o la password a essere sbagliata.
                    _ => {
                        tracing::warn!(%username, "login rifiutato");
                        send(
                            wire,
                            &ServerMessage::Error {
                                code: ErrorCode::InvalidCredentials,
                                detail: "credenziali non valide".into(),
                            },
                        )
                        .await?;
                    }
                }
            }

            ClientMessage::PositionUpdate { at, position } => {
                let Some(user) = autenticato.as_ref().map(|(u, _)| *u) else {
                    send(wire, &non_autenticato()).await?;
                    continue;
                };

                // Una coordinata assurda non deve entrare nello storico
                if !position.is_valid() {
                    send(
                        wire,
                        &ServerMessage::Error {
                            code: ErrorCode::InvalidPosition,
                            detail: "coordinate fuori intervallo".into(),
                        },
                    )
                    .await?;
                    continue;
                }

                tracing::debug!(%user, %at, lat = position.lat, lon = position.lon, "posizione");
                storage.save_position(user, at, position).await?;

                // La macchina a stati decide se questa posizione e' un cambio.
                if let Some(t) = registry.on_position(user, at, position) {
                    storage.apply_transition(user, t.to, t.at).await?;
                }

              
                let state = registry.state_of(user).unwrap_or(UserState::Stopped);
                send(wire, &ServerMessage::PositionAck { state }).await?;
            }

            ClientMessage::Chat { text } => {
                let Some((user, _)) = autenticato.as_ref() else {
                    send(wire, &non_autenticato()).await?;
                    continue;
                };
                // Il messaggio va all'amministratore: senza registro delle
                // sessioni, per ora si puo' solo annotarlo.
                tracing::info!(%user, %text, "messaggio dal camionista");
            }

            ClientMessage::Heartbeat => send(wire, &ServerMessage::Pong).await?,
            ClientMessage::Logout => break,
        }
    }

    Ok(())
}

/// Dialogo con l'amministratore.
async fn admin_loop(
    wire: &mut Wire,
    shutdown: &CancellationToken,
    storage: &StorageHandle,
    registry: &Registry,
) -> anyhow::Result<()> {
    loop {
        let frame = tokio::select! {
            _ = shutdown.cancelled() => break,
            frame = wire.next() => match frame {
                None => break,
                Some(frame) => frame?,
            },
        };

        match decode::<AdminMessage>(&frame[..])? {
            AdminMessage::Login { username, .. } => {
                tracing::info!(%username, "login amministratore (finto)");
                send(
                    wire,
                    &AdminReply::LoginOk {
                        user: stub::AMMINISTRATORE,
                        username,
                    },
                )
                .await?;
            }
            AdminMessage::ListFleet => {
                send(
                    wire,
                    &AdminReply::Fleet {
                        entries: registry.fleet(),
                    },
                )
                .await?;
            }
            // risolve il periodo, estrae dal database intervalli e
            // posizioni, chiama la creazione del repor e lo rispedisce.
            AdminMessage::Query { user, spec } => {
                let now = Utc::now();
                let range = TimeRange::resolve(spec, now);
                let intervals = storage.intervals_for(user, range).await?;
                let positions = storage.positions_for(user, range).await?;
                let report = crate::admin::AdminHandler::build_report(
                    user, spec, range, &intervals, &positions, now,
                );

                send(wire, &AdminReply::QueryResult(Box::new(report))).await?;
            }
            AdminMessage::SendDirect { to, text } => {
                tracing::info!(%to, %text, "messaggio diretto (non ancora instradato)")
            }
            AdminMessage::SendBroadcast { text } => {
                tracing::info!(%text, "broadcast (non ancora instradato)")
            }
            AdminMessage::Heartbeat => send(wire, &AdminReply::Pong).await?,
            AdminMessage::Logout => break,
        }
    }

    Ok(())
}

/// Invia un errore nella forma che il destinatario sa leggere.
async fn reply_error(
    wire: &mut Wire,
    role: Role,
    code: ErrorCode,
    detail: String,
) -> anyhow::Result<()> {
    match role {
        Role::Trucker => send(wire, &ServerMessage::Error { code, detail }).await,
        Role::Admin => send(wire, &AdminReply::Error { code, detail }).await,
    }
}

/// Serializza un messaggio e lo scrive come un frame solo.
async fn send<T: serde::Serialize>(wire: &mut Wire, msg: &T) -> anyhow::Result<()> {
    wire.send(encode(msg)?.into()).await?;
    Ok(())
}

/// L'errore restituito a chi manda messaggi prima di aver fatto login.
fn non_autenticato() -> ServerMessage {
    ServerMessage::Error {
        code: ErrorCode::NotAuthenticated,
        detail: "serve il login prima di inviare posizioni o messaggi".into(),
    }
}
