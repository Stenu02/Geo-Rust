use clap::Parser;
use geo_client::simulator::{PositionSimulator, Strategy};
use std::path::PathBuf;
use std::time::Duration;
use geo_core::Position;
use geo_net::connection::{ClientHandle, Command, ConnectOptions, ReconnectPolicy};
use geo_protocol::Role;
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

    /// esegue la registrazione prima del login (risolve il blocco sollevato dal revisore)
    #[arg(long, default_value_t = false)]
    register: bool,

    // strategie di simulazione
    /// 1. percorso del file traccia (se usato attiva Strategy::File)
    #[arg(long, env = "GEO_TRACK")]
    track: Option<PathBuf>,

    /// 2. attiva Strategy::Interpolated (es: "45.0,7.0;46.0,7.0")
    #[arg(long)]
    interpolated: Option<String>,

    /// 4. attiva Strategy::Random (es: "45.0,7.0;5.0" origin_lat,origin_lon;radius_km)
    #[arg(long)]
    random: Option<String>,

    /// fattore di accelerazione del tempo (default 1 = scala reale)
    #[arg(long, default_value_t = 1)]
    time_scale: u32,

    /// disabilita la TUI per test di carico
    #[arg(long, default_value_t = false)]
    headless: bool,
}

/// helper per il parsing delle coordinate dalle stringhe CLI per Random e Interpolated
fn parse_pos(s: &str) -> anyhow::Result<Position> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 2 {
        anyhow::bail!("Formato coordinata errato. Usa: lat,lon");
    }
    let lat: f64 = parts[0].trim().parse()?;
    let lon: f64 = parts[1].trim().parse()?;
    Ok(Position::new(lat, lon))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    // selezione della strategia corretta in base agli argomenti CLI passati
    let strategy = if let Some(path) = args.track {
        Strategy::File { path }
    } else if let Some(interp_str) = args.interpolated {
        let parts: Vec<&str> = interp_str.split(';').collect();
        if parts.len() != 2 {
            anyhow::bail!("Formato interpolated errato. Usa: lat1,lon1;lat2,lon2");
        }
        let from = parse_pos(parts[0])?;
        let to = parse_pos(parts[1])?;
        Strategy::Interpolated { from, to, duration_secs: 600, pauses: vec![] } // durata fissa 10 min per test
    } else if let Some(rnd_str) = args.random {
        let parts: Vec<&str> = rnd_str.split(';').collect();
        if parts.len() != 2 {
            anyhow::bail!("Formato random errato. Usa: lat,lon;raggio_km");
        }
        let origin = parse_pos(parts[0])?;
        let radius_km: f64 = parts[1].trim().parse()?;
        Strategy::Random { origin, radius_km }
    } else {
        Strategy::Manual
    };

    let mut simulator = PositionSimulator::new(strategy.clone())?;

    tracing::info!(
        user = %args.username,
        server = %args.server,
        time_scale = args.time_scale,
        headless = args.headless,
        "Avvio client camionista"
    );

    // usa ConnectOptions di geo-net
    let opts = ConnectOptions {
        server_addr: args.server.clone(),
        username: args.username.clone(),
        password: args.password.clone(),
        role: Role::Trucker,
        register: args.register,
    };

    let handle = ClientHandle::connect(opts, ReconnectPolicy::default()).await?;
    tracing::info!("Connessione al server stabilita.");

    let scale = args.time_scale.max(1);
    let interval_duration = Duration::from_secs(30) / scale;
    let mut ticker = tokio::time::interval(interval_duration);
    let mut elapsed_secs = 0i64;

    let command_tx = handle.command_tx.clone();
    let event_rx = handle.event_rx;

    // canale watch: permette al loop asincrono di aggiornare l'interfaccia
    // con le ultime coordinate calcolate senza bloccarsi
    let (pos_tx, pos_rx) = watch::channel(None);
    let is_manual = matches!(strategy, Strategy::Manual);

    // canale mpsc: cattura l'inserimento manuale delle coordinate dalla TUI
    let (manual_pos_tx, mut manual_pos_rx) = tokio::sync::mpsc::channel::<Position>(10);

    // task in background per l'emissione delle posizioni e l'ascolto di aggiornamenti manuali
    tokio::spawn(async move {
        loop {
            tokio::select! {
                // scatta il timer di 30 secondi
                _ = ticker.tick() => {
                    let pos = simulator.next_position(elapsed_secs);

                    if args.headless {
                        tracing::info!(elapsed_secs, lat = pos.lat, lon = pos.lon, "Invio posizione.");
                    }

                    // comunica la nuova posizione alla UI
                    let _ = pos_tx.send(Some(pos));

                    if command_tx.send(Command::PositionUpdate { position: pos }).await.is_err() {
                        break; // rete irraggiungibile o programma terminato
                    }
                    elapsed_secs += 30;
                }

                // l'utente ha digitato "/pos lat,lon" dalla TUI
                Some(new_pos) = manual_pos_rx.recv() => {
                    if is_manual {
                        simulator.set_manual_position(new_pos);
                    }
                }
            }
        }
    });

    if args.headless {
        let mut event_rx = event_rx;
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

            // per permettere alla TUI di inviare comandi di chat e coordinate manuali, passa entrambi i Sender
            if let Err(e) = ui.run(handle.command_tx.clone(), manual_pos_tx, event_rx, pos_rx).await {
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