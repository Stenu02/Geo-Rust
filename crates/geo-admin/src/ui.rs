use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table, Paragraph, Wrap, Tabs},
    text::Line,
    Frame, Terminal,
};
use serde::{Deserialize, Serialize};
use std::fs;
use crossterm::event::{self, Event, KeyCode};
use std::time::Duration;
use geo_core::analytics::MovementReport;

use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct MockData {
    pub fleet: Vec<FleetEntry>,
    pub chats: HashMap<String, Vec<ChatEntry>>,
    pub query_report: MovementReport,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FleetEntry {
    pub user_id: String,
    pub state: String,
    pub last_pos: String,
    pub last_update: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ChatEntry {
    pub sender: String,
    pub text: String,
}

pub struct AdminUi {
    pub data: MockData,
    pub input: String,
    pub data_path: String,
    pub selected_tab: usize,
    pub selected_chat_index: usize,
}

impl AdminUi {
    pub fn new(data_path: &str) -> anyhow::Result<Self> {
        let bytes = fs::read(data_path)?;
        let data: MockData = geo_protocol::codec::decode(&bytes)?;
        Ok(Self { 
            data, 
            input: String::new(),
            data_path: data_path.to_string(),
            selected_tab: 0,
            selected_chat_index: 0,
        })
    }

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> anyhow::Result<()> {
        loop {
            terminal.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == crossterm::event::KeyEventKind::Press {
                        match key.code {
                            KeyCode::Esc => break,
                            KeyCode::Tab => {
                                self.selected_tab = (self.selected_tab + 1) % 2;
                            }
                            KeyCode::BackTab => { // if supported
                                self.selected_tab = (self.selected_tab + 1) % 2;
                            }
                            KeyCode::Up => {
                                if self.selected_tab == 0 && self.selected_chat_index > 0 {
                                    self.selected_chat_index -= 1;
                                }
                            }
                            KeyCode::Down => {
                                if self.selected_tab == 0 && self.selected_chat_index + 1 < self.data.fleet.len() {
                                    self.selected_chat_index += 1;
                                }
                            }
                            KeyCode::Char(c) => self.input.push(c),
                            KeyCode::Backspace => { self.input.pop(); }
                            KeyCode::Enter => {
                                if !self.input.is_empty() && self.selected_tab == 0 && !self.data.fleet.is_empty() {
                                    let active_user = self.data.fleet[self.selected_chat_index].user_id.clone();
                                    self.data.chats.entry(active_user).or_default().push(ChatEntry { sender: "ADMIN".to_string(), text: self.input.clone() });
                                    self.input.clear();
                                    
                                    if let Ok(bytes) = geo_protocol::codec::encode(&self.data) {
                                        let _ = fs::write(&self.data_path, bytes);
                                    }
                                }
                            }
                            _ => {}
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
            .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
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
    }

    fn draw_fleet(&self, f: &mut Frame, area: Rect) {
        let header_cells = ["Utente", "Stato", "Posizione", "Aggiornamento"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow)));
        let header = Row::new(header_cells)
            .style(Style::default().bg(Color::DarkGray))
            .height(1)
            .bottom_margin(1);

        let rows = self.data.fleet.iter().map(|item| {
            let cells = vec![
                Cell::from(item.user_id.clone()),
                Cell::from(item.state.clone()),
                Cell::from(item.last_pos.clone()),
                Cell::from(item.last_update.clone()),
            ];
            Row::new(cells).height(1)
        });

        let table = Table::new(rows, [
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Flotta (Live) "));

        f.render_widget(table, area);
    }

    fn draw_queries(&self, f: &mut Frame, area: Rect) {
        let rep = &self.data.query_report;
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
    }

    fn draw_chat(&self, f: &mut Frame, area: Rect) {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
            .split(area);

        // Sinistra: Lista Utenti per la chat
        let mut list_items = Vec::new();
        for (i, truck) in self.data.fleet.iter().enumerate() {
            let style = if i == self.selected_chat_index {
                Style::default().bg(Color::DarkGray).fg(Color::Yellow)
            } else {
                Style::default()
            };
            list_items.push(ratatui::widgets::ListItem::new(truck.user_id.clone()).style(style));
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
        let active_user = if !self.data.fleet.is_empty() {
            self.data.fleet[self.selected_chat_index].user_id.clone()
        } else {
            String::new()
        };

        if let Some(history) = self.data.chats.get(&active_user) {
            for msg in history {
                chat_history.push_str(&format!("[{}]: {}\n", msg.sender, msg.text));
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
            .block(Block::default().borders(Borders::ALL).title(format!(" Chat con {} ", active_user)))
            .wrap(Wrap { trim: true }) 
            .scroll((scroll_offset, 0));

        f.render_widget(history_p, v_chunks[0]);

        let input_text = format!("> {}█", self.input);
        let input_p = Paragraph::new(input_text)
            .block(Block::default().borders(Borders::ALL).title(" Invia Messaggio (Premi Invio) "));

        f.render_widget(input_p, v_chunks[1]);
    }
}
