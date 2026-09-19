//! Punto di ingresso: legge gli argomenti e avvia. Nient'altro.
//!
//! La logica sta in lib.rs, cosi' i test possono avviare un server senza
//! lanciare un processo.

use clap::Parser;
use geo_server::{auth, storage::StorageHandle, Server, ServerConfig};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(name = "geo-server", about = "Server GeoRuggine")]
struct Args {
    /// Indirizzo e porta su cui mettersi in ascolto.
    #[arg(long, env = "GEO_BIND", default_value = "0.0.0.0:7878")]
    bind: SocketAddr,

    /// Percorso del database. Mai relativo implicito: si romperebbe cambiando
    /// directory di lavoro o piattaforma.
    #[arg(long, env = "GEO_DB", default_value = "georuggine.db")]
    db: PathBuf,

    /// File CSV su cui scrivere il tempo di CPU consumato.
    #[arg(long, env = "GEO_LOG", default_value = "cpu_usage.csv")]
    log: PathBuf,

    /// Crea un account amministratore e termina, senza avviare il server.
    ///
    /// I camionisti si registrano da soli dalla rete, l'amministratore no: lo
    /// crea chi gestisce la macchina del server. Se la registrazione via rete
    /// potesse assegnare quel ruolo, basterebbe un client per ottenerlo.
    #[arg(long, value_name = "USERNAME")]
    create_admin: Option<String>,

    /// Password del nuovo amministratore.
    ///
    /// Nascosta da --help: si digita al prompt. Resta accettata per gli script,
    /// ma sulla riga di comando finirebbe nel titolo della finestra, nella
    /// cronologia della shell e nell'elenco dei processi.
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
    tracing_subscriber::fmt().with_timer(LocalTimer).init();
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

    // Il segnale va agganciato prima di `run`, che consuma il server.
    ascolta_interruzione(server.shutdown_handle());

    server.run().await
}

/// Trasforma Ctrl-C in una richiesta di chiusura ordinata.
///
/// `ctrl_c` copre sia Unix sia Windows, quindi non serve codice separato per
/// piattaforma.
fn ascolta_interruzione(chiusura: CancellationToken) {
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "impossibile ascoltare Ctrl-C");
            return;
        }

        // Non interrompe niente: cancella il token, e il ciclo di accettazione
        // esce da solo dopo aver chiuso i lavori periodici.
        tracing::info!("Ctrl-C ricevuto: chiusura ordinata, premere ancora per uscire subito");
        chiusura.cancel();

        // Installato il gestore, i segnali successivi non terminano piu' il
        // processo da soli: senza questa via d'uscita una chiusura che si
        // impuntasse lascerebbe il terminale senza modo di fermare il server.
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::warn!("secondo Ctrl-C: uscita immediata");
            // 128 piu' il numero del segnale, come vuole la convenzione.
            std::process::exit(130);
        }
    });
}

/// Crea un account con ruolo amministratore e termina.
///
/// Riusa le stesse funzioni della registrazione di un camionista: stessa
/// impronta Argon2, stesso inserimento, stesso rifiuto dei nomi gia' presi.
/// Cambia solo il ruolo scritto nella tabella.
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
            // Conferma: una password invisibile e sbagliata si scoprirebbe solo
            // al primo accesso mancato.
            if scelta != rpassword::prompt_password("Ripeti la password: ")? {
                anyhow::bail!("le due password non coincidono");
            }
            scelta
        }
    };

    // Le stesse regole della registrazione dalla rete.
    auth::valida_password(&password).map_err(|e| anyhow::anyhow!(e))?;

    let storage = StorageHandle::spawn(db)?;
    let phc = auth::hash_password_async(password).await?;
    let user = storage.create_user(username, &phc, "admin").await?;

    println!("amministratore '{username}' creato (id {}).", user.0);
    Ok(())
}
