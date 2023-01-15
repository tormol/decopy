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

use std::{fs, io::Read, mem, process::exit, thread};
use std::num::{NonZeroU16, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, mpsc};

extern crate clap;
use clap::Parser;

extern crate sha2;
use sha2::{Sha256, Digest};

#[derive(Parser, Debug)]
#[command(arg_required_else_help=true, author, version, about, long_about=None)]
struct Args {
    #[arg(short, long, value_name="NUMBER_OF_IO_THREADS", default_value_t=NonZeroU16::new(2).unwrap())]
    io_threads: NonZeroU16,
    #[arg(short='t', long, value_name="NUBMER_OF_HASHER_THREADS", default_value_t=NonZeroU16::new(4).unwrap())]
    hasher_threads: NonZeroU16,
    #[arg(short='b', long, value_name="MAX_BUFFER_SIZE_IN_KB", default_value_t=NonZeroUsize::new(1024).unwrap())]
    max_buffer_size: NonZeroUsize,
    #[arg(required = true)]
    roots: Vec<PathBuf>,
}

/// A vector that is always fully initialized.
#[derive(Default, Debug)]
struct UsedBuffer {
    buffer: Box<[u8]>,
    length: usize,
    position_in_file: usize,
}

#[derive(Default, Debug)]
struct FilePoolData {
    stop: bool,
    to_read: Vec<PathBuf>,
    to_hash: Vec<(PathBuf, mpsc::Receiver<UsedBuffer>)>,
    unused_buffers: Vec<Box<[u8]>>,
}

#[derive(Default, Debug)]
struct FilePool {
    max_buffer_size: usize,
    data: Mutex<FilePoolData>,
    wake_reader: Condvar,
    wake_hasher: Condvar,
}

fn read_file_part(
        file_path: &Path,  file: &mut fs::File,
        pos: &mut usize,  buf: &mut Box<[u8]>,
        tx: &mut mpsc::Sender<UsedBuffer>,
) -> bool {
    match file.read(buf) {
        Err(e) => {
            println!("{} reading failed after {} bytes: {}", file_path.display(), *pos, e);
            let empty = UsedBuffer {
                buffer: Box::default(),
                length: 0,
                position_in_file: *pos,
            };
            tx.send(empty).unwrap();
            true
        }
        Ok(0) => {
            true
        }
        Ok(n) => {
            let buf = UsedBuffer {
                buffer: mem::replace(buf, Box::default()),
                length: n,
                position_in_file: *pos,
            };
            *pos += n;
            tx.send(buf).unwrap();
            false
        }
    }
}

fn read_files(pool: Arc<FilePool>, io_thread_number: u16) {
    enum State {
        IncompleteFile{path: PathBuf,  file: fs::File,  pos: usize,  tx: mpsc::Sender<UsedBuffer>},
        NextFile,
    }
    let mut state = State::NextFile;
    let mut buf = Box::<[u8]>::default();

    'relock: loop {
        let mut lock = pool.data.lock().unwrap();

        'reuse: loop {
            if buf.is_empty() {
                buf = lock.unused_buffers.pop().unwrap_or_else(|| {
                    vec![0u8; pool.max_buffer_size].into_boxed_slice()
                });
            }
            if let State::IncompleteFile{ path, file, pos, tx} = &mut state {
                drop(lock);
                if read_file_part(&path, file, pos, &mut buf, tx) {
                    state = State::NextFile;
                }
                continue 'relock;
            } else if let Some(path) = lock.to_read.pop() {
                drop(lock);
                println!("io thread {} reading {}", io_thread_number, path.display());
                let mut file = fs::File::open(&path).unwrap_or_else(|e| {
                    eprintln!("Cannot open  {}: {}", path.display(), e);
                    exit(2);
                });
                let (mut tx, rx) = mpsc::channel();
                let mut pos = 0;

                if !read_file_part(&path, &mut file, &mut pos, &mut buf, &mut tx) {
                    state = State::IncompleteFile{ path: path.clone(), file, pos, tx };
                }
                lock = pool.data.lock().unwrap();
                lock.to_hash.push((path, rx));
                // there's some hurry up and wait here,
                // but I'll have more hasher threads than IO threads,
                // so this thread not having to immediately relock is more important.
                pool.wake_hasher.notify_one();
                continue 'reuse;
            } else if lock.stop {
                break 'relock;
            } else {
                lock = pool.wake_reader.wait(lock).unwrap();
            }
        }
    }
}

fn hash_files(pool: Arc<FilePool>, hasher_thread_number: u16) {
    let mut hasher = Sha256::new();
    let mut buf = UsedBuffer::default();
    'relock: loop {
        let mut lock = pool.data.lock().unwrap();

        'reuse: loop {
            if !buf.buffer.is_empty() {
                lock.unused_buffers.push(buf.buffer);
                buf.buffer = Box::default();
            }
            if let Some((path, rx)) = lock.to_hash.pop() {
                buf = match rx.try_recv() {
                    Ok(buf) => buf,
                    Err(mpsc::TryRecvError::Empty) => {
                        println!("{} is empty", path.display());
                        continue 'reuse;
                    },
                    Err(e) => {
                        drop(lock);
                        panic!("first recv unexpectedly failed: {}", e);
                    },
                };
                drop(lock);

                println!("hasher thread {} hashing {}", hasher_thread_number, path.display());
                hasher.update(&buf.buffer[..buf.length]);

                for next in rx.into_iter() {
                    if next.length == 0 {
                        hasher.reset();
                        continue 'relock;
                    }

                    lock = pool.data.lock().unwrap();
                    lock.unused_buffers.push(buf.buffer);
                    drop(lock);
                    buf = next;
                    hasher.update(&buf.buffer[..buf.length]);
                }

                let hash_result = hasher.finalize_reset();
                println!("{} {} bytes {:#x}",
                        path.display(),
                        buf.position_in_file+buf.length,
                        hash_result
                );
                continue 'relock;
            } else if lock.stop {
                break 'relock;
            } else {
                lock = pool.wake_hasher.wait(lock).unwrap();
            }
        }
    }
}

fn main() {
    let args = Args::parse();

    let mut pool = FilePool::default();
    pool.max_buffer_size = usize::from(args.max_buffer_size) * 1024;
    let pool = Arc::new(pool);

    let mut hasher_threads = Vec::with_capacity(u16::from(args.hasher_threads).into());
    for n in (1..=args.hasher_threads.into()).into_iter() {
        let pool = pool.clone();
        let builder = thread::Builder::new().name(format!("hasher_{}", n));
        let thread = builder.spawn(move || hash_files(pool, n) ).unwrap();
        hasher_threads.push(thread);
    }

    let mut io_threads = Vec::with_capacity(u16::from(args.io_threads).into());
    for n in (1..=args.io_threads.into()).into_iter() {
        let pool = pool.clone();
        let builder = thread::Builder::new().name(format!("io_{}", n));
        let thread = builder.spawn(move || read_files(pool, n) ).unwrap();
        io_threads.push(thread);
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
            let mut lock = pool.data.lock().unwrap();
            lock.to_read.push(entry_path);
            drop(lock);
            pool.wake_reader.notify_one();
        }
    }

    let mut lock = pool.data.lock().unwrap();
    lock.stop = true;
    drop(lock);
    for thread in io_threads {
        thread.join().unwrap();
    }
    for thread in hasher_threads {
        thread.join().unwrap();
    }
}
