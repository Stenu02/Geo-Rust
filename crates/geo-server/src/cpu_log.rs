//! Log del tempo di CPU ogni 2 minuti (R17).
//!
//! La specifica dice "i DETTAGLI sul tempo di CPU": il solo totale cumulativo e'
//! povero. Formato consigliato:
//!
//!   timestamp_iso, cpu_user_ms, cpu_system_ms, cpu_total_ms,
//!   delta_user_ms, delta_system_ms, cpu_percent_intervallo,
//!   num_thread, rss_kb, client_connessi
//!
//! Il DELTA e la PERCENTUALE sull'intervallo sono cio' che rende il log informativo:
//! dicono quanto ha consumato il server IN QUEI DUE MINUTI, non da quando e' partito.
//! `client_connessi` permette di correlare consumo e carico, e rende molto piu'
//! solida la sezione prestazioni del report (R18).
//!
//! ATTACCO: P2, fase F8.

/// Campione di utilizzo CPU del processo corrente.
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuTimes {
    pub user_ms: u64,
    pub system_ms: u64,
}

/// Unica funzione platform-specific del progetto (R16).
/// Tenerla isolata qui evita che i `#[cfg]` si spargano nel codice.
pub fn cpu_times() -> std::io::Result<CpuTimes> {
    #[cfg(unix)]
    {
        // getrusage(RUSAGE_SELF)
        todo!("F8 — P2 (Unix)")
    }
    #[cfg(windows)]
    {
        // GetProcessTimes
        todo!("F8 — P2 (Windows)")
    }
}
