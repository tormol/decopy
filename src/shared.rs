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

pub use crate::available_buffers::AvailableBuffers;

use std::{io, path::Path};
use std::sync::{Arc, Condvar, Mutex, mpsc};

#[derive(Clone, Copy, Debug)]
pub enum ReadType {File, Directory}

#[derive(Debug)]
pub struct ReadQueue {
    pub queue: Vec<(Arc<Path>, ReadType)>,
    pub stop_now: bool,
    pub working: u32,
}
impl Default for ReadQueue {
    fn default() -> Self {
        ReadQueue { queue: Vec::new(), stop_now: false, working: 0, }
    }
}

#[derive(Debug)]
pub enum FilePart {
    /// A vector that is always fully initialized.
    Chunk{buffer: Box<[u8]>,  length: usize},
    Error(io::Error),
}

#[derive(Debug)]
pub struct HashQueue {
    pub queue: Vec<(Arc<Path>, mpsc::Receiver<FilePart>)>,
    pub stop_now: bool,
    pub stop_when_empty: bool,
}
impl Default for HashQueue {
    fn default() -> Self {
        HashQueue { queue: Vec::new(), stop_now: false, stop_when_empty: false, }
    }
}

#[derive(Debug)]
pub struct Shared {
    pub to_read: Mutex<ReadQueue>,
    pub reader_waker: Condvar,
    pub to_hash: Mutex<HashQueue>,
    pub hasher_waker: Condvar,
    pub buffers: AvailableBuffers,
}

impl Shared {
    pub fn new(buffers: AvailableBuffers) -> Arc<Self> {
        Arc::new(Shared {
            to_read: Mutex::new(ReadQueue::default()),
            reader_waker: Condvar::new(),
            to_hash: Mutex::new(HashQueue::default()),
            hasher_waker: Condvar::new(),
            buffers,
        })
    }
}
