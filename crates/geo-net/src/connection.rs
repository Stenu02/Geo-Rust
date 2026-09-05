//! Gestione della connessione TCP verso il server
//! Gestisce handshake, invio comandi, ricezione eventi e riconnessione automatica

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use geo_core::Position;
use geo_protocol::{
    decode, encode, frame_codec, AdminMessage, AdminReply, ClientMessage, ErrorCode, Hello, Role,
    ServerMessage, PROTOCOL_VERSION,
};
use serde::{de::DeserializeOwned, Serialize};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use chrono::Utc;
use std::time::Duration;

use super::events::Event;

/// Comandi che il client invia al task di rete
#[derive(Debug, Clone)]
pub enum Command {
    PositionUpdate { position: Position },
    Chat { text: String },
    Logout,
}

/// Opzioni di connessione per generalizzare l'accesso (utile anche per l'amministratore)
#[derive(Debug, Clone)]
pub struct ConnectOptions {
    pub server_addr: String,
    pub username: String,
    pub password: String,
    pub role: Role,
    pub register: bool,
}

/// Policy per gestire i tempi di riconnessione
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_secs(2),
            max_backoff: Duration::from_secs(60),
        }
    }
}

/// Canali di comunicazione tra il main e il task di rete in background
pub struct ClientHandle {
    pub command_tx: mpsc::Sender<Command>,
    pub event_rx: mpsc::Receiver<Event>,
}

/// Socket TCP con framing a lunghezza prefissa già applicato: ogni frame è un messaggio intero, non un pezzo di stream
pub type ClientFramed = Framed<TcpStream, LengthDelimitedCodec>;

/// Serializza e invia un messaggio sul socket
/// (pubblica per permetterne l'uso anche da geo-admin)
pub async fn send_msg<T: Serialize>(framed: &mut ClientFramed, msg: &T) -> anyhow::Result<()> {
    let bytes = encode(msg)?;
    framed.send(Bytes::from(bytes)).await?;
    Ok(())
}

/// Riceve e deserializza il prossimo messaggio dal socket
/// (pubblica per permetterne l'uso anche da geo-admin)
pub async fn recv_msg<T: DeserializeOwned>(framed: &mut ClientFramed) -> anyhow::Result<T> {
    let frame = framed.next().await.ok_or_else(|| anyhow::anyhow!("Connessione chiusa."))??;
    Ok(decode(&frame)?)
}

impl ClientHandle {
    /// Stabilisce la connessione iniziale bloccante per validare le credenziali
    pub async fn connect(opts: ConnectOptions, policy: ReconnectPolicy) -> anyhow::Result<Self> {
        // ClientHandle gestisce esclusivamente gli eventi specifici del camionista
        if opts.role != Role::Trucker {
            anyhow::bail!("ClientHandle è progettato solo per i camionisti.");
        }

        let framed = connect_and_auth(&opts).await?;

        let (command_tx, command_rx) = mpsc::channel::<Command>(32);
        let (event_tx, event_rx) = mpsc::channel::<Event>(32);

        tokio::spawn(run_connection(framed, command_rx, event_tx, opts, policy));

        Ok(Self { command_tx, event_rx })
    }
}

/// Stabilisce il socket TCP ed esegue la sequenza Hello/Login
/// (pubblica per geo-admin, può usarla per creare il socket di rete)
pub async fn connect_and_auth(opts: &ConnectOptions) -> anyhow::Result<ClientFramed> {
    let stream = TcpStream::connect(&opts.server_addr).await?;
    let mut framed = Framed::new(stream, frame_codec());

    // l'handshake dichiara il ruolo dinamicamente in base alle opzioni
    send_msg(&mut framed, &Hello { protocol_version: PROTOCOL_VERSION, role: opts.role }).await?;

    match opts.role {
        Role::Trucker => {
            match recv_msg::<ServerMessage>(&mut framed).await? {
                ServerMessage::Welcome { .. } => {}
                other => return Err(anyhow::anyhow!("Handshake fallito: {:?}.", other)),
            }

            // Gestisce la registrazione se richiesta dal client prima di fare login
            if opts.register {
                send_msg(&mut framed, &ClientMessage::Register {
                    username: opts.username.clone(),
                    password: opts.password.clone()
                }).await?;

                match recv_msg::<ServerMessage>(&mut framed).await? {
                    ServerMessage::RegisterOk { .. } => {}
                    ServerMessage::Error { code: ErrorCode::UsernameTaken, .. } => {}
                    other => return Err(anyhow::anyhow!("Registrazione fallita: {:?}.", other)),
                }
            }

            send_msg(&mut framed, &ClientMessage::Login { username: opts.username.clone(), password: opts.password.clone() }).await?;
            match recv_msg::<ServerMessage>(&mut framed).await? {
                ServerMessage::LoginOk { .. } => Ok(framed),
                other => Err(anyhow::anyhow!("Login fallito: {:?}.", other)),
            }
        }
        Role::Admin => {
            match recv_msg::<AdminReply>(&mut framed).await? {
                AdminReply::Welcome { .. } => {}
                other => return Err(anyhow::anyhow!("Handshake fallito: {:?}.", other)),
            }

            send_msg(&mut framed, &AdminMessage::Login {
                username: opts.username.clone(),
                password: opts.password.clone()
            }).await?;

            match recv_msg::<AdminReply>(&mut framed).await? {
                AdminReply::LoginOk { .. } => Ok(framed),
                other => Err(anyhow::anyhow!("Login admin fallito: {:?}.", other)),
            }
        }
    }
}

/// Loop asincrono: smista comandi, riceve frame, manda heartbeat e gestisce le disconnessioni
async fn run_connection(
    mut framed: ClientFramed,
    mut command_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::Sender<Event>,
    opts: ConnectOptions,
    policy: ReconnectPolicy,
) {
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    let mut backoff = policy.initial_backoff;

    // Notifica iniziale alla UI
    let _ = event_tx.send(Event::Connected).await;
    let _ = event_tx.send(Event::LoggedIn { username: opts.username.clone() }).await;

    loop {
        tokio::select! {
            // Elabora i comandi in uscita richiesti dal main
            cmd = command_rx.recv() => {
                match cmd {
                    Some(Command::PositionUpdate { position }) => {
                        let _ = send_msg(&mut framed, &ClientMessage::PositionUpdate { at: Utc::now(), position }).await;
                    }
                    Some(Command::Chat { text }) => {
                        let _ = send_msg(&mut framed, &ClientMessage::Chat { text }).await;
                    }
                    Some(Command::Logout) => {
                        let _ = send_msg(&mut framed, &ClientMessage::Logout).await;
                        break;
                    }
                    None => break,
                }
            }
            // Elabora i messaggi in ingresso dal server
            frame = framed.next() => {
                match frame {
                    Some(Ok(bytes)) => {
                        if let Ok(msg) = decode::<ServerMessage>(&bytes) {
                            match msg {
                                ServerMessage::PositionAck { state } => {
                                    let _ = event_tx.send(Event::PositionAck { state }).await;
                                }
                                ServerMessage::Chat { text, broadcast, at } => {
                                    let _ = event_tx.send(Event::ChatReceived { text, broadcast, at }).await;
                                }
                                ServerMessage::Error { code, detail } => {
                                    let _ = event_tx.send(Event::ServerError { code, detail }).await;
                                }
                                _ => {}
                            }
                        }
                    }
                    // Avvia il processo di riconnessione con backoff esponenziale in caso la connessione cadesse
                    _ => {
                        let _ = event_tx.send(Event::Disconnected { reason: "Connessione persa, tentativo di riconnessione...".into() }).await;

                        loop {
                            tokio::time::sleep(backoff).await;

                            // Forza la registrazione a false durante la riconnessione per non duplicare l'account
                            let mut reconnect_opts = opts.clone();
                            reconnect_opts.register = false;

                            match connect_and_auth(&reconnect_opts).await {
                                Ok(new_framed) => {
                                    framed = new_framed;
                                    // Ripristina il backoff usando la policy
                                    backoff = policy.initial_backoff;

                                    // Avvisa la UI che si è di nuovo online
                                    let _ = event_tx.send(Event::Connected).await;
                                    let _ = event_tx.send(Event::LoggedIn { username: opts.username.clone() }).await;
                                    break;
                                }
                                Err(_) => {
                                    // Scala il backoff usando la policy
                                    backoff = (backoff * 2).min(policy.max_backoff);
                                }
                            }
                        }
                    }
                }
            }
            // Mantiene attivo il socket
            _ = heartbeat.tick() => {
                let _ = send_msg(&mut framed, &ClientMessage::Heartbeat).await;
            }
        }
    }
}