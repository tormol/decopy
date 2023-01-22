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

use std::ffi::OsStr;
use std::path::{MAIN_SEPARATOR, Component, Path};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os="wasi")]
use std::os::wasi::ffi::OsStrExt;
#[cfg(windows)]
use std::{fmt::Write, os::windows::ffi::OsStrExt};
#[cfg(not(any(unix, target_os="wasi", windows)))]
use std::char::REPLACEMENT_CHARACTER;

/// Based on `ascii::AsciiChar::as_printable_char()`.
fn printable_char(c: char) -> char {
    match c as u32 {
        // Use replacement characters between codepoint 0x2400 ('␀') and 0x241f (`␟`).
        0x0..=0x1f => char::from_u32('␀' as u32 + c as u32).unwrap(),
        // The replacement characther for delete is at codepoint 0x2421, not 0x247f.
        127 => '␡',
        _ => c,
    }
}

fn printable_str(s: &str,  out: &mut String) {
    if s.bytes().all(|b| b > 0x1f  &&  b != 127 ) {
        // common case of no escaping necessary
        out.push_str(s);
    } else {
        for c in s.chars() {
            out.push(printable_char(c));
        }
    }
}

#[cfg(any(unix, target_os="wasi"))]
fn printable_cp1252(s: &OsStr,  out: &mut String) {
    // Windows-1252 only differs from ISO-8859-1 and unicode scalar values in
    // the range 0x80..0xa0: https://encoding.spec.whatwg.org/windows-1252.html
    const CP1252_SPECIAL: [char; 32] = [
        '€', '', '‚', 'ƒ', '‟', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '', 'Ž', '',
        '', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '', 'ž', 'Ÿ',
    ];
    for &b in s.as_bytes() {
        let display = match b {
            0x00..=0x1f => char::from_u32('␀' as u32 + b as u32).unwrap(),
            0x7f => '␡',
            0x00..=0x9f => CP1252_SPECIAL[b as usize - 0x80],
            _ => b as char,
        };
        out.push(display);
    }
}

#[cfg(windows)]
fn printable_ucs2(s: &OsStr,  out: &mut String) {
    // This would finally be an opportunity to dog-food encode_unicode,
    // but I can't be bothered.
    // And if a file name has unmatched surrogate pairs,
    // only displaying BMP characters is probably OK.
    for ucs2 in s.encode_wide() {
        if let Some(c) = char::from_u32(ucs2) {
            out.push(printable_char(c));
        } else {
            let _ = write!(out, "\\x{{{:04X}}}", ucs2);
        }
    }
}

#[cfg(not(any(unix, target_os="wasi", windows)))]
fn not_printable(s: &OsStr,  out: &mut String) {
    // UTF-8 decoding has already failed, so there's no way to get anything from it
    for _ in 0..s.len() {
        out.push(REPLACEMENT_CHARACTER);
    }
}

/// Decode a path and escape control characters including newline and tab.
///
/// The simplest way would be to use `write!(buf, "{:?}", path)` (which escapes newlines),
/// but removing the wrapping quotes is a hassle.
///
/// Therefore do a lot better:
///
/// * Decode each directory or component of the path independently,
///   so that one directory with a non-UTF-8 name won't ruin or affect the rest of the path.
/// * Replace control characters with symbols for them:
///   `␀` (which I doubt any file system allows) `␁..=␟` and `␡`.
/// * Escape newline and tab: They're covered by the point above,
///   and are replaced with `␊` and `␉`.
/// * On unix and wasm, assume Windows-1252 (latin-1) if UTF-8 decoding fails.
///   The control character replacements are still performed.
/// * On Windows, don't decode any multi-unit codepoints in the name if UTF-16 decoding fails;
///   Instead escape them on the form `\x{DBAD}`.
///   The control character replacements are still performed.
///
/// On other operating system families (if there are any?), the name is completely replaced with
/// `�` characters if UTF-8 decoding fails.
pub fn write_printable(path: &Path,  out: &mut String) {
    let mut need_separator = false;
    for part in path.components() {
        if let Component::RootDir = part {
            out.push(MAIN_SEPARATOR);
            need_separator = false;
        } else {
            if need_separator {
                out.push(MAIN_SEPARATOR);
            }
            need_separator = true;

            match part.as_os_str().to_str() {
                Some(utf8) => printable_str(utf8, out),
                #[cfg(any(unix, target_os="wasi"))]
                None => printable_cp1252(part.as_os_str(), out),
                #[cfg(windows)]
                None => printable_ucs2(part.as_os_str(), out),
                #[cfg(not(any(unix, target_os="wasi", windows)))]
                None => not_printable(part.as_os_str(), out),
            }
        }
    }
}
