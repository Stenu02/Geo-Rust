use clap::Parser;
use geo_admin::ui::AdminUi;
use std::io::{self, Write};
use crossterm::{
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use ratatui::{backend::CrosstermBackend, Terminal};

#[derive(Parser)]
#[command(name = "geo-admin", about = "Console amministratore GeoRuggine")]
struct Args {
    #[arg(long, env = "GEO_SERVER", default_value = "127.0.0.1:7878")]
    server: String,

    #[arg(long, env = "GEO_ADMIN_USER")]
    username: Option<String>,

    #[arg(long, env = "GEO_ADMIN_PASS")]
    password: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Parsing argomenti
    let args = Args::parse();
    
    // Controllo inserimenti parziali
    if args.username.is_some() ^ args.password.is_some() {
        eprintln!("Errore: Inserimento parziale non consentito.");
        eprintln!("Metodi di inserimento corretti:");
        eprintln!("  argomenti completi: --username <USER> --password <PASS>");
        eprintln!("  avvia il programma senza argomenti e inseriscili quando richiesto.");
        std::process::exit(1);
    }

    let mut current_user = args.username;
    let mut current_pass = args.password;

    // 2. Sistema di Login (tentativi illimitati)
    loop {
        let user = match &current_user {
            Some(u) => u.clone(),
            None => {
                print!("Username: ");
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                input.trim().to_string()
            }
        };

        let pass = match &current_pass {
            Some(p) => p.clone(),
            None => {
                print!("Password: ");
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                input.trim().to_string()
            }
        };

        if user == "admin" && pass == "123" {
            println!("Login concesso. Avvio interfaccia...");
            break;
        } else {
            println!("Credenziali errate. Accesso negato.\n");
            // Resettiamo le credenziali per richiederle da input al prossimo ciclo
            current_user = None;
            current_pass = None;
        }
    }

    // Qui in futuro ci sarà la logica di connessione asincrona tramite tokio
    // let stream = tokio::net::TcpStream::connect(&args.server).await?;

    // 3. Setup del terminale ed esecuzione UI Mock
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Il mock data deve trovarsi in assets rispetto alla cwd di esecuzione
    let mut ui = AdminUi::new("assets/mock_admin_data.bin")?;
    ui.run(&mut terminal)?;

    // Ripristino del terminale
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
