//! `format_time`: calendar fields from a Unix time and a UTC offset, in integer arithmetic only,
//! so every platform writes the same text.

use alloc::format;
use alloc::string::{String, ToString};

const MS_PER_DAY: i64 = 86_400_000;
const MS_PER_MINUTE: i64 = 60_000;

/// The tokens, each spelling before any shorter one that starts it, so the longest one matches.
const TOKENS: [(&str, Token); 12] = [
    ("yyyy", Token::Year),
    ("MM", Token::MonthPadded),
    ("M", Token::Month),
    ("dd", Token::DayPadded),
    ("d", Token::Day),
    ("HH", Token::HourPadded),
    ("H", Token::Hour),
    ("hh", Token::ClockHourPadded),
    ("h", Token::ClockHour),
    ("mm", Token::MinutePadded),
    ("ss", Token::SecondPadded),
    ("a", Token::Meridiem),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    Year,
    MonthPadded,
    Month,
    DayPadded,
    Day,
    HourPadded,
    Hour,
    ClockHourPadded,
    ClockHour,
    MinutePadded,
    SecondPadded,
    Meridiem,
}

/// Writes `pattern` for the local time `tz_offset_minutes` away from the Unix time `ms`,
/// flooring a fractional `ms`. Tokens match longest first, and any other character, a letter
/// that starts no token included, is copied as it is.
pub(super) fn format(ms: f64, pattern: &str, tz_offset_minutes: i32) -> Option<String> {
    let offset = i64::from(tz_offset_minutes).checked_mul(MS_PER_MINUTE)?;
    let local = whole_ms(ms)?.checked_add(offset)?;
    let fields = Fields::at(local);
    let mut out = String::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        let token = TOKENS
            .iter()
            .find_map(|(spelling, token)| rest.strip_prefix(spelling).map(|after| (*token, after)));
        if let Some((token, after)) = token {
            fields.write(token, &mut out);
            rest = after;
        } else {
            let mut chars = rest.chars();
            if let Some(c) = chars.next() {
                out.push(c);
            }
            rest = chars.as_str();
        }
    }
    Some(out)
}

/// `ms` floored to a whole millisecond, when that fits an `i64`.
fn whole_ms(ms: f64) -> Option<i64> {
    let floored = ms.floor();
    // `i64::MIN as f64` is exactly -2^63, and `i64::MAX as f64` rounds up to 2^63.
    let fits = floored >= i64::MIN as f64 && floored < i64::MAX as f64;
    fits.then_some(floored as i64)
}

/// The calendar and clock fields of one local time.
struct Fields {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
}

impl Fields {
    fn at(local_ms: i64) -> Fields {
        let days = local_ms.div_euclid(MS_PER_DAY);
        let seconds = local_ms.rem_euclid(MS_PER_DAY) / 1000;
        let (year, month, day) = civil_from_days(days);
        Fields {
            year,
            month,
            day,
            hour: seconds / 3600,
            minute: seconds / 60 % 60,
            second: seconds % 60,
        }
    }

    fn write(&self, token: Token, out: &mut String) {
        let text = match token {
            Token::Year if self.year < 0 => format!("-{:04}", -self.year),
            Token::Year => format!("{:04}", self.year),
            Token::MonthPadded => format!("{:02}", self.month),
            Token::Month => self.month.to_string(),
            Token::DayPadded => format!("{:02}", self.day),
            Token::Day => self.day.to_string(),
            Token::HourPadded => format!("{:02}", self.hour),
            Token::Hour => self.hour.to_string(),
            Token::ClockHourPadded => format!("{:02}", self.clock_hour()),
            Token::ClockHour => self.clock_hour().to_string(),
            Token::MinutePadded => format!("{:02}", self.minute),
            Token::SecondPadded => format!("{:02}", self.second),
            Token::Meridiem => String::from(if self.hour < 12 { "AM" } else { "PM" }),
        };
        out.push_str(&text);
    }

    /// The hour on a 12-hour clock, where midnight and noon are 12.
    fn clock_hour(&self) -> i64 {
        match self.hour % 12 {
            0 => 12,
            hour => hour,
        }
    }
}

/// The proleptic Gregorian year, month and day of a day count from 1970-01-01, after Howard
/// Hinnant's `civil_from_days`. The count comes from an `i64` of milliseconds, so it stays under
/// 2^37 in size and no step can overflow.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // Days counted from 0000-03-01, so each 400-year era ends with its leap day.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // Months counted from March.
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The inverse of `civil_from_days`, written independently from the day counts of whole
    /// years and months.
    fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
        let leap = |year: i64| (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let month_lengths = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut days = 0;
        if year >= 1970 {
            for y in 1970..year {
                days += if leap(y) { 366 } else { 365 };
            }
        } else {
            for y in year..1970 {
                days -= if leap(y) { 366 } else { 365 };
            }
        }
        for m in 1..month {
            days += month_lengths[(m - 1) as usize] + i64::from(m == 2 && leap(year));
        }
        days + day - 1
    }

    #[test]
    fn test_civil_from_days_known_dates_match() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(47_540), (2100, 2, 28));
        assert_eq!(civil_from_days(47_541), (2100, 3, 1));
        assert_eq!(civil_from_days(-719_162), (1, 1, 1));
        assert_eq!(civil_from_days(-719_528), (0, 1, 1));
        assert_eq!(civil_from_days(-719_529), (-1, 12, 31));
    }

    #[test]
    fn test_civil_from_days_round_trips_across_five_centuries() {
        for days in (-200_000..=200_000).step_by(7) {
            let (year, month, day) = civil_from_days(days);
            assert!(
                (1..=12).contains(&month) && (1..=31).contains(&day),
                "{days}"
            );
            assert_eq!(days_from_civil(year, month, day), days);
        }
    }

    #[test]
    fn test_format_extreme_times_never_overflow() {
        for days in [
            i64::MIN.div_euclid(MS_PER_DAY),
            i64::MAX.div_euclid(MS_PER_DAY),
        ] {
            let (_, month, day) = civil_from_days(days);
            assert!(
                (1..=12).contains(&month) && (1..=31).contains(&day),
                "{days}"
            );
        }
        assert!(format(i64::MAX as f64 - 1e10, "yyyy-MM-dd", 840).is_some());
        assert!(format(i64::MIN as f64, "yyyy-MM-dd", 0).is_some());
        assert_eq!(format(i64::MIN as f64, "yyyy", -1), None);
        assert_eq!(format(i64::MAX as f64, "yyyy", 0), None);
        assert_eq!(format(1e300, "yyyy", 0), None);
    }
}
