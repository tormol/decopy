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

use crate::bytes::Bytes;

use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[repr(C, align(128))] // avoid false sharing
pub struct ThreadInfo {
    thread_name: String,
    processed_bytes: AtomicUsize,
}

impl ThreadInfo {
    pub fn new(thread_name: String) -> ThreadInfo {
        ThreadInfo {
            thread_name,
            processed_bytes: AtomicUsize::new(0),
        }
    }

    pub fn name(&self) -> &str {
        &self.thread_name
    }

    pub fn processed_bytes(&self) -> Bytes {
        Bytes(self.processed_bytes.load(Ordering::Relaxed))
    }
    pub fn add_bytes(&self,  bytes: usize) {
        self.processed_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

}

impl Debug for ThreadInfo {
    fn fmt(&self,  fmtr: &mut Formatter) -> fmt::Result {
        write!(fmtr, "{}: {}", &self.thread_name, self.processed_bytes())
    }
}

pub fn create_info_array(name_prefix: &str,  count: usize) -> Arc<[ThreadInfo]> {
    let mut infos = Vec::with_capacity(count+1);
    for n in 1..=count {
        let name = format!("{} {}", name_prefix, n);
        let info = ThreadInfo::new(name);
        infos.push(info);
    }
    infos.into()
}
