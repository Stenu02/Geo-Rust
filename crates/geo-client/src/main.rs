use clap::Parser;
use geo_client::simulator::{PositionSimulator, Strategy};
use std::path::PathBuf;
use std::time::Duration;
use geo_net::{connection::{ClientHandle, Command}, events::Event};
use tokio::sync::watch;

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

    // connessione al server tramite geo-net
    let mut handle = ClientHandle::connect(&args.server, args.username, args.password).await?;
    tracing::info!("Connessione al server stabilita.");

    let scale = args.time_scale.max(1);
    let interval_duration = Duration::from_secs(30) / scale;
    let mut ticker = tokio::time::interval(interval_duration);
    let mut elapsed_secs = 0i64;

    // estrae i canali per la rete
    let command_tx = handle.command_tx.clone();
    let event_rx = handle.event_rx;

    // canale per inviare l'aggiornamento locale delle coordinate alla UI
    let (pos_tx, pos_rx) = watch::channel(None);

    // invio periodico delle posizioni, gira in background indipendentemente dalla UI
    tokio::spawn(async move {
        loop {
            ticker.tick().await;
            let pos = simulator.next_position(elapsed_secs);

            // in headless logga l'invio
            if args.headless {
                tracing::info!(elapsed_secs, lat = pos.lat, lon = pos.lon, "Invio posizione.");
            }

            // comunica la nuova posizione alla UI
            let _ = pos_tx.send(Some(pos));

            if command_tx.send(Command::PositionUpdate { position: pos }).await.is_err() {
                // se il canale è chiuso, il task viene fermato
                break;
            }
            elapsed_secs += 30;
        }
    });

    if args.headless {
        // resta in ascolto degli eventi di rete e li logga su terminale
        let mut event_rx = event_rx; // riprende possesso del receiver
        loop {
            if let Some(ev) = event_rx.recv().await {
                tracing::info!("Evento di rete: {:?}.", ev);
            } else {
                tracing::error!("Canale eventi chiuso, uscita.");
                break;
            }
        }
    } else {
        // avvia l'interfaccia grafica
        #[cfg(feature = "tui")]
        {
            use geo_client::tui::ClientUi;
            let mut ui = ClientUi::new();

            // per permettere alla TUI di inviare comandi, utilizza un clone del sender
            if let Err(e) = ui.run(handle.command_tx.clone(), event_rx, pos_rx).await {
                tracing::error!("Errore nell'esecuzione della TUI: {}.", e);
            }
        }

        #[cfg(not(feature = "tui"))]
        {
            tracing::warn!("Il client è stato compilato senza la feature 'tui'. Esecuzione terminata.");
        }
    }

    Ok(())
}