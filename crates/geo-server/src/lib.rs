

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
    /// Segnale di chiusura
    shutdown: CancellationToken,
}

impl Server {
    pub async fn bind(_config: ServerConfig) -> anyhow::Result<Self> {
        todo!("F2 — P2")
    }

    /// Esegue fino alla cancellazione del token di shutdown.
    pub async fn run(self) -> anyhow::Result<()> {
        todo!("F2 — P2")
    }
}
