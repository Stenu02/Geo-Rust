//! Registrazione periodica del tempo di CPU consumato dal server.
//!
//! Il file prodotto e' un CSV: una riga per campione, intestazione in cima.
//! Ogni riga riporta i totali dall'avvio, quanto e' stato consumato NELL'ULTIMO
//! INTERVALLO, e quanti camionisti erano collegati in quel momento.
//!
//! I valori cumulativi da soli direbbero poco: sapere che il server ha usato
//! 40 secondi di CPU da quando e' partito non dice se adesso sia carico o no.
//! Il delta e la percentuale sull'intervallo rispondono a quella domanda, e il
//! numero di collegati permette di correlare consumo e carico.
//!
//! E' l'unico punto del progetto con codice dipendente dal sistema operativo:
//! tenerlo isolato qui evita che i `#[cfg]` si spargano altrove.

use crate::registry::Registry;
use chrono::Utc;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Ogni quanto si campiona. Valore fissato dalla specifica.
pub const INTERVALLO: Duration = Duration::from_secs(120);

/// Tempi di CPU consumati dal processo dall'avvio.
///
/// `user` e' il tempo speso a eseguire il nostro codice, `system` quello speso
/// dentro il kernel per conto nostro — letture, scritture, rete. Distinguerli
/// e' utile: un server di rete che consuma quasi tutto in `system` sta facendo
/// molto I/O, uno che consuma in `user` sta calcolando.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CpuTimes {
    pub user_ms: u64,
    pub system_ms: u64,
    /// Memoria massima occupata dall'avvio, in kilobyte.
    pub max_rss_kb: u64,
}

impl CpuTimes {
    pub fn total_ms(&self) -> u64 {
        self.user_ms + self.system_ms
    }
}

/// Legge i tempi di CPU del processo corrente.
#[cfg(unix)]
pub fn cpu_times() -> std::io::Result<CpuTimes> {
    // SICUREZZA: `getrusage` scrive dentro la struttura che gli passiamo e non
    // conserva il puntatore. La struttura e' azzerata prima della chiamata.
    let mut uso: libc::rusage = unsafe { std::mem::zeroed() };
    let esito = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut uso) };
    if esito != 0 {
        return Err(std::io::Error::last_os_error());
    }

    let in_ms = |t: libc::timeval| (t.tv_sec as u64) * 1000 + (t.tv_usec as u64) / 1000;

    // `ru_maxrss` non ha la stessa unita' ovunque: byte su macOS, kilobyte su
    // Linux. E' una differenza reale fra due sistemi entrambi "unix".
    #[cfg(target_os = "macos")]
    let max_rss_kb = (uso.ru_maxrss as u64) / 1024;
    #[cfg(not(target_os = "macos"))]
    let max_rss_kb = uso.ru_maxrss as u64;

    Ok(CpuTimes {
        user_ms: in_ms(uso.ru_utime),
        system_ms: in_ms(uso.ru_stime),
        max_rss_kb,
    })
}

/// Legge i tempi di CPU del processo corrente.
#[cfg(windows)]
pub fn cpu_times() -> std::io::Result<CpuTimes> {
    // Dichiarate a mano invece di aggiungere una libreria: sono due funzioni,
    // e il collegamento a kernel32 avviene da solo.
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        basso: u32,
        alto: u32,
    }

    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessTimes(
            processo: isize,
            creazione: *mut FileTime,
            uscita: *mut FileTime,
            kernel: *mut FileTime,
            utente: *mut FileTime,
        ) -> i32;
    }

    let (mut creazione, mut uscita) = (FileTime::default(), FileTime::default());
    let (mut kernel, mut utente) = (FileTime::default(), FileTime::default());

    // SICUREZZA: le quattro strutture sono nostre e vivono per tutta la
    // chiamata; la funzione si limita a scriverci dentro.
    let esito = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creazione,
            &mut uscita,
            &mut kernel,
            &mut utente,
        )
    };
    if esito == 0 {
        return Err(std::io::Error::last_os_error());
    }

    // Windows conta in unita' da 100 nanosecondi: diecimila per millisecondo.
    let in_ms = |t: &FileTime| (((t.alto as u64) << 32) | t.basso as u64) / 10_000;

    Ok(CpuTimes {
        user_ms: in_ms(&utente),
        system_ms: in_ms(&kernel),
        // GetProcessTimes non riporta la memoria: servirebbe una chiamata
        // separata a un'altra libreria, che qui non vale il costo.
        max_rss_kb: 0,
    })
}

/// Intestazione del file CSV.
const INTESTAZIONE: &str =
    "istante,cpu_utente_ms,cpu_sistema_ms,cpu_totale_ms,delta_utente_ms,delta_sistema_ms,cpu_percento,memoria_kb,collegati\n";

/// Avvia il campionamento periodico e restituisce il suo task.
///
/// Il primo campione viene scritto subito all'avvio: serve da riferimento, ed
/// e' il consumo del server a riposo prima che arrivi qualcuno.
pub fn spawn(
    percorso: PathBuf,
    intervallo: Duration,
    registry: Registry,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut precedente = CpuTimes::default();
        let mut ultimo_istante = Instant::now();
        // Il primo campione arriva subito, quindi il tempo trascorso e' quasi
        // zero e la percentuale che ne uscirebbe non significa niente. Sul
        // primo campione si riportano solo i totali.
        let mut primo = true;

        // `interval` e non `sleep` in ciclo: `sleep` fa passare il tempo DOPO
        // il lavoro, quindi ogni giro accumula la durata della scrittura e dopo
        // qualche ora i campioni non sono piu' a distanza regolare.
        let mut tick = tokio::time::interval(intervallo);

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = tick.tick() => {
                    let adesso = match cpu_times() {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::error!(error = %e, "lettura dei tempi di CPU fallita");
                            continue;
                        }
                    };

                    let trascorso = ultimo_istante.elapsed();
                    ultimo_istante = Instant::now();

                    let delta_utente = adesso.user_ms.saturating_sub(precedente.user_ms);
                    let delta_sistema = adesso.system_ms.saturating_sub(precedente.system_ms);

                    // Quanta CPU e' stata usata rispetto al tempo trascorso.
                    // Puo' superare il 100%: il server ha piu' thread, e due
                    // thread al lavoro per un secondo fanno due secondi di CPU.
                    let percento = if primo || trascorso.as_millis() == 0 {
                        0.0
                    } else {
                        (delta_utente + delta_sistema) as f64 * 100.0
                            / trascorso.as_millis() as f64
                    };
                    let (delta_utente, delta_sistema) = if primo {
                        (0, 0)
                    } else {
                        (delta_utente, delta_sistema)
                    };
                    primo = false;

                    let riga = format!(
                        "{},{},{},{},{},{},{:.2},{},{}\n",
                        Utc::now().to_rfc3339(),
                        adesso.user_ms,
                        adesso.system_ms,
                        adesso.total_ms(),
                        delta_utente,
                        delta_sistema,
                        percento,
                        adesso.max_rss_kb,
                        registry.count(),
                    );

                    if let Err(e) = accoda(&percorso, &riga) {
                        tracing::error!(error = %e, "scrittura del log CPU fallita");
                    }

                    precedente = adesso;
                }
            }
        }

        tracing::info!("registrazione del tempo di CPU terminata");
    })
}

/// Aggiunge una riga al file, scrivendo l'intestazione se il file non esiste.
///
/// Il file viene aperto e chiuso a ogni campione invece di tenere una maniglia
/// aperta: un campione ogni due minuti non giustifica una risorsa permanente, e
/// cosi' il contenuto e' leggibile con `tail -f` mentre il server gira.
fn accoda(percorso: &Path, riga: &str) -> std::io::Result<()> {
    let nuovo = !percorso.exists();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(percorso)?;
    if nuovo {
        file.write_all(INTESTAZIONE.as_bytes())?;
    }
    file.write_all(riga.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i_tempi_di_cpu_si_leggono() {
        let t = cpu_times().expect("lettura fallita");
        // Il processo di test ha gia' consumato qualcosa per arrivare fin qui.
        assert!(t.total_ms() > 0, "tempo totale nullo: {t:?}");
    }

    #[test]
    fn i_tempi_di_cpu_non_diminuiscono_mai() {
        let prima = cpu_times().unwrap();
        // Un po' di lavoro vero, per far salire il contatore.
        let mut acc = 0u64;
        for i in 0..8_000_000u64 {
            acc = acc.wrapping_add(i);
        }
        assert!(acc > 0);
        let dopo = cpu_times().unwrap();

        assert!(
            dopo.user_ms >= prima.user_ms,
            "il tempo utente e' diminuito"
        );
        assert!(
            dopo.system_ms >= prima.system_ms,
            "il tempo di sistema e' diminuito"
        );
        assert!(
            dopo.total_ms() > prima.total_ms(),
            "il lavoro svolto non ha fatto salire il contatore"
        );
    }
}
