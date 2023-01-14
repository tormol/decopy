/* Copyright 2023 Torbjørn Birch Moltu
 *
 * This program is free software: you can redistribute it and/or modify it under the
 * terms of the GNU General Public License as published by the Free Software Foundation,
 * either version 3 of the License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
 * without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
 * See the GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along with this program.
 * If not, see <https://www.gnu.org/licenses/>.
 */

extern crate sha2;

use std::{env, process::exit, fs, path::PathBuf, io::Read};

use sha2::{Sha256, Digest};

fn usage(name : &str) -> ! {
    eprintln!("Usage: {} <how many threads to use for hashing> <directory>...", name);
    exit(1);
}

fn main() {
    let mut args = env::args_os();
    let name = match args.next() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => String::new(),
    };
    if args.len() == 0 {
        usage(&name)
    }

    let mut buf = [0u8; 64*1024];
    let mut hasher = Sha256::new();
    for dir in args {
        let dir_path = PathBuf::from(dir);
        let dir_path = fs::canonicalize(&dir_path).unwrap_or_else(|e| {
            eprintln!("Cannot canoniicalize {}: {}", dir_path.display(), e);
            exit(2);
        });
        let entries = fs::read_dir(&dir_path).unwrap_or_else(|e| {
            eprintln!("Cannot open {}: {}", dir_path.display(), e);
            exit(2);
        });
        for entry in entries {
            let entry = entry.unwrap_or_else(|e| {
                eprintln!("Error getting entry from {}: {}", dir_path.display(), e);
                exit(2);
            });
            let mut entry_path = dir_path.clone();
            entry_path.push(entry.path());
            let entry_string = entry_path.display();
            let file_type = entry.file_type().unwrap_or_else(|e| {
                eprintln!("Error getting type of {}: {}", entry_string, e);
                exit(2);
            });
            if !file_type.is_file() {
                let file_type = match file_type {
                    t if t.is_dir() => "directory",
                    t if t.is_symlink() => "symlink",
                    _ => "special file",
                };
                println!("{} is a {}, skipping.", entry_string, file_type);
                continue;
            }
            let mut file = fs::File::open(&entry_path).unwrap_or_else(|e| {
                eprintln!("Cannot open  {}: {}", entry_string, e);
                exit(2);
            });
            let mut read = 0usize;
            loop {
                match file.read(&mut buf) {
                    Err(e) => {
                        println!("{} reading failed after {} bytes: {}", entry_string, read, e);
                        break;
                    }
                    Ok(0) => {
                        let hash_result = hasher.finalize_reset();
                        println!("{} {} bytes {:#x}", entry_string, read, hash_result);
                        break;
                    }
                    Ok(n) => {
                        hasher.update(&buf[..n]);
                        read += n;
                    }
                }
            }
        }
    }
}
