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

use crate::shared::*;
use crate::thread_info::*;

use std::{fs, io::Read, process::exit};
use std::sync::{Arc, mpsc};

fn read_dir(dir_path: Arc<PrintablePath>,  shared: &Shared,  thread_info: &ThreadInfo) {
    thread_info.set_state(Opening);
    thread_info.set_working_on(Some(dir_path.clone()));
    let entries = fs::read_dir(dir_path.as_path()).unwrap_or_else(|e| {
        eprintln!("Cannot open {}: {}", dir_path, e);
        exit(1);
    });
    thread_info.set_state(Reading);
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| {
            eprintln!("Error getting entry from {}: {}", dir_path, e);
            exit(1);
        });
        let mut entry_path = dir_path.to_path_buf();
        entry_path.push(entry.path());
        let entry_path = Arc::new(PrintablePath::from(entry_path));
        let file_type = entry.file_type().unwrap_or_else(|e| {
            eprintln!("Error getting type of {}: {}", entry_path, e);
            exit(1);
        });
        let file_type = if file_type.is_file() {
            ReadType::File
        } else if file_type.is_dir() {
            ReadType::Directory
        } else {
            let file_type = if file_type.is_symlink() {"symlink"} else {"special file"};
            thread_info.log_message(format!("{} is a {}, skipping.", entry_path, file_type));
            continue;
        };
        let mut lock = shared.to_read.lock().unwrap();
        lock.queue.push((entry_path, file_type));
        drop(lock);
        shared.reader_waker.notify_one();
    }
}

fn read_file(file_path: Arc<PrintablePath>,  shared: &Shared,  thread_info: &ThreadInfo) {
    thread_info.set_state(Opening);
    thread_info.set_working_on(Some(file_path.clone()));
    let mut file = match fs::File::open(file_path.as_path()) {
        Ok(file) => file,
        Err(e) => {
            thread_info.log_message(format!("Cannot open  {}: {}", file_path, e));
            return;
        }
    };

    let mut buffer = shared.buffers.get_buffer(
            shared.buffers.max_single_buffer_size(),
            thread_info
    );
    let (tx, rx) = mpsc::channel();
    // delay inserting until after first read
    let mut insert = Some((file_path, rx));
    let mut incomplete = true;

    while incomplete {
        thread_info.set_state(Reading);
        match file.read(&mut buffer) {
            Err(e) => {
                tx.send(FilePart::Error(e)).unwrap();
                incomplete = false;
            }
            Ok(0) => {
                incomplete = false;
            }
            Ok(length) => {
                tx.send(FilePart::Chunk{buffer, length}).unwrap();
                thread_info.add_bytes(length);
                buffer = shared.buffers.get_buffer(
                        shared.buffers.max_single_buffer_size(),
                        thread_info
                );
            }
        }
        // now insert it
        if let Some(insert) = insert.take() {
            let mut lock = shared.to_hash.lock().unwrap();
            lock.queue.push(insert);
            drop(lock);
            shared.hasher_waker.notify_one();
        }
    }
    shared.buffers.return_buffer(buffer);
}

pub fn read_files(shared: Arc<Shared>, thread_info: &ThreadInfo) {
    let mut lock = shared.to_read.lock().unwrap();

    loop {
        if lock.stop_now {
            thread_info.set_state(Quit);
            thread_info.set_working_on(None);
            break;
        } else if let Some((path, ty)) = lock.queue.pop() {
            lock.working += 1;
            drop(lock);

            match ty {
                ReadType::File => read_file(path, &shared, thread_info),
                ReadType::Directory => read_dir(path, &shared, thread_info),
            }

            lock = shared.to_read.lock().unwrap();
            lock.working -= 1;
        } else if lock.working == 0 {
            thread_info.set_state(Quit);
            thread_info.set_working_on(None);
            break;
        } else {
            thread_info.set_state(Idle);
            thread_info.set_working_on(None);
            lock = shared.reader_waker.wait(lock).unwrap();
        }
    }
}
