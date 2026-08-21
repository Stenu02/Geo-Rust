//! Dati inventati, usati finche' il database e le analisi non esistono.
//!
//! Sta tutto qui apposta: e' l'unico codice destinato a sparire. Man mano che i
//! pezzi veri arrivano questo file si accorcia, e quando e' vuoto si cancella.

use chrono::Utc;
use geo_core::{MovementReport, Position, RangeSpec, TimeRange, Timestamped, UserId, UserState};
use geo_protocol::FleetEntry;

/// L'utente restituito da qualunque registrazione o login.
pub const UTENTE: UserId = UserId(1);

/// L'amministratore restituito da qualunque login amministratore.
pub const AMMINISTRATORE: UserId = UserId(100);

/// Due veicoli, perche' la console amministratore abbia una tabella da disegnare
/// prima che esista il registro delle sessioni connesse.
pub fn flotta() -> Vec<FleetEntry> {
    let ora = Utc::now();
    vec![
        FleetEntry {
            user: UserId(1),
            username: "camion01".into(),
            state: UserState::Moving,
            last_position: Some(Position::new(45.0618513, 7.6797161)),
            last_update: Some(ora),
        },
        FleetEntry {
            user: UserId(2),
            username: "camion02".into(),
            state: UserState::Stopped,
            last_position: Some(Position::new(44.9009, 8.2065)),
            last_update: Some(ora),
        },
    ]
}

/// Un report con numeri plausibili ma inventati.
///
/// Costruito campo per campo e non con `analyze`: quella funzione non e' ancora
/// implementata, e chiamarla adesso terminerebbe la sessione con un panic.
/// `TimeRange::new` invece esiste gia', quindi il periodo e' vero.
pub fn report(user: UserId, spec: RangeSpec) -> MovementReport {
    let fine = Utc::now();
    let inizio = fine - chrono::Duration::hours(8);

    MovementReport {
        user,
        spec,
        range: TimeRange::new(inizio, fine),
        track: vec![
            Timestamped::new(inizio, Position::new(45.0618513, 7.6797161)),
            Timestamped::new(fine, Position::new(44.9009, 8.2065)),
        ],
        distance_km: 45.0,
        avg_speed_moving_kmh: 60.0,
        avg_speed_overall_kmh: 45.0,
        moving_time: chrono::Duration::minutes(45),
        paused_time: chrono::Duration::minutes(15),
        disconnected_time: chrono::Duration::zero(),
        pause_count: 1,
    }
}
