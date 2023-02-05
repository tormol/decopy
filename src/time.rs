/* Copyright 2019, 2023 Torbjørn Birch Moltu
 *
 * This file is part of Decopy.
 * Decopy is free software: you can redistribute it and/or modify it under the
 * terms of the GNU General Public License as published by the Free Software Foundation,
 * either version 3 of the License, or (at your option) any later version.
 *
 * Decopy is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
 * without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
 * See the GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along with Decopy.
 * If not, see <https://www.gnu.org/licenses/>.
 */

//! std::time cannot format, and this is not worth pulling in chrono for

use std::fmt::{Debug, Display, Formatter, Result as fmtResult};
use std::num::NonZeroU8;
use std::str::FromStr;
use std::time::SystemTime;

/// A type to display a `SystemTime` in a human-readable way.
///
/// The `Display` and `Debug` impls displays it on the form yyyy-mm-dd HH:MM:SS.
/// If alternate display formatting is enabled, only the date is displayed.
/// With debug formatting, this alternate mode is not supported.
///
/// # Limitiations
///
/// * Lacks sub-second precision.
///   The sub-second part of a `SystemTime` is ignored.
/// * Can only display dates between years ±32768.
///   Dates outside that range are will be clamped to the max and min value.
#[derive(Clone,Copy, PartialEq,Eq,Hash, PartialOrd,Ord)]
pub struct PrintableTime {
    year: i16,
    month: NonZeroU8,
    day: NonZeroU8,
    hour: u8,
    minute: u8,
    second: u8,
}

// common conversion and validation functions
const fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

const fn days_in_months(year: i64) -> [u8; 12] {
    let feb = if is_leap(year) {29} else {28};
    [31, feb, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
}

/// Because unwrap() can't be used in `const fn` yet.
const fn to_nonzero(n: u8) -> NonZeroU8 {
    match NonZeroU8::new(n) {
        Some(n) => n,
        None => unreachable!(),
    }
}

const fn try_from_parts(year: i16,  month: u8,  day: u8,  hour: u8,  minute: u8,  second: u8)
 -> Result<PrintableTime, &'static str> {
    if month < 1 || month > 12 {
        return Err("month is outside the range 1..=12");
    } else if day < 1 || day > days_in_months(year as i64)[month as usize-1] as u8 {
        return Err("day is outside the range for the given month and year");
    } else if hour > 23 {
        return Err("hour is outside the range 0..=23");
    } else if minute > 59 || second > 59 {
        return Err("minute or second is outside the range 0..=59");
    } else {
        Ok(PrintableTime {
            year,
            month: to_nonzero(month as u8),
            day: to_nonzero(day as u8),
            hour,
            minute,
            second,
        })
    }
}


impl PrintableTime {
    pub const MAX: Self = Self::new(i16::MAX, 12, 31, 23, 59, 59);
    pub const MIN: Self = Self::new(i16::MIN, 1, 1, 0, 0, 0);

    /// Create a datetime from [year, month, day, hour, minute, second].
    ///
    /// # Panics
    ///
    /// If any field is outside their valid range, such as month: 0 or hour: 24.
    pub const fn new(year: i16,  month: u8,  day: u8,  hour: u8,  minute: u8,  second: u8)
    -> PrintableTime {
        match try_from_parts(year, month, day, hour, minute, second) {
            Ok(datetime) => datetime,
            Err(e) => panic!("{}", e),
        }
    }

    /// Convert timestamp to datetime.
    ///
    /// Should handle all dates between years ±32768.
    /// Dates outside that range are clamped to the nearest representable value:
    /// Either `PrintableTime::MAX` or `PrintableTime::MIN`.
    pub const fn from_timestamp(mut timestamp: i64) -> PrintableTime {
        // Adapted from https://github.com/tormol/tiprotd/blob/master/clients/time32.rs
        let sign: i64 = if timestamp < 0 {-1} else {1};
        let mut days = timestamp / (60*60*24);
        timestamp %= 60*60*24;
        let mut year: i64 = 1970;
        const fn days_in_year(year: i64) -> i64 {if is_leap(year) {366} else {365}}
        if sign >= 0 {
            while days >= days_in_year(year) {
                days -= days_in_year(year);
                year += 1;
            }
        } else {// pre 1970
            if timestamp != 0 {// not 00:00:00
                timestamp += 60*60*24;
                days -= 1;
            }
            loop {
                year -= 1;
                days += days_in_year(year);
                if days >= 0 {
                    break;
                }
            }
        }
        let days_in_month = days_in_months(year);
        let mut months = 0;
        while days >= days_in_month[months] as i64 {
            days -= days_in_month[months] as i64;
            months += 1;
        }

        let hour = timestamp / (60*60);
        timestamp %= 60*60;
        let minute = timestamp / 60;
        timestamp %= 60;

        if year > i16::MAX as i64 {
            PrintableTime::MAX
        } else if year < i16::MIN as i64 {
            PrintableTime::MIN
        } else {
            PrintableTime {
                year: year as i16,
                month: to_nonzero(months as u8 + 1),
                day: to_nonzero(days as u8 + 1),
                hour: hour as u8,
                minute: minute as u8,
                second: timestamp as u8,
            }
        }
    }

    /// Clamp the datetime to be between year 0 and year 9999
    pub const fn clamp_to_yyyy(self) -> Self {
        match self.year {
            0..=9999 => self,
            i16::MIN..=-1 => PrintableTime::new(0, 1, 1, 0, 0, 0),
            10000..=i16::MAX => PrintableTime::new(9999, 12, 31, 23, 59, 59),
        }
    }

    /// Return the datetime as (year, month, day, hour, minute, second).
    #[cfg_attr(not(test), allow(unused))]
    pub const fn to_tuple(self) -> (i16, u8, u8, u8, u8, u8) {
        (self.year, self.month.get(), self.day.get(), self.hour, self.minute, self.second)
    }
    /// Return the datetime as [year, month, day, hour, minute, second].
    #[allow(unused)]
    pub const fn to_array(self) -> [i16; 6] {
        [
                self.year,  self.month.get() as i16,  self.day.get() as i16,
                self.hour as i16,  self.minute as i16,  self.second as i16,
        ]
    }
}

impl Debug for PrintableTime {
    fn fmt(&self,  formatter: &mut Formatter) -> fmtResult {
        write!(formatter, "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                self.year, self.month, self.day,
                self.hour, self.minute, self.second,
        )
    }
}

impl Display for PrintableTime {
    fn fmt(&self,  formatter: &mut Formatter) -> fmtResult {
        if formatter.alternate() {
            write!(formatter, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
        } else {
            Debug::fmt(self, formatter)
        }
    }
}

impl Default for PrintableTime {
    fn default() -> PrintableTime {
        PrintableTime {
            year: 1970,
            month: NonZeroU8::new(1).unwrap(),
            day: NonZeroU8::new(1).unwrap(),
            hour: 0,
            minute: 0,
            second: 0,
        }
    }
}

impl From<SystemTime> for PrintableTime {
    fn from(time: SystemTime) -> PrintableTime {
        match time.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => match i64::try_from(duration.as_secs()) {
                Ok(timestamp) => Self::from_timestamp(timestamp),
                Err(_) => Self::MAX,
            },
            Err(negative) => match i64::try_from(negative.duration().as_secs()) {
                // i64::MIN as u64 does not need to go to the Ok branch,
                // becase it would be clamped to Self::MIN there too.
                Ok(timestamp) => Self::from_timestamp(-timestamp),
                Err(_) => Self::MIN,
            },
        }
    }
}

impl FromStr for PrintableTime {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<PrintableTime, Self::Err> {
        const FORMAT_ERR: &'static str = "must be on the form yyyy-mm-dd HH:MM:SS";
        if s.len() < 16  ||  !s.is_ascii() {
            return Err(FORMAT_ERR);
        }
        let (year, s) = match s[1..7].bytes().position(|b| b == b'-' ) {
            Some(pos_min_1) => s.split_at(pos_min_1+1),
            None => return Err(FORMAT_ERR),
        };
        let s = &s[1..];
        let b = s.as_bytes();
        if s.len() != 14  ||  b[2] != b'-'  || b[5] != b' '  ||  b[8] != b':'  ||  b[11] != b':' {
            return Err(FORMAT_ERR);
        }

        let Ok(year) = i16::from_str(year) else {return Err("year is not a number");};
        let Ok(month) = u8::from_str(&s[..2]) else {return Err("month is not a number");};
        let Ok(day) = u8::from_str(&s[3..5]) else {return Err("day is not a number");};
        let Ok(hour) = u8::from_str(&s[6..8]) else {return Err("hour is not a number");};
        let Ok(minute) = u8::from_str(&s[9..11]) else {return Err("minute is not a number");};
        let Ok(second) = u8::from_str(&s[12..]) else {return Err("second is not a number");};

        try_from_parts(year, month, day, hour, minute, second)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn timestamp_to_date(timestamp: i64) -> (i16, u8, u8, u8, u8, u8) {
        PrintableTime::from_timestamp(timestamp).to_tuple()
    }

    #[test]
    fn timestamp_decoding_i32() {
        assert_eq!(timestamp_to_date(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(timestamp_to_date(60*60*24-1), (1970, 1, 1, 23, 59, 59));
        assert_eq!(timestamp_to_date(60*60*24*(31+1)), (1970, 2, 2, 0, 0, 0));
        assert_eq!(timestamp_to_date(31536000), (1971, 1, 1, 0, 0, 0));
        assert_eq!(timestamp_to_date(39274217), (1971, 3, 31, 13, 30, 17));
        assert_eq!(timestamp_to_date(68214896), (1972, 2, 29, 12, 34, 56));
        assert_eq!(timestamp_to_date(119731017), (1973, 10, 17, 18, 36, 57));
        assert_eq!(timestamp_to_date(951854402), (2000, 2, 29, 20, 00, 02));
        assert_eq!(timestamp_to_date(1551441600), (2019, 3, 1, 12, 00, 00));
        assert_eq!(timestamp_to_date(2147483647), (2038, 1, 19, 3, 14, 7));
        assert_eq!(timestamp_to_date(-1), (1969, 12, 31, 23, 59, 59));
        assert_eq!(timestamp_to_date(-60*60*24), (1969, 12, 31, 0, 0, 0));
        assert_eq!(timestamp_to_date(-60*60*24*365), (1969, 1, 1, 0, 0, 0));
        assert_eq!(timestamp_to_date(-60*60*24*365-1), (1968, 12, 31, 23, 59, 59));
        assert_eq!(timestamp_to_date(-63154739), (1968, 1, 1, 1, 1, 1));
        assert_eq!(timestamp_to_date(-89679601), (1967, 2, 28, 0, 59, 59));
        assert_eq!(timestamp_to_date(-1834750129), (1911, 11, 11, 11, 11, 11));
        assert_eq!(timestamp_to_date(-2147483648), (1901, 12, 13, 20, 45, 52));
    }

    #[test]
    fn timestamp_decoding_i64() {
        assert_eq!(timestamp_to_date(2210112000), (2040, 1, 14, 0, 0, 0));
        assert_eq!(timestamp_to_date(4107542400), (2100, 3, 1, 0, 0, 0));
        assert_eq!(timestamp_to_date(-62167219200), (0, 1, 1, 0, 0, 0));
        assert_eq!(timestamp_to_date(-62167219201), (-1, 12, 31, 23, 59, 59));
        assert_eq!(timestamp_to_date(-62167219201), (-1, 12, 31, 23, 59, 59));
        assert_eq!(timestamp_to_date(-65320000000), (-100, 2, 3, 11, 33, 20));
        assert_eq!(timestamp_to_date(-74790000000), (-400, 1, 1, 0, 0, 0));

        assert_eq!(timestamp_to_date(2041622064000), (i16::MAX, 12, 31, 23, 59, 59));
        assert_eq!(timestamp_to_date(-2041622064000), (i16::MIN, 1, 1, 0, 0, 0));
    }

    #[test]
    fn default() {
        assert_eq!(PrintableTime::default(), PrintableTime::from(SystemTime::UNIX_EPOCH));
        assert_eq!(format!("{}", PrintableTime::default()), "1970-01-01 00:00:00");
        assert_eq!(format!("{:?}", PrintableTime::default()), "1970-01-01 00:00:00");
        assert_eq!(format!("{:#}", PrintableTime::default()), "1970-01-01");
    }

    #[test]
    fn clamp_to_4_digit_year() {
        assert_eq!(
                PrintableTime::new(-10, 5, 5, 9, 9, 9).clamp_to_yyyy(),
                PrintableTime::new(0, 1, 1, 0, 0, 0),
        );
        assert_eq!(
                PrintableTime::new(30000, 9, 9, 21, 21, 21).clamp_to_yyyy(),
                PrintableTime::new(9999, 12, 31, 23, 59, 59),
        );
        assert_eq!(
                PrintableTime::new(0, 10, 10, 12, 12, 12).clamp_to_yyyy(),
                PrintableTime::new(0, 10, 10, 12, 12, 12),
        );
    }

    #[test]
    fn from_str() {
        //fn f(s: &str) -> Retult<PrintableTime, &'static str> {}
        assert_eq!(
                PrintableTime::from_str("2022-02-04 01:40:30").unwrap(),
                PrintableTime::new(2022, 2, 4, 1, 40, 30),
        );
        assert_eq!(
                PrintableTime::from_str("30567-08-09 10:11:12").unwrap(),
                PrintableTime::new(30567, 8, 9, 10, 11, 12),
        );
        assert_eq!(
                PrintableTime::from_str("5-04-03 02:01:00").unwrap(),
                PrintableTime::new(5, 4, 3, 2, 1, 0),
        );
        assert_eq!(
                PrintableTime::from_str("-13-12-11 10:09:08").unwrap(),
                PrintableTime::new(-13, 12, 11, 10, 9, 8),
        );
        assert_eq!(
                PrintableTime::from_str("-12345-06-07 20:21:22").unwrap(),
                PrintableTime::new(-12345, 6, 7, 20, 21, 22),
        );
    }
}
