//! Gestione della connessione di rete verso il server per l'amministratore.
//!
//! Si occupa di instradare i messaggi in uscita dalla UI verso il socket TCP e viceversa.

use geo_protocol::{AdminMessage, AdminReply};
use std::time::Duration;
use tokio::sync::mpsc;
use geo_net::connection::ClientFramed;

/// Loop asincrono: smista i comandi dalla UI al server e inoltra le risposte dal server alla UI.
pub async fn run_admin_connection(
    mut framed: ClientFramed,
    mut command_rx: mpsc::Receiver<AdminMessage>,
    event_tx: mpsc::Sender<AdminReply>,
) {
    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));

    // Richiede la flotta iniziale
    if geo_net::connection::send_msg(&mut framed, &AdminMessage::ListFleet).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            // Comandi generati dalla TUI
            cmd = command_rx.recv() => {
                match cmd {
                    Some(msg) => {
                        let is_logout = matches!(msg, AdminMessage::Logout);
                        if geo_net::connection::send_msg(&mut framed, &msg).await.is_err() {
                            break;
                        }
                        if is_logout {
                            break;
                        }
                    }
                    None => break, // UI terminata
                }
            }
            // Messaggi in ingresso dal server
            reply_res = geo_net::connection::recv_msg::<AdminReply>(&mut framed) => {
                match reply_res {
                    Ok(reply) => {
                        let _ = event_tx.send(reply).await;
                    }
                    Err(_) => {
                        // Disconnessione
                        break;
                    }
                }
            }
            // Mantiene vivo il socket (Heartbeat) e aggiorna la flotta
            _ = heartbeat.tick() => {
                if geo_net::connection::send_msg(&mut framed, &AdminMessage::Heartbeat).await.is_err() {
                    break;
                }
                // Richiede la flotta aggiornata per visualizzare nuovi utenti
                if geo_net::connection::send_msg(&mut framed, &AdminMessage::ListFleet).await.is_err() {
                    break;
                }
            }
        }
    }
}

