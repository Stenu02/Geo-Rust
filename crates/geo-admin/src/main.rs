use geo_admin::ui::AdminUi;
use std::io;
use crossterm::{
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use ratatui::{backend::CrosstermBackend, Terminal};

fn main() -> anyhow::Result<()> {
    // Setup del terminale
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Esecuzione della UI (il mock data deve trovarsi in assets rispetto alla cwd di esecuzione)
    let mut ui = AdminUi::new("assets/mock_admin_data.json")?;
    ui.run(&mut terminal)?;

    // Ripristino del terminale
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
