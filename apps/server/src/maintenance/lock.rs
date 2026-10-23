//! One process at a time works on a data directory. `serve` holds the lock while it runs, and
//! offline commands (`restore`, `upgrade`, and operator jobs while no server runs) take it.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

use crate::init::{Exit, Failure};

pub const LOCK_FILE: &str = "holocron.lock";

/// Held until dropped; the operating system releases it when the process ends.
pub struct DataLock {
    _file: File,
}

impl DataLock {
    /// Takes the lock, or `None` when another process holds it.
    pub fn try_acquire(dir: &Path) -> io::Result<Option<Self>> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(dir.join(LOCK_FILE))?;
        // SAFETY: flock on a descriptor this function owns.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(Some(Self { _file: file }));
        }
        match io::Error::last_os_error() {
            e if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            e => Err(e),
        }
    }

    /// Takes the lock or fails with exit 4, as 0006 names a held lock.
    pub fn acquire(dir: &Path) -> Result<Self, Failure> {
        match Self::try_acquire(dir) {
            Ok(Some(lock)) => Ok(lock),
            Ok(None) => Err(Failure::new(
                Exit::StorageUnavailable,
                "storage_unavailable",
                format!(
                    "{}: another holocron process uses this data directory",
                    dir.display()
                ),
            )),
            Err(e) => Err(Failure::storage(dir, e)),
        }
    }
}
