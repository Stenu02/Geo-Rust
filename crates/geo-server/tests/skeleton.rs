//! Test di integrazione dello skeleton server: si avvia il server su una porta
//! effimera e gli si parla con un client vero, dentro `cargo test`.
//!
//! Questi test sono anche il contratto verso chi scrive il client e la console:
//! documentano esattamente quale sequenza di messaggi il server si aspetta.

use futures::{SinkExt, StreamExt};
use geo_core::{RangeSpec, UserId, UserState};
use geo_protocol::{
    decode, encode, frame_codec, AdminMessage, AdminReply, ClientMessage, ErrorCode, Hello, Role,
    ServerMessage, PROTOCOL_VERSION,
};
use geo_server::{Server, ServerConfig};
use serde::Serialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio_util::sync::CancellationToken;

type Wire = Framed<TcpStream, LengthDelimitedCodec>;

fn config() -> ServerConfig {
    ServerConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        db_path: PathBuf::from("test.db"),
        log_path: PathBuf::from("test.csv"),
        time_scale: 1,
    }
}

/// Avvia un server su porta effimera e restituisce indirizzo e segnale di stop.
async fn avvia() -> (SocketAddr, CancellationToken) {
    let server = Server::bind(config()).await.expect("bind");
    let addr = server.local_addr().expect("local_addr");
    let shutdown = server.shutdown_handle();
    tokio::spawn(server.run());
    (addr, shutdown)
}

async fn invia<T: Serialize>(wire: &mut Wire, msg: &T) {
    wire.send(encode(msg).unwrap().into()).await.unwrap();
}

async fn ricevi<T: serde::de::DeserializeOwned>(wire: &mut Wire) -> T {
    let frame = wire.next().await.expect("nessuna risposta").unwrap();
    decode(&frame[..]).unwrap()
}

/// Si collega e completa l'handshake nel ruolo richiesto.
async fn collega(addr: SocketAddr, role: Role) -> Wire {
    let stream = TcpStream::connect(addr).await.expect("connect");
    let mut wire = Framed::new(stream, frame_codec());
    invia(
        &mut wire,
        &Hello {
            protocol_version: PROTOCOL_VERSION,
            role,
        },
    )
    .await;
    // Consuma il Welcome.
    let _ = wire.next().await.expect("nessun welcome").unwrap();
    wire
}

// --- handshake -------------------------------------------------------------

#[tokio::test]
async fn porta_effimera_assegnata_dal_sistema() {
    let (addr, shutdown) = avvia().await;
    assert_ne!(addr.port(), 0, "il sistema deve assegnare una porta vera");
    shutdown.cancel();
}

#[tokio::test]
async fn camionista_riceve_welcome() {
    let (addr, shutdown) = avvia().await;
    let stream = TcpStream::connect(addr).await.unwrap();
    let mut wire = Framed::new(stream, frame_codec());

    invia(
        &mut wire,
        &Hello {
            protocol_version: PROTOCOL_VERSION,
            role: Role::Trucker,
        },
    )
    .await;

    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::Welcome { protocol_version } => {
            assert_eq!(protocol_version, PROTOCOL_VERSION)
        }
        altro => panic!("atteso Welcome, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn amministratore_riceve_welcome() {
    let (addr, shutdown) = avvia().await;
    let stream = TcpStream::connect(addr).await.unwrap();
    let mut wire = Framed::new(stream, frame_codec());

    invia(
        &mut wire,
        &Hello {
            protocol_version: PROTOCOL_VERSION,
            role: Role::Admin,
        },
    )
    .await;

    match ricevi::<AdminReply>(&mut wire).await {
        AdminReply::Welcome { protocol_version } => {
            assert_eq!(protocol_version, PROTOCOL_VERSION)
        }
        altro => panic!("atteso Welcome, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn versione_incompatibile_rifiutata_e_connessione_chiusa() {
    let (addr, shutdown) = avvia().await;
    let stream = TcpStream::connect(addr).await.unwrap();
    let mut wire = Framed::new(stream, frame_codec());

    invia(
        &mut wire,
        &Hello {
            protocol_version: PROTOCOL_VERSION + 1,
            role: Role::Trucker,
        },
    )
    .await;

    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::Error { code, .. } => assert_eq!(code, ErrorCode::ProtocolVersionMismatch),
        altro => panic!("atteso Error, ricevuto {altro:?}"),
    }
    assert!(
        wire.next().await.is_none(),
        "il server deve chiudere dopo una versione incompatibile"
    );
    shutdown.cancel();
}

// --- dialogo con il camionista ---------------------------------------------

#[tokio::test]
async fn registrazione_e_login_rispondono() {
    let (addr, shutdown) = avvia().await;
    let mut wire = collega(addr, Role::Trucker).await;

    invia(
        &mut wire,
        &ClientMessage::Register {
            username: "camion01".into(),
            password: "segreto".into(),
        },
    )
    .await;
    assert!(matches!(
        ricevi::<ServerMessage>(&mut wire).await,
        ServerMessage::RegisterOk { .. }
    ));

    invia(
        &mut wire,
        &ClientMessage::Login {
            username: "camion01".into(),
            password: "segreto".into(),
        },
    )
    .await;
    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::LoginOk { username, .. } => assert_eq!(username, "camion01"),
        altro => panic!("atteso LoginOk, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn posizione_riceve_ack_con_stato() {
    let (addr, shutdown) = avvia().await;
    let mut wire = collega(addr, Role::Trucker).await;

    invia(
        &mut wire,
        &ClientMessage::PositionUpdate {
            at: chrono::Utc::now(),
            position: geo_core::Position::new(45.0618513, 7.6797161),
        },
    )
    .await;

    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::PositionAck { state } => assert_eq!(state, UserState::Stopped),
        altro => panic!("atteso PositionAck, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn heartbeat_riceve_pong() {
    let (addr, shutdown) = avvia().await;
    let mut wire = collega(addr, Role::Trucker).await;

    invia(&mut wire, &ClientMessage::Heartbeat).await;
    assert!(matches!(
        ricevi::<ServerMessage>(&mut wire).await,
        ServerMessage::Pong
    ));
    shutdown.cancel();
}

#[tokio::test]
async fn logout_chiude_la_sessione() {
    let (addr, shutdown) = avvia().await;
    let mut wire = collega(addr, Role::Trucker).await;

    invia(&mut wire, &ClientMessage::Logout).await;
    assert!(
        wire.next().await.is_none(),
        "dopo il logout il server deve chiudere"
    );
    shutdown.cancel();
}

// --- dialogo con l'amministratore ------------------------------------------

#[tokio::test]
async fn lista_flotta_non_vuota() {
    let (addr, shutdown) = avvia().await;
    let mut wire = collega(addr, Role::Admin).await;

    invia(&mut wire, &AdminMessage::ListFleet).await;
    match ricevi::<AdminReply>(&mut wire).await {
        AdminReply::Fleet { entries } => assert!(!entries.is_empty()),
        altro => panic!("attesa Fleet, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn interrogazione_restituisce_un_report() {
    let (addr, shutdown) = avvia().await;
    let mut wire = collega(addr, Role::Admin).await;

    invia(
        &mut wire,
        &AdminMessage::Query {
            user: UserId(1),
            spec: RangeSpec::Today,
        },
    )
    .await;

    match ricevi::<AdminReply>(&mut wire).await {
        AdminReply::QueryResult(report) => {
            // Il report torna sull'utente e sul periodo richiesti: e' l'unica
            // cosa che si puo' verificare finche' i numeri sono inventati.
            assert_eq!(report.user, UserId(1));
            assert_eq!(report.spec, RangeSpec::Today);
        }
        altro => panic!("atteso QueryResult, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

// --- robustezza ------------------------------------------------------------

#[tokio::test]
async fn client_muto_non_blocca_gli_altri() {
    let (addr, shutdown) = avvia().await;

    // Un client che si collega e non dice niente non deve impedire agli altri
    // di essere serviti.
    let _muto = TcpStream::connect(addr).await.expect("connect");

    let mut wire = collega(addr, Role::Trucker).await;
    invia(&mut wire, &ClientMessage::Heartbeat).await;
    assert!(matches!(
        ricevi::<ServerMessage>(&mut wire).await,
        ServerMessage::Pong
    ));
    shutdown.cancel();
}

#[tokio::test]
async fn piu_camionisti_in_parallelo() {
    let (addr, shutdown) = avvia().await;

    let mut connessioni = Vec::new();
    for _ in 0..5 {
        connessioni.push(collega(addr, Role::Trucker).await);
    }
    for wire in connessioni.iter_mut() {
        invia(wire, &ClientMessage::Heartbeat).await;
    }
    for wire in connessioni.iter_mut() {
        assert!(matches!(
            ricevi::<ServerMessage>(wire).await,
            ServerMessage::Pong
        ));
    }
    shutdown.cancel();
}
