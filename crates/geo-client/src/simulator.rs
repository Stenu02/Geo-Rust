//! Simulazione del movimento dei veicoli
//!
//! Supporta quattro strategie di generazione:
//! file di traccia, interpolazione, posizione manuale e movimento casuale.

use geo_core::geo_math::EARTH_RADIUS_KM;
use geo_core::Position;
use rand::Rng;
use std::path::PathBuf;

/// Strategia usata per generare le posizioni del veicolo
#[derive(Debug, Clone)]
pub enum Strategy {
    /// Legge una traccia dal file nel formato MM:SS;lat;lon
    File { path: PathBuf },

    /// Interpola linearmente tra due posizioni per una durata prefissata,
    /// sospendendo il movimento durante gli intervalli indicati
    Interpolated {
        from: Position,
        to: Position,
        duration_secs: i64,
        pauses: Vec<(i64, i64)>,
    },

    /// Mantiene la posizione impostata manualmente dalla TUI
    Manual,

    /// Genera posizioni casuali entro un raggio dall'origine
    Random {
        origin: Position,
        radius_km: f64,
    },
}

/// Esegue il parsing di una traccia nel formato MM:SS;lat;lon
pub fn parse_track(contents: &str) -> anyhow::Result<Vec<(i64, Position)>> {
    let mut track = Vec::new();

    for (line_index, line) in contents.lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        let mut fields = line.split(';');

        let time_str = fields
            .next().ok_or_else(|| anyhow::anyhow!("Timestamp mancante."))?;

        let lat_str = fields
            .next().ok_or_else(|| anyhow::anyhow!("Latitudine mancante."))?;

        let lon_str = fields
            .next().ok_or_else(|| anyhow::anyhow!("Longitudine mancante."))?;

        if fields.next().is_some() {
            anyhow::bail!("Riga {line_number}: attesi 3 campi separati da ';'");
        }

        let (minutes_str, seconds_str) = time_str
            .split_once(':')
            .ok_or_else(|| {
                anyhow::anyhow!("Riga {line_number}: formato timestamp non valido '{time_str}', atteso MM:SS.")
            })?;

        let minutes: i64 = minutes_str.trim().parse().map_err(|_| {
            anyhow::anyhow!("Riga {line_number}: minuti non validi.")
        })?;

        let seconds: i64 = seconds_str.trim().parse().map_err(|_| {
            anyhow::anyhow!("Riga {line_number}: secondi non validi.")
        })?;

        let total_seconds = minutes * 60 + seconds;

        // Supporta sia il formato decimale con virgola sia quello con punto
        let lat: f64 = lat_str.trim().replace(',', ".").parse().map_err(|_| {
            anyhow::anyhow!("Riga {line_number}: latitudine non valida.")
        })?;

        let lon: f64 = lon_str.trim().replace(',', ".").parse().map_err(|_| {
            anyhow::anyhow!("Riga {line_number}: longitudine non valida.")
        })?;

        let position = Position::new(lat, lon);

        if !position.is_valid() {
            anyhow::bail!("Riga {line_number}: coordinate ({lat},{lon}) fuori range.");
        }

        track.push((total_seconds, position));
    }

    Ok(track)
}

/// Genera una traccia pseudo-casuale entro radius_km dall'origine
/// I punti vengono distanziati temporalmente di interval_sec secondi
pub fn random_generator(origin: Position, radius_km: f64, points_to_generate: usize, interval_sec: i64) -> Vec<(i64, Position)> {
    let mut rng = rand::thread_rng();
    let mut points = Vec::with_capacity(points_to_generate);

    for index in 0..points_to_generate {
        let timestamp = index as i64 * interval_sec;
        let distance_km = rng.gen_range(0.0..=radius_km);
        let angle_rad = rng.gen_range(0.0..(2.0 * std::f64::consts::PI));

        let position = offset_position(origin, distance_km, angle_rad);
        points.push((timestamp, position));
    }

    points
}

/// Calcola la posizione ottenuta spostandosi di una distanza e un angolo
/// rispetto alla posizione di origine
pub fn offset_position(origin: Position, distance_km: f64, angle_rad: f64) -> Position {
    let radians_to_degrees = 180.0 / std::f64::consts::PI;

    let delta_lat =
        (distance_km * angle_rad.cos()) / EARTH_RADIUS_KM * radians_to_degrees;

    let delta_lon =
        (distance_km * angle_rad.sin())
            / (EARTH_RADIUS_KM * origin.lat.to_radians().cos())
            * radians_to_degrees;

    Position::new(origin.lat + delta_lat, origin.lon + delta_lon)
}

/// Gestisce la generazione della posizione corrente del client
/// Il simulatore mantiene internamente lo stato della posizione e non
/// accede direttamente alla persistenza
pub struct PositionSimulator {
    strategy: Strategy,
    track_points: Vec<(i64, Position)>,
    file_index: usize,
    current_pos: Position,
}

impl PositionSimulator {
    /// Inizializza il simulatore e prepara le tracce necessarie
    pub fn new(strategy: Strategy) -> anyhow::Result<Self> {
        let (track_points, current_pos) = match &strategy {
            Strategy::File { path } => {
                let content = std::fs::read_to_string(path)?;
                let points = parse_track(&content)?;

                let initial_position = points
                    .first()
                    .map(|(_, position)| *position)
                    .unwrap_or(Position::new(0.0, 0.0));

                (points, initial_position)
            }

            Strategy::Interpolated { from, .. } => {
                (Vec::new(), *from)
            }

            Strategy::Manual => {
                // Posizione iniziale usata finché la TUI non riceve /pos
                (Vec::new(), Position::new(45.0703, 7.6869))
            }

            Strategy::Random {
                origin,
                radius_km,
            } => {
                if !origin.is_valid() {
                    anyhow::bail!("Origine della strategia random non valida.");
                }

                if *radius_km < 0.0 {
                    anyhow::bail!("Il raggio della strategia random non può essere negativo.");
                }

                let points = random_generator(*origin, *radius_km, 100, 30);
                (points, *origin)
            }
        };

        Ok(Self {
            strategy,
            track_points,
            file_index: 0,
            current_pos,
        })
    }

    /// Restituisce la posizione corrispondente al tempo simulato indicato
    pub fn next_position(&mut self, elapsed_secs: i64) -> Position {
        match &self.strategy {
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

            Strategy::Interpolated {
                from,
                to,
                duration_secs,
                pauses,
            } => {
                if *duration_secs <= 0 {
                    self.current_pos = *to;
                    return self.current_pos;
                }

                // Durante una pausa la posizione resta invariata
                let is_paused = pauses
                    .iter()
                    .any(|&(start, end)| elapsed_secs >= start && elapsed_secs < end);

                if is_paused {
                    return self.current_pos;
                }

                // Le pause già concluse non devono far avanzare il percorso
                let paused_so_far: i64 = pauses
                    .iter()
                    .filter(|&&(_, end)| end <= elapsed_secs)
                    .map(|&(start, end)| end - start)
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

    /// Imposta manualmente la posizione corrente del veicolo
    pub fn set_manual_position(&mut self, position: Position) {
        self.current_pos = position;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_core::geo_math::haversine_km;

    #[test]
    fn test_track_parser_comma_decimal() {
        let contents =
            "00:00;45,0618513;7,6797161\n00:30;45,0601242;7,6851330";

        let track = parse_track(contents).expect("Il parsing doveva avvenire.");

        assert_eq!(track.len(), 2);
        assert_eq!(track[0].0, 0);
        assert_eq!(
            track[0].1,
            Position::new(45.0618513, 7.6797161)
        );
    }

    #[test]
    fn test_track_wrong_fields() {
        assert!(parse_track("00:00;45,0618513").is_err());
    }

    #[test]
    fn test_track_out_of_range_coordinate() {
        assert!(parse_track("00:00;95,0;7,6797161").is_err());
    }

    #[test]
    fn test_random_generator_points_count() {
        let origin = Position::new(45.0, 7.0);
        let points = random_generator(origin, 5.0, 10, 30);

        assert_eq!(points.len(), 10);
        assert_eq!(points[0].0, 0);
        assert_eq!(points[1].0, 30);
    }

    #[test]
    fn test_random_generator_stays_within_radius() {
        let origin = Position::new(45.0, 7.0);
        let radius_km = 5.0;
        let points = random_generator(origin, radius_km, 50, 30);

        for (_, position) in &points {
            let distance = haversine_km(&origin, position);

            assert!(
                distance <= radius_km,
                "Punto a {distance} km, oltre il raggio di {radius_km} km."
            );
        }
    }

    #[test]
    fn test_random_generator_interval() {
        let origin = Position::new(45.0, 7.0);
        let points = random_generator(origin, 5.0, 3, 30);

        assert_eq!(points[0].0, 0);
        assert_eq!(points[1].0, 30);
        assert_eq!(points[2].0, 60);
    }

    #[test]
    fn test_offset_position_north() {
        let origin = Position::new(45.0, 7.0);
        let result = offset_position(origin, 10.0, 0.0);

        assert_eq!(result.lon, origin.lon);
        assert!(result.lat > origin.lat);
    }

    #[test]
    fn test_simulator_interpolated_midpoint() {
        let from = Position::new(45.0, 7.0);
        let to = Position::new(46.0, 7.0);

        let strategy = Strategy::Interpolated {
            from,
            to,
            duration_secs: 100,
            pauses: vec![],
        };

        let mut simulator = PositionSimulator::new(strategy).unwrap();
        let position = simulator.next_position(50);

        assert!((position.lat - 45.5).abs() < f64::EPSILON);
        assert!((position.lon - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_simulator_interpolated_reaches_destination() {
        let from = Position::new(45.0, 7.0);
        let to = Position::new(46.0, 7.0);

        let strategy = Strategy::Interpolated {
            from,
            to,
            duration_secs: 100,
            pauses: vec![],
        };

        let mut simulator = PositionSimulator::new(strategy).unwrap();

        assert_eq!(simulator.next_position(100), to);
        assert_eq!(simulator.next_position(200), to);
    }

    #[test]
    fn test_simulator_interpolated_with_pauses() {
        let from = Position::new(45.0, 7.0);
        let to = Position::new(46.0, 7.0);

        let strategy = Strategy::Interpolated {
            from,
            to,
            duration_secs: 100,
            pauses: vec![(30, 90)],
        };

        let mut simulator = PositionSimulator::new(strategy).unwrap();

        assert_eq!(simulator.next_position(0), from);

        let position_before_pause = simulator.next_position(30);
        let position_during_pause = simulator.next_position(60);

        assert_eq!(position_before_pause, position_during_pause);
    }

    #[test]
    fn test_random_rejects_negative_radius() {
        let strategy = Strategy::Random {
            origin: Position::new(45.0, 7.0),
            radius_km: -1.0,
        };
        assert!(PositionSimulator::new(strategy).is_err());
    }

    #[test]
    fn test_random_rejects_invalid_origin() {
        let strategy = Strategy::Random {
            origin: Position::new(200.0, 7.0),
            radius_km: 5.0,
        };
        assert!(PositionSimulator::new(strategy).is_err());
    }

    /// All'istante esatto in cui una pausa finisce, il movimento è già ripreso
   
    #[test]
    fn test_simulator_interpolated_resumes_exactly_at_pause_end() {
        let from = Position::new(45.0, 7.0);
        let to = Position::new(46.0, 7.0);

        let strategy = Strategy::Interpolated {
            from,
            to,
            duration_secs: 100,
            pauses: vec![(30, 90)],
        };

        let mut simulator = PositionSimulator::new(strategy).unwrap();

        let during_pause = simulator.next_position(89);
        let at_pause_end = simulator.next_position(90);

        assert_ne!(
            during_pause, at_pause_end,
            "alla fine esatta della pausa il movimento deve essere già ripreso"
        );

        // A t=90, il tempo effettivo è 90 - 60 (durata della pausa) = 30s -> ratio 0.3
        let expected_lat = from.lat + (to.lat - from.lat) * 0.3;
        assert!((at_pause_end.lat - expected_lat).abs() < 1e-9);
    }

    #[test]
    fn test_simulator_interpolated_no_jump_after_pause() {
        let from = Position::new(45.0, 7.0);
        let to = Position::new(46.0, 7.0);

        let strategy = Strategy::Interpolated {
            from,
            to,
            duration_secs: 100,
            pauses: vec![(30, 90)],
        };

        let mut simulator = PositionSimulator::new(strategy).unwrap();

        let position_before_pause = simulator.next_position(29);
        let position_after_pause = simulator.next_position(91);

        let delta_lat =
            (position_after_pause.lat - position_before_pause.lat).abs();

        assert!(
            delta_lat < 0.05,
            "Salto troppo grande dopo la pausa: {delta_lat}."
        );
    }
}