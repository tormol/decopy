/* Copyright 2023 Torbjørn Birch Moltu
 *
 * This file is part of Decopy.
 * Decopy is free software: you can redistribute it and/or modify it under the
 * terms of the GNU General Public License as published by the Free Software Foundation,
 * either version 3 of the License, or (at your option) any later version.
 *
 * Decopy is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
 * without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
 * See the GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along with Decopy.
 * If not, see <https://www.gnu.org/licenses/>.
 */

pub use crate::available_buffers::AvailableBuffers;
pub use crate::bytes::Bytes;
pub use crate::path_decoding::PrintablePath;
pub use crate::time::PrintableTime;

use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::io;
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::sync::atomic::{AtomicBool, Ordering};

use fxhash::FxBuildHasher;

#[derive(Clone, Debug, PartialEq,Eq,Hash)]
pub struct UnreadFile {
    pub path: Arc<PrintablePath>,
    pub modified: PrintableTime,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub enum ToRead {
    File(UnreadFile),
    Directory(Arc<PrintablePath>),
}

#[derive(Default)]
pub struct ReadQueue {
    pub queue: Vec<ToRead>,
    pub stop_now: bool,
    pub working: u32,
}
impl Debug for ReadQueue {
    fn fmt(&self,  fmtr: &mut Formatter) -> fmt::Result {
        fmtr.debug_struct("ReadQueue")
            .field("queue_length", &self.queue.len())
            .field("stop_now", &self.stop_now)
            .field("working", &self.working)
            .finish()
    }
}

#[derive(Debug)]
pub enum FilePart {
    /// A vector that is always fully initialized.
    Chunk{buffer: Box<[u8]>,  length: usize},
    Error(io::Error),
}

#[derive(Default)]
pub struct HashQueue {
    pub queue: Vec<(UnreadFile, mpsc::Receiver<FilePart>)>,
    pub stop_now: bool,
    pub stop_when_empty: bool,
}
impl Debug for HashQueue {
    fn fmt(&self,  fmtr: &mut Formatter) -> fmt::Result {
        fmtr.debug_struct("HashQueue")
            .field("queue_length", &self.queue.len())
            .field("stop_now", &self.stop_now)
            .field("stop_when_empty", &self.stop_when_empty)
            .finish()
    }
}

#[derive(Clone,Copy)]
struct Hex<const N: usize>([u8; N]);
impl<const N: usize> Debug for Hex<N> {
    fn fmt(&self,  fmtr: &mut Formatter) -> fmt::Result {
        for &byte in &self.0 {
            write!(fmtr, "{:02x}", byte)?;
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq,Eq,Hash)]
pub struct HashedFile {
    pub path: Arc<PrintablePath>,
    pub modified: PrintableTime,
    pub apparent_size: u64,
    pub read_size: u64,
    pub hash: [u8; 32],
}
impl Debug for HashedFile {
    fn fmt(&self,  fmtr: &mut Formatter) -> fmt::Result {
        fmtr.debug_struct("HashedFile")
            .field("path", &self.path)
            .field("modified", &self.modified)
            .field("apparent_size", &Bytes(self.apparent_size))
            .field("read_size", &Bytes(self.read_size))
            .field("hash", &Hex(self.hash))
            .finish()
    }
}

#[derive(Default)]
pub struct PreviouslyRead {
    files: HashMap<Arc<PrintablePath>, (UnreadFile, AtomicBool), FxBuildHasher>,
}
impl PreviouslyRead {
    pub fn insert(&mut self,  file: UnreadFile) {
        self.files.insert(file.path.clone(), (file, AtomicBool::new(false)));
    }
    pub fn check_unchanged(&self,  file: &UnreadFile) -> bool {
        if let Some((ref info, ref still_exists)) = self.files.get(&file.path) {
            still_exists.store(true, Ordering::SeqCst);
            info == file
        } else {
            false
        }
    }
    pub fn get_not_found(&self) -> impl Iterator<Item=&Arc<PrintablePath>> {
        self.files.values().filter_map(|(ref file, ref exists)| {
            match exists.load(Ordering::Acquire) {
                true => None,
                false => Some(&file.path),
            }
        })
    }
}
impl Debug for PreviouslyRead {
    fn fmt(&self,  fmtr: &mut Formatter) -> fmt::Result {
        fmtr.debug_struct("PreviouslyRead")
            .field("files", &self.files.len())
            .finish()
    }
}

#[derive(Debug)]
pub struct Shared {
    pub previously_read: PreviouslyRead,
    pub to_read: Mutex<ReadQueue>,
    pub reader_waker: Condvar,
    pub to_hash: Mutex<HashQueue>,
    pub hasher_waker: Condvar,
    pub buffers: AvailableBuffers,
    pub finished: Mutex<mpsc::Sender<HashedFile>>,
}

impl Shared {
    pub fn new(buffers: AvailableBuffers,  finished: mpsc::Sender<HashedFile>) -> Self {
        Shared {
            previously_read: PreviouslyRead::default(),
            to_read: Mutex::new(ReadQueue::default()),
            reader_waker: Condvar::new(),
            to_hash: Mutex::new(HashQueue::default()),
            hasher_waker: Condvar::new(),
            buffers,
            finished: Mutex::new(finished),
        }
    }
}
