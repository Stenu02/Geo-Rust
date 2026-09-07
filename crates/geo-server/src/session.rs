//! La conversazione con un singolo collegato.
//!
//! `handle` fa la presentazione iniziale, poi passa a `trucker_loop` o
//! `admin_loop` secondo il ruolo dichiarato. I due cicli leggono un messaggio
//! alla volta e rispondono, appoggiandosi a `storage` per il database e a
//! `registry` per sapere chi e' collegato.

use crate::auth;
use crate::registry::Registry;
use crate::storage::{StorageHandle, UsernameTaken};
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use geo_core::{TimeRange, UserId, UserState};
use geo_protocol::{
    decode, encode, frame_codec, AdminMessage, AdminReply, ClientMessage, ErrorCode, Hello, Role,
    ServerMessage, PROTOCOL_VERSION,
};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio_util::sync::CancellationToken;

type Wire = Framed<TcpStream, LengthDelimitedCodec>;

/// Quanto si aspetta il primo messaggio prima di chiudere la connessione.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Segue una connessione dalla presentazione fino alla chiusura.
pub async fn handle(
    stream: TcpStream,
    shutdown: CancellationToken,
    storage: StorageHandle,
    registry: Registry,
) -> anyhow::Result<()> {
    let peer = stream.peer_addr()?;
    let mut wire = Framed::new(stream, frame_codec());

    // Presentazione: se non arriva o non e' valida si chiude qui.
    let Some(hello) = handshake(&mut wire).await? else {
        return Ok(());
    };

    tracing::info!(%peer, role = ?hello.role, "handshake completato");

    // Chi si e' autenticato su questa connessione. Sta qui e non nel ciclo
    // perche' serve anche dopo, per la chiusura ordinata.
    let mut sessione: Option<(UserId, String)> = None;

    // Il ruolo decide come vanno letti tutti i messaggi successivi: gli stessi
    // byte significano cose diverse per un camionista e per un coordinatore.
    let esito = match hello.role {
        Role::Trucker => {
            trucker_loop(&mut wire, &shutdown, &storage, &registry, &mut sessione).await
        }
        Role::Admin => admin_loop(&mut wire, &shutdown, &storage, &registry).await,
    };

    if let Some((user, username)) = sessione {
        let ora = Utc::now();
        if let Some(t) = registry.on_disconnect(user, ora) {
            if let Err(e) = storage.apply_transition(user, t.to, t.at).await {
                tracing::error!(%user, error = %e, "chiusura intervallo fallita");
            }
        }
        registry.unregister(user);
        tracing::info!(%peer, %username, "camionista scollegato");
    }

    tracing::info!(%peer, "sessione chiusa");
    esito
}

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

    // Versioni diverse leggerebbero gli stessi byte in modo diverso.
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

    // Il benvenuto va scritto nella forma che il destinatario sa leggere.
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

/// Impronta finta contro cui verificare quando l'utente non esiste.
///
/// Serve a far durare uguale il rifiuto, cosi' il tempo di risposta non
/// distingue un nome sbagliato da una password sbagliata.
const PHC_FITTIZIO: &str = "$argon2id$v=19$m=19456,t=2,p=1$mPPAx9/sJgdFwPpOmZkZzQ$RW7w1/7mKTOGp6CFI7okMaRciTuid7mzwPmOJuhtQRI";

/// Dialogo con un camionista, fino alla disconnessione o alla chiusura.
async fn trucker_loop(
    wire: &mut Wire,
    shutdown: &CancellationToken,
    storage: &StorageHandle,
    registry: &Registry,
    autenticato: &mut Option<(UserId, String)>,
) -> anyhow::Result<()> {
    // Le due vie da cui il server puo' scrivere a questo camionista: la sua
    // casella personale e il canale degli annunci. Restano vuote finche' non
    // si autentica, perche' prima nessuno sa a chi scrivere.
    let mut posta: Option<mpsc::Receiver<ServerMessage>> = None;
    let mut annunci: Option<broadcast::Receiver<ServerMessage>> = None;

    loop {
        // Quattro cose possono succedere e si aspettano tutte insieme: il task
        // dorme finche' non ne accade una.
        //
        // Quando una casella non esiste ancora si aspetta su `pending()`, che
        // non finisce mai: quel ramo non puo' vincere finche' non c'e' login.
        let frame = tokio::select! {
            _ = shutdown.cancelled() => break,

            // Messaggio diretto a questo camionista.
            msg = async {
                match posta.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match msg {
                    Some(m) => send(wire, &m).await?,
                    // Casella chiusa: il registro ha sostituito questa voce
                    // con un altro accesso dello stesso utente.
                    None => posta = None,
                }
                continue;
            }

            // Annuncio diretto a tutta la flotta.
            msg = async {
                match annunci.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match msg {
                    Ok(m) => send(wire, &m).await?,
                    // Sessione troppo lenta: qualche annuncio e' andato perso.
                    // Vale adesso, non ha senso recuperarlo dopo.
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(persi = n, "annunci persi per lentezza");
                    }
                    Err(broadcast::error::RecvError::Closed) => annunci = None,
                }
                continue;
            }

            // Messaggio in arrivo dal client.
            frame = wire.next() => match frame {
                // Niente da leggere: il client ha chiuso.
                None => break,
                Some(frame) => frame?,
            },
        };

        match decode::<ClientMessage>(&frame[..])? {
            // Creazione di un nuovo account.
            ClientMessage::Register { username, password } => {
                // Prima del calcolo dell'impronta, non dopo: costa una
                // trentina di millisecondi che una registrazione da rifiutare
                // non deve far pagare al server.
                if let Err(motivo) = auth::valida_credenziali(&username, &password) {
                    tracing::warn!(%username, %motivo, "registrazione rifiutata");
                    send(
                        wire,
                        &ServerMessage::Error {
                            code: ErrorCode::InvalidCredentials,
                            detail: motivo,
                        },
                    )
                    .await?;
                    continue;
                }

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
            // Accesso di un utente gia' registrato.
            ClientMessage::Login { username, password } => {
                // Un secondo accesso sulla stessa connessione non ha senso.
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

                // Qui il ruolo non serve: basta esistere e sapere la password.
                let (utente, phc) = match storage.find_user(&username).await? {
                    Some((u, p, _ruolo)) => (Some(u), p),
                    None => (None, PHC_FITTIZIO.to_string()),
                };
                let corretta = auth::verify_password_async(password, phc).await;

                match (utente, corretta) {
                    (Some(user), true) => {
                        tracing::info!(%username, %user, "login effettuato");
                        // Da qui la sessione puo' anche ricevere: il registro
                        // consegna le due estremita' di lettura, e i primi due
                        // rami dell'attesa smettono di essere inerti.
                        let (rx_posta, rx_annunci) = registry.register(user, username.clone());
                        posta = Some(rx_posta);
                        annunci = Some(rx_annunci);

                        // Da sconnesso a fermo. Va scritta subito: apre il
                        // primo intervallo della sessione, e senza le analisi
                        // avrebbero un buco.
                        let ora = Utc::now();
                        if let Some(t) = registry.on_login(user, ora) {
                            storage.apply_transition(user, t.to, t.at).await?;
                        }

                        *autenticato = Some((user, username.clone()));
                        send(wire, &ServerMessage::LoginOk { user, username }).await?;

                        // Arretrati: i messaggi ricevuti mentre non era
                        // collegato arrivano adesso, in ordine di invio. La
                        // lettura li segna anche come consegnati, quindi al
                        // collegamento successivo non ricompaiono.
                        for (text, at) in storage.take_pending(user).await? {
                            send(
                                wire,
                                &ServerMessage::Chat {
                                    text,
                                    broadcast: false,
                                    at,
                                },
                            )
                            .await?;
                        }
                    }
                    // Stesso messaggio in entrambi i casi: non si rivela se sia
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
                let Some(user) = autenticato.as_ref().map(|(u, _)| *u) else {
                    send(wire, &non_autenticato()).await?;
                    continue;
                };

                // Una coordinata assurda non deve entrare nello storico: da li'
                // in poi falserebbe distanze e velocita'.
                if !position.is_valid() {
                    send(
                        wire,
                        &ServerMessage::Error {
                            code: ErrorCode::InvalidPosition,
                            detail: "coordinate fuori intervallo".into(),
                        },
                    )
                    .await?;
                    continue;
                }

                tracing::debug!(%user, %at, lat = position.lat, lon = position.lon, "posizione");
                storage.save_position(user, at, position).await?;

                // La macchina a stati decide se questa posizione e' un cambio,
                // e nel caso restituisce la transizione da salvare.
                if let Some(t) = registry.on_position(user, at, position) {
                    storage.apply_transition(user, t.to, t.at).await?;
                }

                let state = registry.state_of(user).unwrap_or(UserState::Stopped);
                send(wire, &ServerMessage::PositionAck { state }).await?;
            }

            ClientMessage::Chat { text } => {
                let Some((user, username)) = autenticato.as_ref() else {
                    send(wire, &non_autenticato()).await?;
                    continue;
                };
                // Va a chi sta guardando la console, gia' nella forma che
                // questa sa leggere e con dentro il nome di chi scrive.
                let ora = Utc::now();
                let letto_da = registry.verso_admin(AdminReply::Chat {
                    from: *user,
                    username: username.clone(),
                    text: text.clone(),
                    at: ora,
                });
                // Salvato comunque: lo storico non dipende da chi stava
                // guardando in quel momento.
                storage
                    .save_message(Some(*user), None, &text, ora, letto_da > 0)
                    .await?;
                tracing::info!(%user, %text, letto_da, "messaggio dal camionista");
            }

            ClientMessage::Heartbeat => send(wire, &ServerMessage::Pong).await?,
            ClientMessage::Logout => break,
        }
    }

    Ok(())
}

/// Dialogo con il coordinatore, fino alla chiusura.
async fn admin_loop(
    wire: &mut Wire,
    shutdown: &CancellationToken,
    storage: &StorageHandle,
    registry: &Registry,
) -> anyhow::Result<()> {
    // Si iscrive a cio' che i camionisti scrivono.
    let mut dai_camionisti = registry.subscribe_admin();

    // Resta vuoto finche' non arriva un accesso valido: dichiararsi
    // coordinatore nella presentazione non basta, va dimostrato.
    let mut autenticato: Option<UserId> = None;

    loop {
        let frame = tokio::select! {
            _ = shutdown.cancelled() => break,

            // Un camionista ha scritto: il messaggio va mostrato in console.
            msg = dai_camionisti.recv() => {
                match msg {
                    Ok(m) => send(wire, &m).await?,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(persi = n, "messaggi persi per lentezza");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
                continue;
            }

            frame = wire.next() => match frame {
                None => break,
                Some(frame) => frame?,
            },
        };

        match decode::<AdminMessage>(&frame[..])? {
            AdminMessage::Login { username, password } => {
                if autenticato.is_some() {
                    send(
                        wire,
                        &AdminReply::Error {
                            code: ErrorCode::NotAuthorized,
                            detail: "sessione gia' autenticata".into(),
                        },
                    )
                    .await?;
                    continue;
                }

                // Si verifica una password anche quando l'utente non esiste,
                // cosi' il tempo di risposta non rivela chi e' registrato.
                let (utente, phc, ruolo) = match storage.find_user(&username).await? {
                    Some((u, p, r)) => (Some(u), p, r),
                    None => (None, PHC_FITTIZIO.to_string(), String::new()),
                };
                let corretta = auth::verify_password_async(password, phc).await;

                // Tre condizioni e non due: l'utente esiste, la password e'
                // giusta, e il ruolo e' quello di coordinatore. Senza l'ultima,
                // un camionista che si dichiarasse tale entrerebbe con le
                // proprie credenziali e vedrebbe l'intera flotta.
                match (utente, corretta, ruolo.as_str()) {
                    (Some(user), true, "admin") => {
                        tracing::info!(%username, %user, "login amministratore");
                        autenticato = Some(user);
                        send(wire, &AdminReply::LoginOk { user, username }).await?;
                    }
                    // Un solo messaggio per tutti e tre i rifiuti: non si
                    // rivela quale delle tre condizioni sia mancata.
                    _ => {
                        tracing::warn!(%username, %ruolo, "login amministratore rifiutato");
                        send(
                            wire,
                            &AdminReply::Error {
                                code: ErrorCode::InvalidCredentials,
                                detail: "credenziali non valide".into(),
                            },
                        )
                        .await?;
                    }
                }
            }
            AdminMessage::ListFleet => {
                if autenticato.is_none() {
                    send(wire, &admin_non_autenticato()).await?;
                    continue;
                }
                send(
                    wire,
                    &AdminReply::Fleet {
                        entries: registry.fleet(),
                    },
                )
                .await?;
            }
            // Risolve il periodo, legge intervalli e posizioni dal database,
            // costruisce il resoconto e lo rimanda indietro.
            AdminMessage::Query { user, spec } => {
                if autenticato.is_none() {
                    send(wire, &admin_non_autenticato()).await?;
                    continue;
                }
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
                if autenticato.is_none() {
                    send(wire, &admin_non_autenticato()).await?;
                    continue;
                }
                let ora = Utc::now();
                // Falso se il camionista non e' collegato o se la sua casella
                // e' piena: non e' un errore, e' cio' che serve per decidere se
                // metterlo in coda.
                let consegnato = registry.send_to(
                    to,
                    ServerMessage::Chat {
                        text: text.clone(),
                        broadcast: false,
                        at: ora,
                    },
                );
                // Se non e' stato consegnato resta in coda e il destinatario lo
                // ricevera' al collegamento successivo.
                storage
                    .save_message(None, Some(to), &text, ora, consegnato)
                    .await?;
                tracing::info!(%to, %text, consegnato, "messaggio diretto");
            }
            AdminMessage::SendBroadcast { text } => {
                if autenticato.is_none() {
                    send(wire, &admin_non_autenticato()).await?;
                    continue;
                }
                let ora = Utc::now();
                // Un annuncio va solo a chi e' collegato adesso. Zero
                // destinatari non e' un errore, significa flotta vuota.
                let destinatari = registry.annuncia(ServerMessage::Chat {
                    text: text.clone(),
                    broadcast: true,
                    at: ora,
                });
                // Si salva come gia' consegnato: chi era assente non lo deve
                // recuperare dopo.
                storage.save_message(None, None, &text, ora, true).await?;
                tracing::info!(%text, destinatari, "annuncio a tutta la flotta");
            }
            AdminMessage::Heartbeat => send(wire, &AdminReply::Pong).await?,
            AdminMessage::Logout => break,
        }
    }

    Ok(())
}

/// Manda un errore nella forma che il destinatario sa leggere.
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

/// Serializza un messaggio e lo scrive preceduto dalla sua lunghezza.
async fn send<T: serde::Serialize>(wire: &mut Wire, msg: &T) -> anyhow::Result<()> {
    wire.send(encode(msg)?.into()).await?;
    Ok(())
}

/// Risposta a chi manda messaggi prima di essersi autenticato.
fn non_autenticato() -> ServerMessage {
    ServerMessage::Error {
        code: ErrorCode::NotAuthenticated,
        detail: "serve il login prima di inviare posizioni o messaggi".into(),
    }
}

/// Risposta a un coordinatore che agisce prima di essersi autenticato.
fn admin_non_autenticato() -> AdminReply {
    AdminReply::Error {
        code: ErrorCode::NotAuthenticated,
        detail: "serve il login".into(),
    }
}
