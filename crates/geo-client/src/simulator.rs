/// simulazione movimento veicoli
use geo_core::Position;
use std::path::PathBuf;

/// strategie per la simulazione
#[derive(Debug, Clone)]
pub enum Strategy {
    /// 1. lettura file con coordinate e tempi
    File { path: PathBuf },
    /// 2. punto di partenza e punto di arrivo, con eventuali pause
    Interpolated {
        from: Position,
        to: Position,
        duration_secs: i64,
        pauses: Vec<(i64, i64)>,
    },
    /// 3. inserimento manuale delle coordinate
    Manual,
    /// 4. generatore pseudo-casuale di posizioni
    Random { origin: Position, radius_km: f64 },
}

/// legge file della traccia
pub fn parse_track(_contents: &str) -> anyhow::Result<Vec<(i64, Position)>> {
    todo!("F3")
}