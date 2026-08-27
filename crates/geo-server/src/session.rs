//! Ogni connessione accettata dal server diventa qui un task a se' stante:
//! `handle` fa l'handshake, poi smista verso `trucker_loop` o `admin_loop` a
//! seconda del ruolo dichiarato. I due cicli leggono un messaggio alla volta e
//! rispondono, appoggiandosi a `storage` per il database e a `registry` per
//! sapere e aggiornare chi e' collegato — non fanno mai questo lavoro da soli.

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

/// Quanto si attende il primo frame prima di chiudere.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

// Gestisce una singola connessione fino alla sua chiusura.
// Funzione chiamata in lib, per ogni connessione accettata
pub async fn handle(
    stream: TcpStream,
    shutdown: CancellationToken,
    storage: StorageHandle,
    registry: Registry,
) -> anyhow::Result<()> {
    let peer = stream.peer_addr()?;
    let mut wire = Framed::new(stream, frame_codec());

    //Chiamata alla funzione handshake
    let Some(hello) = handshake(&mut wire).await? else {
        return Ok(());
    };

    tracing::info!(%peer, role = ?hello.role, "handshake completato");

    // Chi si e' autenticato su questa connessione. Vive qui e non dentro il
    // ciclo perche' serve anche DOPO, per la chiusura ordinata.
    let mut sessione: Option<(UserId, String)> = None;

    // Il ruolo dichiarato nell'handshake decide come vanno letti tutti i frame
    // successivi: gli stessi byte sono un ClientMessage o un AdminMessage.
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

    // Controllo sulla struttura Hello
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

    // Messaggio di Welcome in base al ruolo
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

/// Hash contro cui verificare quando l'utente non esiste.
const PHC_FITTIZIO: &str = "$argon2id$v=19$m=19456,t=2,p=1$mPPAx9/sJgdFwPpOmZkZzQ$RW7w1/7mKTOGp6CFI7okMaRciTuid7mzwPmOJuhtQRI";

/// Dialogo con un camionista, fino alla disconnessione o alla chiusura del server.
async fn trucker_loop(
    wire: &mut Wire,
    shutdown: &CancellationToken,
    storage: &StorageHandle,
    registry: &Registry,
    autenticato: &mut Option<(UserId, String)>,
) -> anyhow::Result<()> {
    // Le due vie da cui possono arrivare messaggi DAL SERVER verso questo
    // camionista. Restano vuote finche' non fa login: prima di allora nessuno
    // sa a chi scrivere.
    //
    //   posta   -> messaggi diretti a lui solo
    //   annunci -> messaggi mandati a tutta la flotta
    let mut posta: Option<mpsc::Receiver<ServerMessage>> = None;
    let mut annunci: Option<broadcast::Receiver<ServerMessage>> = None;

    loop {
        // Il frame si estrae qui
        // Quattro cose possono succedere, e si aspettano tutte insieme: qui il
        // task dorme finche' non ne accade una.
        //
        // I due rami delle caselle usano `pending()` quando la casella non
        // esiste ancora (prima del login): e' un'attesa che non finisce mai,
        // quindi quel ramo semplicemente non puo' mai vincere.
        let frame = tokio::select! {
            _ = shutdown.cancelled() => break,

            // 1. Un messaggio DIRETTO a questo camionista.
            msg = async {
                match posta.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match msg {
                    Some(m) => send(wire, &m).await?,
                    // La casella si e' chiusa: succede se il registro ha
                    // sostituito questa voce con un altro login dello stesso
                    // utente. Questa sessione non deve piu' ricevere.
                    None => posta = None,
                }
                continue;
            }

            // 2. Un annuncio a tutta la flotta.
            msg = async {
                match annunci.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match msg {
                    Ok(m) => send(wire, &m).await?,
                    // Questa sessione era troppo lenta e ha perso qualche
                    // annuncio: si registra e si continua. Un annuncio vale
                    // adesso, non ha senso recuperarlo dopo.
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(persi = n, "annunci persi per lentezza");
                    }
                    Err(broadcast::error::RecvError::Closed) => annunci = None,
                }
                continue;
            }

            // 3. Un messaggio dal client.
            frame = wire.next() => match frame {
                // Nessun frame: il client ha chiuso la connessione.
                None => break,
                Some(frame) => frame?,
            },
        };

        match decode::<ClientMessage>(&frame[..])? {
            //Registrazione
            ClientMessage::Register { username, password } => {
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
            //Login
            ClientMessage::Login { username, password } => {
                //Già autenticato
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

                // Al camionista il ruolo non interessa: gli basta esistere e
                // sapere la propria password.
                let (utente, phc) = match storage.find_user(&username).await? {
                    Some((u, p, _ruolo)) => (Some(u), p),
                    None => (None, PHC_FITTIZIO.to_string()),
                };
                let corretta = auth::verify_password_async(password, phc).await;

                match (utente, corretta) {
                    (Some(user), true) => {
                        tracing::info!(%username, %user, "login effettuato");
                        // Da qui in poi questa sessione puo' RICEVERE: il
                        // registro consegna le due estremita' di lettura, e i
                        // rami 1 e 2 del select! smettono di essere inerti.
                        let (rx_posta, rx_annunci) = registry.register(user, username.clone());
                        posta = Some(rx_posta);
                        annunci = Some(rx_annunci);

                        // SCONNESSO -> FERMO. La transizione va scritta subito:
                        // e' l'apertura del primo intervallo di questa sessione,
                        // e senza di essa le analisi avrebbero un buco.
                        let ora = Utc::now();
                        if let Some(t) = registry.on_login(user, ora) {
                            storage.apply_transition(user, t.to, t.at).await?;
                        }

                        *autenticato = Some((user, username.clone()));
                        send(wire, &ServerMessage::LoginOk { user, username }).await?;

                        // Coda offline: i messaggi diretti arrivati mentre non
                        // era collegato gli vengono consegnati adesso, in
                        // ordine di invio. Vengono marcati come consegnati
                        // dentro la stessa transazione che li legge, quindi al
                        // prossimo login non ricompaiono.
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
                    // Stesso identico messaggio nei due casi: non si rivela se sia
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

                // Una coordinata assurda non deve entrare nello storico
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

                // La macchina a stati decide se questa posizione e' un cambio.
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
                // Il messaggio va a chi sta guardando la console. Viaggia gia'
                // nella forma che l'amministratore sa leggere, con dentro il
                // nome di chi scrive: la console non deve andarselo a cercare.
                let ora = Utc::now();
                let letto_da = registry.verso_admin(AdminReply::Chat {
                    from: *user,
                    username: username.clone(),
                    text: text.clone(),
                    at: ora,
                });
                // Salvato comunque, anche se nessun amministratore era
                // collegato: lo storico della chat non dipende da chi stava
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

/// Dialogo con l'amministratore.
async fn admin_loop(
    wire: &mut Wire,
    shutdown: &CancellationToken,
    storage: &StorageHandle,
    registry: &Registry,
) -> anyhow::Result<()> {
    // L'amministratore ascolta cio' che i camionisti scrivono.
    let mut dai_camionisti = registry.subscribe_admin();

    // Chi si e' autenticato su questa connessione. Come per il camionista,
    // resta None finche' non arriva un login valido: dichiararsi
    // amministratore nell'handshake non basta, va dimostrato.
    let mut autenticato: Option<UserId> = None;

    loop {
        let frame = tokio::select! {
            _ = shutdown.cancelled() => break,

            // Un messaggio scritto da un camionista, da mostrare in console.
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

                // Stessa difesa del login camionista: si verifica sempre una
                // password, anche quando l'utente non esiste, cosi' il tempo di
                // risposta non rivela chi e' registrato e chi no.
                let (utente, phc, ruolo) = match storage.find_user(&username).await? {
                    Some((u, p, r)) => (Some(u), p, r),
                    None => (None, PHC_FITTIZIO.to_string(), String::new()),
                };
                let corretta = auth::verify_password_async(password, phc).await;

                // Tre condizioni, non due: l'utente deve esistere, la password
                // deve essere giusta, e il ruolo deve essere quello di
                // amministratore. Senza l'ultima, un camionista che dichiara
                // Role::Admin nell'handshake entrerebbe con le proprie
                // credenziali e vedrebbe l'intera flotta.
                match (utente, corretta, ruolo.as_str()) {
                    (Some(user), true, "admin") => {
                        tracing::info!(%username, %user, "login amministratore");
                        autenticato = Some(user);
                        send(wire, &AdminReply::LoginOk { user, username }).await?;
                    }
                    // Un solo messaggio per tutti e tre i motivi di rifiuto:
                    // non si rivela se l'utente non esista, se la password sia
                    // sbagliata o se manchi il permesso.
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
            // risolve il periodo, estrae dal database intervalli e
            // posizioni, chiama la creazione del repor e lo rispedisce.
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
                // `send_to` restituisce false se il camionista non e' collegato
                // oppure se la sua casella e' piena. Non e' un errore del
                // server: e' l'informazione che serve per decidere se mettere
                // il messaggio in coda per il prossimo login.
                let consegnato = registry.send_to(
                    to,
                    ServerMessage::Chat {
                        text: text.clone(),
                        broadcast: false,
                        at: ora,
                    },
                );
                // Se non e' stato consegnato resta in coda con delivered = 0,
                // e il destinatario lo ricevera' al prossimo login.
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
                // Un annuncio va ai camionisti COLLEGATI in questo momento:
                // e' cio' che dice la specifica. Zero destinatari non e' un
                // errore, significa flotta vuota.
                let destinatari = registry.annuncia(ServerMessage::Chat {
                    text: text.clone(),
                    broadcast: true,
                    at: ora,
                });
                // Un annuncio si salva sempre come gia' consegnato: vale per
                // chi c'era, e chi era assente non lo deve recuperare dopo.
                storage.save_message(None, None, &text, ora, true).await?;
                tracing::info!(%text, destinatari, "annuncio a tutta la flotta");
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

/// L'errore restituito a chi manda messaggi prima di aver fatto login.
fn non_autenticato() -> ServerMessage {
    ServerMessage::Error {
        code: ErrorCode::NotAuthenticated,
        detail: "serve il login prima di inviare posizioni o messaggi".into(),
    }
}

/// L'errore per un amministratore che agisce prima di aver fatto login.
fn admin_non_autenticato() -> AdminReply {
    AdminReply::Error {
        code: ErrorCode::NotAuthenticated,
        detail: "serve il login".into(),
    }
}
