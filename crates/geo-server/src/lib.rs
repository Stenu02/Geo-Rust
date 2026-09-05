//! Il server: apertura della porta di ascolto e accettazione delle connessioni.

//! Possiede le tre risorse condivise e ne consegna una copia a ogni sessione:
//!   storage    coda verso l'unico proprietario della connessione al database
//!   registry   mappa di chi è collegato in questo momento
//!   shutdown   segnale di chiusura,

pub mod admin;
pub mod auth;
pub mod cpu_log;
pub mod evaluator;
pub mod registry;
pub mod session;
pub mod storage;

use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::registry::Registry;
use crate::storage::StorageHandle;

/// Schema incorporato nel binario
pub const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, Clone)]

///Contenitore di impostazioni di configurazioni
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub db_path: std::path::PathBuf,
    pub log_path: std::path::PathBuf,
}

pub struct Server {
    ///Porta di acolto
    listener: TcpListener,
    config: ServerConfig,
    /// Attore proprietario del database
    storage: StorageHandle,
    /// Chi e' collegato adesso. Anche questo condiviso da tutte le sessioni.
    registry: Registry,
    /// Segnale di chiusura
    shutdown: CancellationToken,
}

impl Server {
    /// Apre il socket di ascolto.
    pub async fn bind(config: ServerConfig) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(config.bind).await?;
        // Avvia l'attore storage prima di accettare connessioni.
        let storage = StorageHandle::spawn(&config.db_path)?;

        // Se l'esecuzione precedente e' finita male, sul database restano
        // intervalli senza fine. E' necessario chiuderli prima di accettare connessioni.
        let pendenti = storage.close_dangling(chrono::Utc::now()).await?;
        if pendenti > 0 {
            tracing::warn!(
                pendenti,
                "chiusi intervalli rimasti aperti da un'esecuzione precedente"
            );
        }
        Ok(Self {
            listener,
            config,
            storage,
            registry: Registry::new(),
            shutdown: CancellationToken::new(),
        })
    }

    pub fn local_addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    pub fn shutdown_handle(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    /// L'attore del database, per chi deve preparare dati prima dell'avvio.
    pub fn storage(&self) -> &StorageHandle {
        &self.storage
    }

    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Ciclo di accettazione, fino alla richiesta di chiusura.
    pub async fn run(self) -> anyhow::Result<()> {
        let valutatore = evaluator::spawn(
            self.registry.clone(),
            self.storage.clone(),
            self.shutdown.clone(),
        );

        let shutdown_clone = self.shutdown.clone();
        let log_path = self.config.log_path.clone();
        let cpu_logger = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(120));
            // First tick fires immediately, so let's skip the write on the very first tick if we want,
            // or just write it. Let's write the cumulative CPU time every 2 minutes.
            // Creiamo il file log e l'header se non esiste
            if !log_path.exists() {
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                    use std::io::Write;
                    let _ = writeln!(f, "timestamp,cpu_time_ms");
                }
            }

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Ok(cpu_dur) = cpu_log::get_cpu_time() {
                            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                                use std::io::Write;
                                let now = chrono::Local::now().to_rfc3339();
                                let _ = writeln!(f, "{},{}", now, cpu_dur.as_millis());
                            }
                        }
                    }
                    _ = shutdown_clone.cancelled() => {
                        break;
                    }
                }
            }
        });

        tracing::info!(addr = %self.local_addr()?, "server in ascolto");

        loop {
            tokio::select! {
                //Segnale di chiusura
                _ = self.shutdown.cancelled() => {
                    tracing::info!("chiusura richiesta");
                    break;
                }
                //Nuova connessione da accettare
                accepted = self.listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        let shutdown = self.shutdown.clone();
                        let storage = self.storage.clone();
                        let registry = self.registry.clone();
                        tokio::spawn(async move {
                            if let Err(e) = session::handle(stream, shutdown, storage, registry).await {
                                tracing::warn!(%peer, error = %e, "sessione terminata con errore");
                            }
                        });
                    }
                    Err(e) => tracing::warn!(error = %e, "accept fallito"),
                }
            }
        }

        // Si aspetta che il controllo periodico chiuda, cosi' un'eventuale
        // scrittura in corso arriva a termine prima che il processo esca.
        let _ = valutatore.await;
        let _ = cpu_logger.await;

        Ok(())
    }
}
