//! simulazione movimento veicoli

use geo_core::Position;
use std::path::PathBuf;
use tokio::time::Interval;
use rand::Rng;

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

/// legge file della traccia (formato MM:SS;lat;lon)
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

pub fn random_generator(origin: Position, radius_km: f64, points_to_generate: i64, interval_sec: i64) -> anyhow::Result<Vec<(i64, Position)>> {
    let mut rng = rand::thread_rng();
    let distance_km: f64 = rng.gen_range(0.0..radius_km);
    let angle_rad: f64 = rng.gen_range(0.0..(2.0 * std::f64::consts::PI));

    todo!()
}

pub fn offset_position(origin: Position, distance_km: f64, angle_rad: f64) -> Position {
    let earth_radius: f64 = 6371.0;

    let delta_lat = (distance_km * angle_rad.cos()) / earth_radius  * (180.0/std::f64::consts::PI);
    let delta_lon = (distance_km * angle_rad.sin()) / (earth_radius  * origin.lat.to_radians().cos()) * (180.0/std::f64::consts::PI);

    let new_lat = origin.lat + delta_lat;
    let new_lon = origin.lon + delta_lon;

    Position::new(new_lat, new_lon)
}

/// test
#[cfg(test)]
mod tests {
    use super::*;

    /// caso base
    #[test]
    fn test_track_parser_comma_decimal() {
        let strings = "00:00;45,0618513;7,6797161\n00:30;45,0601242;7,6851330";
        let track = parse_track(strings).expect("Il parsing doveva avvenire");

        // controllo lunghezza
        assert_eq!(track.len(), 2);

        // controllo contenuto
        assert_eq!(track[0].0, 0);
        assert_eq!(track[0].1, Position::new(45.0618513, 7.6797161));
    }

    /// caso riga con numero di campi errato
    #[test]
    fn test_track_wrong_fields() {
        let strings = "00:00;45,0618513"; // manca il campo della longitudine
        let result = parse_track(strings);

        // controllo che restituisca errore
        assert!(result.is_err());
    }

    /// caso coordinate fuori range
    #[test]
    fn test_track_out_of_range_coordinate() {
        let strings = "00:00;95,0;7,6797161"; // 95 gradi non è una latitudine valida
        let result = parse_track(strings);

        // controllo che restituisca errore
        assert!(result.is_err());
    }

    #[test]
    fn test_offset_position_north() {
        let origin = Position::new(45.0, 7.0);
        let result = offset_position(origin, 10.0, 0.0);
        assert_eq!(result.lon, origin.lon);
        assert!(result.lat > origin.lat);
    }
}