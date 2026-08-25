//! gestione della connessione TCP verso il server
//! gestisce handshake, invio comandi, ricezione eventi e riconnessione automatica

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use geo_core::Position;
use geo_protocol::{decode, encode, frame_codec, ClientMessage, Hello, Role, ServerMessage, PROTOCOL_VERSION};
use serde::{de::DeserializeOwned, Serialize};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use chrono::Utc;
use std::time::Duration;

use super::events::Event;

/// comandi che il client invia al task di rete
#[derive(Debug, Clone)]
pub enum Command {
    PositionUpdate { position: Position },
    Chat { text: String },
    Logout,
}

/// canali di comunicazione tra il main e il task di rete in background
pub struct ClientHandle {
    pub command_tx: mpsc::Sender<Command>,
    pub event_rx: mpsc::Receiver<Event>,
}

type ClientFramed = Framed<TcpStream, LengthDelimitedCodec>;

/// serializza e invia un messaggio sul socket
async fn send_msg<T: Serialize>(framed: &mut ClientFramed, msg: &T) -> anyhow::Result<()> {
    let bytes = encode(msg)?;
    framed.send(Bytes::from(bytes)).await?;
    Ok(())
}

/// riceve e deserializza il prossimo messaggio dal socket
async fn recv_msg<T: DeserializeOwned>(framed: &mut ClientFramed) -> anyhow::Result<T> {
    let frame = framed.next().await.ok_or_else(|| anyhow::anyhow!("Connessione chiusa."))??;
    Ok(decode(&frame)?)
}

impl ClientHandle {
    /// stabilisce la connessione iniziale bloccante per validare le credenziali
    pub async fn connect(server_addr: String, username: String, password: String) -> anyhow::Result<Self> {
        let framed = connect_and_auth(&server_addr, &username, &password).await?;

        let (command_tx, command_rx) = mpsc::channel::<Command>(32);
        let (event_tx, event_rx) = mpsc::channel::<Event>(32);

        tokio::spawn(run_connection(
            framed, command_rx, event_tx, server_addr, username, password
        ));

        Ok(Self { command_tx, event_rx })
    }
}

/// stabilisce il socket tcp ed esegue la sequenza hello/login
async fn connect_and_auth(addr: &str, user: &str, pass: &str) -> anyhow::Result<ClientFramed> {
    let stream = TcpStream::connect(addr).await?;
    let mut framed = Framed::new(stream, frame_codec());

    send_msg(&mut framed, &Hello { protocol_version: PROTOCOL_VERSION, role: Role::Trucker }).await?;
    match recv_msg::<ServerMessage>(&mut framed).await? {
        ServerMessage::Welcome { .. } => {}
        other => return Err(anyhow::anyhow!("Handshake fallito: {:?}.", other)),
    }

    send_msg(&mut framed, &ClientMessage::Login { username: user.into(), password: pass.into() }).await?;
    match recv_msg::<ServerMessage>(&mut framed).await? {
        ServerMessage::LoginOk { .. } => Ok(framed),
        other => Err(anyhow::anyhow!("Login fallito: {:?}.", other)),
    }
}

/// loop asincrono: smista comandi, riceve frame, manda heartbeat e gestisce le disconnessioni
async fn run_connection(
    mut framed: ClientFramed,
    mut command_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::Sender<Event>,
    server_addr: String,
    username: String,
    password: String,
) {
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    let mut backoff = Duration::from_secs(2);

    // notifica iniziale alla UI
    let _ = event_tx.send(Event::Connected).await;
    let _ = event_tx.send(Event::LoggedIn { username: username.clone() }).await;

    loop {
        tokio::select! {
            // elabora i comandi in uscita richiesti dal main
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
                        break; // termina il task
                    }
                    None => break, // canale distrutto dal main
                }
            }
            // elabora i messaggi in ingresso dal server
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
                    // avvia il processo di riconnessione con backoff esponenziale in caso la connessione cadesse
                    _ => {
                        let _ = event_tx.send(Event::Disconnected { reason: "Connessione persa, tentativo di riconnessione...".into() }).await;

                        loop {
                            tokio::time::sleep(backoff).await;
                            match connect_and_auth(&server_addr, &username, &password).await {
                                Ok(new_framed) => {
                                    framed = new_framed;
                                    backoff = Duration::from_secs(2); // resetta il tempo di attesa

                                    // avvisa la UI che si è di nuovo online
                                    let _ = event_tx.send(Event::Connected).await;
                                    let _ = event_tx.send(Event::LoggedIn { username: username.clone() }).await;
                                    break;
                                }
                                Err(_) => {
                                    // raddoppia l'attesa fino a un massimo di 60s
                                    backoff = (backoff * 2).min(Duration::from_secs(60));
                                }
                            }
                        }
                    }
                }
            }
            // mantiene attivo il socket
            _ = heartbeat.tick() => {
                let _ = send_msg(&mut framed, &ClientMessage::Heartbeat).await;
            }
        }
    }
}