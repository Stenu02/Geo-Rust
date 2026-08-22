//! simulazione del movimento dei veicoli
//! quattro strategie di generazione posizioni

use geo_core::Position;
use geo_core::geo_math::EARTH_RADIUS_KM;
use std::path::PathBuf;
use rand::Rng;

/// strategia di simulazione del movimento scelta per un client
#[derive(Debug, Clone)]
pub enum Strategy {
    /// 1. lettura di un file contenente coordinate e tempi
    File { path: PathBuf },
    /// 2. punto di partenza e di arrivo, con durata ed eventuali pause
    Interpolated {
        from: Position,
        to: Position,
        duration_secs: i64,
        pauses: Vec<(i64, i64)>,
    },
    /// 3. inserimento manuale delle coordinate tramite interfaccia
    Manual,
    /// 4. generatore pseudo-casuale di posizioni entro un raggio da un'origine
    Random { origin: Position, radius_km: f64 },
}

/// esegue il parsing di un file traccia in formato MM:SS;lat;lon
pub fn parse_track(contents: &str) -> anyhow::Result<Vec<(i64, Position)>> {
    let mut track = Vec::new();

    for (line_id, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() { continue; }

        let parts: Vec<&str> = line.split(';').collect();
        if parts.len() != 3 {
            return Err(anyhow::anyhow!("Riga {}: trovati {} campi al posto di 3.", line_id + 1, parts.len()));
        }

        let time_str = parts[0].trim();
        let time_parts: Vec<&str> = time_str.split(':').collect();
        if time_parts.len() != 2 {
            return Err(anyhow::anyhow!("Riga {}: formato timestamp non valido {}, atteso MM:SS", line_id + 1, time_str));
        }
        let minutes: i64 = time_parts[0].trim().parse()?;
        let seconds: i64 = time_parts[1].trim().parse()?;
        let tot_seconds = minutes * 60 + seconds;

        // la virgola decimale va sostituita prima del parsing
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

/// genera posizioni pseudo-casuali (points_to_generate) entro un raggio in km (radius_km)
/// da una posizione d'origine (origin), campionate ogni lasso di tempo predeterminato in secondi (interval_sec)
pub fn random_generator(origin: Position, radius_km: f64, points_to_generate: usize, interval_sec: i64) -> Vec<(i64, Position)> {
    let mut rng = rand::thread_rng();
    let mut points = Vec::with_capacity(points_to_generate);

    for i in 0..points_to_generate {
        let timestamp = (i as i64) * interval_sec;
        let distance_km = rng.gen_range(0.0..=radius_km);
        let angle_rad = rng.gen_range(0.0..(2.0 * std::f64::consts::PI));
        let pos = offset_position(origin, distance_km, angle_rad);
        points.push((timestamp, pos));
    }
    points
}

/// calcola la posizione risultante spostandosi di una determinata distanza in km (distance_km) lungo
/// un angolo in radianti (angle_rad) a partire da una posizione d'origine (origin)
pub fn offset_position(origin: Position, distance_km: f64, angle_rad: f64) -> Position {
    let delta_lat = (distance_km * angle_rad.cos()) / EARTH_RADIUS_KM * (180.0 / std::f64::consts::PI);
    let delta_lon = (distance_km * angle_rad.sin()) / (EARTH_RADIUS_KM * origin.lat.to_radians().cos()) * (180.0 / std::f64::consts::PI);

    Position::new(origin.lat + delta_lat, origin.lon + delta_lon)
}

/// fornisce la posizione corrente del client, secondo la strategia scelta (non tocca SQLite)
pub struct PositionSimulator {
    strategy: Strategy,
    track_points: Vec<(i64, Position)>,
    file_index: usize,
    current_pos: Position,
}

impl PositionSimulator {
    /// inizializza il simulatore, precalcolando i punti se la strategia lo richiede
    pub fn new(strategy: Strategy) -> anyhow::Result<Self> {
        let mut track_points = Vec::new();
        let current_pos = match &strategy {
            Strategy::File { path } => {
                let content = std::fs::read_to_string(path)?;
                track_points = parse_track(&content)?;
                track_points.first().map(|(_, p)| *p).unwrap_or(Position::new(0.0, 0.0))
            }
            Strategy::Interpolated { from, .. } => *from,
            Strategy::Manual => Position::new(45.0703, 7.6869),
            Strategy::Random { origin, radius_km } => {
                track_points = random_generator(*origin, *radius_km, 100, 30);
                *origin
            }
        };
        Ok(Self { strategy, track_points, file_index: 0, current_pos })
    }

    /// restituisce la posizione al secondo simulato (elapsed_secs)
    pub fn next_position(&mut self, elapsed_secs: i64) -> Position {
        match &self.strategy {
            // campionamento sequenziale da punti precalcolati (per strategie 1 e 4)
            Strategy::File { .. } | Strategy::Random { .. } => {
                if self.track_points.is_empty() {
                    return self.current_pos;
                }
                while self.file_index + 1 < self.track_points.len()
                    && self.track_points[self.file_index + 1].0 <= elapsed_secs
                {
                    self.file_index += 1;
                }
                self.current_pos = self.track_points[self.file_index].1;
                self.current_pos
            }
            Strategy::Interpolated { from, to, duration_secs, pauses } => {
                if *duration_secs <= 0 {
                    return *to;
                }

                let is_paused = pauses.iter().any(|&(start, end)| elapsed_secs >= start && elapsed_secs <= end);
                if is_paused {
                    return self.current_pos;
                }

                // il tempo delle pause già concluse va tolto da elapsed_secs,
                // altrimenti la posizione "salta" in avanti appena la pausa
                // finisce invece di riprendere da dove si era fermata
                let paused_so_far: i64 = pauses.iter()
                    .filter(|&&(_, end)| end <= elapsed_secs)
                    .map(|(start, end)| end - start)
                    .sum();
                let effective_secs = elapsed_secs - paused_so_far;

                let progress_ratio = (effective_secs as f64 / *duration_secs as f64).clamp(0.0, 1.0);
                self.current_pos = Position::new(
                    from.lat + (to.lat - from.lat) * progress_ratio,
                    from.lon + (to.lon - from.lon) * progress_ratio,
                );
                self.current_pos
            }
            Strategy::Manual => self.current_pos,
        }
    }

    /// imposta manualmente la posizione corrente (per strategia 3)
    pub fn set_manual_position(&mut self, pos: Position) {
        self.current_pos = pos;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_core::geo_math::haversine_km;

    // metodo parse_track
    /// caso base: virgola decimale e timestamp corretti
    #[test]
    fn test_track_parser_comma_decimal() {
        let strings = "00:00;45,0618513;7,6797161\n00:30;45,0601242;7,6851330";
        let track = parse_track(strings).expect("Il parsing doveva avvenire.");
        assert_eq!(track.len(), 2);
        assert_eq!(track[0].0, 0);
        assert_eq!(track[0].1, Position::new(45.0618513, 7.6797161));
    }

    /// errore atteso: numero di campi sbagliato
    #[test]
    fn test_track_wrong_fields() {
        assert!(parse_track("00:00;45,0618513").is_err());
    }

    /// errore atteso: coordinate fuori range
    #[test]
    fn test_track_out_of_range_coordinate() {
        assert!(parse_track("00:00;95,0;7,6797161").is_err());
    }

    // metodo random_generator
    /// verifica quantità di punti generati e passo temporale fra due punti
    #[test]
    fn test_random_generator_points_count() {
        let origin = Position::new(45.0, 7.0);
        let pts = random_generator(origin, 5.0, 10, 30);
        assert_eq!(pts.len(), 10);
        assert_eq!(pts[0].0, 0);
        assert_eq!(pts[1].0, 30);
    }

    /// verifica che ogni punto generato resti entro il raggio dichiarato
    #[test]
    fn test_random_generator_stays_within_radius() {
        let origin = Position::new(45.0, 7.0);
        let radius_km = 5.0;
        let pts = random_generator(origin, radius_km, 50, 30);

        for (_, pos) in &pts {
            let distance = haversine_km(&origin, pos);
            assert!(distance <= radius_km, "Punto a {distance} km, oltre il raggio di {radius_km} km.");
        }
    }

    /// verifica che l'intervallo di emissione predefinito sia di 30 secondi
    #[test]
    fn test_emission_interval() {
        let origin = Position::new(45.0, 7.0);
        // genera 3 punti con intervallo forzato a 30 secondi
        let pts = random_generator(origin, 5.0, 3, 30);

        assert_eq!(pts[0].0, 0);
        assert_eq!(pts[1].0, 30);
        assert_eq!(pts[2].0, 60);
    }

    // metodo offset_position
    /// spostamento verso nord: deve alterare solo la latitudine
    #[test]
    fn test_offset_position_north() {
        let origin = Position::new(45.0, 7.0);
        let result = offset_position(origin, 10.0, 0.0);
        assert_eq!(result.lon, origin.lon);
        assert!(result.lat > origin.lat);
    }

    // PositionSimulator::next_position
    /// durante una pausa la posizione resta invariata
    #[test]
    fn test_simulator_interpolated_with_pauses() {
        let from = Position::new(45.0, 7.0);
        let to = Position::new(46.0, 7.0);
        let strategy = Strategy::Interpolated { from, to, duration_secs: 100, pauses: vec![(30, 90)] };
        let mut sim = PositionSimulator::new(strategy).unwrap();

        assert_eq!(sim.next_position(0), from);
        let p1 = sim.next_position(30);
        let p2 = sim.next_position(60);
        assert_eq!(p1, p2);
    }

    /// dopo la pausa la posizione riprende gradualmente, senza saltare
    /// in avanti per il tempo passato fermi
    #[test]
    fn test_simulator_interpolated_no_jump_after_pause() {
        let from = Position::new(45.0, 7.0);
        let to = Position::new(46.0, 7.0);
        let strategy = Strategy::Interpolated { from, to, duration_secs: 100, pauses: vec![(30, 90)] };
        let mut sim = PositionSimulator::new(strategy).unwrap();

        let p_before = sim.next_position(29);
        let p_after = sim.next_position(91);
        let delta_lat = (p_after.lat - p_before.lat).abs();
        assert!(delta_lat < 0.05, "Salto troppo grande dopo la pausa: {delta_lat}.");
    }
}