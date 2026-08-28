//! Manda un annuncio a tutta la flotta, da riga di comando.
//!
//! Serve a provare il broadcast finche' la console amministratore non offre
//! un modo di selezionare "a tutti".
//!
//!   cargo run -p geo-server --example manda_broadcast -- \
//!       127.0.0.1:7878 capoflotta segreto-admin "il testo del messaggio"

use futures::{SinkExt, StreamExt};
use geo_protocol::{
    decode, encode, frame_codec, AdminMessage, AdminReply, Hello, Role, PROTOCOL_VERSION,
};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 4 {
        eprintln!("uso: manda_broadcast <server> <utente> <password> <testo>");
        std::process::exit(1);
    }
    let (server, utente, password, testo) = (&a[0], &a[1], &a[2], &a[3]);

    let stream = TcpStream::connect(server).await?;
    let mut wire = Framed::new(stream, frame_codec());

    // 1. handshake come amministratore
    wire.send(
        encode(&Hello {
            protocol_version: PROTOCOL_VERSION,
            role: Role::Admin,
        })?
        .into(),
    )
    .await?;
    let _ = wire.next().await;

    // 2. login: da qui in poi il server verifica password E ruolo
    wire.send(
        encode(&AdminMessage::Login {
            username: utente.clone(),
            password: password.clone(),
        })?
        .into(),
    )
    .await?;
    match decode::<AdminReply>(&wire.next().await.unwrap()?[..])? {
        AdminReply::LoginOk { user, .. } => println!("login riuscito come {user}"),
        altro => {
            eprintln!("login rifiutato: {altro:?}");
            std::process::exit(1);
        }
    }

    // 3. l'annuncio a tutti i collegati
    wire.send(
        encode(&AdminMessage::SendBroadcast {
            text: testo.clone(),
        })?
        .into(),
    )
    .await?;
    println!("annuncio inviato: \"{testo}\"");

    // Il server non manda conferma per un annuncio: si lascia il tempo di
    // recapitarlo prima di chiudere la connessione.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    Ok(())
}
