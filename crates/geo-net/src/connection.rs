//! gestione della connessione TCP verso il server: handshake, invio comandi,
//! ricezione eventi, heartbeat periodico
//! nasconde ogni dettaglio di rete dietro ClientHandle

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use geo_core::Position;
use geo_protocol::{decode, encode, frame_codec, ClientMessage, Hello, Role, ServerMessage, PROTOCOL_VERSION};
use serde::{de::DeserializeOwned, Serialize};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use chrono::Utc;

use super::events::Event;

/// comandi che main.rs può inviare dopo che la connessione è stabilita ed autenticata
#[derive(Debug, Clone)]
pub enum Command {
    PositionUpdate { position: Position },
    Chat { text: String },
    Logout,
}

/// canale per inviare comandi, canale per ricevere eventi
pub struct ClientHandle {
    pub command_tx: mpsc::Sender<Command>,
    pub event_rx: mpsc::Receiver<Event>,
}

type ClientFramed = Framed<TcpStream, LengthDelimitedCodec>;

/// serializza e invia un messaggio sul thread
async fn send_msg<T: Serialize>(framed: &mut ClientFramed, msg: &T) -> anyhow::Result<()> {
    let bytes = encode(msg)?;
    framed.send(Bytes::from(bytes)).await?;
    Ok(())
}

/// riceve e deserializza il prossimo messaggio dal thread
async fn recv_msg<T: DeserializeOwned>(framed: &mut ClientFramed) -> anyhow::Result<T> {
    let frame = framed.next().await.ok_or_else(|| anyhow::anyhow!("Connessione chiusa dal server."))??;
    Ok(decode(&frame)?)
}

impl ClientHandle {
    /// stabilisce la connessione, esegue l'handshake (Hello -> Welcome, Login -> LoginOk)
    /// e avvia il task di rete in background
    pub async fn connect(server_addr: &str, username: String, password: String) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(server_addr).await?;
        let mut framed = Framed::new(stream, frame_codec());

        send_msg(&mut framed, &Hello { protocol_version: PROTOCOL_VERSION, role: Role::Trucker }).await?;
        let welcome: ServerMessage = recv_msg(&mut framed).await?;
        match welcome {
            ServerMessage::Welcome { .. } => {}
            ServerMessage::Error { code, detail } => {
                return Err(anyhow::anyhow!("Handshake rifiutato: {:?} - {}.", code, detail));
            }
            other => return Err(anyhow::anyhow!("Risposta inattesa all'handshake: {:?}.", other)),
        }

        send_msg(&mut framed, &ClientMessage::Login { username, password }).await?;
        let login_reply: ServerMessage = recv_msg(&mut framed).await?;
        match login_reply {
            ServerMessage::LoginOk { .. } => {}
            ServerMessage::Error { code, detail } => {
                return Err(anyhow::anyhow!("Login rifiutato: {:?} - {}.", code, detail));
            }
            other => return Err(anyhow::anyhow!("Risposta inattesa al login: {:?}.", other)),
        }

        let (command_tx, command_rx) = mpsc::channel::<Command>(32);
        let (event_tx, event_rx) = mpsc::channel::<Event>(32);

        tokio::spawn(run_connection(framed, command_rx, event_tx));

        Ok(Self { command_tx, event_rx })
    }
}

/// task in background: smista comandi in uscita, frame in entrata e heartbeat periodico
async fn run_connection(mut framed: ClientFramed, mut command_rx: mpsc::Receiver<Command>, event_tx: mpsc::Sender<Event>) {
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(10));

    loop {
        tokio::select! {
            cmd = command_rx.recv() => {
                match cmd {
                    Some(command) => {
                        match command {
                            Command::PositionUpdate { position } => {
                                let msg = ClientMessage::PositionUpdate { at: Utc::now(), position };
                                let _ = send_msg(&mut framed, &msg).await;
                            }
                            Command::Chat { text } => {
                                let msg = ClientMessage::Chat { text };
                                let _ = send_msg(&mut framed, &msg).await;
                            }
                            Command::Logout => {
                                let _ = send_msg(&mut framed, &ClientMessage::Logout).await;
                                break;
                            }
                        }
                    }
                    None => break, // rilasciato il Sender, nessuno manderà più comandi
                }
            }
            frame = framed.next() => {
                match frame {
                    Some(Ok(bytes)) => {
                        if let Ok(msg) = decode::<ServerMessage>(&bytes) {
                            match msg {
                                ServerMessage::PositionAck { state } => {
                                    let event = Event::PositionAck { state };
                                    let _ = event_tx.send(event).await;
                                }
                                ServerMessage::Chat { text, broadcast, at } => {
                                    let event = Event::ChatReceived { text, broadcast, at: Utc::now() };
                                    let _ = event_tx.send(event).await;
                                }
                                ServerMessage::Error { code, detail } => {
                                    let event = Event::ServerError { code, detail };
                                    let _ = event_tx.send(event).await;
                                }
                                _ => {}
                            }
                        }

                    }
                    _ => {
                        let _ = event_tx.send(Event::Disconnected { reason: "Connessione persa".into() }).await;
                        break;
                    }
                }
            }
            _ = heartbeat.tick() => {
                let _ = send_msg(&mut framed, &ClientMessage::Heartbeat).await;
            }
        }
    }
}