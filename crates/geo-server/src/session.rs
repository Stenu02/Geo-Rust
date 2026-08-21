//! Il dialogo con un singolo client, dall'handshake alla chiusura.
//!
//! Descrive la FORMA della conversazione: chi sta parlando, quali messaggi
//! arrivano, cosa si risponde. Non sa come si salva un dato ne' come si instrada
//! un messaggio: quelle cose vivono in storage, registry e admin.
//!
//! Le risposte sono ancora cablate. I valori inventati stanno in `stub`.

use crate::stub;
use futures::{SinkExt, StreamExt};
use geo_core::UserState;
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
pub async fn handle(stream: TcpStream, shutdown: CancellationToken) -> anyhow::Result<()> {
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
        Role::Trucker => trucker_loop(&mut wire, &shutdown).await,
        Role::Admin => admin_loop(&mut wire, &shutdown).await,
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

/// Dialogo con un camionista, fino alla disconnessione o alla chiusura del server.
async fn trucker_loop(wire: &mut Wire, shutdown: &CancellationToken) -> anyhow::Result<()> {
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
            // L'utente vero e la verifica della password arrivano con storage e auth.
            ClientMessage::Register { username, .. } => {
                tracing::info!(%username, "registrazione (finta)");
                send(wire, &ServerMessage::RegisterOk { user: stub::UTENTE }).await?;
            }
            ClientMessage::Login { username, .. } => {
                tracing::info!(%username, "login (finto)");
                send(
                    wire,
                    &ServerMessage::LoginOk {
                        user: stub::UTENTE,
                        username,
                    },
                )
                .await?;
            }
            // Lo stato e' sempre lo stesso: la macchina a stati non e' ancora
            // collegata, e chiamarla adesso farebbe scattare un todo!().
            ClientMessage::PositionUpdate { at, position } => {
                tracing::debug!(%at, lat = position.lat, lon = position.lon, "posizione");
                send(
                    wire,
                    &ServerMessage::PositionAck {
                        state: UserState::Stopped,
                    },
                )
                .await?;
            }
            // Il messaggio va all'amministratore: senza registro delle sessioni,
            // per ora si puo' solo annotarlo.
            ClientMessage::Chat { text } => tracing::info!(%text, "messaggio dal camionista"),
            ClientMessage::Heartbeat => send(wire, &ServerMessage::Pong).await?,
            ClientMessage::Logout => break,
        }
    }

    Ok(())
}

/// Dialogo con l'amministratore.
async fn admin_loop(wire: &mut Wire, shutdown: &CancellationToken) -> anyhow::Result<()> {
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
            // Qui passera' la giuntura verso admin.rs: estrarre dal database,
            // chiamare le analisi, rispedire il report.
            AdminMessage::Query { user, spec } => {
                send(
                    wire,
                    &AdminReply::QueryResult(Box::new(stub::report(user, spec))),
                )
                .await?;
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
