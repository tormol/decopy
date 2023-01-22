#![cfg(unix)]
use std::{env, fs, slice};
use std::ffi::OsStr;
use std::io::ErrorKind::*;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::exit;

fn main() {
    const GOAL: &str = "all253";
    match fs::create_dir(GOAL) {
        Ok(()) => {},
        Err(e) if e.kind() == AlreadyExists => {},
        Err(e) => {
            eprintln!("Cannot create {}: {}", GOAL, e);
            exit(1);
        }
    }
    if let Err(e) = env::set_current_dir(GOAL) {
        eprintln!("Cannot cd to {}: {}", GOAL, e);
        exit(1);
    }
    for c in 0..256 {
        let c = c as u8;
        let slice: &[u8] = slice::from_ref(&c);
        let p = Path::new(OsStr::from_bytes(slice));
        if let Err(e) = fs::write(p, &[]) {
            eprintln!("Cannot create {:?}: {}", p, e);
        }
    }
}
