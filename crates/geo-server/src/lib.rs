pub mod admin;
pub mod auth;
pub mod cpu_log;
pub mod evaluator;
pub mod registry;
pub mod session;
pub mod storage;
pub mod stub;

use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::storage::StorageHandle;

/// Schema incorporato nel binario
pub const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub db_path: std::path::PathBuf,
    pub log_path: std::path::PathBuf,
    pub time_scale: u32,
}

pub struct Server {
    listener: TcpListener,
    config: ServerConfig,
    /// Attore proprietario del database, condiviso (clonato) da tutte le sessioni.
    storage: StorageHandle,
    /// Segnale di chiusura
    shutdown: CancellationToken,
}

impl Server {
    /// Apre il socket di ascolto.
    pub async fn bind(config: ServerConfig) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(config.bind).await?;
        // Avvia l'attore storage una volta sola, prima di accettare connessioni.
        let storage = StorageHandle::spawn(&config.db_path)?;
        Ok(Self {
            listener,
            config,
            storage,
            shutdown: CancellationToken::new(),
        })
    }

    pub fn local_addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    pub fn shutdown_handle(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Ciclo di accettazione, fino alla richiesta di chiusura.
    pub async fn run(self) -> anyhow::Result<()> {
        tracing::info!(addr = %self.local_addr()?, "server in ascolto");

        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => {
                    tracing::info!("chiusura richiesta");
                    break;
                }
                accepted = self.listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        let shutdown = self.shutdown.clone();
                        let storage = self.storage.clone();
                        tokio::spawn(async move {
                            if let Err(e) = session::handle(stream, shutdown, storage).await {
                                tracing::warn!(%peer, error = %e, "sessione terminata con errore");
                            }
                        });
                    }
                    Err(e) => tracing::warn!(error = %e, "accept fallito"),
                }
            }
        }

        Ok(())
    }
}
