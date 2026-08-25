//! Il controllo periodico degli stati.
//!
//! E' qui che l'inattivita' prolungata diventa davvero un passaggio a FERMO.
//! Non puo' avvenire alla ricezione di una posizione, perche' un camionista che
//! smette di trasmettere non manda piu' niente: senza qualcuno che guardi
//! l'orologio, resterebbe IN MOVIMENTO per sempre.

use crate::registry::Registry;
use crate::storage::StorageHandle;
use chrono::Utc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Ogni quanto si valutano gli stati.
///
/// Dieci secondi: abbastanza fine da non ritardare visibilmente il passaggio a
/// FERMO, abbastanza rado da non pesare con molti collegati.
///
/// Non incide sulla PRECISIONE della transizione: la macchina a stati la data
/// all'istante esatto in cui la soglia e' scaduta, non a quando il tick se n'e'
/// accorto. Il tick decide solo quanto tardi lo si viene a sapere.
const TICK: Duration = Duration::from_secs(10);

/// Avvia il controllo periodico e restituisce il suo task.
pub fn spawn(
    registry: Registry,
    storage: StorageHandle,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // `interval` e non `sleep` dentro un ciclo: `sleep` fa passare il tempo
        // DOPO il lavoro, quindi ogni giro accumula la durata dell'elaborazione
        // e dopo qualche ora i campioni non sono piu' regolari.
        let mut tick = tokio::time::interval(TICK);

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = tick.tick() => {
                    let ora = Utc::now();

                    // Il registry valuta tutti i collegati e restituisce le
                    // transizioni scattate, a lucchetto gia' rilasciato. Le
                    // scritture avvengono qui fuori: dentro un lucchetto non si
                    // puo' aspettare.
                    for (user, t) in registry.tick_all(ora) {
                        tracing::info!(%user, da = ?t.from, a = ?t.to, "transizione per inattivita'");
                        if let Err(e) = storage.apply_transition(user, t.to, t.at).await {
                            tracing::error!(%user, error = %e, "scrittura transizione fallita");
                        }
                    }
                }
            }
        }

        tracing::info!("controllo periodico terminato");
    })
}
