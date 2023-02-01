/* Copyright 2019, 2023 Torbjørn Birch Moltu
 *
 * This file is part of Deduplicator.
 * Deduplicator is free software: you can redistribute it and/or modify it under the
 * terms of the GNU General Public License as published by the Free Software Foundation,
 * either version 3 of the License, or (at your option) any later version.
 *
 * Deduplicator is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
 * without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
 * See the GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along with Deduplicator.
 * If not, see <https://www.gnu.org/licenses/>.
 */

//! std::time cannot format, and this is not worth pulling in chrono for

use std::fmt::{Debug, Display, Formatter, Result};
use std::num::NonZeroU8;
use std::time::SystemTime;

/// Convert timestamp to datetime retured as (year, month, day, hour, minute, second).
///
/// Copied from https://github.com/tormol/tiprotd/blob/master/clients/time32.rs
/// and extended to 64-bit.  
/// Should handle all dates between years ±32768.
fn timestamp_to_date(mut ts: i64) -> (i16, u8, u8, u8, u8, u8) {
    // This was written for 32-bit
    let sign: i64 = if ts < 0 {-1} else {1};
    let mut days = ts / (60*60*24);
    ts %= 60*60*24;
    let mut year: i64 = 1970;
    fn isleap(year: i64) -> bool {year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)}
    fn daysinyear(year: i64) -> i64 {if isleap(year) {366} else {365}}
    if sign >= 0 {
        while days >= daysinyear(year) {
            days -= daysinyear(year);
            year += 1;
        }
    } else {// pre 1970
        if ts != 0 {// not 00:00:00
            ts += 60*60*24;
            days -= 1;
        }
        loop {
            year -= 1;
            days += daysinyear(year);
            if days >= 0 {
                break;
            }
        }
    }
    // println!("year: {}, is leap year: {}, day of year: {}, second of day: {}", year, isleap(year), days, ts);
    let feb = if isleap(year) {29} else {28};
    let days_in_month = [31, feb, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut months = 0;
    while days >= days_in_month[months] {
        days -= days_in_month[months];
        months += 1;
    }

    let hour = ts / (60*60);
    ts %= 60*60;
    let minute = ts / 60;
    ts %= 60;
    (year as i16, months as u8 + 1, days as u8 + 1, hour as u8, minute as u8, ts as u8)
}

#[derive(Clone,Copy, PartialEq,Eq, PartialOrd,Ord)]
pub struct PrintableTime {
    year: i16,
    month: NonZeroU8,
    day: NonZeroU8,
    hour: u8,
    minute: u8,
    second: u8,
}

impl Debug for PrintableTime {
    fn fmt(&self,  formatter: &mut Formatter) -> Result {
        write!(formatter, "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                self.year, self.month, self.day,
                self.hour, self.minute, self.second,
        )
    }
}

impl Display for PrintableTime {
    fn fmt(&self,  formatter: &mut Formatter) -> Result {
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
        let ts = match time.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => duration.as_secs() as i64,
            Err(negative) => -(negative.duration().as_secs() as i64),
        };
        let parts = timestamp_to_date(ts);
        PrintableTime {
            year: parts.0,
            month: NonZeroU8::new(parts.1).unwrap(),
            day: NonZeroU8::new(parts.2).unwrap(),
            hour: parts.3,
            minute: parts.4,
            second: parts.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        //assert_eq!(timestamp_to_date(2041622064000), (66666, 6, 6, 0, 0, 0));
    }

    #[test]
    fn default() {
        assert_eq!(PrintableTime::default(), PrintableTime::from(SystemTime::UNIX_EPOCH));
        assert_eq!(format!("{}", PrintableTime::default()), "1970-01-01 00:00:00");
        assert_eq!(format!("{:?}", PrintableTime::default()), "1970-01-01 00:00:00");
        assert_eq!(format!("{:#}", PrintableTime::default()), "1970-01-01");
    }
}
