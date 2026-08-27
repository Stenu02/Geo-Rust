//! Supervisore: lancia il server come processo figlio, ne monitora la terminazione
//! e lo riavvia se esce in modo anomalo.
//!
//! E' un bin dentro geo-server e non un crate a se' perche' gli serve lo stesso
//! codice #[cfg] di cpu_log per leggere i tempi CPU del figlio.
//!
//! Nota multipiattaforma: terminare un figlio funziona diversamente su Unix
//! (SIGTERM) e su Windows. Gestite entrambi.
//!

fn main() -> anyhow::Result<()> {
    todo!("da implementare")
}
