/* Copyright 2023 Torbjørn Birch Moltu
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

use std::fmt::{self, Display, Debug, Formatter};
use std::num::IntErrorKind::*;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;

/// A type to display and parse numbers with 1024-based B/KB/.../EB units.
///
/// It takes a `u64` to be able to display file sizes.  
/// For memory sizes, it has methods to easily convert to and from `usize`.
/// (When parsing a memory size, you probably want to impose a lower limit
/// than `usize::MAX` anyway.)
///
/// The `Display` impl uses uppercase letters, with no space after the number
/// or i before the B.
/// The parsing is more liberal about the letters: it accepts lowercase i
/// after prefixes (Ki/Mi/Gi/...), and lowercase letters as long as both the b
/// and any prefix is lowercase. (But lowercase k is always accepted).
#[derive(Clone,Copy, Default, Debug, PartialEq,Eq, Hash, PartialOrd,Ord)]
#[repr(transparent)]
pub struct Bytes(pub u64);

impl From<u64> for Bytes {
    fn from(bytes: u64) -> Bytes {
        Bytes(bytes)
    }
}
impl From<Bytes> for u64 {
    fn from(bytes: Bytes) -> u64 {
        bytes.0
    }
}

impl From<usize> for Bytes {
    fn from(bytes: usize) -> Bytes {
        assert!(std::mem::size_of::<usize>() <= 8, "Unsupported architecture");
        Bytes(bytes as u64)
    }
}

impl Deref for Bytes {
    type Target = u64;
    fn deref(&self) -> &u64 {
        &self.0
    }
}
impl DerefMut for Bytes {
    fn deref_mut(&mut self) -> &mut u64 {
        &mut self.0
    }
}

#[derive(Clone,Copy, Debug)]
pub struct WithSymbol {
    pub whole: u16,
    /// ASCII digit
    pub fraction: u8,
    /// ASCII uppercase letter or space.
    pub symbol: u8,
}

impl Bytes {
    // The bigger sizes, Z,Y,R,Q, are outside the range of `u64`.
    const PREFIX_SYMBOLS: [u8; 7] = *b" KMGTPE";
    pub const fn new(bytes: u64) -> Self {
        Bytes(bytes)
    }
    pub const fn as_u64(self) -> u64 {
        self.0
    }
    pub const fn to_usize_saturating(self) -> usize {
        if self.0 <= usize::MAX as u64 {self.0 as usize} else {usize::MAX}
    }
    pub fn with_symbol(self) -> WithSymbol {
        let mut whole = self.0;
        let mut symbol = 0;
        while whole >> 10 != 0 {
            symbol += 1;
            whole >>= 10;
        }
        WithSymbol {
            whole: whole as u16,
            fraction: b'0',
            symbol: Bytes::PREFIX_SYMBOLS[symbol],
        }
    }
    pub fn rounded_with_fraction(self) -> WithSymbol {
        let mut whole = self.0;
        let mut fraction = 0;
        let mut symbol = 0;
        while whole >> 10 != 0 {
            symbol += 1;
            fraction = whole & 1023;
            whole >>= 10;
        }
        let symbol = Bytes::PREFIX_SYMBOLS[symbol];
        let mut msf = fraction / 100;
        if fraction % 100 >= 50 {
            msf += 1;
        }
        if msf > 9 {
            whole += 1;
            msf = 0;
        }
        WithSymbol {
            whole: whole as u16,
            fraction: msf as u8 + b'0',
            symbol: symbol,
        }
    }
}

impl Display for Bytes {
    fn fmt(&self,  fmtr: &mut Formatter) -> fmt::Result {
        if self.0 < 1024 {
            write!(fmtr, "{}B", self.0)
        } else if fmtr.alternate() {
            let WithSymbol{ whole, fraction, symbol } = self.rounded_with_fraction();
            write!(fmtr, "{}.{} {}B", whole, fraction as char, symbol as char)
        } else {
            let WithSymbol{ whole, symbol, .. } = self.with_symbol();
            write!(fmtr, "{}{}B", whole, symbol as char)
        }
    }
}

impl FromStr for Bytes {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "0" {
            // only case where no unit is allowed
            return Ok(Bytes::new(0));
        }
        let mut digits = 0;
        for b in s.as_bytes() {
            if !b.is_ascii_digit() {
                break;
            }
            digits += 1;
        }
        if digits == 0 {
            return Err("missing number");
        }
        let number = match u64::from_str(&s[..digits]) {
            Ok(number) => number,
            Err(ref e) if e.kind() == &PosOverflow => return Err("overflow"),
            Err(e) => unreachable!("number parsing should only fail with overflow, not {}", e),
        };
        let shift = match &s[digits..] {
            "B" | "b" => 0,
            "K" | "k" | "KB" | "kB" | "kb" | "KiB" | "kib" | "kiB" => 10,
            "M" | "m" | "MB" | "mb" | "MiB" | "mib" => 20,
            "G" | "g" | "GB" | "gb" | "GiB" | "gib" => 30,
            "T" | "t" | "TB" | "tb" | "TiB" | "tib" => 40,
            "P" | "p" | "PB" | "pb" | "PiB" | "pib" => 50,
            "E" | "e" | "EB" | "eb" | "EiB" | "eib" => 60,
            "" => return Err("missing unit"),
            _ => return Err("unrecognized unit"),
        };
        if number << shift != number.rotate_left(shift) {
            return Err("overflow");
        }
        Ok(Bytes(number << shift))
    }
}
