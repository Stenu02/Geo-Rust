use geo_protocol::{AdminMessage, AdminReply};
use std::time::Duration;
use tokio::sync::mpsc;
use geo_net::connection::ClientFramed;

/// loop asincrono: smista comandi dalla UI al server e inoltra le risposte dal server alla UI
pub async fn run_admin_connection(
    mut framed: ClientFramed,
    mut command_rx: mpsc::Receiver<AdminMessage>,
    event_tx: mpsc::Sender<AdminReply>,
) {
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));

    // La tabella flotta non arriva da sola: il server manda `Fleet` SOLO in
    // risposta a una `ListFleet`, e non notifica di sua iniziativa chi entra o
    // esce. Chiedendola una volta sola la console mostrerebbe per sempre la
    // fotografia scattata al login: un camionista collegato dopo non
    // comparirebbe mai, e stato e posizione di quelli gia' in tabella
    // resterebbero congelati.
    //
    // Il primo tick di `interval` scatta subito, quindi questo timer copre
    // anche la richiesta iniziale.
    let mut refresh_flotta = tokio::time::interval(Duration::from_secs(2));

    loop {
        tokio::select! {
            // Comandi generati dalla TUI
            cmd = command_rx.recv() => {
                match cmd {
                    Some(msg) => {
                        let _ = geo_net::connection::send_msg(&mut framed, &msg).await;
                        if matches!(msg, AdminMessage::Logout) {
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
            // Ridomanda la flotta: e' cosi' che la tabella resta viva.
            _ = refresh_flotta.tick() => {
                let _ = geo_net::connection::send_msg(&mut framed, &AdminMessage::ListFleet).await;
            }
            // Mantiene vivo il socket (Heartbeat)
            _ = heartbeat.tick() => {
                let _ = geo_net::connection::send_msg(&mut framed, &AdminMessage::Heartbeat).await;
            }
        }
    }
}

