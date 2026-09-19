//! Punto di ingresso del client camionista
//! Collega il simulatore di movimento al layer di rete, garantendo l'invio
//! delle posizioni a intervalli regolari senza bloccare l'interfaccia

use clap::Parser;
use geo_client::simulator::{PositionSimulator, Strategy};
use geo_core::Position;
use geo_net::connection::{ClientHandle, Command, ConnectOptions, ReconnectPolicy};
use geo_protocol::Role;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Parser)]
#[command(name = "geo-client", about = "Client camionista")]
struct Args {
    #[arg(long, env = "GEO_SERVER", default_value = "127.0.0.1:7878")]
    server: String,

    #[arg(long, env = "GEO_USER")]
    username: String,

    /// Nascosta da --help: la password si digita al prompt. L'argomento resta
    /// per gli script, che la passano dall'ambiente con GEO_PASS.
    #[arg(long, env = "GEO_PASS", hide = true)]
    password: Option<String>,

    /// Permette di creare l'utente al volo se il DB è vuoto
    #[arg(long, default_value_t = false)]
    register: bool,

    /// Percorso file traccia (attiva lettura file)
    #[arg(long, env = "GEO_TRACK")]
    track: Option<PathBuf>,

    /// Partenza e arrivo (attiva interpolazione)
    #[arg(long)]
    interpolated: Option<String>,

    /// Genera posizioni entro un raggio dall'origine (es: "45.0,7.0;5.0")
    #[arg(long)]
    random: Option<String>,

    /// Modifica il fattore temporale per la demo (default 1 = tempi reali)
    #[arg(long, default_value_t = 1)]
    time_scale: u32,

    /// Disabilita la TUI per test di carico e riduzione overhead
    #[arg(long, default_value_t = false)]
    headless: bool,
}

/// Helper per il parsing delle coordinate dalle stringhe CLI per Random e Interpolated
fn parse_pos(s: &str) -> anyhow::Result<Position> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 2 {
        anyhow::bail!("Formato coordinata errato. Usa: lat,lon.");
    }
    let lat: f64 = parts[0].trim().parse()?;
    let lon: f64 = parts[1].trim().parse()?;
    Ok(Position::new(lat, lon))
}

struct LocalTimer;

impl tracing_subscriber::fmt::time::FormatTime for LocalTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z"))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_timer(LocalTimer).init();
    let mut args = Args::parse();

    // Se non arriva dall'ambiente la si chiede adesso, senza mostrarla.
    let password = match args.password.take() {
        Some(p) => p,
        None => chiedi_password(args.register)?,
    };

    // Il simulatore viene configurato in base ai parametri CLI
    // Se non c'è nulla, ricade su Manual
    let strategy = if let Some(path) = args.track {
        Strategy::File { path }
    } else if let Some(interp_str) = args.interpolated {
        let parts: Vec<&str> = interp_str.split(';').collect();
        if parts.len() != 2 {
            anyhow::bail!("Formato interpolated errato. Usa: lat1,lon1;lat2,lon2.");
        }
        Strategy::Interpolated {
            from: parse_pos(parts[0])?,
            to: parse_pos(parts[1])?,
            duration_secs: 600,
            pauses: vec![],
        }
    } else if let Some(rnd_str) = args.random {
        let parts: Vec<&str> = rnd_str.split(';').collect();
        if parts.len() != 2 {
            anyhow::bail!("Formato random errato. Usa: lat,lon;raggio_km.");
        }
        Strategy::Random {
            origin: parse_pos(parts[0])?,
            radius_km: parts[1].trim().parse()?,
        }
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

    // Delega a geo-net la gestione della rete e dei retry
    let opts = ConnectOptions {
        server_addr: args.server.clone(),
        username: args.username.clone(),
        password,
        role: Role::Trucker,
        register: args.register,
    };
    let handle = ClientHandle::connect(opts, ReconnectPolicy::default()).await?;
    tracing::info!("Connessione al server stabilita.");

    // Viene usato tokio::time::interval per evitare l'accumulo di deriva temporale
    let scale = args.time_scale.max(1);
    let interval_duration = Duration::from_secs(30) / scale;
    let mut ticker = tokio::time::interval(interval_duration);
    let mut elapsed_secs = 0i64;

    let command_tx = handle.command_tx.clone();
    let event_rx = handle.event_rx;

    // Canali per comunicare con la UI senza bloccare il runtime async
    let (pos_tx, pos_rx) = watch::channel(None);
    let (manual_pos_tx, mut manual_pos_rx) = tokio::sync::mpsc::channel::<Position>(10);
    let is_manual = matches!(strategy, Strategy::Manual);

    // Task asincrono dedicato all'avanzamento del simulatore e all'invio rete
    tokio::spawn(async move {
        loop {
            tokio::select! {
                // Scatta il timer di invio
                _ = ticker.tick() => {
                    let pos = simulator.next_position(elapsed_secs);

                    if args.headless {
                        tracing::info!(elapsed_secs, lat = pos.lat, lon = pos.lon, "Invio posizione.");
                    }

                    let _ = pos_tx.send(Some(pos));

                    if command_tx.send(Command::PositionUpdate { position: pos }).await.is_err() {
                        break;
                    }
                    elapsed_secs += 30;
                }

                // Intercetta le coordinate inserite a mano dalla TUI
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
        #[cfg(feature = "tui")]
        {
            use geo_client::tui::ClientUi;
            let mut ui = ClientUi::new();
            if let Err(e) = ui
                .run(handle.command_tx.clone(), manual_pos_tx, event_rx, pos_rx)
                .await
            {
                tracing::error!("Errore nell'esecuzione della TUI: {}.", e);
            }
        }

        #[cfg(not(feature = "tui"))]
        {
            tracing::warn!(
                "Il client è stato compilato senza la feature 'tui'. Esecuzione terminata."
            );
        }
    }

    Ok(())
}

/// Legge la password senza mostrarla a schermo.
///
/// In registrazione la fa ripetere: un refuso creerebbe un account con una
/// password che non si conosce, e lo si scoprirebbe solo al collegamento
/// successivo.
fn chiedi_password(registrazione: bool) -> anyhow::Result<String> {
    let password = rpassword::prompt_password("Password: ")?;
    if registrazione && password != rpassword::prompt_password("Ripeti la password: ")? {
        anyhow::bail!("le due password non coincidono");
    }
    Ok(password)
}
