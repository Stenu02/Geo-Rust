//! Punto di ingresso: parsing degli argomenti e avvio. Nient'altro.
//! La logica sta in lib.rs, per poterla testare senza lanciare un processo.

use clap::Parser;
use geo_server::{auth, storage::StorageHandle, Server, ServerConfig};
use std::net::SocketAddr;
use std::path::PathBuf;

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

    /// Accelerazione temporale per la demo. Default 1 = tempi reali di specifica.
    #[arg(long, default_value_t = 1)]
    time_scale: u32,

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
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
        time_scale: args.time_scale,
    })
    .await?;

    server.run().await
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
    let password = password.ok_or_else(|| {
        anyhow::anyhow!("manca la password: impostare la variabile d'ambiente GEO_ADMIN_PASSWORD")
    })?;

    if password.len() < 8 {
        anyhow::bail!("la password deve essere di almeno 8 caratteri");
    }

    let storage = StorageHandle::spawn(db)?;
    let phc = auth::hash_password_async(password).await?;
    let user = storage.create_user(username, &phc, "admin").await?;

    println!("amministratore '{username}' creato (id {}).", user.0);
    Ok(())
}
