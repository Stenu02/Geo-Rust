//! Il dialogo con un singolo client, dall'handshake alla chiusura.
//!
//! Descrive la FORMA della conversazione: chi sta parlando, quali messaggi
//! arrivano, cosa si risponde. Non sa come si salva un dato ne' come si instrada
//! un messaggio: quelle cose vivono in storage, registry e admin.
//!
//! Le risposte sono ancora cablate. I valori inventati stanno in `stub`.

use crate::auth;
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

/// La connessione con il framing gia' applicato: si legge e si scrive per messaggi
/// interi, non per byte.
type Wire = Framed<TcpStream, LengthDelimitedCodec>;

/// Quanto si attende il primo frame prima di chiudere.
///
/// Senza questo limite un client che si collega e tace terrebbe occupato un task
/// per sempre, e bastano poche connessioni simili per esaurire le risorse.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Gestisce una singola connessione fino alla sua chiusura.
pub async fn handle(
    stream: TcpStream,
    shutdown: CancellationToken,
    storage: StorageHandle,
) -> anyhow::Result<()> {
    let peer = stream.peer_addr()?;
    let mut wire = Framed::new(stream, frame_codec());

    // Se l'handshake non va a buon fine il client e' gia' stato avvisato:
    // qui resta solo da chiudere.
    let Some(hello) = handshake(&mut wire).await? else {
        return Ok(());
    };

    tracing::info!(%peer, role = ?hello.role, "handshake completato");

    // Il ruolo dichiarato nell'handshake decide come vanno letti tutti i frame
    // successivi: gli stessi byte sono un ClientMessage o un AdminMessage.
    let esito = match hello.role {
        Role::Trucker => trucker_loop(&mut wire, &shutdown, &storage).await,
        Role::Admin => admin_loop(&mut wire, &shutdown, &storage).await,
    };

    tracing::info!(%peer, "sessione chiusa");
    esito
}

/// Legge il primo frame, verifica la versione di protocollo e risponde `Welcome`.
///
/// Restituisce `None` quando la connessione va chiusa: timeout scaduto, client
/// che se ne va prima di parlare, oppure versione di protocollo incompatibile.
///
/// Il primo frame di ogni connessione e' sempre un `Hello`: MessagePack non porta
/// con se' il tipo, quindi e' la posizione nel dialogo a dire come interpretarlo.
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

    // Meglio un errore chiaro adesso che byte interpretati male a meta' sessione.
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

    // Il saluto ha due forme, una per ciascun interlocutore: il camionista e
    // l'amministratore leggono enum diversi.
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
) -> anyhow::Result<()> {
    // Chi e' collegato su questa sessione. Resta None finche' non arriva un login
    // valido. E' una variabile locale, non uno stato condiviso: ogni connessione
    // ha la propria e muore con lei.
    let mut autenticato: Option<(UserId, String)> = None;

    loop {
        // Il frame si estrae qui e si usa sotto: chiamare `send` dentro il ramo
        // terrebbe `wire` prestato due volte.
        let frame = tokio::select! {
            _ = shutdown.cancelled() => break,
            frame = wire.next() => match frame {
                // Nessun frame: il client ha chiuso la connessione.
                None => break,
                Some(frame) => frame?,
            },
        };

        match decode::<ClientMessage>(&frame[..])? {
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
                    // L'errore vero finisce nel log, al client va un messaggio
                    // generico: i dettagli del database non escono da qui.
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

            ClientMessage::Login { username, password } => {
                // Una sessione non deve poter cambiare identita' a meta' strada:
                // le posizioni finirebbero attribuite all'utente sbagliato.
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

                // Si verifica SEMPRE, anche quando l'utente non esiste: in quel
                // caso contro PHC_FITTIZIO. Cosi' il tempo di risposta non
                // dipende dall'esistenza dell'utente.
                let (utente, phc) = match storage.find_user(&username).await? {
                    Some((u, p)) => (Some(u), p),
                    None => (None, PHC_FITTIZIO.to_string()),
                };
                let corretta = auth::verify_password_async(password, phc).await;

                match (utente, corretta) {
                    (Some(user), true) => {
                        tracing::info!(%username, %user, "login effettuato");
                        autenticato = Some((user, username.clone()));
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
                let Some((user, _)) = &autenticato else {
                    send(wire, &non_autenticato()).await?;
                    continue;
                };
                // Lo stato e' ancora sempre lo stesso: la macchina a stati non e'
                // collegata. Il resto del ramo e' pero' gia' vero.
                tracing::debug!(%user, %at, lat = position.lat, lon = position.lon, "posizione");
                send(
                    wire,
                    &ServerMessage::PositionAck {
                        state: UserState::Stopped,
                    },
                )
                .await?;
            }

            ClientMessage::Chat { text } => {
                let Some((user, _)) = &autenticato else {
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
                        entries: stub::flotta(),
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
