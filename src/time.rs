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

use std::fmt::{Display, Formatter, Result};
use std::time::SystemTime;

/// Convert timestamp to datetime retured as [year, month, day, hour, minute, second].
///
/// Copied from https://github.com/tormol/tiprotd/blob/master/clients/time32.rs
/// and extended to 64-bit.  
/// Might have bugs outside 1904-2038, especially before year 0.
fn timestamp_to_date(mut ts: i64) -> [i16; 6] {
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
    [year as i16, months as i16+1, days as i16+1, hour as i16, minute as i16, ts as i16]
}

#[derive(Clone,Copy, Default, Debug, PartialEq,Eq, PartialOrd,Ord)]
pub struct PrintableTime {
    year: i16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

impl Display for PrintableTime {
    fn fmt(&self,  formatter: &mut Formatter) -> Result {
        match write!(formatter, "{:04}-{:02}-{:02}", self.year, self.month, self.day) {
            Ok(()) if !formatter.alternate() => {
                write!(formatter, " {:02}:{:02}:{:02}", self.hour, self.minute, self.second)
            },
            result => result,
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
            year: parts[0] as i16,  month: parts[1] as u8,  day: parts[2] as u8,
            hour: parts[3] as u8,  minute: parts[4] as u8,  second: parts[5] as u8,
        }
    }
}
