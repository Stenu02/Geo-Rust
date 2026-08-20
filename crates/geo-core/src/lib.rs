/// Modello del dominio: coordinate, stati, intervalli, analisi.
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

/// Ogni quanto il client invia la propria posizione. Fissato dalla specifica.
pub const POSITION_INTERVAL_SECS: i64 = 30;

/// Dopo quanti secondi a coordinate invariate si passa da IN MOVIMENTO a FERMO.
pub const STOP_THRESHOLD_SECS: i64 = 180;

/// Sotto questa differenza due coordinate sono considerate uguali. 1e-6 gradi valgono circa 11 cm.
pub const COORD_EPSILON_DEG: f64 = 1e-6;

///Funzioni di conversione
pub fn position_interval() -> Duration {
    Duration::seconds(POSITION_INTERVAL_SECS)
}

pub fn stop_threshold() -> Duration {
    Duration::seconds(STOP_THRESHOLD_SECS)
}
