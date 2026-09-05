//! Monitoraggio del tempo di esecuzione CPU.
//!
//! Fornisce funzioni multipiattaforma per misurare il tempo di CPU consumato
//! dal processo corrente.

use std::time::Duration;

#[cfg(windows)]
/// Calcola il tempo di CPU consumato dal processo corrente (implementazione Windows).
pub fn get_cpu_time() -> anyhow::Result<Duration> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    use windows_sys::Win32::Foundation::FILETIME;

    let mut creation_time = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let mut exit_time = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let mut kernel_time = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let mut user_time = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };

    unsafe {
        let process = GetCurrentProcess();
        if GetProcessTimes(process, &mut creation_time, &mut exit_time, &mut kernel_time, &mut user_time) == 0 {
            anyhow::bail!("GetProcessTimes fallito");
        }
    }

    let k_time = ((kernel_time.dwHighDateTime as u64) << 32) | (kernel_time.dwLowDateTime as u64);
    let u_time = ((user_time.dwHighDateTime as u64) << 32) | (user_time.dwLowDateTime as u64);
    
    // FILETIME rappresenta il numero di intervalli da 100 nanosecondi.
    let total_100ns = k_time + u_time;
    Ok(Duration::from_nanos(total_100ns * 100))
}

#[cfg(unix)]
/// Calcola il tempo di CPU consumato dal processo corrente (implementazione Unix).
pub fn get_cpu_time() -> anyhow::Result<Duration> {
    use libc::{getrusage, rusage, RUSAGE_SELF};
    use std::mem::MaybeUninit;

    let mut usage = MaybeUninit::<rusage>::uninit();
    if unsafe { getrusage(RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        anyhow::bail!("getrusage fallito");
    }
    
    let usage = unsafe { usage.assume_init() };
    
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

    #[test]
    fn test_cpu_campionatura_monotona() {
        let t1 = get_cpu_time().unwrap();
        // Spin lock per consumare CPU
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < 50 {
            std::hint::black_box(1 + 1);
        }
        let t2 = get_cpu_time().unwrap();
        assert!(t2 >= t1, "Il tempo di CPU deve essere monotono: t2={:?} < t1={:?}", t2, t1);
    }
}

