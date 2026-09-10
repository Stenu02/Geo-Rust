use std::cmp::{max, min};
use crate::state_machine::UserState;
use crate::timerange::TimeRange;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Rappresenta un periodo continuo durante il quale un utente è rimasto in un certo stato.
///
/// `end == None` significa "intervallo ancora aperto" (lo stato corrente).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StateInterval {
    pub state: UserState,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
}


impl StateInterval {
    pub fn open(state: UserState, start: DateTime<Utc>) -> Self {
        Self { state, start, end: None }
    }

    pub fn closed(state: UserState, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { state, start, end: Some(end) }
    }

    pub fn is_open(&self) -> bool {
        self.end.is_none()
    }

    /// La fine usata nei calcoli: quella reale, oppure `now` per un intervallo aperto.
    fn effective_end(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        self.end.unwrap_or(now)
    }

    /// Durata dell'intervallo. Per un intervallo ancora aperto serve `now` per
    /// chiuderlo virtualmente: anche qui il tempo arriva come parametro, mai da `Utc::now()`.
    pub fn duration(&self, now: DateTime<Utc>) -> Duration {
        self.effective_end(now) - self.start
    }

    /// Interseca l'intervallo con un periodo, restituendo `None` se non si sovrappongono.
    ///
    /// Per esempio: un utente è in movimento dalle 23:40 alle 00:20.
    /// Senza troncamento, l'analisi del "giorno corrente" conta anche i 20 minuti che
    /// appartengono al giorno precedente.
    ///
    /// Un intervallo tutto dentro il periodo torna intero; uno che sborda da un
    /// lato torna tagliato al confine; uno che copre l'intero periodo torna
    /// ridotto al periodo; uno del tutto esterno non torna affatto.
    pub fn clip_to(&self, range: &TimeRange, now: DateTime<Utc>) -> Option<StateInterval> {
        let clipped_start = max(self.start, range.start);
        let clipped_end = min(self.effective_end(now), range.end);

        if clipped_start < clipped_end {
            return Some(Self::closed(self.state, clipped_start, clipped_end));
        }
        None
    }
}

/// Somma la durata degli intervalli in un dato stato, dentro un periodo.
pub fn total_in(
    intervals: &[StateInterval],
    state: UserState,
    range: &TimeRange,
    now: DateTime<Utc>,
) -> Duration {
    let mut total_duration = Duration::zero();
    for interval in intervals {
        if interval.state == state {
            if let Some(clipped) = interval.clip_to(range, now) {
                total_duration+= clipped.duration(now);
            }
        }
    }
    total_duration
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Un istante UTC su un giorno di riferimento fisso (2026-08-20).
    fn utc(h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 20, h, mi, 0).unwrap()
    }

    fn moving(start: DateTime<Utc>, end: DateTime<Utc>) -> StateInterval {
        StateInterval::closed(UserState::Moving, start, end)
    }

    // Il periodo di analisi usato dai casi di clip: [10:00, 20:00).
    fn day_range() -> TimeRange {
        TimeRange::new(utc(10, 0), utc(20, 0))
    }

    #[test]
    fn test_clip_case_a_inside() {
        let iv = moving(utc(12, 0), utc(15, 0));
        let c = iv.clip_to(&day_range(), utc(23, 0)).unwrap();
        assert_eq!((c.start, c.end), (utc(12, 0), Some(utc(15, 0))));
    }

    #[test]
    fn test_clip_case_b_starts_before() {
        let iv = moving(utc(8, 0), utc(12, 0));
        let c = iv.clip_to(&day_range(), utc(23, 0)).unwrap();
        assert_eq!((c.start, c.end), (utc(10, 0), Some(utc(12, 0))));
    }

    #[test]
    fn test_clip_case_c_ends_after() {
        let iv = moving(utc(18, 0), utc(22, 0));
        let c = iv.clip_to(&day_range(), utc(23, 0)).unwrap();
        assert_eq!((c.start, c.end), (utc(18, 0), Some(utc(20, 0))));
    }

    #[test]
    fn test_clip_case_d_covers_period() {
        let iv = moving(utc(8, 0), utc(23, 0));
        let c = iv.clip_to(&day_range(), utc(23, 0)).unwrap();
        assert_eq!((c.start, c.end), (utc(10, 0), Some(utc(20, 0))));
    }

    #[test]
    fn test_clip_case_e_no_overlap() {
        assert!(moving(utc(21, 0), utc(22, 0)).clip_to(&day_range(), utc(23, 0)).is_none());
        // Toccare esattamente il confine non è una sovrapposizione (semiaperto, `<` stretto).
        assert!(moving(utc(6, 0), utc(10, 0)).clip_to(&day_range(), utc(23, 0)).is_none());
    }

    #[test]
    fn test_interval_clipping_at_midnight() {
        // L'esempio del commento: movimento 23:40 -> 00:20; il "giorno corrente"
        // (2026-08-20) deve contare solo i 20 minuti dopo mezzanotte.
        let range = TimeRange::new(utc(0, 0), Utc.with_ymd_and_hms(2026, 8, 21, 0, 0, 0).unwrap());
        let iv = moving(
            Utc.with_ymd_and_hms(2026, 8, 19, 23, 40, 0).unwrap(),
            utc(0, 20),
        );
        let c = iv.clip_to(&range, utc(12, 0)).unwrap();
        assert_eq!(c.start, utc(0, 0));
        assert_eq!(c.duration(utc(12, 0)), Duration::minutes(20));
    }

    #[test]
    fn test_clip_open_interval_uses_now() {
        // Intervallo aperto che inizia alle 15:00, il troncamento avviene  a `now`.
        let iv = StateInterval::open(UserState::Moving, utc(15, 0));
        let c = iv.clip_to(&day_range(), utc(18, 0)).unwrap();
        assert_eq!((c.start, c.end), (utc(15, 0), Some(utc(18, 0))));
    }

    #[test]
    fn test_duration_open_vs_closed() {
        assert_eq!(moving(utc(12, 0), utc(15, 0)).duration(utc(23, 0)), Duration::hours(3));
        let open = StateInterval::open(UserState::Moving, utc(12, 0));
        assert_eq!(open.duration(utc(14, 0)), Duration::hours(2));
    }

    #[test]
    fn test_total_in_balances() {
        // Intervalli contigui che partizionano la linea del tempo; su [10:00, 20:00) i
        // tre stati devono sommare esattamente alla durata del periodo.
        let now = utc(23, 0);
        let range = day_range();
        let intervals = [
            StateInterval::closed(UserState::Disconnected, utc(9, 0), utc(10, 30)),
            StateInterval::closed(UserState::Stopped, utc(10, 30), utc(12, 0)),
            StateInterval::closed(UserState::Moving, utc(12, 0), utc(18, 0)),
            StateInterval::closed(UserState::Stopped, utc(18, 0), utc(21, 0)),
        ];

        let moving_t = total_in(&intervals, UserState::Moving, &range, now);
        let stopped_t = total_in(&intervals, UserState::Stopped, &range, now);
        let disc_t = total_in(&intervals, UserState::Disconnected, &range, now);

        assert_eq!(moving_t, Duration::hours(6));
        assert_eq!(stopped_t, Duration::minutes(90) + Duration::hours(2));
        assert_eq!(disc_t, Duration::minutes(30));
        // L'invariante che cattura buchi ed errori di troncamento off-by-one:
        assert_eq!(moving_t + stopped_t + disc_t, range.duration());
    }
}
