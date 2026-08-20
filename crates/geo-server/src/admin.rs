//! Gestione delle interrogazioni dell'amministratore (R12).
//!
//! Riceve `AdminMessage::Query`, estrae intervalli e posizioni dal database,
//! chiama `geo_core::analytics::analyze` e rispedisce un `MovementReport` pronto.
//!
//! IL CALCOLO AVVIENE QUI, non nel client admin: il professore e' stato esplicito
//! ("Devi rendere disponibile una applicazione lato server che fa interrogazioni").
//!
//! ATTACCO: P2 + P4 insieme, fase F5.

pub struct AdminHandler;
