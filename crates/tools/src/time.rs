//! Model cycle times. A cycle is written `YYYYMMDDHH` in UTC.

const DAY_S: i64 = 86_400;

/// Unix seconds of the start of a cycle such as `2026111112`.
pub fn cycle_to_unix(cycle: &str) -> Option<i64> {
    if cycle.len() != 10 || !cycle.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = |r: std::ops::Range<usize>| cycle[r].parse::<i64>().ok();
    let (y, m, d, h) = (n(0..4)?, n(4..6)?, n(6..8)?, n(8..10)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 {
        return None;
    }
    Some(days_from_civil(y, m, d) * DAY_S + h * 3600)
}

/// The cycle string of a unix time that falls on a whole hour.
pub fn unix_to_cycle(unix: i64) -> String {
    let (y, m, d) = civil_from_days(unix.div_euclid(DAY_S));
    let h = unix.rem_euclid(DAY_S) / 3600;
    format!("{y:04}{m:02}{d:02}{h:02}")
}

/// `2026-11-11 12:00` for display.
pub fn format_utc(unix: i64) -> String {
    let (y, m, d) = civil_from_days(unix.div_euclid(DAY_S));
    let s = unix.rem_euclid(DAY_S);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", s / 3600, s % 3600 / 60)
}

// Howard Hinnant's civil-date algorithms.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_round_trip() {
        assert_eq!(cycle_to_unix("1970010100"), Some(0));
        assert_eq!(cycle_to_unix("2026111112"), Some(1_794_398_400));
        for cycle in ["2024022918", "2026123100", "2027010106"] {
            assert_eq!(unix_to_cycle(cycle_to_unix(cycle).unwrap()), cycle);
        }
        assert_eq!(cycle_to_unix("20261111"), None);
        assert_eq!(cycle_to_unix("2026131100"), None);
        assert_eq!(format_utc(1_794_398_400 + 90 * 60), "2026-11-11 13:30");
    }
}
