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

use std::{fs, io::Read};
use std::sync::{Arc, mpsc};

fn read_dir(dir_path: Arc<PrintablePath>,  shared: &Shared,  thread_info: &ThreadInfo) {
    thread_info.set_state(Opening);
    thread_info.set_working_on(Some(dir_path.clone()));
    let entries = match fs::read_dir(dir_path.as_path()) {
        Ok(entries) => entries,
        Err(e) => {
            thread_info.log_message(format!("Cannot open {}: {}", dir_path, e));
            return;
        }
    };
    thread_info.set_state(Reading);
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                thread_info.log_message(format!("Error getting entry from {}: {}", dir_path, e));
                continue;
            }
        };
        let mut entry_path = dir_path.to_path_buf();
        entry_path.push(entry.path());
        let entry_path = Arc::new(PrintablePath::from(entry_path));

        let file_type = match entry.file_type() {
            Ok(typ) => typ,
            Err(e) => {
                thread_info.log_message(format!("Error getting type of {}: {}", entry_path, e));
                continue;
            }
        };
        let to_read = if file_type.is_file() {
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(e) => {
                    thread_info.log_message(format!("Error getting metadata of {}: {}",
                            entry_path,
                            e,
                    ));
                    continue;
                }
            };
            let modified = match metadata.modified() {
                Ok(modified) => modified,
                Err(e) => match metadata.created() {
                    Ok(created) => {
                        thread_info.log_message(format!(
                                "Cannot get modification time for {}: {}, using creation time",
                                entry_path,
                                e,
                        ));
                        created
                    },
                    Err(_) => {
                        thread_info.log_message(format!(
                                "Cannot get modification or creation time for {}: {}",
                                entry_path,
                                e,
                        ));
                        continue;
                    },
                },
            };
            let modified = PrintableTime::from(modified).clamp_to_yyyy();

            let unread = UnreadFile { path: entry_path, modified, size: metadata.len(), };
            if shared.previously_read.check_unchanged(&unread) {
                continue;
            }
            ToRead::File(unread)
        } else if file_type.is_dir() {
            ToRead::Directory(entry_path)
        } else {
            let file_type = if file_type.is_symlink() {"symlink"} else {"special file"};
            thread_info.log_message(format!("{} is a {}, skipping.", entry_path, file_type));
            continue;
        };

        let mut lock = shared.to_read.lock().unwrap();
        lock.queue.push(to_read);
        drop(lock);
        shared.reader_waker.notify_one();
    }
}

fn read_file(file_info: UnreadFile,  shared: &Shared,  thread_info: &ThreadInfo) {
    thread_info.set_state(Opening);
    thread_info.set_working_on(Some(file_info.path.clone()));
    let mut file = match fs::File::open(file_info.path.as_path()) {
        Ok(file) => file,
        Err(e) => {
            thread_info.log_message(format!("Cannot open  {}: {}", file_info.path, e));
            return;
        }
    };

    let mut remaining_size = usize::try_from(file_info.size)
            .unwrap_or(shared.buffers.max_single_buffer_size());
    let mut buffer = shared.buffers.get_buffer(remaining_size, thread_info);

    let (tx, rx) = mpsc::channel();
    // delay inserting until after first read
    let mut insert = Some((file_info, rx));
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
                remaining_size = match remaining_size.checked_sub(length) {
                    Some(remaining) => remaining,
                    None => shared.buffers.max_single_buffer_size(),
                };
                buffer = shared.buffers.get_buffer(remaining_size, thread_info);
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
        } else if let Some(to_read) = lock.queue.pop() {
            lock.working += 1;
            drop(lock);

            match to_read {
                ToRead::File(file) => read_file(file, &shared, thread_info),
                ToRead::Directory(path) => read_dir(path, &shared, thread_info),
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
