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

use crate::path_decoding::*;
use crate::shared::*;
use crate::thread_info::*;

use std::sync::{Arc, mpsc};

use sha2::{Sha256, Digest};

fn hash_file(
        file_path: Arc<PrintablePath>,  parts: mpsc::Receiver<FilePart>,
        hasher: &mut sha2::Sha256,  thread_info: &ThreadInfo,
        buffers: &AvailableBuffers,
) {
    let mut position = 0;

    for part in parts.into_iter() {
        match part {
            FilePart::Chunk{buffer, length} => {
                if position == 0 {
                    thread_info.set_state(Hashing);
                    thread_info.set_working_on(Some(file_path.clone()));
                }
                hasher.update(&buffer[..length]);
                thread_info.add_bytes(length);
                position += length;
                buffers.return_buffer(buffer);
            },
            FilePart::Error(e) => {
                thread_info.log_message(format!("{} got IO error after {} bytes: {}",
                        file_path,
                        position,
                        e
                ));
                hasher.reset();
                return;
            },
        }
    }

    if position == 0 {
        println!("{} is empty", file_path);
    } else {
        let hash_result = hasher.finalize_reset();
        println!("{} {} bytes {:#x}", file_path, position, hash_result);
    }
}

pub fn hash_files(shared: Arc<Shared>,  thread_info: &ThreadInfo) {
    let mut hasher = Sha256::new();
    let mut lock = shared.to_hash.lock().unwrap();

    loop {
        if lock.stop_now {
            thread_info.set_state(Quit);
            thread_info.set_working_on(None);
            break;
        } else if let Some((path, rx)) = lock.queue.pop() {
            drop(lock);
            hash_file(path, rx, &mut hasher, thread_info, &shared.buffers);
            lock = shared.to_hash.lock().unwrap();
        } else if lock.stop_when_empty {
            thread_info.set_state(Quit);
            thread_info.set_working_on(None);
            break;
        } else {
            thread_info.set_state(Idle);
            thread_info.set_working_on(None);
            lock = shared.hasher_waker.wait(lock).unwrap();
        }
    }
}
