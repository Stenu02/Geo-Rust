//! Lettura del tempo di CPU consumato dal processo.
//!
//! E' l'unico punto del server che cambia a seconda del sistema operativo:
//! tenerlo isolato qui evita che i `#[cfg]` si spargano altrove.

use std::time::Duration;

/// Tempo di CPU del processo su Windows, utente piu' kernel.
#[cfg(windows)]
pub fn get_cpu_time() -> anyhow::Result<Duration> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    use windows_sys::Win32::Foundation::FILETIME;

    // Servono tutti e quattro anche se ne useremo solo due: la funzione li
    // vuole tutti come parametri in uscita.
    let mut creation_time = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let mut exit_time = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let mut kernel_time = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let mut user_time = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };

    // Chiamata al sistema: scrive dentro le quattro strutture qui sopra e
    // restituisce zero in caso di errore.
    unsafe {
        let process = GetCurrentProcess();
        if GetProcessTimes(process, &mut creation_time, &mut exit_time, &mut kernel_time, &mut user_time) == 0 {
            anyhow::bail!("GetProcessTimes fallito");
        }
    }

    // Ogni tempo arriva spezzato in due meta' da 32 bit, da ricomporre.
    let k_time = ((kernel_time.dwHighDateTime as u64) << 32) | (kernel_time.dwLowDateTime as u64);
    let u_time = ((user_time.dwHighDateTime as u64) << 32) | (user_time.dwLowDateTime as u64);

    // FILETIME rappresenta il numero di intervalli da 100 nanosecondi.
    let total_100ns = k_time + u_time;
    Ok(Duration::from_nanos(total_100ns * 100))
}

/// Tempo di CPU del processo su Unix, utente piu' sistema.
#[cfg(unix)]
pub fn get_cpu_time() -> anyhow::Result<Duration> {
    use libc::{getrusage, rusage, RUSAGE_SELF};
    use std::mem::MaybeUninit;

    // La struttura la riempie il sistema, quindi si parte da memoria non
    // inizializzata invece di azzerarla per niente.
    let mut usage = MaybeUninit::<rusage>::uninit();
    if unsafe { getrusage(RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        anyhow::bail!("getrusage fallito");
    }

    // Sicuro: getrusage ha restituito zero, quindi ha scritto la struttura.
    let usage = unsafe { usage.assume_init() };

    // `ru_utime` e' il tempo speso a eseguire il nostro codice, `ru_stime`
    // quello speso dentro il kernel per conto nostro. Entrambi arrivano
    // divisi in secondi e microsecondi.
    let u_sec = usage.ru_utime.tv_sec as u64;
    let u_usec = usage.ru_utime.tv_usec as u64;
    let s_sec = usage.ru_stime.tv_sec as u64;
    let s_usec = usage.ru_stime.tv_usec as u64;

    let total_sec = u_sec + s_sec;
    let total_usec = u_usec + s_usec;

    Ok(Duration::from_secs(total_sec) + Duration::from_micros(total_usec))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Il tempo di CPU e' cumulativo: non puo' mai diminuire.
    #[test]
    fn test_cpu_campionatura_monotona() {
        let t1 = get_cpu_time().unwrap();
        // Ciclo a vuoto per consumare CPU misurabile fra i due campioni.
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < 50 {
            std::hint::black_box(1 + 1);
        }
        let t2 = get_cpu_time().unwrap();
        assert!(t2 >= t1, "Il tempo di CPU deve essere monotono: t2={:?} < t1={:?}", t2, t1);
    }
}
