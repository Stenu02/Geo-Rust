//! Controllo periodico degli stati.
//!
//! Il passaggio a FERMO non puo' scattare quando arriva una posizione: un
//! camionista che smette di trasmettere non manda piu' niente. Serve qualcuno
//! che guardi l'orologio a intervalli regolari, e quel qualcuno e' questo task.

use crate::registry::Registry;
use crate::storage::StorageHandle;
use chrono::Utc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Ogni quanto si rivalutano gli stati.
///
/// Non cambia la precisione della transizione, che porta comunque la data in
/// cui la soglia e' scaduta: decide solo con quanto ritardo la si scopre.
const TICK: Duration = Duration::from_secs(10);

/// Avvia il controllo periodico e restituisce il suo task.
pub fn spawn(
    registry: Registry,
    storage: StorageHandle,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // `interval` e non `sleep`: sleep conta il tempo dopo il lavoro, quindi
        // ogni giro accumulerebbe la durata dell'elaborazione.
        let mut tick = tokio::time::interval(TICK);

        loop {
            tokio::select! {
                // Chiusura del server: si esce dal ciclo.
                _ = shutdown.cancelled() => break,

                _ = tick.tick() => {
                    // Un solo istante per tutti, cosi' le transizioni di questo
                    // giro portano la stessa data.
                    let ora = Utc::now();

                    // Il registry restituisce le transizioni a lucchetto gia'
                    // rilasciato: le scritture avvengono qui fuori perche'
                    // dentro un lucchetto non si puo' aspettare.
                    for (user, t) in registry.tick_all(ora) {
                        tracing::info!(%user, da = ?t.from, a = ?t.to, "transizione per inattivita'");

                        // Un errore di scrittura non deve fermare il ciclo:
                        // gli altri camionisti vanno valutati comunque.
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
