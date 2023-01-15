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

extern crate clap;
#[cfg(target_os="linux")]
extern crate ioprio;
extern crate sha2;
extern crate thread_priority;

mod available_buffers;

use std::{fs, io::Read, process::exit, thread};
use std::num::{NonZeroU16, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, mpsc};

use clap::Parser;

use sha2::{Sha256, Digest};

use thread_priority::{ThreadBuilder, ThreadPriority};
#[cfg(unix)]
use thread_priority::unix::{NormalThreadSchedulePolicy, ThreadSchedulePolicy};

#[derive(Parser, Debug)]
#[command(arg_required_else_help=true, author, version, about, long_about=None)]
struct Args {
    #[arg(short, long, value_name="NUMBER_OF_IO_THREADS", default_value_t=NonZeroU16::new(2).unwrap())]
    io_threads: NonZeroU16,
    #[arg(short='t', long, value_name="NUBMER_OF_HASHER_THREADS", default_value_t=NonZeroU16::new(4).unwrap())]
    hasher_threads: NonZeroU16,
    #[arg(short='b', long, value_name="MAX_BUFFER_SIZE_IN_KB", default_value_t=NonZeroUsize::new(1024).unwrap())]
    max_buffer_size: NonZeroUsize,
    #[arg(short, long, value_name="MAX_MEMORY_USAGE_OF_BUFFERS_IN_MB")]
    max_buffers_memory: Option<NonZeroUsize>,
    #[arg(required = true)]
    roots: Vec<PathBuf>,
}

/// A vector that is always fully initialized.
#[derive(Default, Debug)]
struct UsedBuffer {
    buffer: Box<[u8]>,
    length: usize,
}

#[derive(Default, Debug)]
struct FilePoolData {
    stop: bool,
    to_read: Vec<PathBuf>,
    to_hash: Vec<(PathBuf, mpsc::Receiver<UsedBuffer>)>,
}

#[derive(Debug)]
struct FilePool {
    data: Mutex<FilePoolData>,
    wake_reader: Condvar,
    wake_hasher: Condvar,
    buffers: available_buffers::AvailableBuffers,
}

fn read_file_part(
        file_path: &Path,  file: &mut fs::File,  pos: &mut usize,
        tx: &mut mpsc::Sender<UsedBuffer>,
        thread_name: &str,
        buffer_pool: &available_buffers::AvailableBuffers,
) -> bool {
    let mut buf = buffer_pool.get_buffer(buffer_pool.max_single_buffer_size(), thread_name);
    match file.read(&mut buf) {
        Err(e) => {
            println!("{} reading failed after {} bytes: {}", file_path.display(), *pos, e);
            let empty = UsedBuffer {
                buffer: Box::default(),
                length: 0,
            };
            tx.send(empty).unwrap();
            buffer_pool.return_buffer(buf);
            true
        }
        Ok(0) => {
            buffer_pool.return_buffer(buf);
            true
        }
        Ok(n) => {
            let buf = UsedBuffer {
                buffer: buf,
                length: n,
            };
            *pos += n;
            tx.send(buf).unwrap();
            false
        }
    }
}

fn read_files(pool: Arc<FilePool>, thread_name: String) {
    enum State {
        IncompleteFile{path: PathBuf,  file: fs::File,  pos: usize,  tx: mpsc::Sender<UsedBuffer>},
        NextFile,
    }
    let mut state = State::NextFile;

    'relock: loop {
        let mut lock = pool.data.lock().unwrap();

        'reuse: loop {
            if let State::IncompleteFile{ path, file, pos, tx} = &mut state {
                drop(lock);
                if read_file_part(
                        &path, file, pos, tx,
                        &thread_name, &pool.buffers,
                    ) {
                    state = State::NextFile;
                }
                continue 'relock;
            } else if let Some(path) = lock.to_read.pop() {
                drop(lock);
                println!("{} reading {}", thread_name, path.display());
                let mut file = fs::File::open(&path).unwrap_or_else(|e| {
                    eprintln!("Cannot open  {}: {}", path.display(), e);
                    exit(2);
                });
                let (mut tx, rx) = mpsc::channel();
                let mut pos = 0;

                if !read_file_part(
                        &path, &mut file, &mut pos, &mut tx,
                        &thread_name, &pool.buffers,
                    ) {
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
    'relock: loop {
        let mut lock = pool.data.lock().unwrap();

        'reuse: loop {
            if let Some((path, rx)) = lock.to_hash.pop() {
                let buf = match rx.try_recv() {
                    Ok(buf) => buf,
                    Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => {
                        println!("{} is empty", path.display());
                        continue 'reuse;
                    },
                };
                drop(lock);

                println!("hasher thread {} hashing {}", hasher_thread_number, path.display());
                hasher.update(&buf.buffer[..buf.length]);
                let mut size = buf.length;
                pool.buffers.return_buffer(buf.buffer);

                for buf in rx.into_iter() {
                    if buf.length == 0 {
                        // IO error
                        hasher.reset();
                        continue 'relock;
                    }
                    hasher.update(&buf.buffer[..buf.length]);
                    size += buf.length;
                    pool.buffers.return_buffer(buf.buffer);
                }

                let hash_result = hasher.finalize_reset();
                println!("{} {} bytes {:#x}", path.display(), size, hash_result);
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
    let pool = Arc::new(FilePool {
        data: Mutex::new(FilePoolData::default()),
        wake_reader: Condvar::new(),
        wake_hasher: Condvar::new(),
        buffers: available_buffers::AvailableBuffers::new(
            match args.max_buffers_memory {
                Some(size) => usize::from(size).saturating_mul(1024*1024),
                None => isize::MAX as usize,
            },
            usize::from(args.max_buffer_size).saturating_mul(1024),
        ),
    });

    // Keep my desktop responsive
    #[cfg(target_os="linux")]
    {
        let this = ioprio::Target::Process(ioprio::Pid::this());
        let priority = ioprio::Class::BestEffort(ioprio::BePriorityLevel::lowest());
        if let Err(e) = ioprio::set_priority(this, ioprio::Priority::new(priority)) {
            eprintln!("Failed to set IO priority to {:?}: {}", priority, e);
        }
    }

    let mut hasher_threads = Vec::with_capacity(u16::from(args.hasher_threads).into());
    for n in (1..=args.hasher_threads.into()).into_iter() {
        let pool = pool.clone();
        let builder = ThreadBuilder::default()
            .name(format!("hasher_{}", n))
            .priority(ThreadPriority::Min);
        #[cfg(unix)]
        let builder = builder.policy(
                ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Batch)
        );
        let thread = builder.spawn(move |priority_result| {
            if let Err(e) = priority_result {
                eprintln!("Failed lowering thread priority: {:?}", e);
            }
            hash_files(pool, n)
        }).unwrap();
        hasher_threads.push(thread);
    }

    let mut io_threads = Vec::with_capacity(u16::from(args.io_threads).into());
    for n in (1..=args.io_threads.into()).into_iter() {
        let thread_name = format!("io_{}", n);
        let pool = pool.clone();
        let builder = thread::Builder::new().name(thread_name.clone());
        let thread = builder.spawn(move || read_files(pool, thread_name) ).unwrap();
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
