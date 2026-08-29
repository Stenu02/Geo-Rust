//! Interfaccia utente (TUI) per la console amministratore.
//!
//! Gestisce il rendering a schermo intero tramite `ratatui` e l'elaborazione degli 
//! eventi della tastiera, aggiornando lo stato locale (flotta, chat, report).

use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table, Paragraph, Wrap, Tabs},
    text::Line,
    Frame, Terminal,
};
use crossterm::event::{self, Event, KeyCode};
use std::time::Duration;
use std::collections::HashMap;
use tokio::sync::mpsc;

use geo_core::analytics::MovementReport;
use geo_core::{UserId, RangeSpec};
use geo_protocol::{AdminMessage, AdminReply, FleetEntry};

/// Rappresenta lo stato dell'interfaccia utente dell'amministratore.
pub struct AdminUi {
    pub fleet: Vec<FleetEntry>,
    pub chats: HashMap<UserId, Vec<(String, String)>>,
    pub query_report: Option<Box<MovementReport>>,
    pub input: String,
    pub selected_tab: usize,
    pub selected_chat_index: usize,
    pub typing_mode: bool,
    pub cursor_pos: usize,
    pub last_error: Option<String>,
    command_tx: mpsc::Sender<AdminMessage>,
    event_rx: mpsc::Receiver<AdminReply>,
}

impl AdminUi {
    /// Crea una nuova istanza dell'interfaccia utente, inizializzando i canali di comunicazione.
    pub fn new(command_tx: mpsc::Sender<AdminMessage>, event_rx: mpsc::Receiver<AdminReply>) -> Self {
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
            command_tx,
            event_rx,
        }
    }

    /// Avvia il ciclo principale (event loop) della UI.
    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> anyhow::Result<()> {
        loop {
            // Elabora gli eventi di rete in ingresso
            let mut disconnected = false;
            loop {
                match self.event_rx.try_recv() {
                    Ok(reply) => {
                        match reply {
                            AdminReply::Fleet { entries } => {
                                self.fleet = entries;
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
                            AdminReply::Chat { from, username, text, .. } => {
                                self.chats.entry(from).or_default().push((username, text));
                            }
                            AdminReply::Error { code, detail } => {
                                self.last_error = Some(format!("{:?}: {}", code, detail));
                            }
                            _ => {}
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
                    if key.kind == crossterm::event::KeyEventKind::Press {
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
                                            let _ = self.command_tx.try_send(AdminMessage::SendBroadcast {
                                                text: msg.to_string(),
                                            });
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
                                                    let _ = self.command_tx.try_send(AdminMessage::Query {
                                                        user: active_user,
                                                        spec: s
                                                    });
                                                } else {
                                                    self.last_error = Some("Errore: flotta vuota, nessun utente selezionato".to_string());
                                                }
                                            } else {
                                                self.last_error = Some("Comando /report non valido. Usa: oggi, settimana, mese".to_string());
                                            }
                                            self.input.clear();
                                            self.typing_mode = false;
                                            self.cursor_pos = 0;
                                        } else if self.selected_tab == 0 && !self.fleet.is_empty() {
                                            let active_user = self.fleet[self.selected_chat_index].user;
                                            
                                            // Invia al server
                                            let _ = self.command_tx.try_send(AdminMessage::SendDirect {
                                                to: active_user,
                                                text: text.clone(),
                                            });

                                            // Mostra subito in locale
                                            self.chats.entry(active_user).or_default().push(("ADMIN".to_string(), text));
                                            self.input.clear();
                                            self.typing_mode = false;
                                            self.cursor_pos = 0;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            match key.code {
                                KeyCode::Esc => {
                                    let _ = self.command_tx.try_send(AdminMessage::Logout);
                                    break;
                                }
                                KeyCode::Tab => {
                                    self.selected_tab = (self.selected_tab + 1) % 2;
                                }
                                KeyCode::BackTab => {
                                    self.selected_tab = (self.selected_tab + 1) % 2;
                                }
                                KeyCode::Char('m') | KeyCode::Char('M') => {
                                    self.selected_tab = 0;
                                }
                                KeyCode::Char('t') | KeyCode::Char('T') => {
                                    self.typing_mode = true;
                                }
                                KeyCode::Char('q') | KeyCode::Char('Q') => {
                                    if self.selected_tab == 0 && !self.fleet.is_empty() {
                                        let active_user = self.fleet[self.selected_chat_index].user;
                                        let _ = self.command_tx.try_send(AdminMessage::Query { 
                                            user: active_user, 
                                            spec: RangeSpec::Today 
                                        });
                                    }
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
                                _ => {}
                            }
                        }
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
            .constraints([
                Constraint::Length(3), 
                Constraint::Min(0),
                Constraint::Length(if self.last_error.is_some() { 3 } else { 0 })
            ].as_ref())
            .split(area);

        let titles = vec![
            Line::from(" Flotta & Chat (Live) "),
            Line::from(" Analisi Dati (Interrogazioni) ")
        ];
        
        let tabs = Tabs::new(titles)
            .block(Block::default().borders(Borders::ALL).title(" Menu Principale (Premi TAB per cambiare) "))
            .highlight_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
            .select(self.selected_tab);
            
        f.render_widget(tabs, chunks[0]);

        if self.selected_tab == 0 {
            // Tab 0: Flotta & Chat
            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(chunks[1]);
            
            self.draw_fleet(f, main_chunks[0]);
            self.draw_chat(f, main_chunks[1]);
        } else {
            // Tab 1: Analisi
            self.draw_queries(f, chunks[1]);
        }

        if let Some(err) = &self.last_error {
            let p = Paragraph::new(err.clone())
                .block(Block::default().borders(Borders::ALL).title(" Errore ").border_style(Style::default().fg(Color::Red)))
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

        let rows = self.fleet.iter().enumerate().map(|(i, item)| {
            let pos_str = match &item.last_position {
                Some(p) => format!("{:.4}, {:.4}", p.lat, p.lon),
                None => "Sconosciuta".to_string(),
            };
            let update_str = match item.last_update {
                Some(dt) => dt.format("%H:%M:%S").to_string(),
                None => "-".to_string(),
            };

            let cells = vec![
                Cell::from(item.username.clone()),
                Cell::from(format!("{:?}", item.state)),
                Cell::from(pos_str),
                Cell::from(update_str),
            ];
            
            let mut row = Row::new(cells).height(1);
            if i == self.selected_chat_index {
                row = row.style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
            }
            row
        });

        let table = Table::new(rows, [
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Flotta (Live) - Premi Q per Report "));

        f.render_widget(table, area);
    }

    fn draw_queries(&self, f: &mut Frame, area: Rect) {
        if let Some(rep) = &self.query_report {
            let moving_secs = rep.moving_time.num_seconds();
            let paused_secs = rep.paused_time.num_seconds();
            let disc_secs = rep.disconnected_time.num_seconds();
            
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
                rep.range.start,
                rep.range.end,
                rep.distance_km,
                rep.avg_speed_moving_kmh,
                rep.avg_speed_overall_kmh,
                moving_secs / 60, moving_secs % 60,
                paused_secs / 60, paused_secs % 60, rep.pause_count,
                disc_secs / 60, disc_secs % 60
            );

            let p = Paragraph::new(text)
                .block(Block::default().borders(Borders::ALL).title(" Report Analitico "))
                .wrap(Wrap { trim: true });

            f.render_widget(p, area);
        } else {
            let p = Paragraph::new("Nessun report generato. Seleziona un utente e premi Q nella flotta.")
                .block(Block::default().borders(Borders::ALL).title(" Report Analitico "));
            f.render_widget(p, area);
        }
    }

    fn draw_chat(&self, f: &mut Frame, area: Rect) {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
            .split(area);

        // Sinistra: Lista Utenti per la chat
        let mut list_items = Vec::new();
        for (i, truck) in self.fleet.iter().enumerate() {
            let style = if i == self.selected_chat_index {
                Style::default().bg(Color::DarkGray).fg(Color::Yellow)
            } else {
                Style::default()
            };
            list_items.push(ratatui::widgets::ListItem::new(truck.username.clone()).style(style));
        }
        let list = ratatui::widgets::List::new(list_items)
            .block(Block::default().borders(Borders::ALL).title(" Utenti (\u{2191}\u{2193}) "));
        f.render_widget(list, h_chunks[0]);

        // Destra: Chat attiva
        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(h_chunks[1]);

        let mut chat_history = String::new();
        let (active_user_id, active_username) = if !self.fleet.is_empty() {
            (self.fleet[self.selected_chat_index].user, self.fleet[self.selected_chat_index].username.clone())
        } else {
            (UserId(0), String::new())
        };

        if let Some(history) = self.chats.get(&active_user_id) {
            for (sender, text) in history {
                chat_history.push_str(&format!("[{}]: {}\n", sender, text));
            }
        }

        let lines_count = chat_history.lines().count() as u16;
        let history_height = v_chunks[0].height.saturating_sub(2);
        
        let scroll_offset = if lines_count > history_height {
            lines_count - history_height
        } else {
            0
        };

        let history_p = Paragraph::new(chat_history)
            .block(Block::default().borders(Borders::ALL).title(format!(" Chat con {} ", active_username)))
            .wrap(Wrap { trim: true }) 
            .scroll((scroll_offset, 0));

        f.render_widget(history_p, v_chunks[0]);

        let mut input_text_chars: Vec<char> = self.input.chars().collect();
        if self.typing_mode {
            if self.cursor_pos <= input_text_chars.len() {
                input_text_chars.insert(self.cursor_pos, '█');
            } else {
                input_text_chars.push('█');
            }
        }
        let input_text = format!("> {}", input_text_chars.into_iter().collect::<String>());
        let input_title = if self.typing_mode {
            " Invia Messaggio (Premi ESC per annullare, Invio per inviare) "
        } else {
            " Premi 'T' per scrivere un messaggio "
        };
        let input_color = if self.typing_mode { Color::Yellow } else { Color::White };
        let input_p = Paragraph::new(input_text)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(input_color)).title(input_title));

        f.render_widget(input_p, v_chunks[1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use geo_protocol::{AdminMessage, AdminReply};

    #[test]
    fn test_admin_ui_initialization() {
        let (command_tx, _command_rx) = mpsc::channel::<AdminMessage>(32);
        let (_event_tx, event_rx) = mpsc::channel::<AdminReply>(32);
        
        let ui = AdminUi::new(command_tx, event_rx);
        
        assert!(ui.fleet.is_empty());
        assert!(ui.chats.is_empty());
        assert!(ui.query_report.is_none());
        assert!(ui.input.is_empty());
        assert_eq!(ui.selected_tab, 0);
        assert_eq!(ui.selected_chat_index, 0);
        assert!(!ui.typing_mode);
        assert_eq!(ui.cursor_pos, 0);
        assert!(ui.last_error.is_none());
    }
}
