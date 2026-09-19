//! Interfaccia utente (TUI) per la console amministratore.
//!
//! Gestisce il rendering a schermo intero tramite `ratatui` e l'elaborazione degli
//! eventi della tastiera, aggiornando lo stato locale (flotta, chat, report).

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap},
    Frame, Terminal,
};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

use geo_core::analytics::MovementReport;
use geo_core::{RangeSpec, UserId};
use geo_protocol::{AdminMessage, AdminReply, FleetEntry};

/// Rappresenta i dati e lo stato dell'interfaccia utente dell'amministratore.
pub struct AppState {
    pub fleet: Vec<FleetEntry>,
    pub chats: HashMap<UserId, Vec<(String, String)>>,
    pub query_report: Option<Box<MovementReport>>,
    pub input: String,
    pub selected_tab: usize,
    pub selected_chat_index: usize,
    pub typing_mode: bool,
    pub cursor_pos: usize,
    pub last_error: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            fleet: Vec::new(),
            chats: HashMap::new(),
            query_report: None,
            input: String::new(),
            selected_tab: 0,
            selected_chat_index: 0,
            typing_mode: false,
            cursor_pos: 0,
            last_error: None,
        }
    }

    pub fn handle_network_reply(&mut self, reply: AdminReply) {
        match reply {
            AdminReply::Fleet { entries } => {
                // Salva l'utente attualmente selezionato per mantenerlo attivo
                let current_selected_user =
                    self.fleet.get(self.selected_chat_index).map(|e| e.user);

                self.fleet = entries;

                // Aggiorna l'indice per evitare panic se la flotta si riduce
                if let Some(user) = current_selected_user {
                    if let Some(new_idx) = self.fleet.iter().position(|e| e.user == user) {
                        self.selected_chat_index = new_idx;
                    } else {
                        self.selected_chat_index = self
                            .selected_chat_index
                            .min(self.fleet.len().saturating_sub(1));
                    }
                } else {
                    self.selected_chat_index = self
                        .selected_chat_index
                        .min(self.fleet.len().saturating_sub(1));
                }
            }
            AdminReply::StateChanged { user, state, at } => {
                if let Some(entry) = self.fleet.iter_mut().find(|e| e.user == user) {
                    entry.state = state;
                    entry.last_update = Some(at);
                }
            }
            AdminReply::QueryResult(report) => {
                self.query_report = Some(report);
                self.selected_tab = 1; // Cambio automatico al tab delle interrogazioni
            }
            AdminReply::Chat {
                from,
                username,
                text,
                at,
            } => {
                let time = at.with_timezone(&chrono::Local).format("%H:%M:%S");
                let sender = format!("[{time}]             {username}");
                self.chats.entry(from).or_default().push((sender, text));
            }
            AdminReply::Error { code, detail } => {
                self.last_error = Some(format!("{:?}: {}", code, detail));
            }
            _ => {}
        }
    }

    pub fn handle_key_event(
        &mut self,
        key: KeyEvent,
        command_tx: &mpsc::Sender<AdminMessage>,
    ) -> bool {
        self.last_error = None;
        if self.typing_mode {
            match key.code {
                KeyCode::Esc => {
                    self.typing_mode = false;
                    self.input.clear();
                    self.cursor_pos = 0;
                }
                KeyCode::Left => {
                    if self.cursor_pos > 0 {
                        self.cursor_pos -= 1;
                    }
                }
                KeyCode::Right => {
                    if self.cursor_pos < self.input.chars().count() {
                        self.cursor_pos += 1;
                    }
                }
                KeyCode::Up => {
                    self.cursor_pos = 0;
                }
                KeyCode::Down => {
                    self.cursor_pos = self.input.chars().count();
                }
                KeyCode::Char(c) => {
                    let mut chars: Vec<char> = self.input.chars().collect();
                    chars.insert(self.cursor_pos, c);
                    self.input = chars.into_iter().collect();
                    self.cursor_pos += 1;
                }
                KeyCode::Backspace => {
                    if self.cursor_pos > 0 {
                        let mut chars: Vec<char> = self.input.chars().collect();
                        chars.remove(self.cursor_pos - 1);
                        self.input = chars.into_iter().collect();
                        self.cursor_pos -= 1;
                    }
                }
                KeyCode::Enter => {
                    if !self.input.is_empty() {
                        let text = self.input.clone();
                        if let Some(msg) = text.strip_prefix("/all ") {
                            let _ = command_tx.try_send(AdminMessage::SendBroadcast {
                                text: msg.to_string(),
                            });

                            let time = chrono::Local::now().format("%H:%M:%S");
                            let sender = format!("[{time}] [BROADCAST] ADMIN");
                            for entry in &self.fleet {
                                self.chats
                                    .entry(entry.user)
                                    .or_default()
                                    .push((sender.clone(), msg.to_string()));
                            }

                            self.input.clear();
                            self.typing_mode = false;
                            self.cursor_pos = 0;
                        } else if let Some(cmd) = text.strip_prefix("/report ") {
                            let spec = match cmd.trim().to_lowercase().as_str() {
                                "oggi" => Some(RangeSpec::Today),
                                "settimana" => Some(RangeSpec::CurrentWeek),
                                "mese" => Some(RangeSpec::CurrentMonth),
                                _ => None,
                            };
                            if let Some(s) = spec {
                                if !self.fleet.is_empty() {
                                    let active_user = self.fleet[self.selected_chat_index].user;
                                    let _ = command_tx.try_send(AdminMessage::Query {
                                        user: active_user,
                                        spec: s,
                                    });
                                } else {
                                    self.last_error = Some(
                                        "Errore: flotta vuota, nessun utente selezionato"
                                            .to_string(),
                                    );
                                }
                            } else {
                                self.last_error = Some(
                                    "Comando /report non valido. Usa: oggi, settimana, mese"
                                        .to_string(),
                                );
                            }
                            self.input.clear();
                            self.typing_mode = false;
                            self.cursor_pos = 0;
                        } else if self.selected_tab == 0 && !self.fleet.is_empty() {
                            let active_user = self.fleet[self.selected_chat_index].user;

                            // Invia al server
                            if command_tx
                                .try_send(AdminMessage::SendDirect {
                                    to: active_user,
                                    text: text.clone(),
                                })
                                .is_err()
                            {
                                self.last_error = Some(
                                    "Errore di connessione: impossibile inviare il messaggio."
                                        .to_string(),
                                );
                            } else {
                                // Mostra subito in locale
                                let time = chrono::Local::now().format("%H:%M:%S");
                                let sender = format!("[{time}]             ADMIN");
                                self.chats
                                    .entry(active_user)
                                    .or_default()
                                    .push((sender, text));
                                self.input.clear();
                                self.typing_mode = false;
                                self.cursor_pos = 0;
                            }
                        }
                    }
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Esc => {
                    let _ = command_tx.try_send(AdminMessage::Logout);
                    return false; // Segnala di uscire dal loop
                }
                KeyCode::Tab => {
                    self.selected_tab = (self.selected_tab + 1) % 2;
                }
                KeyCode::BackTab => {
                    self.selected_tab = (self.selected_tab + 1) % 2;
                }
                KeyCode::Char('t') | KeyCode::Char('T') => {
                    self.typing_mode = true;
                }
                KeyCode::Up => {
                    if self.selected_tab == 0 && self.selected_chat_index > 0 {
                        self.selected_chat_index -= 1;
                    }
                }
                KeyCode::Down => {
                    if self.selected_tab == 0 && self.selected_chat_index + 1 < self.fleet.len() {
                        self.selected_chat_index += 1;
                    }
                }
                KeyCode::Char('q') | KeyCode::Char('Q') => {
                    self.selected_tab = 1;
                    if !self.fleet.is_empty() {
                        let active_user = self.fleet[self.selected_chat_index].user;
                        let _ = command_tx.try_send(AdminMessage::Query {
                            user: active_user,
                            spec: RangeSpec::Today,
                        });
                    }
                }
                KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') => {
                    if self.selected_tab == 1 {
                        let active_user = if let Some(rep) = &self.query_report {
                            Some(rep.user)
                        } else if !self.fleet.is_empty() {
                            Some(self.fleet[self.selected_chat_index].user)
                        } else {
                            None
                        };

                        if let Some(user) = active_user {
                            let spec = match key.code {
                                KeyCode::Char('1') => RangeSpec::Today,
                                KeyCode::Char('2') => RangeSpec::CurrentWeek,
                                KeyCode::Char('3') => RangeSpec::CurrentMonth,
                                _ => unreachable!(),
                            };
                            let _ = command_tx.try_send(AdminMessage::Query { user, spec });
                        }
                    }
                }
                _ => {}
            }
        }
        true
    }
}

/// Gestore del ciclo principale dell'interfaccia utente.
pub struct AdminUi {
    pub state: AppState,
    command_tx: mpsc::Sender<AdminMessage>,
    event_rx: mpsc::Receiver<AdminReply>,
}

impl AdminUi {
    /// Crea una nuova istanza dell'interfaccia utente, inizializzando i canali di comunicazione.
    pub fn new(
        command_tx: mpsc::Sender<AdminMessage>,
        event_rx: mpsc::Receiver<AdminReply>,
    ) -> Self {
        Self {
            state: AppState::new(),
            command_tx,
            event_rx,
        }
    }

    /// Avvia il ciclo principale (event loop) della UI.
    pub fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> anyhow::Result<()> {
        loop {
            // Elabora gli eventi di rete in ingresso
            let mut disconnected = false;
            loop {
                match self.event_rx.try_recv() {
                    Ok(reply) => {
                        let mut request_fleet = false;
                        match &reply {
                            AdminReply::StateChanged { user, .. } => {
                                if !self.state.fleet.iter().any(|e| e.user == *user) {
                                    request_fleet = true;
                                }
                            }
                            AdminReply::Chat { from, .. } => {
                                if !self.state.fleet.iter().any(|e| e.user == *from) {
                                    request_fleet = true;
                                }
                            }
                            _ => {}
                        }
                        self.state.handle_network_reply(reply);

                        if request_fleet {
                            let _ = self.command_tx.try_send(AdminMessage::ListFleet);
                        }
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if disconnected {
                return Err(anyhow::anyhow!("Connessione al server persa."));
            }

            terminal.draw(|f| self.draw(f))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == crossterm::event::KeyEventKind::Press
                        && !self.state.handle_key_event(key, &self.command_tx)
                    {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn draw(&self, f: &mut Frame) {
        let area = f.size();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Length(3),
                    Constraint::Min(0),
                    Constraint::Length(if self.state.last_error.is_some() {
                        3
                    } else {
                        0
                    }),
                ]
                .as_ref(),
            )
            .split(area);

        let titles = vec![
            Line::from(" Flotta & Chat (Live) "),
            Line::from(" Analisi Dati (Interrogazioni) "),
        ];

        let tabs = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Menu Principale (Premi TAB per cambiare) "),
            )
            .highlight_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
            .select(self.state.selected_tab);

        f.render_widget(tabs, chunks[0]);

        if self.state.selected_tab == 0 {
            // Tab 0: Flotta & Chat
            let tab0_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
                .split(chunks[1]);

            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(tab0_chunks[0]);

            self.draw_fleet(f, main_chunks[0]);
            self.draw_chat(f, main_chunks[1]);
            self.draw_commands_reminder(f, tab0_chunks[1]);
        } else {
            // Tab 1: Analisi
            self.draw_queries(f, chunks[1]);
        }

        if let Some(err) = &self.state.last_error {
            let p = Paragraph::new(err.clone())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Errore ")
                        .border_style(Style::default().fg(Color::Red)),
                )
                .style(Style::default().fg(Color::Red));
            f.render_widget(p, chunks[2]);
        }
    }

    fn draw_fleet(&self, f: &mut Frame, area: Rect) {
        let header_cells = ["Utente", "Stato", "Posizione", "Aggiornamento"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow)));
        let header = Row::new(header_cells)
            .style(Style::default().bg(Color::DarkGray))
            .height(1)
            .bottom_margin(1);

        let rows = self.state.fleet.iter().enumerate().map(|(i, item)| {
            let pos_str = match &item.last_position {
                Some(p) => format!("{:.4}, {:.4}", p.lat, p.lon),
                None => "Sconosciuta".to_string(),
            };
            let update_str = match item.last_update {
                Some(dt) => dt
                    .with_timezone(&chrono::Local)
                    .format("%H:%M:%S")
                    .to_string(),
                None => "-".to_string(),
            };

            let cells = vec![
                Cell::from(item.username.clone()),
                Cell::from(format!("{:?}", item.state)),
                Cell::from(pos_str),
                Cell::from(update_str),
            ];

            let mut row = Row::new(cells).height(1);
            if i == self.state.selected_chat_index {
                row = row.style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
            }
            row
        });

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Flotta (Live) - Premi Q per Report "),
        );

        f.render_widget(table, area);
    }

    fn draw_queries(&self, f: &mut Frame, area: Rect) {
        if let Some(rep) = &self.state.query_report {
            let moving_secs = rep.moving_time.num_seconds();
            let paused_secs = rep.paused_time.num_seconds();
            let disc_secs = rep.disconnected_time.num_seconds();

            let start_str = rep
                .range
                .start
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S");
            let end_str = rep
                .range
                .end
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S");

            let text = format!(
                "Utente analizzato: {}\n\n\
                 Periodo di analisi: {}\n\
                 - Da: {}\n\
                 - A:  {}\n\n\
                 Distanza totale percorsa: {:.2} km\n\
                 Velocità media (in moto): {:.1} km/h\n\
                 Velocità media (globale): {:.1} km/h\n\n\
                 Tempo in movimento: {} min {} sec\n\
                 Tempo in pausa: {} min {} sec ({} soste effettuate)\n\
                 Tempo disconnesso: {} min {} sec",
                rep.user,
                rep.spec.label(),
                start_str,
                end_str,
                rep.distance_km,
                rep.avg_speed_moving_kmh,
                rep.avg_speed_overall_kmh,
                moving_secs / 60,
                moving_secs % 60,
                paused_secs / 60,
                paused_secs % 60,
                rep.pause_count,
                disc_secs / 60,
                disc_secs % 60
            );

            let p = Paragraph::new(text)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Report Analitico (Premi: 1=Oggi, 2=Settimana, 3=Mese) "),
                )
                .wrap(Wrap { trim: true });

            f.render_widget(p, area);
        } else {
            let p = Paragraph::new(
                "Nessun report generato. Seleziona un utente e premi Q nella flotta.",
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Report Analitico (Premi: 1=Oggi, 2=Settimana, 3=Mese) "),
            );
            f.render_widget(p, area);
        }
    }

    fn draw_chat(&self, f: &mut Frame, area: Rect) {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(25), Constraint::Percentage(75)].as_ref())
            .split(area);

        // Sinistra: Lista Utenti per la chat
        let mut list_items = Vec::new();
        for (i, truck) in self.state.fleet.iter().enumerate() {
            let style = if i == self.state.selected_chat_index {
                Style::default().bg(Color::DarkGray).fg(Color::Yellow)
            } else {
                Style::default()
            };
            list_items.push(ratatui::widgets::ListItem::new(truck.username.clone()).style(style));
        }
        let list = ratatui::widgets::List::new(list_items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Utenti (\u{2191}\u{2193}) "),
        );
        f.render_widget(list, h_chunks[0]);

        // Destra: Chat attiva
        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(h_chunks[1]);

        let mut chat_history = String::new();
        let (active_user_id, active_username) = if !self.state.fleet.is_empty() {
            (
                self.state.fleet[self.state.selected_chat_index].user,
                self.state.fleet[self.state.selected_chat_index]
                    .username
                    .clone(),
            )
        } else {
            (UserId(0), String::new())
        };

        if let Some(history) = self.state.chats.get(&active_user_id) {
            for (sender, text) in history {
                chat_history.push_str(&format!("{}: {}\n", sender, text));
            }
        }

        let lines_count = chat_history.lines().count() as u16;
        let history_height = v_chunks[0].height.saturating_sub(2);

        let scroll_offset = lines_count.saturating_sub(history_height);

        let history_p = Paragraph::new(chat_history)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Chat con {} ", active_username)),
            )
            .wrap(Wrap { trim: true })
            .scroll((scroll_offset, 0));

        f.render_widget(history_p, v_chunks[0]);

        let mut input_text_chars: Vec<char> = self.state.input.chars().collect();
        if self.state.typing_mode {
            if self.state.cursor_pos <= input_text_chars.len() {
                input_text_chars.insert(self.state.cursor_pos, '█');
            } else {
                input_text_chars.push('█');
            }
        }
        let input_text = format!("> {}", input_text_chars.into_iter().collect::<String>());
        let input_title = if self.state.typing_mode {
            " Invia Messaggio (Premi ESC per annullare, Invio per inviare) "
        } else {
            " Premi 'T' per scrivere un messaggio "
        };
        let input_color = if self.state.typing_mode {
            Color::Yellow
        } else {
            Color::White
        };
        let input_p = Paragraph::new(input_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(input_color))
                .title(input_title),
        );

        f.render_widget(input_p, v_chunks[1]);
    }

    fn draw_commands_reminder(&self, f: &mut Frame, area: Rect) {
        let reminder_p = Paragraph::new(" /all <msg>, /report <oggi|settimana|mese> ")
            .block(Block::default().borders(Borders::ALL).title(" Comandi "));
        f.render_widget(reminder_p, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use geo_protocol::{AdminMessage, AdminReply};
    use tokio::sync::mpsc;

    fn make_key_event(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    /// Testa l'inizializzazione dello stato dell'interfaccia utente.
    #[test]
    fn test_admin_ui_initialization() {
        let (command_tx, _command_rx) = mpsc::channel::<AdminMessage>(32);
        let (_event_tx, event_rx) = mpsc::channel::<AdminReply>(32);

        let ui = AdminUi::new(command_tx, event_rx);

        assert!(ui.state.fleet.is_empty());
        assert!(ui.state.chats.is_empty());
        assert!(ui.state.query_report.is_none());
        assert!(ui.state.input.is_empty());
        assert_eq!(ui.state.selected_tab, 0);
        assert_eq!(ui.state.selected_chat_index, 0);
        assert!(!ui.state.typing_mode);
        assert_eq!(ui.state.cursor_pos, 0);
        assert!(ui.state.last_error.is_none());
    }

    /// Testa che l'invio di un messaggio vuoto non generi alcuna azione.
    #[test]
    fn test_empty_message_ignored() {
        let (command_tx, mut command_rx) = mpsc::channel::<AdminMessage>(32);
        let mut state = AppState::new();
        state.typing_mode = true;

        // Pressione invio su input vuoto
        state.handle_key_event(make_key_event(KeyCode::Enter), &command_tx);

        // Deve rimanere in typing mode e non mandare messaggi
        assert!(state.typing_mode);
        assert!(command_rx.try_recv().is_err());
    }

    /// Testa l'invio di un messaggio diretto con successo e il caso di flotta vuota.
    #[test]
    fn test_direct_message_success_and_empty_fleet() {
        let (command_tx, mut command_rx) = mpsc::channel::<AdminMessage>(32);
        let mut state = AppState::new();

        // CASO FLOTTA VUOTA
        state.typing_mode = true;
        state.input = "ciao".to_string();

        state.handle_key_event(make_key_event(KeyCode::Enter), &command_tx);

        // Nessun messaggio inviato, input non cancellato, ancora in typing mode
        assert!(command_rx.try_recv().is_err());
        assert_eq!(state.input, "ciao");
        assert!(state.typing_mode);

        // CASO DI SUCCESSO
        state.fleet.push(FleetEntry {
            user: UserId(100),
            username: "test_truck_1".to_string(),
            state: geo_core::UserState::Stopped,
            last_position: None,
            last_update: None,
        });

        state.handle_key_event(make_key_event(KeyCode::Enter), &command_tx);

        // Messaggio inviato
        let sent = command_rx.try_recv().expect("Messaggio non inviato!");
        match sent {
            AdminMessage::SendDirect { to, text } => {
                assert_eq!(to, UserId(100));
                assert_eq!(text, "ciao");
            }
            _ => panic!("Messaggio errato inviato: {:?}", sent),
        }

        // La chat locale si popola
        assert_eq!(state.chats.get(&UserId(100)).unwrap()[0].1, "ciao");
        // Typing mode azzerato
        assert!(!state.typing_mode);
        assert!(state.input.is_empty());
    }

    /// Testa l'invio di un messaggio broadcast.
    #[test]
    fn test_broadcast_command_all() {
        let (command_tx, mut command_rx) = mpsc::channel::<AdminMessage>(32);
        let mut state = AppState::new();
        state.typing_mode = true;

        // Invia /all test anche con flotta vuota
        state.input = "/all attenzione a tutti".to_string();
        state.handle_key_event(make_key_event(KeyCode::Enter), &command_tx);

        let sent = command_rx
            .try_recv()
            .expect("Messaggio broadcast non inviato");
        match sent {
            AdminMessage::SendBroadcast { text } => assert_eq!(text, "attenzione a tutti"),
            _ => panic!("Messaggio errato"),
        }
        assert!(!state.typing_mode);
        assert!(state.input.is_empty());
    }

    /// Testa i comandi di report.
    #[test]
    fn test_report_commands() {
        let (command_tx, mut command_rx) = mpsc::channel::<AdminMessage>(32);
        let mut state = AppState::new();

        // Comando non valido
        state.typing_mode = true;
        state.input = "/report anno".to_string(); // "anno" non è un comando valido
        state.handle_key_event(make_key_event(KeyCode::Enter), &command_tx);
        assert!(state.last_error.is_some());
        assert!(state
            .last_error
            .as_ref()
            .unwrap()
            .contains("Comando /report non valido"));

        // Comando valido ma flotta vuota
        state.typing_mode = true;
        state.input = "/report oggi".to_string();
        state.handle_key_event(make_key_event(KeyCode::Enter), &command_tx);
        assert!(state.last_error.is_some());
        assert!(state
            .last_error
            .as_ref()
            .unwrap()
            .contains("Errore: flotta vuota"));

        // Comando valido e flotta con veicoli
        state.fleet.push(FleetEntry {
            user: UserId(2),
            username: "test_truck_2".to_string(),
            state: geo_core::UserState::Moving,
            last_position: None,
            last_update: None,
        });

        // Test oggi
        state.typing_mode = true;
        state.input = "/report oggi".to_string();
        state.handle_key_event(make_key_event(KeyCode::Enter), &command_tx);
        match command_rx.try_recv().unwrap() {
            AdminMessage::Query { user, spec } => {
                assert_eq!(user, UserId(2));
                assert_eq!(spec, RangeSpec::Today);
            }
            _ => panic!("Tipo messaggio errato"),
        }

        // Test settimana
        state.typing_mode = true;
        state.input = "/report settimana".to_string();
        state.handle_key_event(make_key_event(KeyCode::Enter), &command_tx);
        match command_rx.try_recv().unwrap() {
            AdminMessage::Query { spec, .. } => assert_eq!(spec, RangeSpec::CurrentWeek),
            _ => panic!("Tipo messaggio errato"),
        }
    }

    /// Testa la funzionalità del tasto Q.
    #[test]
    fn test_q_key_functionality() {
        let (command_tx, mut command_rx) = mpsc::channel::<AdminMessage>(32);
        let mut state = AppState::new();

        // Flotta vuota
        state.handle_key_event(make_key_event(KeyCode::Char('q')), &command_tx);
        assert_eq!(state.selected_tab, 1);
        assert!(command_rx.try_recv().is_err());

        // Flotta con utente
        state.selected_tab = 0;
        state.fleet.push(FleetEntry {
            user: UserId(42),
            username: "test_truck".to_string(),
            state: geo_core::UserState::Stopped,
            last_position: None,
            last_update: None,
        });

        state.handle_key_event(make_key_event(KeyCode::Char('Q')), &command_tx);
        assert_eq!(state.selected_tab, 1);

        match command_rx.try_recv().unwrap() {
            AdminMessage::Query { user, spec } => {
                assert_eq!(user, UserId(42));
                assert_eq!(spec, RangeSpec::Today);
            }
            _ => panic!("Messaggio errato inviato"),
        }
    }

    /// Testa i tasti 1, 2, 3 nel tab Analisi.
    #[test]
    fn test_analisi_time_span_keys() {
        let (command_tx, mut command_rx) = mpsc::channel::<AdminMessage>(32);
        let mut state = AppState::new();

        state.fleet.push(FleetEntry {
            user: UserId(10),
            username: "truck_10".to_string(),
            state: geo_core::UserState::Stopped,
            last_position: None,
            last_update: None,
        });

        // In tab 0 i tasti 1,2,3 non devono inviare query
        state.selected_tab = 0;
        state.handle_key_event(make_key_event(KeyCode::Char('1')), &command_tx);
        assert!(command_rx.try_recv().is_err());

        // In tab 1 i tasti 1,2,3 inviano query
        state.selected_tab = 1;

        state.handle_key_event(make_key_event(KeyCode::Char('1')), &command_tx);
        match command_rx.try_recv().unwrap() {
            AdminMessage::Query { user, spec } => {
                assert_eq!(user, UserId(10));
                assert_eq!(spec, RangeSpec::Today);
            }
            _ => panic!("Messaggio errato"),
        }

        state.handle_key_event(make_key_event(KeyCode::Char('2')), &command_tx);
        match command_rx.try_recv().unwrap() {
            AdminMessage::Query { user, spec } => {
                assert_eq!(user, UserId(10));
                assert_eq!(spec, RangeSpec::CurrentWeek);
            }
            _ => panic!("Messaggio errato"),
        }

        state.handle_key_event(make_key_event(KeyCode::Char('3')), &command_tx);
        match command_rx.try_recv().unwrap() {
            AdminMessage::Query { user, spec } => {
                assert_eq!(user, UserId(10));
                assert_eq!(spec, RangeSpec::CurrentMonth);
            }
            _ => panic!("Messaggio errato"),
        }

        // In typing mode i tasti digitano testo e non inviano query
        state.typing_mode = true;
        state.handle_key_event(make_key_event(KeyCode::Char('1')), &command_tx);
        assert!(command_rx.try_recv().is_err());
        assert_eq!(state.input, "1");
    }
}
