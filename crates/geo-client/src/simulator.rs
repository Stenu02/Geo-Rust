// simulazione movimento veicoli
use geo_core::Position;
use std::path::PathBuf;

// strategie per la simulazione
#[derive(Debug, Clone)]
pub enum Strategy {
    // 1. lettura file con coordinate e tempi
    File { path: PathBuf },
    // 2. punto di partenza e punto di arrivo, con eventuali pause
    Interpolated {
        from: Position,
        to: Position,
        duration_secs: i64,
        pauses: Vec<(i64, i64)>,
    },
    // 3. inserimento manuale delle coordinate
    Manual,
    // 4. generatore pseudo-casuale di posizioni
    Random { origin: Position, radius_km: f64 },
}

// legge file della traccia (formato MM:SS;lat;lon)
pub fn parse_track(contents: &str) -> anyhow::Result<Vec<(i64, Position)>> {
    let mut track = Vec::new();

    for (line_id, line) in contents.lines().enumerate() {
        let line = line.trim();
        // se la riga è vuota, saltala
        if line.is_empty() { continue; }

        let parts: Vec<&str> = line.split(';').collect();
        if parts.len() != 3 {
            return Err(anyhow::anyhow!("Riga {}: trovati {} campi al posto di 3.", line_id + 1, parts.len()));
        }

        // parsing del timestamp MM:SS in secondi totali
        let time_str = parts[0].trim();
        let time_parts: Vec<&str> = time_str.split(':').collect();
        if time_parts.len() != 2 {
            return Err(anyhow::anyhow!("Riga {}: formato timestamp non valido {}, atteso MM:SS", line_id + 1, time_str));
        }
        let minutes: i64 = time_parts[0].trim().parse()?;
        let seconds: i64 = time_parts[1].trim().parse()?;
        let tot_seconds = minutes * 60 + seconds;

        // parsing della latitudine e longitudine sostituendo la virgola con il punto
        let lat_str = parts[1].trim().replace(',', ".");
        let lon_str = parts[2].trim().replace(',', ".");
        let lat: f64 = lat_str.parse()?;
        let lon: f64 = lon_str.parse()?;

        let pos = Position::new(lat, lon);
        if !pos.is_valid() {
            return Err(anyhow::anyhow!("Riga {}: coordinate ({},{}) fuori range", line_id + 1, lat, lon));
        }

        track.push((tot_seconds, pos));
    }
    Ok(track)
}