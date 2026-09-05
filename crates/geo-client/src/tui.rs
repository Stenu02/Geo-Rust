//! TUI del camionista
//! Interfaccia a terminale divisa in 3 sezioni
//! Implementa chat bidirezionale e inserimento coordinate manuale
//! Gira all'interno di un loop asincrono
//! La lettura bloccante della tastiera viene isolata in spawn_blocking per non
//! congelare l'elaborazione dei pacchetti di rete in ingresso

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

    /// Gestisce il rendering e ascolta rete e tastiera
    pub async fn run(
        &mut self,
        command_tx: mpsc::Sender<Command>,
        manual_pos_tx: mpsc::Sender<Position>,
        mut event_rx: mpsc::Receiver<NetEvent>,
        mut pos_rx: watch::Receiver<Option<Position>>,
    ) -> anyhow::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

        let (key_tx, mut key_rx) = mpsc::channel::<CEvent>(16);
        std::thread::spawn(move || {
            loop {
                if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                    if let Ok(ev) = event::read() {
                        if key_tx.blocking_send(ev).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        loop {
            terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(5),
                        Constraint::Min(10),
                        Constraint::Length(3)
                    ].as_ref())
                    .split(f.size());

                let pos_str = self.last_pos.map_or_else(
                    || "In attesa... (usa /pos lat,lon per input manuale)".to_string(),
                    |p| format!("Lat: {:.5}, Lon: {:.5}", p.lat, p.lon)
                );

                let state_str = match self.current_state {
                    UserState::Disconnected => "SCONNESSO",
                    UserState::Stopped => "FERMO",
                    UserState::Moving => "IN MOVIMENTO",
                };

                let status_p = Paragraph::new(format!("Stato: {state_str}.\nPosizione: {pos_str}."))
                    .block(Block::default().title(" Sensore ").borders(Borders::ALL));
                f.render_widget(status_p, chunks[0]);

                // Calcolo scroll per mantenere il focus sugli ultimi messaggi
                let chat_text = self.chat_history.join("\n");
                let lines_count = chat_text.lines().count() as u16;
                let history_height = chunks[1].height.saturating_sub(2);
                let scroll_offset = lines_count.saturating_sub(history_height);

                let chat_p = Paragraph::new(chat_text)
                    .block(Block::default().title(" Chat ").borders(Borders::ALL))
                    .wrap(Wrap { trim: true })
                    .scroll((scroll_offset, 0));
                f.render_widget(chat_p, chunks[1]);

                let input_p = Paragraph::new(format!("> {}█", self.input))
                    .block(Block::default().title(" Invia ").borders(Borders::ALL));
                f.render_widget(input_p, chunks[2]);
            })?;

            // Gestione concorrente degli eventi (rete o tastiera)
            tokio::select! {
                // Ricezione eventi di rete
                net_event = event_rx.recv() => {
                    let Some(ev) = net_event else { break };
                    match ev {
                        NetEvent::Connected => self.chat_history.push("[SISTEMA] Connesso al server.".into()),
                        NetEvent::LoggedIn { username } => self.chat_history.push(format!("[SISTEMA] Loggato come {username}.")),
                        NetEvent::ChatReceived { text, broadcast, at } => {
                            let prefix = if broadcast { "[BROADCAST]" } else { "[DIRETTO]" };
                            let time = at.with_timezone(&chrono::Local).format("%H:%M:%S");
                            self.chat_history.push(format!("{prefix} [{time}] Admin: {text}"));
                        }
                        // La TUI si adegua passivamente allo stato calcolato dal server
                        NetEvent::PositionAck { state } => self.current_state = state,
                        NetEvent::Disconnected { reason } => {
                            self.current_state = UserState::Disconnected;
                            self.chat_history.push(format!("[SISTEMA] {reason}"));
                        }
                        NetEvent::ServerError { code, detail } => self.chat_history.push(format!("[ERRORE] {code:?}: {detail}")),
                    }
                }

                Ok(_) = pos_rx.changed() => self.last_pos = *pos_rx.borrow(),

                // Tastiera su thread separato per evitare blocking
                Some(input_event) = key_rx.recv() => {
                    if let CEvent::Key(key) = input_event {
                        if key.kind == KeyEventKind::Press {
                            match key.code {
                                KeyCode::Esc => break,
                                KeyCode::Char(c) => self.input.push(c),
                                KeyCode::Backspace => { self.input.pop(); }
                                KeyCode::Enter => {
                                    if self.input.is_empty() { continue; }
                                    let msg = self.input.clone();

                                    // Intercetta il comando per forzare la posizione
                                    if msg.starts_with("/pos ") {
                                        let parts: Vec<&str> = msg[5..].split(',').collect();
                                        if parts.len() == 2 {
                                            if let (Ok(lat), Ok(lon)) = (parts[0].trim().parse::<f64>(), parts[1].trim().parse::<f64>()) {
                                                let _ = manual_pos_tx.send(Position::new(lat, lon)).await;
                                                self.chat_history.push(format!("[SISTEMA] Posizione impostata a ({}, {}).", lat, lon));
                                            } else {
                                                self.chat_history.push("[ERRORE] Formato numeri non valido. Usa: /pos lat,lon.".to_string());
                                            }
                                        } else {
                                            self.chat_history.push("[ERRORE] Sintassi errata. Usa: /pos lat,lon.".to_string());
                                        }
                                    } else {
                                        // Invio risposta in chat
                                        self.chat_history.push(format!("Tu: {msg}"));
                                        let _ = command_tx.send(Command::Chat { text: msg }).await;
                                    }
                                    self.input.clear();
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        // Ripristino del terminale prima di chiudere
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        Ok(())
    }
}