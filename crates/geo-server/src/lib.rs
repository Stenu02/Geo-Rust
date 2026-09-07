//! Il server: apre la porta di ascolto e accetta le connessioni.
//!
//! Possiede tre risorse condivise e ne consegna una copia a ogni sessione:
//! `storage` per parlare con il database, `registry` per sapere chi e'
//! collegato, `shutdown` per il segnale di chiusura.

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

/// Schema delle tabelle, incorporato nell'eseguibile a tempo di compilazione.
pub const SCHEMA: &str = include_str!("schema.sql");

/// Impostazioni con cui il server viene avviato.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub db_path: std::path::PathBuf,
    pub log_path: std::path::PathBuf,
}

pub struct Server {
    /// Socket su cui arrivano le connessioni.
    listener: TcpListener,
    config: ServerConfig,
    /// Coda verso l'unico task che tocca il database.
    storage: StorageHandle,
    /// Chi e' collegato adesso, condiviso da tutte le sessioni.
    registry: Registry,
    /// Cancellandolo si chiedono la chiusura a tutti i task insieme.
    shutdown: CancellationToken,
}

impl Server {
    /// Apre la porta di ascolto e prepara le risorse condivise.
    pub async fn bind(config: ServerConfig) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(config.bind).await?;

        // L'attore del database parte prima di accettare connessioni: le
        // sessioni devono trovarlo gia' pronto.
        let storage = StorageHandle::spawn(&config.db_path)?;

        // Se l'esecuzione precedente e' finita male restano intervalli di stato
        // senza fine. Vanno chiusi adesso, prima che arrivi qualcuno.
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

    /// Indirizzo effettivo: con la porta 0 il sistema ne assegna una libera.
    pub fn local_addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    /// Copia del segnale di chiusura, per chi deve poter fermare il server.
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
        // Controllo periodico degli stati: vive quanto il server.
        let valutatore = evaluator::spawn(
            self.registry.clone(),
            self.storage.clone(),
            self.shutdown.clone(),
        );

        let shutdown_clone = self.shutdown.clone();
        let log_path = self.config.log_path.clone();

        // Registrazione del tempo di CPU, un campione ogni due minuti.
        let cpu_logger = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(120));

            // Intestazione solo alla creazione: riavviando il server si
            // continua ad accodare sul file esistente.
            if !log_path.exists() {
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                    use std::io::Write;
                    let _ = writeln!(f, "timestamp,cpu_time_ms");
                }
            }

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Il file si riapre a ogni campione, cosi' resta
                        // leggibile mentre il server e' in funzione.
                        if let Ok(cpu_dur) = cpu_log::get_cpu_time() {
                            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                                use std::io::Write;
                                let now = chrono::Utc::now().to_rfc3339();
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
                // Chiusura richiesta: si smette di accettare.
                _ = self.shutdown.cancelled() => {
                    tracing::info!("chiusura richiesta");
                    break;
                }

                accepted = self.listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        // Ogni connessione diventa un task a se': una sessione
                        // lenta non rallenta le altre.
                        let shutdown = self.shutdown.clone();
                        let storage = self.storage.clone();
                        let registry = self.registry.clone();
                        tokio::spawn(async move {
                            if let Err(e) = session::handle(stream, shutdown, storage, registry).await {
                                tracing::warn!(%peer, error = %e, "sessione terminata con errore");
                            }
                        });
                    }
                    // Un accept fallito non e' motivo per chiudere il server.
                    Err(e) => tracing::warn!(error = %e, "accept fallito"),
                }
            }
        }

        // Si aspettano i due task periodici: una scrittura in corso arriva a
        // termine prima che il processo esca.
        let _ = valutatore.await;
        let _ = cpu_logger.await;

        Ok(())
    }
}
