//! Valutazione periodica degli stati: e' QUI che i 3 minuti di R7 scattano davvero.
//!
//! Chiama `geo_core::StateMachine::on_tick` per ogni utente connesso a intervalli
//! regolari (10 secondi e' un buon compromesso: abbastanza fine da non ritardare
//! visibilmente la transizione, abbastanza rado da non pesare).
//!
//! Usare `tokio::time::interval`, NON `sleep` in ciclo: sleep accumula deriva.
//!
//! ATTACCO: P2, fase F3.

pub struct Evaluator;
