/* Copyright 2023 Torbjørn Birch Moltu
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

use std::borrow::Borrow;
use std::cmp::min;
use std::ffi::{OsStr, OsString};
use std::fmt::{self, Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::path::{MAIN_SEPARATOR, Component, Path, PathBuf};
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(target_os="wasi")]
use std::os::wasi::ffi::{OsStrExt, OsStringExt};
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

pub fn is_printable_str(path: &str) -> bool {
    path.bytes().all(|b| b > 0x1f  &&  b != 127 )
}

pub fn as_printable(path: &Path) -> Option<&str> {
    path.to_str().filter(|&s| is_printable_str(s) )
}

fn printable_str(s: &str,  out: &mut String) {
    if is_printable_str(s) {
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
            0x80..=0x9f => CP1252_SPECIAL[b as usize - 0x80],
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

pub fn display_path(printable: &str,  buf: &mut String,  terminal_width: usize) {
    let already_written = buf.chars().rev().take_while(|&c| c != '\n' ).count();
    let max = match terminal_width.checked_sub(already_written) {
        None | Some(0..=15) => !0, // too low, ignore limit
        Some(remaining) => remaining,
    };
    let full_length = printable.chars().count();
    if full_length <= max {
        buf.push_str(printable);
        return;
    }

    let start = buf.len();
    buf.reserve(max);

    // first check if abrreviating long components is enough
    // aaa../bbb../ccc../ddd/eee.txt
    let abbreviatable_part = match Path::new(printable).extension() {
        Some(ext) => &printable[..printable.len()-1-ext.len()],
        None => printable,
    };
    let abbreviate_away = abbreviatable_part.split(MAIN_SEPARATOR)
            .map(|p| p.chars().count().saturating_sub(5) )
            .sum::<usize>() as isize;
    let must_hide = (full_length - max) as isize;
    if abbreviate_away >= must_hide {
        let mut to_abbreviate = must_hide;
        let mut first = true;
        for component in abbreviatable_part.split(MAIN_SEPARATOR) {
            if !first {
                buf.push(MAIN_SEPARATOR);
            }
            first = false;
            if to_abbreviate <= 0 || component.len() < 6 {
                buf.push_str(component);
            } else {
                let chars = component.chars().count() as isize;
                if chars < 6 {
                    buf.push_str(component);
                } else {
                    let abbreviate_now = min(chars-5, to_abbreviate);
                    let show = (chars - abbreviate_now) as usize - 2;
                    let show_bytes = match component.char_indices().nth(show as usize) {
                        Some((pos, _)) => pos,
                        None => component.len(), // should not happen
                    };
                    buf.push_str(&component[..show_bytes]);
                    buf.push_str("..");
                    to_abbreviate -= abbreviate_now;
                }
            }
        }
        // print the extension
        buf.push_str(&printable[abbreviatable_part.len()..]);
        if buf[start..].chars().count() > max {
            panic!("Wrote more than {} available characters\nin ({}): {}\nout ({}): {}\n",
                    max,
                    printable.chars().count(),
                    printable,
                    buf[start..].chars().count(),
                    &buf[start..],
            );
        }
        return;
    }

    // put ... in the middle, but try to put it in front of a path delimiter
    // aaaa/bbbb.../gggg/hhhh
    let mut after_chars = max - max/2;
    let mut first_dir_after_at = None;
    let mut after_start = printable.char_indices()
            .rev()
            .inspect(|&(pos, c)| {
                if c == MAIN_SEPARATOR {
                    first_dir_after_at = Some(pos);
                }
             })
            .nth(after_chars-1)
            .unwrap().0;
    if let Some(at) = first_dir_after_at {
        after_chars += printable[after_start..at].chars().count();
        after_start = at;
    }

    let before_chars = (max - after_chars) - 3;
    let before_end = printable.char_indices().nth(before_chars).unwrap().0;
    buf.push_str(&printable[..before_end]);
    buf.push_str("...");
    buf.push_str(&printable[after_start..]);
    if buf[start..].chars().count() > max {
        panic!("Wrote more than {} available characters\nin ({}): {}\nout ({}): {}\n",
                max,
                printable.chars().count(),
                printable,
                buf[start..].chars().count(),
                &buf[start..],
        );
    }
}

#[derive(Clone, Default)]
pub struct PrintablePath {
    printable: String,
    original: Option<PathBuf>,
}

impl PrintablePath {
    /// Get the lossisly converted printable version of the path.
    pub fn as_str(&self) -> &str {
        &&self.printable
    }

    /// Get the original path as bytes.
    ///
    /// Will fail for non-UTF-8 paths on Windows, but not on UNIX or WASI.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        if let Some(ref original) = &self.original {
            #[cfg(any(unix, target_os="wasi"))]
            {Some(original.as_os_str().as_bytes())}
            #[cfg(not(any(unix, target_os="wasi")))]
            None
        } else {
            Some(self.printable.as_bytes())
        }
    }

    pub fn display_within(&self,  buf: &mut String,  terminal_width: usize) {
        display_path(self.as_str(), buf, terminal_width)
    }

    pub fn as_path(&self) -> &Path {
        match &self.original {
            Some(ref path) => path.as_path(),
            None => Path::new(self.printable.as_str()),
        }
    }

    /// Whether the printable version is identical to the original.
    pub fn is_printable(&self) -> bool {
        self.original.is_none()
    }

    pub fn add(&self,  entry: PathBuf) -> Self {
        let mut entry_path = self.as_path().to_path_buf();
        entry_path.push(entry);
        PrintablePath::from(entry_path)
    }
}

impl Display for PrintablePath {
    fn fmt(&self,  fmtr: &mut Formatter) -> fmt::Result {
        fmtr.write_str(&self.printable)
    }
}

impl Debug for PrintablePath {
    fn fmt(&self,  fmtr: &mut Formatter) -> fmt::Result {
        match &self.original {
            Some(ref path) => Debug::fmt(path, fmtr),
            None => Debug::fmt(&self.printable, fmtr),
        }
    }
}

impl PartialEq for PrintablePath {
    fn eq(&self,  other: &Self) -> bool {
        match (&self.original, &other.original) {
            (Some(ref a), Some(ref b)) => a == b,
            (None, None) => self.printable == other.printable,
            _ => false,
        }
    }
}

impl Eq for PrintablePath {}

impl Hash for PrintablePath {
    fn hash<H: Hasher>(&self,  hasher: &mut H) {
        self.printable.hash(hasher)
    }
}

impl Deref for PrintablePath {
    type Target = Path;
    fn deref(&self) -> &Path {
        self.as_path()
    }
}

impl Borrow<Path> for PrintablePath {
    fn borrow(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<Path> for PrintablePath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl From<PathBuf> for PrintablePath {
    fn from(path: PathBuf) -> PrintablePath {
        let original = match path.into_os_string().into_string() {
            Ok(s) if is_printable_str(&s) => {
                return PrintablePath { printable: s,  original: None };
            },
            // get back original 
            Ok(utf8) => PathBuf::from(utf8),
            Err(not_utf8) => PathBuf::from(not_utf8),
        };

        let mut printable = String::new();
        write_printable(&original, &mut printable);
        PrintablePath { printable,  original: Some(original) }
    }
}

impl From<&Path> for PrintablePath {
    fn from(path: &Path) -> PrintablePath {
        if let Some(s) = as_printable(path) {
            PrintablePath { printable: s.to_string(),  original: None }
        } else {
            let mut printable = String::new();
            write_printable(path, &mut printable);
            PrintablePath { printable,  original: Some(path.to_owned()) }
        }
    }
}

impl From<PrintablePath> for PathBuf {
    fn from(printable: PrintablePath) -> PathBuf {
        match printable.original {
            Some(path_buf) => path_buf,
            None => PathBuf::from(printable.printable),
        }
    }
}

impl TryFrom<Vec<u8>> for PrintablePath {
    type Error = &'static str;
    fn try_from(path: Vec<u8>) -> Result<Self, Self::Error> {
        match String::from_utf8(path) {
            Ok(utf8) if is_printable_str(&utf8) => {
                Ok(PrintablePath { printable: utf8,  original: None })
            },
            Ok(utf8) => {
                let mut printable = String::new();
                write_printable(Path::new(&utf8), &mut printable);
                Ok(PrintablePath { printable,  original: Some(PathBuf::from(utf8)) })
            },
            #[cfg(any(unix, target_os="wasi"))]
            Err(err) => {
                let path = PathBuf::from(OsString::from_vec(err.into_bytes()));
                let mut printable = String::new();
                write_printable(&path, &mut printable);
                Ok(PrintablePath { printable, original: Some(path) })
            },
            #[cfg(not(any(unix, target_os="wasi")))]
            Err(_) => Err("Cannot convert from non-UTF-8 paths on Windows")
        }
    }
}
