//! TUI camionista
//! interfaccia a terminale divisa in 3 sezioni:
//! - pannello di stato (mostra le ultime coordinate inviate e lo stato di connessione)
//! - cronologia chat (messaggi in entrata e uscita)
//! - barra di testo (per scrivere all'admin)

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
    style::{Color, Style},
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

    /// avvia il loop di rendering asincrono e la gestione degli eventi
    /// si interrompe solo quando l'utente preme esc
    pub async fn run(
        &mut self,
        mut command_tx: mpsc::Sender<Command>,
        mut event_rx: mpsc::Receiver<NetEvent>,
        mut pos_rx: watch::Receiver<Option<Position>>,
    ) -> anyhow::Result<()> {
        // setup terminale
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        loop {
            // disegno dell'interfaccia
            terminal.draw(|f| {
                // suddivide lo schermo in tre righe:
                // 1. stato (3 righe fisse)
                // 2. chat (tutto lo spazio rimanente)
                // 3. input (3 righe fisse)
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(5),
                        Constraint::Min(10),
                        Constraint::Length(3)
                    ].as_ref())
                    .split(f.size());

                // stato sensore
                let pos_str = match &self.last_pos {
                    Some(p) => format!("Lat: {:.5}, Lon: {:.5}", p.lat, p.lon),
                    None => "In attesa del primo campionamento...".to_string(),
                };

                let state_str = match self.current_state {
                    UserState::Disconnected => "SCONNESSO",
                    UserState::Stopped => "FERMO",
                    UserState::Moving => "IN MOVIMENTO",
                };

                let status_text = format!("Stato: {}.\nPosizione: {}.", state_str, pos_str);
                let status_p = Paragraph::new(status_text)
                    .block(Block::default().title(" Stato Sensore ").borders(Borders::ALL));
                f.render_widget(status_p, chunks[0]);

                // chat
                // unisce lo storico della chat e lo mostra al centro
                let chat_text = self.chat_history.join("\n");
                let chat_p = Paragraph::new(chat_text)
                    .block(Block::default().title(" Chat (R14) ").borders(Borders::ALL))
                    .wrap(Wrap { trim: true });
                f.render_widget(chat_p, chunks[1]);

                // barra di testo
                // mostra il testo digitato con un finto cursore visivo
                let input_text = format!("> {}█", self.input);
                let input_p = Paragraph::new(input_text)
                    .block(Block::default().title(" Invia Messaggio ").borders(Borders::ALL));
                f.render_widget(input_p, chunks[2]);
            })?;

            // gestione concorrente degli eventi (rete o tastiera)
            tokio::select! {
                // evento di rete (messaggio ricevuto dal server)
                net_event = event_rx.recv() => {
                    if let Some(ev) = net_event {
                        match ev {
                            NetEvent::ChatReceived { text, broadcast, at } => {
                                let prefix = if broadcast { "[BROADCAST]" } else { "[DIRETTO]" };
                                self.chat_history.push(format!("{} Admin: {}", prefix, text));
                            }
                            NetEvent::PositionAck { state } => {
                                // aggiorna l'interfaccia con lo stato calcolato dal server.
                                self.current_state = state;
                            }
                            NetEvent::Disconnected { reason } => {
                                self.current_state = UserState::Disconnected;
                                self.chat_history.push(format!("[SISTEMA] Disconnesso: {}", reason));
                            }
                            _ => {}
                        }
                    } else {
                        // se il canale è chiuso, esce dal loop
                        break;
                    }
                }

                // aggiorna le coordinate dal simulatore
                Ok(_) = pos_rx.changed() => {
                    self.last_pos = *pos_rx.borrow();
                }

                // evento da tastiera (deve essere in un thread separato spawn_blocking
                // per non bloccare l'executor di tokio mentre aspetta che l'utente scriva)
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
                                KeyCode::Esc => break, // termina il programma
                                KeyCode::Char(c) => self.input.push(c), // scrive il carattere
                                KeyCode::Backspace => { self.input.pop(); } // cancella l'ultimo carattere
                                KeyCode::Enter => {
                                    // invia il messaggio
                                    if !self.input.is_empty() {
                                        let msg = self.input.clone();
                                        self.chat_history.push(format!("Tu: {}", msg));

                                        // usa il clone del canale per comunicare a geo-net di inviare i byte
                                        let _ = command_tx.send(Command::Chat { text: msg }).await;
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