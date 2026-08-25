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
        // DB in memoria, privato per ogni server: i test girano in parallelo e un
        // file condiviso li farebbe bloccare a vicenda sul lock SQLite.
        db_path: PathBuf::from(":memory:"),
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

/// Registra un camionista e fa login: molte prove partono da qui.
async fn autenticato(addr: SocketAddr, nome: &str) -> Wire {
    let mut wire = collega(addr, Role::Trucker).await;
    invia(
        &mut wire,
        &ClientMessage::Register {
            username: nome.into(),
            password: "segreto".into(),
        },
    )
    .await;
    let _: ServerMessage = ricevi(&mut wire).await;
    invia(
        &mut wire,
        &ClientMessage::Login {
            username: nome.into(),
            password: "segreto".into(),
        },
    )
    .await;
    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::LoginOk { .. } => wire,
        altro => panic!("login fallito: {altro:?}"),
    }
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
    let mut wire = autenticato(addr, "camion01").await;

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

// --- autenticazione ------------------------------------------------------

#[tokio::test]
async fn posizione_prima_del_login_rifiutata() {
    let (addr, shutdown) = avvia().await;
    let mut wire = collega(addr, Role::Trucker).await;

    invia(
        &mut wire,
        &ClientMessage::PositionUpdate {
            at: chrono::Utc::now(),
            position: geo_core::Position::new(45.0, 7.0),
        },
    )
    .await;

    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::Error { code, .. } => assert_eq!(code, ErrorCode::NotAuthenticated),
        altro => panic!("atteso NotAuthenticated, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn registrazione_duplicata_rifiutata() {
    let (addr, shutdown) = avvia().await;
    let mut wire = autenticato(addr, "camion01").await;

    // Stesso nome, seconda volta.
    invia(
        &mut wire,
        &ClientMessage::Register {
            username: "camion01".into(),
            password: "altra".into(),
        },
    )
    .await;

    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::Error { code, .. } => assert_eq!(code, ErrorCode::UsernameTaken),
        altro => panic!("atteso UsernameTaken, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn password_sbagliata_rifiutata() {
    let (addr, shutdown) = avvia().await;
    let mut wire = collega(addr, Role::Trucker).await;

    invia(
        &mut wire,
        &ClientMessage::Register {
            username: "camion01".into(),
            password: "giusta".into(),
        },
    )
    .await;
    let _: ServerMessage = ricevi(&mut wire).await;

    invia(
        &mut wire,
        &ClientMessage::Login {
            username: "camion01".into(),
            password: "sbagliata".into(),
        },
    )
    .await;

    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::Error { code, .. } => assert_eq!(code, ErrorCode::InvalidCredentials),
        altro => panic!("atteso InvalidCredentials, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn utente_inesistente_stesso_errore_di_password_errata() {
    let (addr, shutdown) = avvia().await;
    let mut wire = collega(addr, Role::Trucker).await;

    invia(
        &mut wire,
        &ClientMessage::Login {
            username: "mai-registrato".into(),
            password: "qualunque".into(),
        },
    )
    .await;

    // Deve essere indistinguibile da una password sbagliata: non si rivela
    // quali username esistono.
    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::Error { code, detail } => {
            assert_eq!(code, ErrorCode::InvalidCredentials);
            assert_eq!(detail, "credenziali non valide");
        }
        altro => panic!("atteso InvalidCredentials, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn secondo_login_sulla_stessa_sessione_rifiutato() {
    let (addr, shutdown) = avvia().await;
    let mut wire = autenticato(addr, "camion01").await;

    invia(
        &mut wire,
        &ClientMessage::Login {
            username: "camion01".into(),
            password: "segreto".into(),
        },
    )
    .await;

    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::Error { code, .. } => assert_eq!(code, ErrorCode::NotAuthorized),
        altro => panic!("atteso NotAuthorized, ricevuto {altro:?}"),
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
async fn la_flotta_elenca_i_camionisti_collegati() {
    let (addr, shutdown) = avvia().await;

    // Due camionisti si collegano e fanno login.
    let _c1 = autenticato(addr, "camion01").await;
    let _c2 = autenticato(addr, "camion02").await;

    let mut admin = collega(addr, Role::Admin).await;
    invia(&mut admin, &AdminMessage::ListFleet).await;

    match ricevi::<AdminReply>(&mut admin).await {
        AdminReply::Fleet { entries } => {
            assert_eq!(entries.len(), 2, "attesi due camionisti collegati");
            assert_eq!(entries[0].username, "camion01");
            assert_eq!(entries[1].username, "camion02");
            // Appena collegati sono fermi e senza posizione nota.
            assert_eq!(entries[0].state, UserState::Stopped);
            assert_eq!(entries[0].last_position, None);
        }
        altro => panic!("attesa Fleet, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn la_flotta_e_vuota_senza_camionisti() {
    let (addr, shutdown) = avvia().await;
    let mut admin = collega(addr, Role::Admin).await;
    invia(&mut admin, &AdminMessage::ListFleet).await;

    match ricevi::<AdminReply>(&mut admin).await {
        AdminReply::Fleet { entries } => assert!(entries.is_empty()),
        altro => panic!("attesa Fleet, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn chi_si_scollega_sparisce_dalla_flotta() {
    let (addr, shutdown) = avvia().await;

    let c1 = autenticato(addr, "camion01").await;
    let mut admin = collega(addr, Role::Admin).await;

    invia(&mut admin, &AdminMessage::ListFleet).await;
    match ricevi::<AdminReply>(&mut admin).await {
        AdminReply::Fleet { entries } => assert_eq!(entries.len(), 1),
        altro => panic!("attesa Fleet, ricevuto {altro:?}"),
    }

    // Il camionista chiude la connessione di colpo.
    drop(c1);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    invia(&mut admin, &AdminMessage::ListFleet).await;
    match ricevi::<AdminReply>(&mut admin).await {
        AdminReply::Fleet { entries } => assert!(
            entries.is_empty(),
            "chi si scollega non deve restare nella tabella"
        ),
        altro => panic!("attesa Fleet, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn la_posizione_inviata_compare_nella_flotta() {
    let (addr, shutdown) = avvia().await;
    let mut c1 = autenticato(addr, "camion01").await;

    let ora = chrono::Utc::now();
    invia(
        &mut c1,
        &ClientMessage::PositionUpdate {
            at: ora,
            position: geo_core::Position::new(45.0618513, 7.6797161),
        },
    )
    .await;
    let _: ServerMessage = ricevi(&mut c1).await;

    let mut admin = collega(addr, Role::Admin).await;
    invia(&mut admin, &AdminMessage::ListFleet).await;

    match ricevi::<AdminReply>(&mut admin).await {
        AdminReply::Fleet { entries } => {
            let p = entries[0].last_position.expect("posizione mancante");
            assert!((p.lat - 45.0618513).abs() < 1e-9);
            assert_eq!(entries[0].last_update, Some(ora));
        }
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

// --- posizioni, stati, persistenza ----------------------------------------

#[tokio::test]
async fn il_cambio_di_coordinata_porta_a_movimento() {
    let (addr, shutdown) = avvia().await;
    let mut wire = autenticato(addr, "camion01").await;
    let t0 = chrono::Utc::now();

    // La prima posizione non e' un "cambio": si resta fermi.
    invia(
        &mut wire,
        &ClientMessage::PositionUpdate {
            at: t0,
            position: geo_core::Position::new(45.0618513, 7.6797161),
        },
    )
    .await;
    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::PositionAck { state } => assert_eq!(state, UserState::Stopped),
        altro => panic!("atteso PositionAck, ricevuto {altro:?}"),
    }

    // La seconda, diversa, fa scattare il movimento.
    invia(
        &mut wire,
        &ClientMessage::PositionUpdate {
            at: t0 + chrono::Duration::seconds(30),
            position: geo_core::Position::new(45.0601242, 7.6851330),
        },
    )
    .await;
    match ricevi::<ServerMessage>(&mut wire).await {
        ServerMessage::PositionAck { state } => assert_eq!(state, UserState::Moving),
        altro => panic!("atteso PositionAck, ricevuto {altro:?}"),
    }

    shutdown.cancel();
}

#[tokio::test]
async fn coordinate_invalide_rifiutate() {
    let (addr, shutdown) = avvia().await;
    let mut wire = autenticato(addr, "camion01").await;

    // NOTA: il caso (NaN, 7.0) manca di proposito. `Position::is_valid` lo
    // accetta — con NaN ogni confronto e' falso, quindi il controllo di
    // intervallo cade nel ramo "valido" — e la posizione arriva fino a SQLite,
    // che la rifiuta facendo cadere la sessione. Va corretto in geo-core.
    for (lat, lon) in [(91.0, 7.0), (45.0, 181.0), (-91.0, 7.0), (45.0, -181.0)] {
        invia(
            &mut wire,
            &ClientMessage::PositionUpdate {
                at: chrono::Utc::now(),
                position: geo_core::Position::new(lat, lon),
            },
        )
        .await;
        match ricevi::<ServerMessage>(&mut wire).await {
            ServerMessage::Error { code, .. } => assert_eq!(code, ErrorCode::InvalidPosition),
            altro => panic!("attesa ({lat}, {lon}) rifiutata, ricevuto {altro:?}"),
        }
    }
    shutdown.cancel();
}

#[tokio::test]
async fn le_posizioni_finiscono_nel_report_dell_amministratore() {
    let (addr, shutdown) = avvia().await;
    let mut camion = autenticato(addr, "camion01").await;

    // Tre posizioni lungo la rotta Torino -> Asti.
    let t0 = chrono::Utc::now();
    for (i, (lat, lon)) in [
        (45.0618513, 7.6797161),
        (45.0601242, 7.6851330),
        (45.0577891, 7.6923447),
    ]
    .iter()
    .enumerate()
    {
        invia(
            &mut camion,
            &ClientMessage::PositionUpdate {
                at: t0 + chrono::Duration::seconds(30 * i as i64),
                position: geo_core::Position::new(*lat, *lon),
            },
        )
        .await;
        let _: ServerMessage = ricevi(&mut camion).await;
    }

    // L'amministratore interroga: il tragitto non deve essere vuoto.
    let mut admin = collega(addr, Role::Admin).await;
    invia(
        &mut admin,
        &AdminMessage::Query {
            user: UserId(1),
            spec: RangeSpec::Today,
        },
    )
    .await;

    match ricevi::<AdminReply>(&mut admin).await {
        AdminReply::QueryResult(report) => {
            assert_eq!(
                report.track.len(),
                3,
                "le tre posizioni devono essere salvate"
            );
            assert!(
                report.distance_km > 0.0,
                "una polilinea di tre punti distinti ha distanza positiva"
            );
        }
        altro => panic!("atteso QueryResult, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}

#[tokio::test]
async fn il_login_apre_un_intervallo_di_stato() {
    let (addr, shutdown) = avvia().await;
    let _camion = autenticato(addr, "camion01").await;

    let mut admin = collega(addr, Role::Admin).await;
    invia(
        &mut admin,
        &AdminMessage::Query {
            user: UserId(1),
            spec: RangeSpec::Today,
        },
    )
    .await;

    match ricevi::<AdminReply>(&mut admin).await {
        AdminReply::QueryResult(report) => {
            // Il login ha aperto un intervallo FERMO ancora in corso: la durata
            // delle pause e' quindi maggiore di zero.
            assert!(
                report.paused_time > chrono::Duration::zero(),
                "il login deve aver aperto un intervallo FERMO"
            );
        }
        altro => panic!("atteso QueryResult, ricevuto {altro:?}"),
    }
    shutdown.cancel();
}
