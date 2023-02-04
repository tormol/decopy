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

use std::mem::ManuallyDrop;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rusqlite::{Connection, Statement};

#[derive(Debug)]
pub struct Sqlite {
    connection: ManuallyDrop<Connection>,
    hashed_rx: mpsc::Receiver<HashedFile>,
}

impl Drop for Sqlite {
    fn drop(&mut self) {
        let connection = unsafe { ManuallyDrop::take(&mut self.connection) };
        connection.close().expect("closing database");
    }
}

impl Sqlite {
    /// Open the database read-write, or exit on failure.
    pub fn open(path: &Path,  hashed_rx: mpsc::Receiver<HashedFile>) -> Self {
        let connection = Connection::open(path)
                .expect("open database");
        let db = Self { connection: ManuallyDrop::new(connection),  hashed_rx };
        db.prepare();
        return db;
    }

    /// Open the database read-write, or exit on failure.
    pub fn new_in_memory(hashed_rx: mpsc::Receiver<HashedFile>) -> Self {
        let connection = Connection::open_in_memory()
                .expect("create in-memory database");
        let db = Self { connection: ManuallyDrop::new(connection),  hashed_rx };
        db.prepare();
        return db;
    }

    fn prepare(&self) {
        self.connection.execute(
                "CREATE TABLE IF NOT EXISTS hashed (
                    path BLOB PRIMARY KEY NOT NULL,
                    printable_path TEXT NOT NULL,
                    modified TEXT NOT NULL,
                    apparent_size INTEGER NOT NULL,
                    read_size INTEGER NOT NULL,
                    hash BLOB NOT NULL
                )",
                (), // empty list of parameters.
        ).expect("create table");
    }

    pub fn save_hashed(&mut self,  insert_interval: Duration) {
        fn insert_hashed(statement: &mut Statement,  insert: HashedFile) {
            statement.insert(params!(
                    insert.path.as_bytes(),
                    insert.path.as_str(),
                    insert.modified.to_string(),
                    insert.apparent_size,
                    insert.read_size,
                    insert.hash,
            )).expect("insert hash");
        }
        while let Ok(file) = self.hashed_rx.recv() {
            let oldest = Instant::now();
            let transaction = self.connection.transaction().expect("start transaction");
            let mut statement = transaction.prepare("INSERT OR REPLACE
                    INTO hashed (path, printable_path, modified, apparent_size, read_size, hash)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
            ).expect("create INSERT OR REPLACE statement");
            insert_hashed(&mut statement, file);
            let mut timeout = insert_interval;
            while let Ok(file) = self.hashed_rx.recv_timeout(timeout) {
                timeout = insert_interval - Instant::elapsed(&oldest);
                insert_hashed(&mut statement, file);
            }
            statement.finalize().expect("finalize insert statement");
            transaction.commit().expect("commit inserts");
        }
    }
}
