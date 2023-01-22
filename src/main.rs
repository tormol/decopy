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

extern crate arc_swap;
extern crate clap;
#[cfg(target_os="linux")]
extern crate ioprio;
extern crate sha2;
extern crate thread_priority;

mod path_decoding;
mod thread_info;
mod available_buffers;
mod bytes;
mod shared;
mod read;
mod hash;

use bytes::*;
use path_decoding::*;
use hash::*;
use read::*;
use shared::*;
use thread_info::*;

use std::{fmt::Write, fs, num::NonZeroU16, path::PathBuf, process::exit, thread};
use std::io::{Write as ioWrite, stderr};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

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

    let (log_channel, log_messages) = mpsc::channel::<String>();
    let io_info = create_info_array(
            "io",
            u16::from(args.io_threads).into(),
            log_channel.clone()
    );
    let hasher_info = create_info_array(
            "hasher",
            u16::from(args.hasher_threads).into(),
            log_channel
    );

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
        to_read.queue.push((Arc::<PathBuf>::from(dir_path), ReadType::Directory));
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
    for i in 0..hasher_info.len() {
        let shared = shared.clone();
        let hasher_info = hasher_info.clone();
        let builder = ThreadBuilder::default()
                .name(hasher_info[i].name())
                .priority(ThreadPriority::Min);
        #[cfg(unix)]
        let builder = builder.policy(
                ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Batch)
        );
        let thread = builder.spawn(move |priority_result| {
            if let Err(e) = priority_result {
                eprintln!("Failed lowering thread priority: {:?}", e);
            }
            let info = &hasher_info[i];
            hash_files(shared, info)
        }).unwrap();
        hasher_threads.push((thread, 0usize));
    }

    // start IO threads
    let mut io_threads = Vec::with_capacity(u16::from(args.io_threads).into());
    for i in 0..io_info.len() {
        let shared = shared.clone();
        let io_info = io_info.clone();
        let builder = thread::Builder::new().name(io_info[i].name().to_string());
        let thread = builder.spawn(move || {
            let info = &io_info[i];
            read_files(shared, info);
        }).unwrap();
        io_threads.push((thread, 0usize));
    }

    // buffer output but also allow lookback
    let mut display: String;
    // show state of each thread while wait for IO threads to finish
    // undo the first "erase last frame"
    display = "\n".repeat(io_info.len()+hasher_info.len()+1);

    let mut prev = Instant::now();
    loop {
        let now = Instant::now();
        let mut read = 0;
        for (info, (_, prev_read)) in io_info.iter().zip(&mut io_threads) {
            let current = info.processed_bytes();
            read += current.0 - *prev_read;
            *prev_read = current.0;
        }
        let mut hashed = 0;
        for (info, (_, prev_hashed)) in hasher_info.iter().zip(&mut hasher_threads) {
            let current = info.processed_bytes();
            hashed += current.0 - *prev_hashed;
            *prev_hashed = current.0;
        }

        // go to beginning of line n up, and erease to end of screen
        write!(&mut display, "\u{1b}[{}F\u{1b}[0J", io_info.len()+hasher_info.len()+1).unwrap();
        // print logs (these are not erased, and will be visible in scrollback)
        while let Ok(message) = log_messages.try_recv() {
            display.push_str(&message);
            display.push('\n');
        }

        // display state of each thread
        for thread in io_info.iter().chain(hasher_info.iter()) {
            write!(&mut display, "{:10} {:?}", thread.name(), thread.state()).unwrap();
            thread.view_working_on(|path| {
                if let Some(path) = path {
                    display.push(' ');
                    write_printable(path, &mut display);
                }
            });
            display.push('\n');
        }

        read = read*(now-prev).as_micros() as usize/1_000_000;
        hashed = hashed*(now-prev).as_micros() as usize/1_000_000;
        prev = now;
        writeln!(&mut display,
                "reading {:#}/s, hashing {:#}/s, buffer memory allocated: {:#}",
                Bytes::new(read),
                Bytes::new(hashed),
                Bytes::new(shared.buffers.current_buffers_size()),
        ).unwrap();

        let lock = shared.to_read.lock().unwrap();
        if (lock.queue.is_empty() && lock.working == 0) || lock.stop_now {
            break;
        }
        drop(lock);
        stderr().write_all(display.as_bytes()).unwrap();
        display.clear();
        thread::sleep(Duration::from_millis(333)-(Instant::now()-now));
    }

    // tell hashers they can stop now
    let mut lock = shared.to_hash.lock().unwrap();
    lock.stop_when_empty = true;
    drop(lock);

    shared.reader_waker.notify_all();
    for (thread, _) in io_threads {
        eprintln!("joining reader");
        thread.join().unwrap();
    }

    shared.hasher_waker.notify_all();
    for (thread, _) in hasher_threads {
        eprintln!("joining hasher");
        thread.join().unwrap();
    }
}
