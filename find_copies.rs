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

use std::{env, fs, io::Read, path::PathBuf, process::exit, thread};
use std::sync::{Arc, Mutex, Condvar};
use std::sync::atomic::{AtomicBool, Ordering};
use sha2::{Sha256, Digest};

fn usage(name: &str) -> ! {
    eprintln!("Usage: {} <how many threads to use for hashing> <directory>...", name);
    exit(1);
}

#[derive(Default, Debug)]
struct FilePool {
    stop: AtomicBool,
    to_process: Mutex<Vec<PathBuf>>,
    waker: Condvar,
}

fn hash_file(file_path: PathBuf,  buf: &mut[u8],  hasher: &mut Sha256) {
    let mut file = fs::File::open(&file_path).unwrap_or_else(|e| {
        eprintln!("Cannot open  {}: {}", file_path.display(), e);
        exit(2);
    });
    let mut read = 0usize;
    loop {
        match file.read(buf) {
            Err(e) => {
                hasher.reset();
                println!("{} reading failed after {} bytes: {}", file_path.display(), read, e);
                break;
            }
            Ok(0) => {
                let hash_result = hasher.finalize_reset();
                println!("{} {} bytes {:#x}", file_path.display(), read, hash_result);
                break;
            }
            Ok(n) => {
                hasher.update(&buf[..n]);
                read += n;
            }
        }
    }
}

fn main() {
    let mut args = env::args_os();
    let name = match args.next() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => String::new(),
    };
    let hasher_threads = args.next().unwrap_or_else(|| usage(&name) )
        .to_str().unwrap_or_else(|| usage(&name) )
        .parse::<u32>().unwrap_or_else(|_| usage(&name) );
    if args.len() == 0 {
        usage(&name)
    }

    let pool = Arc::new(FilePool::default());
    let mut threads = Vec::with_capacity(hasher_threads as usize);
    for n in (1..=hasher_threads).into_iter() {
        let pool = pool.clone();
        let builder = thread::Builder::new().name(format!("hasher_{}", n));
        let thread = builder.spawn(move || {
            let mut buf = [0u8; 64*1024];
            let mut hasher = Sha256::new();
            'relock: loop {
                let mut lock = pool.to_process.lock().unwrap();
                'reuse: loop {
                    if let Some(file) = lock.pop() {
                        drop(lock);
                        println!("thread {} reading {}", n, file.display());
                        hash_file(file, &mut buf, &mut hasher);
                        break 'reuse;
                    } else if pool.stop.load(Ordering::Relaxed) {
                        break 'relock;
                    } else {
                        lock = pool.waker.wait_while(lock, |lock| lock.is_empty() ).unwrap();
                    }
                }
            }
        }).unwrap();
        threads.push(thread);
    }

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
            let file_type = entry.file_type().unwrap_or_else(|e| {
                eprintln!("Error getting type of {}: {}", entry_path.display(), e);
                exit(2);
            });
            if !file_type.is_file() {
                let file_type = match file_type {
                    t if t.is_dir() => "directory",
                    t if t.is_symlink() => "symlink",
                    _ => "special file",
                };
                println!("{} is a {}, skipping.", entry_path.display(), file_type);
                continue;
            }
            println!("found file {}", entry_path.display());
            let mut lock = pool.to_process.lock().unwrap();
            lock.push(entry_path);
            drop(lock);
            pool.waker.notify_one();
        }
    }

    let lock = pool.to_process.lock().unwrap();
    pool.stop.store(true, Ordering::Relaxed);
    drop(lock);
    for thread in threads {
        thread.join().unwrap();
    }
}
