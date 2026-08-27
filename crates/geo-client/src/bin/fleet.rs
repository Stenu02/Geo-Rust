//! lancia N client come processi figli
//!
//! usare std::process::Command per avviare istanze multiple di geo-client in modalità --headless

use clap::Parser;
use std::process::{Command, Stdio};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fleet", about = "Avvia una flotta di N client headless")]
struct Args {
    /// numero di veicoli da avviare
    #[arg(short, long, default_value_t = 15)]
    n: usize,

    /// indirizzo del server
    #[arg(long, default_value = "127.0.0.1:7878")]
    server: String,

    /// prefisso per gli username (es: "camion" -> "camion_1", "camion_2")
    #[arg(long, default_value = "camion")]
    prefix: String,

    /// password uguale per tutti i client simulati
    #[arg(long, default_value = "123")]
    password: String,

    /// percorso del file traccia (opzionale)
    #[arg(long)]
    track: Option<PathBuf>,

    /// fattore di accelerazione del tempo
    #[arg(long, default_value_t = 1)]
    time_scale: u32,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut children = Vec::new();

    println!("Avvio di {} client in modalità headless...", args.n);

    for i in 1..=args.n {
        let username = format!("{}_{}", args.prefix, i);

        // usare cargo run per lanciare il binario geo-client
        let mut cmd = Command::new("cargo");
        cmd.arg("run")
            .arg("--bin")
            .arg("geo-client")
            .arg("--") // delimitatore per passare gli argomenti al programma
            .arg("--server").arg(&args.server)
            .arg("--username").arg(&username)
            .arg("--password").arg(&args.password)
            .arg("--time-scale").arg(args.time_scale.to_string())
            .arg("--headless") // obbligatorio per evitare sovrapposizioni delle TUI
            .arg("--register"); // serve per autogenerare gli utenti nel DB!

        if let Some(track) = &args.track {
            cmd.arg("--track").arg(track);
        }

        // sopprime stdout/stderr per non inondare il terminale principale
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        match cmd.spawn() {
            Ok(child) => {
                println!("Avviato client {} (PID: {}).", username, child.id());
                children.push(child);
            }
            Err(e) => {
                eprintln!("Errore nell'avvio del client {}: {}.", username, e);
            }
        }
    }

    println!("Flotta avviata. In attesa della terminazione dei processi (premi Ctrl+C per forzare).");

    // attendere gli exit code dei processi figli
    for mut child in children {
        match child.wait() {
            Ok(status) => println!("Processo {} terminato con exit code: {}.", child.id(), status),
            Err(e) => eprintln!("Errore nell'attesa del processo {}: {}.", child.id(), e),
        }
    }

    Ok(())
}