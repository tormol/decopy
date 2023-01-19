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

use crate::thread_info::*;

use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::sync::{Condvar, Mutex, TryLockError};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Stores unused `Box<[u8]>` buffers so that they don't need to be re-allocated or re-initialized,
/// and makes them available to any thread.
///
/// Buffers of any size can be returned, up to a limit set at construction time.
/// That limit is itself limitied to maximum 4 GiB, and as a sanity check, minimum 512 bytes.
pub struct AvailableBuffers {
    /// A map used as a multimap:
    /// The second u32 in the key is used as a counter to allow having multiple boxes of the same size.
    map: Mutex<BTreeMap<(u32, u32), Box<[u8]>>>,
    starving: Condvar,
    /// Tracks size of buffers given out plus currently in the map.
    current_buffers_size: AtomicUsize,
    max_buffers_size: usize,
    max_single_buffer: u32,
}

impl Debug for AvailableBuffers {
    fn fmt(&self,  fmtr: &mut Formatter) -> fmt::Result {
        let map_info = match self.map.try_lock() {
            Ok(ref map) if map.is_empty() => "{empty}".to_string(),
            Ok(map) => {
                format!("{{{} buffers between {} and {} bytes in size}}",
                        map.len(),
                        map.first_key_value().unwrap().0.0,
                        map.last_key_value().unwrap().0.0,
                )
            },
            Err(TryLockError::WouldBlock) => "{locked}".to_string(),
            Err(TryLockError::Poisoned(_)) => "{poisoned}".to_string(),
        };
        fmtr.debug_struct("AvailableBuffers")
            .field("map", &map_info)
            .field("starving", &self.starving)
            .field("current_buffers_size", &self.current_buffers_size.load(Ordering::Relaxed))
            .field("max_buffers_size", &self.max_buffers_size)
            .field("max_single_buffer", &self.max_single_buffer)
            .finish()
    }
}

impl AvailableBuffers {
    pub const MIN_BUFFER_SIZE: usize = 512;
    pub fn new(max_buffers_size: usize,  max_single_buffer_size: usize) -> Result<Self, &'static str> {
        if max_single_buffer_size > u32::MAX as usize {
            return Err("max single buffer size is too big");
        } else if max_buffers_size > isize::MAX as usize {
            return Err("max buffers size is too big");
        } else if max_single_buffer_size < Self::MIN_BUFFER_SIZE {
            return Err("max single buffer size is too small");
        } else if max_buffers_size < max_single_buffer_size {
            return Err("max buffers size is less than max single buffer size")
        }
        Ok(AvailableBuffers {
            map: Mutex::new(BTreeMap::new()),
            starving: Condvar::new(),
            current_buffers_size: AtomicUsize::new(0),
            max_buffers_size: max_buffers_size.into(),
            max_single_buffer: max_single_buffer_size as u32,
        })
    }

    pub fn get_buffer(&self,  requested_size: usize,  thread_info: &ThreadInfo) -> Box<[u8]> {
        if requested_size == 0 {
            return Box::default();
        }
        let requested_size = requested_size.clamp(
                Self::MIN_BUFFER_SIZE.max(self.max_single_buffer as usize/128),
                self.max_single_buffer as usize,
        );
        let key = (requested_size as u32, 0);
        let mut map = self.map.lock().unwrap();
        let mut buffer = loop {
            // see if there is something big enough
            if let Some((&next, _)) = map.range(key..).next() {
                let buffer = map.remove(&next).unwrap();
                if buffer.len() <= requested_size * 2 {
                    return buffer;
                }
                // too big (this might deprive others of memory)
                let release = buffer.len() - requested_size;
                self.current_buffers_size.fetch_sub(release, Ordering::Relaxed);
                drop(map);
                let mut to_shrink = buffer.into_vec();
                to_shrink.truncate(requested_size);
                to_shrink.shrink_to_fit();
                break to_shrink;
            }
            // see if there is something slightly too small
            if let Some((&smaller, _)) = map.range(..key).last() {
                if smaller.0 >= (key.0*9)/10 {
                    return map.remove(&smaller).unwrap();
                } 
            }
            // see if there is enough free space
            let unallocated = self.max_buffers_size as isize
                - self.current_buffers_size.load(Ordering::Relaxed) as isize;
            if (requested_size as isize) <= unallocated {
                // mutex prevents any other thread from allocating
                self.current_buffers_size.fetch_add(requested_size, Ordering::Relaxed);
                drop(map);
                break vec![0u8; requested_size];
            }
            // see if there is a buffer that can be grown within the limit.
            let need_to_release = requested_size as isize - unallocated;
            if let Some((&remove, _)) = map.range((need_to_release as u32, 0)..).next() {
                let to_grow = map.remove(&remove).unwrap();
                let increase = requested_size - to_grow.len();
                self.current_buffers_size.fetch_add(increase, Ordering::Relaxed);
                drop(map);
                let mut to_grow = to_grow.into_vec();
                to_grow.resize(requested_size, 0);
                break to_grow;
            }
            // wait
            thread_info.set_state(WaitingForMemory);
            map = self.starving.wait(map).unwrap();
        };

        // check if resized or allocated buffer has unused capacity
        let extra_capacity = buffer.capacity() - buffer.len();
        if extra_capacity > 0 {
            self.current_buffers_size.fetch_add(extra_capacity, Ordering::SeqCst);
            eprintln!("vec of size {} has extra capacity {}", buffer.len(), extra_capacity);
            buffer.resize(buffer.capacity(), 0);
        }
        if buffer.len() != requested_size {
            eprintln!("requested {} got {}", requested_size, buffer.len());
        }
        buffer.into_boxed_slice()
    }

    pub fn return_buffer(&self,  buffer: Box<[u8]>) {
        // reject trying to add too small or too big buffers
        if buffer.len() < Self::MIN_BUFFER_SIZE  ||  buffer.len() > self.max_buffers_size as usize {
            return;
        }
        let size = buffer.len() as u32;
        let mut map = self.map.lock().unwrap();
        let index = if size == self.max_single_buffer {
            // use last instead of range when we are at the limit.
            match map.last_key_value() {
                Some((&(len, index), _)) if len == size => index+1,
                Some((&(len, _), _)) if len > size => {
                    self.current_buffers_size.fetch_sub(buffer.len(), Ordering::Relaxed);
                    drop(map);
                    panic!("map has a buffer of size {}, whcih is bigger than the max of {}",
                            len,
                            self.max_single_buffer
                    );
                },
                _ => 0,
            }
        } else {
            match map.range(..(size+1, 0)).last() {
                Some((&(len, index), _)) if len == size => index+1,
                _ => 0,
            }
        };
        if let Some(buffer) = map.insert((size, index), buffer) {
            self.current_buffers_size.fetch_sub(buffer.len(), Ordering::Relaxed);
            drop(map);
            panic!("There already is a buffer with index ({}, {})", size, index);
        }
        drop(map);
        // 
        self.starving.notify_all();
    }

    #[allow(dead_code)]
    pub fn current_buffers_size(&self) -> usize {
        self.current_buffers_size.load(Ordering::Relaxed)
    }

    #[allow(dead_code)]
    pub const fn max_memory_usage(&self) -> usize {
        self.max_buffers_size
    }

    #[allow(dead_code)]
    pub const fn max_single_buffer_size(&self) -> usize {
        self.max_single_buffer as usize
    }
}
