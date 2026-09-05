//! Generatore flotta
//! Lancia client multipli come processi figli tramite std::process::Command
//! Utilizzato per i test di carico e l'acquisizione delle metriche CPU

use clap::Parser;
use std::process::{Command, Stdio};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fleet", about = "Avvia una flotta di N client headless")]
struct Args {
    /// Numero di veicoli da avviare
    #[arg(short, long, default_value_t = 15)]
    n: usize,

    /// Indirizzo del server
    #[arg(long, default_value = "127.0.0.1:7878")]
    server: String,

    /// Prefisso per gli username (es: "camion" -> "camion_1", "camion_2")
    #[arg(long, default_value = "camion")]
    prefix: String,

    /// Password identica per tutti i client simulati
    #[arg(long, default_value = "flottaprova")]
    password: String,

    /// Percorso del file traccia (opzionale)
    #[arg(long)]
    track: Option<PathBuf>,

    /// Fattore di accelerazione del tempo
    #[arg(long, default_value_t = 1)]
    time_scale: u32,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut children = Vec::new();

    println!("Avvio di {} client in modalità headless.", args.n);

    // Spawn iterativo di processi figli isolati
    for i in 1..=args.n {
        let username = format!("{}_{}", args.prefix, i);

        let mut cmd = Command::new("cargo");
        cmd.arg("run")
            .arg("--bin")
            .arg("geo-client")
            .arg("--")
            .arg("--server").arg(&args.server)
            .arg("--username").arg(&username)
            .arg("--time-scale").arg(args.time_scale.to_string())
            .arg("--headless") // più TUI in raw mode sullo stesso terminale si scontrerebbero
            .arg("--register") // auto-popola sqlite senza step preliminari
            // Dall'ambiente e non fra gli argomenti: cosi' la password non
            // compare in `ps` una volta per ogni figlio avviato.
            .env("GEO_PASS", &args.password);

        if let Some(track) = &args.track {
            cmd.arg("--track").arg(track);
        }

        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        match cmd.spawn() {
            Ok(child) => {
                println!("Avviato client {} (PID: {}).", username, child.id());
                children.push(child);
            }
            Err(e) => eprintln!("Errore nell'avvio del client {}: {}.", username, e),
        }
    }

    println!("Flotta avviata. In attesa della terminazione dei processi (premere Ctrl+C per forzare).");

    // Attesa esplicita degli exit code dei figli
    for mut child in children {
        match child.wait() {
            Ok(status) => println!("Processo {} terminato con exit code: {}.", child.id(), status),
            Err(e) => eprintln!("Errore nell'attesa del processo {}: {}.", child.id(), e),
        }
    }

    Ok(())
}