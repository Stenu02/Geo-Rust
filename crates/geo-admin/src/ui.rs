use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table, Paragraph, Wrap},
    Frame, Terminal,
};
use serde::Deserialize;
use std::fs;
use crossterm::event::{self, Event, KeyCode};
use std::time::Duration;

#[derive(Deserialize, Debug)]
pub struct MockData {
    pub fleet: Vec<FleetEntry>,
    pub chat: Vec<ChatEntry>,
    pub query_report: QueryReport,
}

#[derive(Deserialize, Debug)]
pub struct FleetEntry {
    pub user_id: String,
    pub state: String,
    pub last_pos: String,
    pub last_update: String,
}

#[derive(Deserialize, Debug)]
pub struct ChatEntry {
    pub sender: String,
    pub text: String,
}

#[derive(Deserialize, Debug)]
pub struct QueryReport {
    pub target_user: String,
    pub distance_km: f64,
    pub avg_speed: f64,
    pub moving_time: String,
    pub pause_time: String,
}

pub struct AdminUi {
    pub data: MockData,
    pub input: String,
}

impl AdminUi {
    pub fn new(data_path: &str) -> anyhow::Result<Self> {
        let content = fs::read_to_string(data_path)?;
        // Rimuove il BOM (Byte Order Mark) invisibile se presente
        let content = content.trim_start_matches('\u{feff}');
        let data: MockData = serde_json::from_str(content)?;
        Ok(Self { data, input: String::new() })
    }

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> anyhow::Result<()> {
        loop {
            terminal.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Esc => break, // Esci con Esc invece di 'q' per poter digitare
                        KeyCode::Char(c) => self.input.push(c),
                        KeyCode::Backspace => { self.input.pop(); }
                        KeyCode::Enter => {
                            if !self.input.is_empty() {
                                self.data.chat.push(ChatEntry { sender: "ADMIN".to_string(), text: self.input.clone() });
                                self.input.clear();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    fn draw(&self, f: &mut Frame) {
        let area = f.size();
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .margin(1)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(area);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(main_chunks[1]);

        self.draw_fleet(f, main_chunks[0]);
        self.draw_queries(f, right_chunks[0]);
        self.draw_chat(f, right_chunks[1]);
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
        let text = format!(
            "Utente analizzato: {}\n\n\
             Distanza totale: {:.1} km\n\
             Velocità media: {:.1} km/h\n\
             Tempo in movimento: {}\n\
             Tempo in pausa: {}",
            rep.target_user, rep.distance_km, rep.avg_speed, rep.moving_time, rep.pause_time
        );

        let p = Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title(" Interrogazioni "))
            .wrap(Wrap { trim: true });

        f.render_widget(p, area);
    }

    fn draw_chat(&self, f: &mut Frame, area: Rect) {
        let mut chat_text = String::new();
        for msg in &self.data.chat {
            chat_text.push_str(&format!("[{}]: {}\n", msg.sender, msg.text));
        }
        
        chat_text.push_str("\n--- INSERISCI MESSAGGIO ---\n");
        chat_text.push_str("> ");
        chat_text.push_str(&self.input);
        // Cursore finto
        chat_text.push_str("█");

        let p = Paragraph::new(chat_text)
            .block(Block::default().borders(Borders::ALL).title(" Chat "))
            .wrap(Wrap { trim: true });

        f.render_widget(p, area);
    }
}
