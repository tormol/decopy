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

use crate::path_decoding::PrintablePath;

use std::fmt::{self, Debug, Formatter};
use std::ops::Deref;
use std::sync::{Arc, Mutex, mpsc::Sender};
use std::sync::atomic::{AtomicUsize, Ordering};

use arc_swap::ArcSwapOption;

#[derive(Clone,Copy, Default, Debug, PartialEq,Eq)]
#[repr(usize)]
pub enum ThreadState {
    #[default]
    Idle = 0,
    Opening = 1,
    WaitingForMemory = 2,
    Reading = 3,
    Hashing = 4,
    Quit = 5,
}
pub use self::ThreadState::*;

impl TryFrom<usize> for ThreadState {
    type Error = ();
    fn try_from(number: usize) -> Result<ThreadState, ()> {
        match number {
            0 => Ok(Idle),
            1 => Ok(Opening),
            2 => Ok(WaitingForMemory),
            3 => Ok(Reading),
            4 => Ok(Hashing),
            5 => Ok(Quit),
            _ => Err(())
        }
    }
}

#[repr(C, align(128))] // avoid false sharing
pub struct ThreadInfo {
    thread_name: String,
    // Sender is not Sync, so can't store it directly because all ThreadInfo are shared together
    // to all threads..
    // Therefore just wrap it in a mutex to make it work:
    // Logging should be rare, so performance is not an issue.
    log_channel: Mutex<Sender<String>>,
    processed_bytes: AtomicUsize,
    state: AtomicUsize,
    working_on: ArcSwapOption<PrintablePath>,
}

impl ThreadInfo {
    pub fn new(thread_name: String,  log_channel: Sender<String>) -> ThreadInfo {
        ThreadInfo {
            thread_name,
            log_channel: Mutex::new(log_channel),
            processed_bytes: AtomicUsize::new(0),
            state: AtomicUsize::new(Idle as usize),
            working_on: ArcSwapOption::empty(),
        }
    }

    pub fn name(&self) -> &str {
        &self.thread_name
    }

    pub fn log_message(&self,  message: String) {
        self.log_channel.lock().unwrap().send(message).unwrap()
    }

    pub fn processed_bytes(&self) -> usize {
        self.processed_bytes.load(Ordering::Relaxed)
    }
    pub fn add_bytes(&self,  bytes: usize) {
        self.processed_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn state(&self) -> ThreadState {
        let number = self.state.load(Ordering::Relaxed);
        ThreadState::try_from(number).unwrap_or_else(|_| {
            panic!("Invalid thread state {} received from thread {}", number, self.name())
        })
    }
    pub fn set_state(&self,  state: ThreadState) {
        self.state.store(state as usize, Ordering::Relaxed)
    }

    pub fn view_working_on<R, F: FnOnce(Option<&PrintablePath>)->R>(&self,  view: F) -> R{
        match self.working_on.load().deref() {
            &Some(ref path) => view(Some(path)),
            &None => view(None),
        }
    }
    pub fn set_working_on(&self,  path: Option<Arc<PrintablePath>>) {
        self.working_on.store(path);
    }
}

impl Debug for ThreadInfo {
    fn fmt(&self,  fmtr: &mut Formatter) -> fmt::Result {
        self.view_working_on(|path| {
            if let Some(path) = path {
                write!(fmtr, "{}: {:?} {}, {}",
                        &self.thread_name,
                        self.state(),
                        path,
                        self.processed_bytes(),
                )
            } else {
                write!(fmtr, "{}: {:?}, {}",
                        &self.thread_name,
                        self.state(),
                        self.processed_bytes()
                )
            }
        })
    }
}

pub fn create_info_array(name_prefix: &str,  count: usize,  log_channel: Sender<String>)
-> Arc<[ThreadInfo]> {
    let mut infos = Vec::with_capacity(count+1);
    for n in 1..=count {
        let name = format!("{} {}", name_prefix, n);
        let info = ThreadInfo::new(name, log_channel.clone());
        infos.push(info);
    }
    infos.into()
}
