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
/// The parsing is more liberal, and accepts both the spaces and i,
/// as well as lowercase letters as long as all retters are lowercase.
/// (kB and kiB are also allowed.)
///
/// # Examples
///
/// See tests in the source file. (doc-tests doesn't work in executables yet.)
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
        while let Some(b' ') = s.as_bytes().get(digits) {
            digits += 1;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_from_str() {
        assert_eq!(Bytes::from_str("0"), Ok(Bytes(0)));
        assert_eq!(Bytes::from_str("0B"), Ok(Bytes(0)));
        assert_eq!(Bytes::from_str("0b"), Ok(Bytes(0)));
        assert_eq!(Bytes::from_str("0 B"), Ok(Bytes(0)));
        assert_eq!(Bytes::from_str("0 b"), Ok(Bytes(0)));
        assert_eq!(Bytes::from_str("0PB"), Ok(Bytes(0)));

        Bytes::from_str("").unwrap_err();
        Bytes::from_str("B").unwrap_err();
        Bytes::from_str("b").unwrap_err();
        Bytes::from_str("0 ").unwrap_err();
        Bytes::from_str("0B ").unwrap_err();
        Bytes::from_str("00").unwrap_err();
        Bytes::from_str("0bb").unwrap_err();
        Bytes::from_str("0MMB").unwrap_err();
    }

    #[test]
    fn from_str() {
        assert_eq!(Bytes::from_str("1KB"), Ok(Bytes(1024)));
        assert_eq!(Bytes::from_str("2kb"), Ok(Bytes(2048)));
        assert_eq!(Bytes::from_str("3kB"), Ok(Bytes(3072)));
        assert_eq!(Bytes::from_str("4KiB"), Ok(Bytes(4096)));
        assert_eq!(Bytes::from_str("4096B"), Ok(Bytes(4096)));
        assert_eq!(Bytes::from_str("1tib"), Ok(Bytes(1024*1024*1024*1024)));

        Bytes::from_str("1").unwrap_err();
        Bytes::from_str("AB").unwrap_err();
        Bytes::from_str("dB").unwrap_err();

        Bytes::from_str("16EB").unwrap_err();
        Bytes::from_str("2QB").unwrap_err();
    }

    #[test]
    fn to_string() {
        assert_eq!(Bytes(0).to_string(), "0B");
        assert_eq!(Bytes(1023).to_string(), "1023B");
        assert_eq!(Bytes(1024).to_string(), "1KB");
        assert_eq!(Bytes(12345).to_string(), "12KB");
        assert_eq!(Bytes(128<<20).to_string(), "128MB");
        assert_eq!(Bytes(512*1024-1).to_string(), "511KB");
        assert_eq!(Bytes(u16::MAX as u64).to_string(), "63KB");
        assert_eq!(Bytes(u16::MAX as u64+1).to_string(), "64KB");
        assert_eq!(Bytes(u32::MAX as u64).to_string(), "3GB");
        assert_eq!(Bytes(u32::MAX as u64+1).to_string(), "4GB");
        assert_eq!(Bytes(u64::MAX).to_string(), "15EB");
    }

    #[test]
    fn from_usize() {
        assert_eq!(Bytes::from(77usize), Bytes(77));
        assert_eq!(Bytes::from(usize::MAX), Bytes(usize::MAX as u64));
    }

    #[test]
    fn to_usize() {
        assert_eq!(Bytes(isize::MAX as u64+10).to_usize_saturating(), isize::MAX as usize+10);
        assert_eq!(Bytes(usize::MAX as u64).to_usize_saturating(), usize::MAX);
        assert_eq!(Bytes((usize::MAX as u64).saturating_add(1)).to_usize_saturating(), usize::MAX);
    }
}
