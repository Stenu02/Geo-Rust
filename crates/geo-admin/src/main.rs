//! Punto di ingresso dell'applicazione `geo-admin`.
//!
//! Effettua il parsing degli argomenti da riga di comando, gestisce il processo
//! di login, e fa partire in background la connessione di rete e in foreground 
//! l'interfaccia utente (TUI).

use clap::Parser;
use geo_admin::tui::AdminUi;
use std::io::{self, Write};
use crossterm::{
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use ratatui::{backend::CrosstermBackend, Terminal};

/// Argomenti da riga di comando per la console dell'amministratore.
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

    let framed = loop {
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
                rpassword::read_password()?
            }
        };

        let opts = geo_net::connection::ConnectOptions {
            server_addr: args.server.clone(),
            username: user,
            password: pass,
            role: geo_protocol::Role::Admin,
            register: false,
        };

        match geo_net::connection::connect_and_auth(&opts).await {
            Ok(f) => {
                println!("Login concesso. Avvio interfaccia...");
                break f;
            }
            Err(e) => {
                println!("Errore di accesso: {}", e);
                current_user = None;
                current_pass = None;
            }
        }
    };

    let (command_tx, command_rx) = tokio::sync::mpsc::channel(32);
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(32);

    tokio::spawn(geo_admin::network::run_admin_connection(framed, command_rx, event_tx));

    // 3. Setup del terminale ed esecuzione UI
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    let mut ui = AdminUi::new(command_tx, event_rx);
    let res = ui.run(&mut terminal);

    // Ripristino del terminale
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let _ = std::panic::take_hook();

    res?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_args_parsing() {
        let args = Args::try_parse_from(["geo-admin", "--server", "192.168.1.1:8080", "--username", "admin", "--password", "secret"]).unwrap();
        assert_eq!(args.server, "192.168.1.1:8080");
        assert_eq!(args.username, Some("admin".to_string()));
        assert_eq!(args.password, Some("secret".to_string()));
    }
}
