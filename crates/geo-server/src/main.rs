//! Punto di ingresso: parsing degli argomenti e avvio. Nient'altro.
//! La logica sta in lib.rs, per poterla testare senza lanciare un processo.

use clap::Parser;
use geo_server::{auth, storage::StorageHandle, Server, ServerConfig};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(name = "geo-server", about = "Server GeoRuggine")]
struct Args {
    #[arg(long, env = "GEO_BIND", default_value = "0.0.0.0:7878")]
    bind: SocketAddr,

    /// Percorso del database. Mai relativo implicito: si romperebbe cambiando
    /// directory di lavoro o piattaforma.
    #[arg(long, env = "GEO_DB", default_value = "georuggine.db")]
    db: PathBuf,

    #[arg(long, env = "GEO_LOG", default_value = "cpu_usage.csv")]
    log: PathBuf,

    /// Crea un account amministratore e termina, senza avviare il server.
    ///
    /// I camionisti si registrano da soli attraverso il protocollo;
    /// l'amministratore no, lo crea chi gestisce la macchina del server.
    /// Sono due entita' distinte anche nel modo in cui nascono: se la
    /// registrazione via rete potesse assegnare il ruolo di amministratore,
    /// chiunque abbia un client potrebbe diventarlo.
    #[arg(long, value_name = "USERNAME")]
    create_admin: Option<String>,

    /// Password del nuovo amministratore.
    ///
    /// Si passa dall'ambiente e non come argomento perche' gli argomenti della
    /// riga di comando sono visibili a chiunque sulla macchina con un `ps`.
    #[arg(long, env = "GEO_ADMIN_PASSWORD", hide = true)]
    admin_password: Option<String>,
}

struct LocalTimer;

impl tracing_subscriber::fmt::time::FormatTime for LocalTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z"))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_timer(LocalTimer)
        .init();
    let args = Args::parse();

    // Modalita' "crea amministratore": fa una cosa sola e termina, senza mai
    // aprire la porta di ascolto.
    if let Some(username) = args.create_admin {
        return crea_amministratore(&args.db, &username, args.admin_password).await;
    }

    let server = Server::bind(ServerConfig {
        bind: args.bind,
        db_path: args.db,
        log_path: args.log,
    })
    .await?;

    // Il segnale va agganciato PRIMA di `run`, che consuma il server.
    ascolta_interruzione(server.shutdown_handle());

    server.run().await
}


fn ascolta_interruzione(chiusura: CancellationToken) {
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "impossibile ascoltare Ctrl-C");
            return;
        }
        tracing::info!("Ctrl-C ricevuto: chiusura ordinata, premere ancora per uscire subito");
        chiusura.cancel();

        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::warn!("secondo Ctrl-C: uscita immediata");
            std::process::exit(130);
        }
    });
}

/// Crea un account con ruolo amministratore e termina.
///
/// Riusa le stesse funzioni della registrazione di un camionista: stesso
/// hashing Argon2 con salt, stesso inserimento, stesso rifiuto dei nomi gia'
/// presi imposto dall'indice UNIQUE. L'unica differenza e' il ruolo scritto
/// nella tabella.
async fn crea_amministratore(
    db: &std::path::Path,
    username: &str,
    password: Option<String>,
) -> anyhow::Result<()> {
    // Da script arriva dall'ambiente, a mano si digita adesso senza mostrarla.
    let password = match password {
        Some(dall_ambiente) => dall_ambiente,
        None => {
            let scelta = rpassword::prompt_password("Password del nuovo amministratore: ")?;
            if scelta != rpassword::prompt_password("Ripeti la password: ")? {
                anyhow::bail!("le due password non coincidono");
            }
            scelta
        }
    };

    // Le stesse regole della registrazione dalla rete: contano i caratteri,
    // non i byte.
    auth::valida_password(&password).map_err(|e| anyhow::anyhow!(e))?;

    let storage = StorageHandle::spawn(db)?;
    let phc = auth::hash_password_async(password).await?;
    let user = storage.create_user(username, &phc, "admin").await?;

    println!("amministratore '{username}' creato (id {}).", user.0);
    Ok(())
}
