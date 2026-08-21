use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table, Paragraph, Wrap},
    Frame, Terminal,
};
use serde::{Deserialize, Serialize};
use std::fs;
use crossterm::event::{self, Event, KeyCode};
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug)]
pub struct MockData {
    pub fleet: Vec<FleetEntry>,
    pub chat: Vec<ChatEntry>,
    pub query_report: QueryReport,
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

#[derive(Serialize, Deserialize, Debug)]
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
    pub data_path: String,
}

impl AdminUi {
    pub fn new(data_path: &str) -> anyhow::Result<Self> {
        let content = fs::read_to_string(data_path)?;
        // Rimuove il BOM (Byte Order Mark) invisibile se presente
        let content = content.trim_start_matches('\u{feff}');
        let data: MockData = serde_json::from_str(content)?;
        Ok(Self { 
            data, 
            input: String::new(),
            data_path: data_path.to_string(),
        })
    }

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> anyhow::Result<()> {
        loop {
            terminal.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    // Controlla che sia una pressione
                    if key.kind == crossterm::event::KeyEventKind::Press {
                        match key.code {
                            KeyCode::Esc => break, // Esci con Esc
                            KeyCode::Char(c) => self.input.push(c),
                            KeyCode::Backspace => { self.input.pop(); }
                            KeyCode::Enter => {
                                if !self.input.is_empty() {
                                    self.data.chat.push(ChatEntry { sender: "ADMIN".to_string(), text: self.input.clone() });
                                    self.input.clear();
                                    
                                    // serde_json::to_string_pretty formatta i dati in JSON leggibile con indentazione
                                    if let Ok(json_string) = serde_json::to_string_pretty(&self.data) {
                                        // fs::write sovrascrive il file con la nuova stringa
                                        let _ = fs::write(&self.data_path, json_string);
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
        // Dividiamo l'area verticalmente: la chat sopra (spazio flessibile) e l'input sotto (3 righe fisse)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(area);

        // --- PANNELLO STORICO CHAT ---
        let mut chat_history = String::new();
        for msg in &self.data.chat {
            chat_history.push_str(&format!("[{}]: {}\n", msg.sender, msg.text));
        }

        let lines_count = chat_history.lines().count() as u16;
        let history_height = chunks[0].height.saturating_sub(2); // -2 per i bordi superiore e inferiore
        
        // Calcola lo scroll solo per la cronologia (evitando problemi con i newline extra che Ratatui elimina col wrap)
        let scroll_offset = if lines_count > history_height {
            lines_count - history_height
        } else {
            0
        };

        let history_p = Paragraph::new(chat_history)
            .block(Block::default().borders(Borders::ALL).title(" Cronologia Chat "))
            .wrap(Wrap { trim: true }) // Il trim può falsare il conteggio righe se ci sono linee vuote, ma ora non le usiamo!
            .scroll((scroll_offset, 0));

        f.render_widget(history_p, chunks[0]);

        // --- PANNELLO INPUT MESSAGGIO ---
        // Mostra il testo digitato con un finto cursore alla fine
        let input_text = format!("> {}█", self.input);
        let input_p = Paragraph::new(input_text)
            .block(Block::default().borders(Borders::ALL).title(" Invia Messaggio "));

        f.render_widget(input_p, chunks[1]);
    }
}
