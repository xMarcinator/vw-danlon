//! Danish public holidays (helligdage) and common contractual days off.

use chrono::{Datelike, Duration, NaiveDate};

use crate::config::HolidayConfig;

/// Easter Sunday for a Gregorian year (anonymous Gregorian algorithm).
pub fn easter_sunday(year: i32) -> NaiveDate {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    NaiveDate::from_ymd_opt(year, month as u32, day as u32).expect("valid easter date")
}

/// All days off for `year` according to the config, with a Danish name.
pub fn days_off(year: i32, cfg: &HolidayConfig) -> Vec<(NaiveDate, &'static str)> {
    let ymd = |m, d| NaiveDate::from_ymd_opt(year, m, d).unwrap();
    let mut out = Vec::new();

    if cfg.danish_public {
        let easter = easter_sunday(year);
        let rel = |days: i64| easter + Duration::days(days);
        out.push((ymd(1, 1), "Nytårsdag"));
        out.push((rel(-3), "Skærtorsdag"));
        out.push((rel(-2), "Langfredag"));
        out.push((easter, "Påskedag"));
        out.push((rel(1), "2. påskedag"));
        // Store bededag was abolished as a public holiday from 2024.
        if year < 2024 {
            out.push((rel(26), "Store bededag"));
        }
        out.push((rel(39), "Kristi himmelfartsdag"));
        out.push((rel(49), "Pinsedag"));
        out.push((rel(50), "2. pinsedag"));
        out.push((ymd(12, 25), "Juledag"));
        out.push((ymd(12, 26), "2. juledag"));
    }
    if cfg.grundlovsdag {
        out.push((ymd(6, 5), "Grundlovsdag"));
    }
    if cfg.christmas_eve {
        out.push((ymd(12, 24), "Juleaftensdag"));
    }
    if cfg.new_years_eve {
        out.push((ymd(12, 31), "Nytårsaftensdag"));
    }

    out.sort_by_key(|(d, _)| *d);
    out.dedup_by_key(|(d, _)| *d);
    debug_assert!(out.iter().all(|(d, _)| d.year() == year));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn easter_dates() {
        assert_eq!(easter_sunday(2024), d(2024, 3, 31));
        assert_eq!(easter_sunday(2025), d(2025, 4, 20));
        assert_eq!(easter_sunday(2026), d(2026, 4, 5));
        assert_eq!(easter_sunday(2027), d(2027, 3, 28));
    }

    #[test]
    fn store_bededag_only_before_2024() {
        let cfg = HolidayConfig::default();
        assert!(
            days_off(2023, &cfg)
                .iter()
                .any(|(_, n)| *n == "Store bededag")
        );
        assert!(
            !days_off(2026, &cfg)
                .iter()
                .any(|(_, n)| *n == "Store bededag")
        );
    }

    #[test]
    fn movable_holidays_2026() {
        let cfg = HolidayConfig::default();
        let days: Vec<_> = days_off(2026, &cfg).into_iter().map(|(d, _)| d).collect();
        assert!(days.contains(&d(2026, 4, 2))); // Skærtorsdag
        assert!(days.contains(&d(2026, 5, 14))); // Kristi himmelfart
        assert!(days.contains(&d(2026, 5, 25))); // 2. pinsedag
    }
}
