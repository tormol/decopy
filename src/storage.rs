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

use crate::shared::*;

use std::mem::ManuallyDrop;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use rusqlite::{Connection, Statement};

#[derive(Debug)]
pub struct Sqlite {
    connection: ManuallyDrop<Connection>,
    hashed_rx: mpsc::Receiver<HashedFile>,
    messages: mpsc::Sender<String>,
}

impl Drop for Sqlite {
    fn drop(&mut self) {
        let connection = unsafe { ManuallyDrop::take(&mut self.connection) };
        connection.close().expect("closing database");
    }
}

impl Sqlite {
    /// Open the database read-write, or exit on failure.
    pub fn open(
            path: &Path,
            hashed_rx: mpsc::Receiver<HashedFile>,
            messages: mpsc::Sender<String>,
    ) -> Self {
        let connection = Connection::open(path)
                .expect("open database");
        let db = Self {
            connection: ManuallyDrop::new(connection),
            hashed_rx,
            messages,
        };
        db.prepare();
        return db;
    }

    /// Open the database read-write, or exit on failure.
    pub fn new_in_memory(hashed_rx: mpsc::Receiver<HashedFile>,  messages: mpsc::Sender<String>)
    -> Self {
        let connection = Connection::open_in_memory()
                .expect("create in-memory database");
        let db = Self {
            connection: ManuallyDrop::new(connection),
            hashed_rx,
            messages,
        };
        db.prepare();
        return db;
    }

    fn prepare(&self) {
        let transaction = concat!("BEGIN\n;", include_str!("../schema.sql"), "COMMIT;\n");
        self.connection.execute_batch(transaction).expect("create schema");
    }

    pub fn get_previously_read(&mut self,
            absolute_path: &PrintablePath,
            preivously_read: &mut PreviouslyRead,
    ) {
        // LIKE and BLOB appear not to work for BLOB,
        // and are probably vulnerable to injection anyway.
        // Therefore BETWEEN must be used,
        // which requires finding the next path after all sub-paths of the prefix.
        let Some(start) = absolute_path.as_bytes() else {
            let _ = self.messages.send("cache is ignored for non-UTF8 paths on Windows".to_string());
            return;
        };
        let mut after = Vec::from(start);
        for i in (0..after.len()).rev() {
            if after[i] == 255 {
                after.pop();
            } else {
                after[i] += 1;
                break;
            }
        }

        let mut stmt = self.connection.prepare("
                SELECT path, modified, apparent_size
                FROM hashed WHERE path BETWEEN ?1 AND ?2"
        ).expect("create SELECT statement");
        let files = stmt.query_map((start, after), |row | {
            let path: Vec<u8> = row.get(0).expect("get path collumn");
            let path = Arc::new(PrintablePath::try_from(path).unwrap());
            let modified = row.get::<_, String>(1)
                    .expect("get modified collumn")
                    .parse::<PrintableTime>()
                    .expect("parse date-time");
            Ok(UnreadFile {
                    path,
                    modified,
                    size: row.get(2).expect("get size collumn"),
            })
        }).expect("get previously hashed files under root");
        for file in files {
            let file = file.expect("get mapped row");
            preivously_read.insert(file);
        }
    }

    pub fn save_hashed(&mut self,  insert_interval: Duration) {
        fn insert_hashed(statement: &mut Statement,  insert: HashedFile) {
            let printable_path = insert.path.as_str();
            let name = Path::new(printable_path).file_name().unwrap_or_default().to_str().unwrap();
            let dir = &printable_path[..printable_path.len()-name.len()]; // with trailing slash
            statement.insert(params!(
                    insert.path.as_bytes(),
                    dir,
                    name,
                    insert.modified.to_string(),
                    insert.apparent_size,
                    insert.read_size,
                    insert.hash,
            )).expect("insert hash");
        }
        while let Ok(file) = self.hashed_rx.recv() {
            let oldest = Instant::now();
            let mut files = 1u32;
            let transaction = self.connection.transaction().expect("start transaction");
            let mut statement = transaction.prepare("INSERT OR REPLACE INTO HASHED
                    (path, printable_dir, printable_name, modified, apparent_size, read_size, hash)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
            ).expect("create INSERT OR REPLACE statement");
            insert_hashed(&mut statement, file);
            let mut timeout = insert_interval;
            while let Ok(file) = self.hashed_rx.recv_timeout(timeout) {
                files += 1;
                insert_hashed(&mut statement, file);
                timeout = match insert_interval.checked_sub(Instant::elapsed(&oldest)) {
                    Some(next) => next,
                    None => break,
                };
            }
            let _ = self.messages.send(format!("committing {} hashed files", files));
            statement.finalize().expect("finalize insert statement");
            transaction.commit().expect("commit inserts");
        }
    }

    pub fn store_roots(&mut self,  roots: &[Arc<PrintablePath>]) {
        let transaction = self.connection.transaction().expect("start transaction");
        let mut statement = transaction.prepare("INSERT OR REPLACE INTO ROOTS
                (path, printable_path) VALUES (?1, ?2)"
        ).expect("create INSERT OR REPLACE statement");
        let inserted = roots.iter().map(|root| {
            statement.execute((root.as_bytes(), root.as_str())).expect("insert into roots")
        }).sum::<usize>();
        statement.finalize().expect("finalize insert statement");
        transaction.commit().expect("commit inserts");
        let _ = self.messages.send(format!("inserted {} roots", inserted));
    }

    pub fn prune(&mut self,  read: &PreviouslyRead) {
        let transaction = self.connection.transaction().expect("start transaction");
        let mut statement = transaction.prepare("DELETE FROM hashed WHERE path = ?1")
            .expect("create INSERT OR REPLACE statement");
        let removed = read.get_not_found()
            .map(|file| statement.execute((file.as_bytes(),)).expect("delete row") )
            .sum::<usize>();
        statement.finalize().expect("finalize delete statement");
        transaction.commit().expect("commit deletes");
        let _ = self.messages.send(format!("pruned {} files", removed));
    }
}
