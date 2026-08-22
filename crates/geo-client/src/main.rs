use clap::Parser;
use geo_client::simulator::{PositionSimulator, Strategy};
use std::path::PathBuf;
use std::time::Duration;

/// argomenti da riga di comando per il client del camionista
#[derive(Parser)]
#[command(name = "geo-client", about = "client camionista")]
struct Args {
    #[arg(long, env = "GEO_SERVER", default_value = "127.0.0.1:7878")]
    server: String,

    #[arg(long, env = "GEO_USER")]
    username: String,

    #[arg(long, env = "GEO_PASS")]
    password: String,

    /// percorso del file traccia
    #[arg(long, env = "GEO_TRACK")]
    track: Option<PathBuf>,

    /// fattore di scala temporale (default 1 = 30s)
    #[arg(long, default_value_t = 1)]
    time_scale: u32,

    /// modalità senza interfaccia testuale per test di carico e flotta
    #[arg(long, default_value_t = false)]
    headless: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    // selezione della strategia: da file se fornito, altrimenti manuale
    let strategy = match args.track {
        Some(path) => Strategy::File { path },
        None => Strategy::Manual,
    };
    let mut simulator = PositionSimulator::new(strategy)?;

    tracing::info!(
        user = %args.username,
        server = %args.server,
        time_scale = args.time_scale,
        headless = args.headless,
        "avvio client camionista"
    );

    // intervallo di campionamento scalato per accelerare l'esecuzione nei test
    let scale = args.time_scale.max(1);
    let interval_duration = Duration::from_secs(30) / scale;
    let mut ticker = tokio::time::interval(interval_duration);
    let mut elapsed_secs = 0i64;

    // ciclo temporaneo per verificare la generazione delle coordinate
    for _ in 0..5 {
        ticker.tick().await;
        let pos = simulator.next_position(elapsed_secs);
        tracing::info!(
            elapsed_secs = elapsed_secs,
            lat = pos.lat,
            lon = pos.lon,
            "Campionamento posizione"
        );
        elapsed_secs += 30;
    }
    Ok(())
}