//! Dominio di GeoRuggine: cosa significano i dati e quali regole li governano.

pub mod analytics;
pub mod geo_math;
pub mod interval;
pub mod position;
pub mod state_machine;
pub mod timerange;

pub use analytics::MovementReport;
pub use interval::StateInterval;
pub use position::{Position, Timestamped, UserId};
pub use state_machine::{StateMachine, Transition, UserState};
pub use timerange::{RangeSpec, TimeRange};

use chrono::Duration;

/// Intervallo di trasmissione della posizione. Valore fissato dalla specifica.
pub const POSITION_INTERVAL_SECS: i64 = 30;

/// Tempo di coordinate invariate dopo il quale IN MOVIMENTO -> FERMO.
pub const STOP_THRESHOLD_SECS: i64 = 180;

/// Tolleranza nel confronto fra due coordinate.
///
/// Confrontare `f64` con `==` è un metodo fragile. In questo caso
/// si opta per considerare equivalenti due coordinate se distano meno di 1e-6 gradi
/// i quali valgono circa 11 cm:
/// è una distanza abbastanza fine da non mascherare un movimento reale,
/// abbastanza grosso da assorbire il rumore di rappresentazione.
pub const COORD_EPSILON_DEG: f64 = 1e-6;

/// Fuso orario aziendale. "Giorno / settimana / mese corrente" sono concetti
/// locali: `TimeRange::resolve` trova la mezzanotte / l'inizio settimana / l'inizio
/// mese locali in questo fuso e riconverte gli estremi in UTC.
///
/// Fissato esplicitamente invece di leggere il fuso della macchina via `chrono::Local`:
/// cosi' `resolve` resta una funzione pura di `now`, i test sui periodi sono
/// deterministici su qualunque macchina e il "giorno" e'
/// quello dell'azienda, non di dove gira il server.
pub const COMPANY_TZ: chrono_tz::Tz = chrono_tz::Europe::Rome;

pub fn position_interval() -> Duration {
    Duration::seconds(POSITION_INTERVAL_SECS)
}

pub fn stop_threshold() -> Duration {
    Duration::seconds(STOP_THRESHOLD_SECS)
}
