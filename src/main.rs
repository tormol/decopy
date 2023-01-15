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
mod bytes;
use bytes::*;
mod shared;
use shared::*;
mod read;
use read::*;
mod hash;
use hash::*;

use std::{fs, num::NonZeroU16, process::exit, sync::Arc, thread, time::Duration};
use std::path::{Path, PathBuf};

use clap::Parser;

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
    #[arg(short='b', long, default_value_t=Bytes::new(1<<20))]
    max_buffer_size: Bytes,
    #[arg(short, long, value_name="MAX_MEMORY_USAGE_OF_BUFFERS", default_value_t=Bytes::new(1<<30))]
    max_buffers_memory: Bytes,
    #[arg(required = true)]
    roots: Vec<PathBuf>,
}

fn main() {
    let args = Args::parse();
    let buffers = AvailableBuffers::new(
            args.max_buffers_memory.into(),
            args.max_buffer_size.into(),
    ).unwrap_or_else(|e| {
        eprintln!("{}", e);
        exit(2);
    });
    let shared = Shared::new(buffers);

    // check root directories and add them to queue
    let mut to_read = shared.to_read.lock().unwrap();
    for dir_path in args.roots {
        let dir_path = fs::canonicalize(&dir_path).unwrap_or_else(|e| {
            eprintln!("Cannot canoniicalize {}: {}", dir_path.display(), e);
            exit(1);
        });
        to_read.queue.push((Arc::<Path>::from(dir_path), ReadType::Directory));
    }
    drop(to_read);

    // Keep my desktop responsive
    #[cfg(target_os="linux")]
    {
        let this = ioprio::Target::Process(ioprio::Pid::this());
        let priority = ioprio::Class::BestEffort(ioprio::BePriorityLevel::lowest());
        if let Err(e) = ioprio::set_priority(this, ioprio::Priority::new(priority)) {
            eprintln!("Failed to set IO priority to {:?}: {}", priority, e);
        }
    }

    // start hasher threads
    let mut hasher_threads = Vec::with_capacity(u16::from(args.hasher_threads).into());
    for n in (1..=args.hasher_threads.into()).into_iter() {
        let thread_name = format!("hasher_{}", n);
        let shared = shared.clone();
        let builder = ThreadBuilder::default()
            .name(thread_name.clone())
            .priority(ThreadPriority::Min);
        #[cfg(unix)]
        let builder = builder.policy(
                ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Batch)
        );
        let thread = builder.spawn(move |priority_result| {
            if let Err(e) = priority_result {
                eprintln!("Failed lowering thread priority: {:?}", e);
            }
            hash_files(shared, thread_name)
        }).unwrap();
        hasher_threads.push(thread);
    }

    // start IO threads
    let mut io_threads = Vec::with_capacity(u16::from(args.io_threads).into());
    for n in (1..=args.io_threads.into()).into_iter() {
        let thread_name = format!("io_{}", n);
        let shared = shared.clone();
        let builder = thread::Builder::new().name(thread_name.clone());
        let thread = builder.spawn(move || read_files(shared, thread_name) ).unwrap();
        io_threads.push(thread);
    }

    // wait for IO threads to finish
    loop {
        eprintln!("buffer memory allocated: {:#}",
                Bytes::new(shared.buffers.current_buffers_size()),
        );

        let lock = shared.to_read.lock().unwrap();
        if (lock.queue.is_empty() && lock.working == 0) || lock.stop_now {
            break;
        }
        drop(lock);
        thread::sleep(Duration::from_millis(500));
    }

    // tell hashers they can stop now
    let mut lock = shared.to_hash.lock().unwrap();
    lock.stop_when_empty = true;
    drop(lock);

    shared.reader_waker.notify_all();
    for thread in io_threads {
        eprintln!("joining reader");
        thread.join().unwrap();
    }

    shared.hasher_waker.notify_all();
    for thread in hasher_threads {
        eprintln!("joining hasher");
        thread.join().unwrap();
    }
}
