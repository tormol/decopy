CREATE TABLE IF NOT EXISTS hashed (
    -- path is the non-decoded absolute path of the file, including filename
    path BLOB PRIMARY KEY NOT NULL,
    -- printable_dir is the parent of the file, i.e. path without file name.
    -- See comment on printable_path for what printable means.
    -- Trailing path separator (/ or \) is included so that printable_path
    -- doesn't have to add it, which would make this schema platform-dependent.
    printable_dir TEXT NOT NULL,
    -- printable_name is the file name without path.
    -- See comment on printable_path for what printable means.
    printable_name TEXT NOT NULL,
    -- printable_path is in most cases the same value as path,
    -- but component that can't be decoded as UTF-8 are decoded as Windows-1252
    -- and control characters (including newline and tab) are replaced with printable variants
    printable_path TEXT NOT NULL GENERATED ALWAYS
        AS (printable_dir || printable_name) VIRTUAL,
    -- modified is the modification time of the file, stored as yyy-mm-dd HH:MM:ss
    modified TEXT NOT NULL CHECK(length(modified)=19),
    -- apparent_size is the reported size of the file, in byte
    apparent_size UNSIGNED INTEGER NOT NULL,
    -- read_size is how many bytes the file contained when read
    read_size UNSIGNED INTEGER NOT NULL,
    -- hash is the SHA-256 hash of the file, stored in binary form
    hash BLOB NOT NULL CHECK(length(hash)=32),
    -- hash_hex is a printable version of hash
    hash_hex TEXT NOT NULL GENERATED ALWAYS
        AS (hex(hash)) VIRTUAL
) WITHOUT ROWID; -- should be faster as long as path is printable and not too long

CREATE UNIQUE INDEX IF NOT EXISTS hashed_path ON hashed (path ASC);
CREATE INDEX IF NOT EXISTS hashed_dir ON hashed (printable_dir ASC);
CREATE INDEX IF NOT EXISTS hashed_name ON hashed (printable_name);
CREATE INDEX IF NOT EXISTS hashed_hash ON hashed (hash);

CREATE TABLE IF NOT EXISTS roots (
    path BLOB PRIMARY KEY NOT NULL,
    printable_path TEXT NOT NULL
) WITHOUT ROWID;
