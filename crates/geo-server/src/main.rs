//! Punto di ingresso: parsing degli argomenti e avvio. Nient'altro.
//! La logica sta in lib.rs, per poterla testare senza lanciare un processo.

use clap::Parser;
use geo_server::{Server, ServerConfig};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "geo-server", about = "Server GeoRuggine")]
struct Args {
    #[arg(long, env = "GEO_BIND", default_value = "0.0.0.0:7878")]
    bind: SocketAddr,

    /// Percorso del database. Mai relativo implicito: romperebbe R16.
    #[arg(long, env = "GEO_DB", default_value = "georuggine.db")]
    db: PathBuf,

    #[arg(long, env = "GEO_LOG", default_value = "cpu_usage.csv")]
    log: PathBuf,

    /// Accelerazione temporale per la demo. Default 1 = tempi reali di specifica.
    #[arg(long, default_value_t = 1)]
    time_scale: u32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let server = Server::bind(ServerConfig {
        bind: args.bind,
        db_path: args.db,
        log_path: args.log,
        time_scale: args.time_scale,
    })
    .await?;

    server.run().await
}
