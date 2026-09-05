//! TUI camionista
//! interfaccia a terminale divisa in 3 sezioni:
//! - pannello di stato (coordinate e connessione)
//! - cronologia chat (messaggi in entrata e uscita)
//! - barra di testo (input verso l'admin)

use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use geo_core::{Position, UserState};
use geo_net::{connection::Command, events::Event as NetEvent};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::{io, time::Duration};
use tokio::sync::{mpsc, watch};

/// stato locale dell'interfaccia grafica
pub struct ClientUi {
    pub input: String,
    pub chat_history: Vec<String>,
    pub current_state: UserState,
    pub last_pos: Option<Position>,
}

impl ClientUi {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            chat_history: Vec::new(),
            current_state: UserState::Disconnected,
            last_pos: None,
        }
    }

    /// gestisce il rendering e ascolta concorrentemente rete e tastiera
    pub async fn run(
        &mut self,
        command_tx: mpsc::Sender<Command>,
        manual_pos_tx: mpsc::Sender<Position>, // Sender per catturare l'input manuale
        mut event_rx: mpsc::Receiver<NetEvent>,
        mut pos_rx: watch::Receiver<Option<Position>>,
    ) -> anyhow::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        loop {
            // disegno dell'interfaccia
            terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(5),
                        Constraint::Min(10),
                        Constraint::Length(3)
                    ].as_ref())
                    .split(f.size());

                let pos_str = match &self.last_pos {
                    Some(p) => format!("Lat: {:.5}, Lon: {:.5}", p.lat, p.lon),
                    None => "In attesa... (usa /pos lat,lon per input manuale)".to_string(),
                };

                let state_str = match self.current_state {
                    UserState::Disconnected => "SCONNESSO",
                    UserState::Stopped => "FERMO",
                    UserState::Moving => "IN MOVIMENTO",
                };

                let status_text = format!("Stato: {}.\nPosizione: {}.", state_str, pos_str);
                let status_p = Paragraph::new(status_text)
                    .block(Block::default().title(" Sensore ").borders(Borders::ALL));
                f.render_widget(status_p, chunks[0]);

                // unisce lo storico della chat e calcola lo scroll per rimanere in fondo
                let chat_text = self.chat_history.join("\n");
                let lines_count = chat_text.lines().count() as u16;
                let history_height = chunks[1].height.saturating_sub(2); // -2 per i bordi

                let scroll_offset = if lines_count > history_height {
                    lines_count - history_height
                } else {
                    0
                };

                let chat_p = Paragraph::new(chat_text)
                    .block(Block::default().title(" Chat ").borders(Borders::ALL))
                    .wrap(Wrap { trim: true })
                    .scroll((scroll_offset, 0));
                f.render_widget(chat_p, chunks[1]);

                // mostra il testo digitato con un finto cursore visivo
                let input_text = format!("> {}█", self.input);
                let input_p = Paragraph::new(input_text)
                    .block(Block::default().title(" Invia ").borders(Borders::ALL));
                f.render_widget(input_p, chunks[2]);
            })?;

            // gestione concorrente degli eventi (rete o tastiera)
            tokio::select! {
                net_event = event_rx.recv() => {
                    if let Some(ev) = net_event {
                        match ev {
                            NetEvent::Connected => {
                                self.chat_history.push("[SISTEMA] Connesso al server.".into());
                            }
                            NetEvent::LoggedIn { username } => {
                                self.chat_history.push(format!("[SISTEMA] Loggato come {}.", username));
                            }
                            NetEvent::ChatReceived { text, broadcast, at } => {
                                let prefix = if broadcast { "[BROADCAST]" } else { "[DIRETTO]" };
                                let time = at.with_timezone(&chrono::Local).format("%H:%M:%S");
                                self.chat_history.push(format!("{} [{}] Admin: {}.", prefix, time, text));
                            }
                            NetEvent::PositionAck { state } => {
                                // aggiorna l'interfaccia con lo stato calcolato dal server
                                self.current_state = state;
                            }
                            NetEvent::Disconnected { reason } => {
                                self.current_state = UserState::Disconnected;
                                self.chat_history.push(format!("[SISTEMA] {}.", reason));
                            }
                            NetEvent::ServerError { code, detail } => {
                                self.chat_history.push(format!("[ERRORE] {:?}: {}.", code, detail));
                            }
                        }
                    } else {
                        break;
                    }
                }

                // aggiorna le coordinate dal simulatore
                Ok(_) = pos_rx.changed() => {
                    self.last_pos = *pos_rx.borrow();
                }

                // thread separato per l'input: evita di bloccare la ricezione della rete in attesa della tastiera
                input_event = tokio::task::spawn_blocking(|| {
                    if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                        event::read().ok()
                    } else {
                        None
                    }
                }) => {
                    if let Ok(Some(CEvent::Key(key))) = input_event {
                        // gestisce solo la pressione dei tasti, ignorando il rilascio
                        if key.kind == KeyEventKind::Press {
                            match key.code {
                                KeyCode::Esc => break,
                                KeyCode::Char(c) => self.input.push(c),
                                KeyCode::Backspace => { self.input.pop(); }
                                KeyCode::Enter => {
                                    if !self.input.is_empty() {
                                        let msg = self.input.clone();

                                        // intercetta il comando manuale per la posizione
                                        if msg.starts_with("/pos ") {
                                            let coords_str = &msg[5..];
                                            let parts: Vec<&str> = coords_str.split(',').collect();
                                            if parts.len() == 2 {
                                                if let (Ok(lat), Ok(lon)) = (parts[0].trim().parse::<f64>(), parts[1].trim().parse::<f64>()) {
                                                    let _ = manual_pos_tx.send(Position::new(lat, lon)).await;
                                                    self.chat_history.push(format!("[SISTEMA] Posizione impostata a ({}, {})", lat, lon));
                                                } else {
                                                    self.chat_history.push("[ERRORE] Formato numeri non valido. Usa: /pos lat,lon".to_string());
                                                }
                                            } else {
                                                self.chat_history.push("[ERRORE] Sintassi errata. Usa: /pos lat,lon".to_string());
                                            }
                                        } else {
                                            // invio normale del messaggio di chat
                                            self.chat_history.push(format!("Tu: {}", msg));
                                            let _ = command_tx.send(Command::Chat { text: msg }).await;
                                        }

                                        self.input.clear();
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        // ripristino del terminale prima di chiudere (evita di corrompere la console dell'utente)
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        Ok(())
    }
}