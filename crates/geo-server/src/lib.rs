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
    pub time_scale: u32,
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
        // Il controllo periodico degli stati vive per tutta la durata del server
        // e si ferma con lo stesso segnale di chiusura.
        let valutatore = evaluator::spawn(
            self.registry.clone(),
            self.storage.clone(),
            self.shutdown.clone(),
        );

        // Registrazione periodica del tempo di CPU. Il primo campione viene
        // scritto subito: e' il consumo a riposo, prima che arrivi qualcuno.
        let misuratore = cpu_log::spawn(
            self.config.log_path.clone(),
            cpu_log::INTERVALLO,
            self.registry.clone(),
            self.shutdown.clone(),
        );

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
        let _ = misuratore.await;

        Ok(())
    }
}
