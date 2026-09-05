//! Dominio di GeoRuggine: cosa significano i dati e quali regole li governano.
//!
//! REGOLA DI CONFINE (vale per ogni file di questo crate):
//! nessuna funzione qui dentro legge l'orologio, apre un socket o tocca il filesystem.
//! Il tempo arriva SEMPRE come parametro.
//!
//! Conseguenza pratica: un viaggio di quattro ore si puo' simulare in due millisecondi,
//! quindi la regola dei 3 minuti (R7) è testabile senza aspettare 3 minuti.
//!
//! Requisiti coperti: R5, R6, R7, R8, R9, R10, R11.

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

/// Intervallo di trasmissione della posizione (R4). Valore fissato dalla specifica.
pub const POSITION_INTERVAL_SECS: i64 = 30;

/// Tempo di coordinate invariate dopo il quale IN MOVIMENTO -> FERMO (R7).
pub const STOP_THRESHOLD_SECS: i64 = 180;

/// Tolleranza nel confronto fra due coordinate.
///
/// Confrontare `f64` con `==` è un metodo fragile. In questo caso
///si opta di considerare equivalenti due coordinate se distano meno di 1e-6 gradi 
/// i quali valgono circa 11 cm:
/// è una distanza abbastanza fine da non mascherare un movimento reale, 
/// abbastanza grosso da assorbire il rumore di rappresentazione.
pub const COORD_EPSILON_DEG: f64 = 1e-6;

/// Fuso orario aziendale. "Giorno / settimana / mese corrente" (R11) sono concetti
/// LOCALI: `TimeRange::resolve` trova la mezzanotte / l'inizio settimana / l'inizio
/// mese locali in questo fuso e riconverte gli estremi in UTC.
///
/// Fissato esplicitamente invece di leggere il fuso della macchina via `chrono::Local`:
/// cosi' `resolve` resta una funzione pura di `now` (la regola di confine del crate),
/// i test di R11 sono deterministici su qualunque host (CI inclusa) e il "giorno" e'
/// quello dell'azienda, non di dove gira il server.
pub const COMPANY_TZ: chrono_tz::Tz = chrono_tz::Europe::Rome;

pub fn position_interval() -> Duration {
    Duration::seconds(POSITION_INTERVAL_SECS)
}

pub fn stop_threshold() -> Duration {
    Duration::seconds(STOP_THRESHOLD_SECS)
}
