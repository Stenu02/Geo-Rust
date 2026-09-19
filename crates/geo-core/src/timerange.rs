use crate::COMPANY_TZ;
use chrono::{DateTime, Datelike, Duration, Months, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};

/// Quale periodo si vuole analizzare. E' cio' che l'amministratore sceglie
/// nell'interfaccia e che viaggia nel protocollo.
/// Requisiti Funzionali rispettati: RF11
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RangeSpec {
    Today,
    CurrentWeek,
    CurrentMonth,
}

impl RangeSpec {
    pub fn label(&self) -> &'static str {
        match self {
            RangeSpec::Today => "giorno corrente",
            RangeSpec::CurrentWeek => "settimana corrente",
            RangeSpec::CurrentMonth => "mese corrente",
        }
    }
}

/// Un periodo concreto, con estremi assoluti. Semiaperto: `[start, end)`.
///
/// Semiaperto e non chiuso perche' altrimenti l'istante di mezzanotte
/// apparterrebbe a due giorni e verrebbe contato due volte.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

fn local_midnight_to_utc(date: NaiveDate) -> DateTime<Utc> {
    let naive = date.and_hms_opt(0, 0, 0).unwrap();
    COMPANY_TZ
        .from_local_datetime(&naive)
        .single() // la mezzanotte e' univoca in Europe/Rome
        .expect("la mezzanotte locale esiste sempre in Europe/Rome")
        .with_timezone(&Utc)
}

impl TimeRange {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { start, end }
    }

    /// Risolve una `RangeSpec` in un periodo concreto, rispetto a un istante di riferimento.
    ///
    /// `now` e' un parametro, non `Utc::now()`: e' cio' che permette di testare il
    /// comportamento a cavallo di mezzanotte, di fine mese e del cambio di ora legale
    /// senza aspettare che accadano.
    pub fn resolve(spec: RangeSpec, now: DateTime<Utc>) -> TimeRange {
        match spec {
            RangeSpec::Today => {
                let today = now.with_timezone(&COMPANY_TZ).date_naive();
                let start = local_midnight_to_utc(today);
                let end = local_midnight_to_utc(today.succ_opt().unwrap());
                TimeRange::new(start, end)
            }
            RangeSpec::CurrentWeek => {
                let today = now.with_timezone(&COMPANY_TZ).date_naive();
                let dfm = today.weekday().num_days_from_monday();
                let monday = today - Duration::days(dfm as i64);
                let next_monday = monday + Duration::days(7);

                let start = local_midnight_to_utc(monday);
                let end = local_midnight_to_utc(next_monday);
                TimeRange::new(start, end)
            }
            RangeSpec::CurrentMonth => {
                let today = now.with_timezone(&COMPANY_TZ).date_naive();
                let first = today.with_day(1).unwrap();
                let next_month_first = first.checked_add_months(Months::new(1)).unwrap();

                let start = local_midnight_to_utc(first);
                let end = local_midnight_to_utc(next_month_first);
                TimeRange::new(start, end)
            }
        }
    }

    pub fn duration(&self) -> Duration {
        self.end - self.start
    }

    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        at >= self.start && at < self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// helper per il funzionamento dei test
    fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        COMPANY_TZ
            .with_ymd_and_hms(y, mo, d, h, mi, s)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn test_today_boundaries() {
        // Giovedi', un giorno ordinario da 24 ore.
        let now = local(2026, 8, 20, 15, 30, 0);
        let r = TimeRange::resolve(RangeSpec::Today, now);
        assert_eq!(r.start, local(2026, 8, 20, 0, 0, 0));
        assert_eq!(r.end, local(2026, 8, 21, 0, 0, 0));
        assert_eq!(r.duration(), Duration::hours(24));
    }

    #[test]
    fn test_current_week_starts_monday() {
        // Giovedi' 2026-08-20 -> [lun 17, lun successivo 24).
        let now = local(2026, 8, 20, 9, 0, 0);
        let r = TimeRange::resolve(RangeSpec::CurrentWeek, now);
        assert_eq!(r.start, local(2026, 8, 17, 0, 0, 0));
        assert_eq!(r.end, local(2026, 8, 24, 0, 0, 0));
        assert_eq!(r.duration(), Duration::days(7));
    }

    #[test]
    fn test_current_month_boundaries() {
        let now = local(2026, 8, 20, 9, 0, 0);
        let r = TimeRange::resolve(RangeSpec::CurrentMonth, now);
        assert_eq!(r.start, local(2026, 8, 1, 0, 0, 0));
        assert_eq!(r.end, local(2026, 9, 1, 0, 0, 0));
    }

    #[test]
    fn test_current_month_december_rollover() {
        // Perche' checked_add_months: dicembre deve passare a gennaio dell'anno dopo.
        let now = local(2026, 12, 15, 9, 0, 0);
        let r = TimeRange::resolve(RangeSpec::CurrentMonth, now);
        assert_eq!(r.start, local(2026, 12, 1, 0, 0, 0));
        assert_eq!(r.end, local(2027, 1, 1, 0, 0, 0));
    }

    #[test]
    fn test_today_dst_spring_forward_is_23h() {
        // Ultima domenica di marzo 2026: l'orologio salta 02:00 -> 03:00, il giorno e' 23h.
        let now = local(2026, 3, 29, 12, 0, 0);
        let r = TimeRange::resolve(RangeSpec::Today, now);
        assert_eq!(r.duration(), Duration::hours(23));
    }

    #[test]
    fn test_today_dst_fall_back_is_25h() {
        // Ultima domenica di ottobre 2026: l'orologio torna 03:00 -> 02:00, il giorno e' 25h.
        let now = local(2026, 10, 25, 12, 0, 0);
        let r = TimeRange::resolve(RangeSpec::Today, now);
        assert_eq!(r.duration(), Duration::hours(25));
    }
}
