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

use std::{fs, io::Read, num::NonZeroU16, path::PathBuf, process::exit, thread};
use std::sync::{Arc, Mutex, Condvar};

extern crate clap;
use clap::Parser;

extern crate sha2;
use sha2::{Sha256, Digest};

#[derive(Parser, Debug)]
#[command(arg_required_else_help=true, author, version, about, long_about=None)]
struct Args {
    #[arg(short='t', long, value_name="NUBMER_OF_HASHER_THREADS", default_value_t=NonZeroU16::new(4).unwrap())]
    hasher_threads: NonZeroU16,
    #[arg(required = true)]
    roots: Vec<PathBuf>,
}

#[derive(Default, Debug)]
struct FilePoolData {
    stop: bool,
    to_process: Vec<PathBuf>,
}

#[derive(Default, Debug)]
struct FilePool {
    data: Mutex<FilePoolData>,
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
    let args = Args::parse();

    let pool = Arc::new(FilePool::default());
    let mut threads = Vec::with_capacity(u16::from(args.hasher_threads).into());
    for n in (1..=args.hasher_threads.into()).into_iter() {
        let pool = pool.clone();
        let builder = thread::Builder::new().name(format!("hasher_{}", n));
        let thread = builder.spawn(move || {
            let mut buf = [0u8; 64*1024];
            let mut hasher = Sha256::new();
            'relock: loop {
                let mut lock = pool.data.lock().unwrap();
                'reuse: loop {
                    if let Some(file) = lock.to_process.pop() {
                        drop(lock);
                        println!("thread {} reading {}", n, file.display());
                        hash_file(file, &mut buf, &mut hasher);
                        break 'reuse;
                    } else if lock.stop {
                        break 'relock;
                    } else {
                        lock = pool.waker.wait_while(lock, |lock| lock.to_process.is_empty() ).unwrap();
                    }
                }
            }
        }).unwrap();
        threads.push(thread);
    }

    for dir in args.roots {
        let dir_path = PathBuf::from(dir);
        let dir_path = fs::canonicalize(&dir_path).unwrap_or_else(|e| {
            eprintln!("Cannot canoniicalize {}: {}", dir_path.display(), e);
            exit(1);
        });
        let entries = fs::read_dir(&dir_path).unwrap_or_else(|e| {
            eprintln!("Cannot open {}: {}", dir_path.display(), e);
            exit(1);
        });
        for entry in entries {
            let entry = entry.unwrap_or_else(|e| {
                eprintln!("Error getting entry from {}: {}", dir_path.display(), e);
                exit(1);
            });
            let mut entry_path = dir_path.clone();
            entry_path.push(entry.path());
            let file_type = entry.file_type().unwrap_or_else(|e| {
                eprintln!("Error getting type of {}: {}", entry_path.display(), e);
                exit(1);
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
            let mut lock = pool.data.lock().unwrap();
            lock.to_process.push(entry_path);
            drop(lock);
            pool.waker.notify_one();
        }
    }

    let mut lock = pool.data.lock().unwrap();
    lock.stop = true;
    drop(lock);
    for thread in threads {
        thread.join().unwrap();
    }
}
