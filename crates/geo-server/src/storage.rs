//! Persistenza (R15). Attore proprietario dell'unica connessione SQLite.
//!
//! PERCHE' UN ATTORE: rusqlite e' SINCRONO. Chiamarlo da un task tokio blocca il
//! thread del runtime e con esso tutti gli altri task assegnati a quel thread.
//! Un solo task possiede la connessione e riceve comandi via mpsc; chi vuole
//! leggere o scrivere allega un canale oneshot per la risposta.
//!
//!   [sessione 1] --+
//!   [sessione 2] --+--> mpsc --> [storage actor] --> SQLite
//!   [evaluator]  --+                   |
//!                                      +--> oneshot --> risposta
//!
//! ATTACCO: P2, fase F2. E' molto probabile che ve lo chiedano all'orale.

pub struct StorageHandle;

impl StorageHandle {
    pub fn spawn(_db_path: &std::path::Path) -> anyhow::Result<Self> {
        todo!("F2 — P2")
    }
}
