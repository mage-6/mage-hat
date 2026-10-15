//! The build's clock, and the one date format MageHat reads.
//!
//! A build is deterministic given its inputs and its clock. The clock decides
//! one thing: an item whose `date` is still to come is scheduled and left out
//! (content.rs), and the news sitemap covers the two days before it
//! (build.rs). `magehat build` reads the wall clock; `--now <iso>` pins it,
//! for a test or a build that must come out the same twice.

use crate::errors::{MageError, Result};

#[derive(Debug, Clone, Copy)]
pub struct Clock {
    /// Seconds since 1970-01-01T00:00:00Z.
    pub now: i64,
    /// Build scheduled items anyway, each marked `scheduled: true`. `dev`
    /// does this so a writer can read the page before its date.
    pub show_scheduled: bool,
}

impl Clock {
    pub fn wall() -> Clock {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        Clock { now, show_scheduled: false }
    }

    /// The clock `--now <iso>` asks for.
    pub fn at(iso: &str) -> Result<Clock> {
        let now = parse_iso(iso).ok_or_else(|| {
            MageError::new(format!("--now needs an ISO 8601 date or timestamp, got {iso:?}")).fix("write it as 2026-10-20 or 2026-10-20T14:30:00Z")
        })?;
        Ok(Clock { now, show_scheduled: false })
    }

    pub fn showing_scheduled(self) -> Clock {
        Clock { show_scheduled: true, ..self }
    }

    /// Days since 1970-01-01 of the clock's date, UTC.
    pub fn today(&self) -> i64 {
        self.now.div_euclid(86400)
    }
}

/// Seconds since the epoch of an ISO 8601 date or timestamp: `2026-10-20`
/// (midnight UTC), `2026-10-20T14:30`, `2026-10-20T14:30:00Z`,
/// `2026-10-20T09:00:00-03:00`, `2026-10-20 14:30:00.250+0100`. No offset
/// means UTC. Anything else is `None`, and the caller treats the item as
/// undated, which is what the feed does with such a date too.
pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = if s.len() > 10 { s.split_at(10) } else { (s, "") };
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || date.len() != 10 || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut secs = days_from_civil(y, m, d) * 86400;
    if rest.is_empty() {
        return Some(secs);
    }
    let rest = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' '))?;
    // Split the time from its zone: Z, or the first + or - after the hour.
    let zone_at = rest.char_indices().skip(1).find(|(_, c)| matches!(c, 'Z' | 'z' | '+' | '-')).map(|(i, _)| i);
    let (time, zone) = match zone_at {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let time = time.split('.').next()?;
    let mut hms = time.split(':');
    let h: i64 = hms.next()?.parse().ok()?;
    let mi: i64 = hms.next()?.parse().ok()?;
    let sec: i64 = match hms.next() {
        Some(x) => x.parse().ok()?,
        None => 0,
    };
    if hms.next().is_some() || !(0..24).contains(&h) || !(0..60).contains(&mi) || !(0..61).contains(&sec) {
        return None;
    }
    secs += h * 3600 + mi * 60 + sec;
    match zone {
        "" | "Z" | "z" => Some(secs),
        _ => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let digits: String = zone[1..].chars().filter(|c| *c != ':').collect();
            if digits.is_empty() || digits.len() > 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let (zh, zm) = digits.split_at(digits.len().min(2));
            let zh: i64 = zh.parse().ok()?;
            let zm: i64 = if zm.is_empty() { 0 } else { zm.parse().ok()? };
            // 09:00 at -03:00 is 12:00 UTC: subtract the offset.
            Some(secs - sign * (zh * 3600 + zm * 60))
        }
    }
}

/// `2026-10-20T14:30:00Z` for a moment, for messages.
pub fn format_iso(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86400));
    let t = secs.rem_euclid(86400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, t % 3600 / 60, t % 60)
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's days_from_civil).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The civil date of a day count since 1970-01-01 (Hinnant's civil_from_days).
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_timestamps_parse_to_utc_seconds() {
        let midnight = parse_iso("2026-10-20").unwrap();
        assert_eq!(format_iso(midnight), "2026-10-20T00:00:00Z");
        assert_eq!(parse_iso("2026-10-20T14:30:00Z"), Some(midnight + 14 * 3600 + 30 * 60));
        assert_eq!(parse_iso("2026-10-20T14:30"), Some(midnight + 14 * 3600 + 30 * 60));
        assert_eq!(parse_iso("2026-10-20 14:30:15.250"), Some(midnight + 14 * 3600 + 30 * 60 + 15));
        // 09:00 in São Paulo is noon UTC.
        assert_eq!(parse_iso("2026-10-20T09:00:00-03:00"), Some(midnight + 12 * 3600));
        assert_eq!(parse_iso("2026-10-20T09:00:00-0300"), Some(midnight + 12 * 3600));
        assert_eq!(parse_iso("2026-10-20T14:00+01"), Some(midnight + 13 * 3600));
        assert_eq!(parse_iso("1970-01-01"), Some(0));
    }

    #[test]
    fn nonsense_is_not_a_date() {
        for bad in ["", "soon", "2026-13-01", "2026-10-2", "2026-10-20T25:00", "2026-10-20X10:00", "20-10-2026", "2026-10-20T10:00+1:00x"] {
            assert_eq!(parse_iso(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn civil_round_trips() {
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 10, 15), (2100, 12, 31)] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
    }

    #[test]
    fn a_pinned_clock_rejects_a_bad_moment() {
        assert!(Clock::at("tomorrow").is_err());
        assert_eq!(Clock::at("2026-10-20").unwrap().today(), days_from_civil(2026, 10, 20));
        assert!(Clock::at("2026-10-20").unwrap().showing_scheduled().show_scheduled);
    }
}
