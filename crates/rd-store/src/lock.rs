//! A lock file next to the database, so two machines don't edit the same synced file at once.
//!
//! `connections.sqlite.lock` holds the owner's host name, pid and last refresh time. The owner refreshes
//! it every 60 s and removes it on clean exit. A lock counts as held when it was written by a live process
//! on this host, or refreshed by another host within the last 3 minutes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rd_core::Timestamp;
use serde::{Deserialize, Serialize};

/// How old another host's lock may get before it counts as abandoned.
pub const STALE_AFTER: Duration = Duration::from_secs(180);

/// How often the owner should call [`DbLock::refresh`].
pub const REFRESH_EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockInfo {
    host: String,
    pid: u32,
    refreshed_at: Timestamp,
}

/// Who holds the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockHolder {
    /// Host name of the other instance.
    pub host: String,
    /// Its process id.
    pub pid: u32,
    /// Time since it last refreshed the lock.
    pub age: Duration,
}

/// Outcome of [`DbLock::acquire`].
#[derive(Debug)]
pub enum LockStatus {
    /// The database is ours.
    Acquired(DbLock),
    /// Another live instance has it open; the UI offers read-only, open anyway, or cancel.
    HeldElsewhere(LockHolder),
}

/// Ownership of a database file. Dropping it removes the lock file if it is still ours.
#[derive(Debug)]
pub struct DbLock {
    path: PathBuf,
    info: LockInfo,
}

impl DbLock {
    /// The lock file path for a database file.
    #[must_use]
    pub fn path_for(database: &Path) -> PathBuf {
        let mut name = database.file_name().unwrap_or_default().to_os_string();
        name.push(".lock");
        database.with_file_name(name)
    }

    /// Takes the lock unless another live instance holds it.
    ///
    /// # Errors
    /// If the lock file can't be read or written.
    pub fn acquire(database: &Path) -> std::io::Result<LockStatus> {
        let me = current();
        let path = Self::path_for(database);
        if let Some(existing) = read(&path)?
            && let Some(holder) = holder(&existing, &me, Timestamp::now(), process_alive)
        {
            return Ok(LockStatus::HeldElsewhere(holder));
        }
        Ok(LockStatus::Acquired(Self::write_new(path, me)?))
    }

    /// Takes the lock even if another instance holds it ("Open anyway").
    ///
    /// # Errors
    /// If the lock file can't be written.
    pub fn force_acquire(database: &Path) -> std::io::Result<Self> {
        Self::write_new(Self::path_for(database), current())
    }

    /// Updates the refresh time; call every [`REFRESH_EVERY`].
    ///
    /// # Errors
    /// If the lock file can't be written.
    pub fn refresh(&mut self) -> std::io::Result<()> {
        self.info.refreshed_at = Timestamp::now();
        write(&self.path, &self.info)
    }

    /// The lock file this handle owns.
    #[must_use]
    pub fn lock_path(&self) -> &Path {
        &self.path
    }

    /// Removes the lock file (same as dropping the handle).
    pub fn release(self) {}

    fn write_new(path: PathBuf, info: LockInfo) -> std::io::Result<Self> {
        write(&path, &info)?;
        Ok(Self { path, info })
    }
}

impl Drop for DbLock {
    fn drop(&mut self) {
        // Only remove the file if nobody took it over with "Open anyway" in the meantime.
        if let Ok(Some(existing)) = read(&self.path)
            && existing.host == self.info.host
            && existing.pid == self.info.pid
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Who holds the lock described by `existing`, judged from `me` at `now`; `None` if it can be taken.
fn holder(
    existing: &LockInfo,
    me: &LockInfo,
    now: Timestamp,
    alive: impl Fn(u32) -> bool,
) -> Option<LockHolder> {
    let age = (now.datetime() - existing.refreshed_at.datetime())
        .try_into()
        .unwrap_or(Duration::ZERO);
    let held = if existing.host == me.host {
        existing.pid != me.pid && alive(existing.pid)
    } else {
        age < STALE_AFTER
    };
    held.then(|| LockHolder {
        host: existing.host.clone(),
        pid: existing.pid,
        age,
    })
}

fn current() -> LockInfo {
    LockInfo {
        host: hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_default(),
        pid: std::process::id(),
        refreshed_at: Timestamp::now(),
    }
}

fn process_alive(pid: u32) -> bool {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).is_some()
}

/// A missing or unreadable lock file counts as no lock.
fn read(path: &Path) -> std::io::Result<Option<LockInfo>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Writes through a temporary file and a rename, so a reader never sees half a lock.
fn write(path: &Path, info: &LockInfo) -> std::io::Result<()> {
    let tmp = path.with_extension("lock.tmp");
    std::fs::write(&tmp, serde_json::to_vec(info).map_err(std::io::Error::other)?)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(host: &str, pid: u32, at: i64) -> LockInfo {
        LockInfo {
            host: host.into(),
            pid,
            refreshed_at: Timestamp::from_unix(at),
        }
    }

    const NOW: i64 = 1_000_000;

    #[test]
    fn same_host_live_pid_holds() {
        let held = holder(
            &info("mac", 7, 0),
            &info("mac", 8, NOW),
            Timestamp::from_unix(NOW),
            |_| true,
        );
        assert_eq!(held.unwrap().pid, 7);
    }

    #[test]
    fn same_host_dead_pid_or_own_pid_is_free() {
        let now = Timestamp::from_unix(NOW);
        assert!(holder(&info("mac", 7, NOW), &info("mac", 8, NOW), now, |_| false).is_none());
        assert!(holder(&info("mac", 8, NOW), &info("mac", 8, NOW), now, |_| true).is_none());
    }

    #[test]
    fn other_host_holds_until_stale() {
        let me = info("mac", 1, NOW);
        let now = Timestamp::from_unix(NOW);
        let fresh = holder(&info("pc", 5, NOW - 60), &me, now, |_| false).unwrap();
        assert_eq!((fresh.host.as_str(), fresh.age), ("pc", Duration::from_secs(60)));
        assert!(holder(&info("pc", 5, NOW - 181), &me, now, |_| true).is_none());
    }

    #[test]
    fn acquire_release_and_reacquire() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("connections.sqlite");
        let LockStatus::Acquired(mut lock) = DbLock::acquire(&db).unwrap() else {
            panic!("free")
        };
        assert_eq!(lock.lock_path(), dir.path().join("connections.sqlite.lock"));
        assert!(lock.lock_path().exists());
        lock.refresh().unwrap();

        // The same process re-opening the file is not blocked by itself.
        let LockStatus::Acquired(again) = DbLock::acquire(&db).unwrap() else {
            panic!("own lock")
        };
        drop(again);
        assert!(!lock.lock_path().exists());
        lock.release();
    }

    #[test]
    fn fresh_lock_from_another_host_is_reported_and_can_be_forced() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("connections.sqlite");
        let path = DbLock::path_for(&db);
        write(&path, &info("other-host", 42, 0)).unwrap();
        let mut other = read(&path).unwrap().unwrap();
        other.refreshed_at = Timestamp::now();
        write(&path, &other).unwrap();

        let LockStatus::HeldElsewhere(holder) = DbLock::acquire(&db).unwrap() else {
            panic!("held")
        };
        assert_eq!((holder.host.as_str(), holder.pid), ("other-host", 42));

        let forced = DbLock::force_acquire(&db).unwrap();
        assert_eq!(read(&path).unwrap().unwrap().pid, std::process::id());
        drop(forced);
        assert!(!path.exists());
    }

    #[test]
    fn stale_or_garbage_lock_is_taken() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("connections.sqlite");
        let path = DbLock::path_for(&db);
        write(&path, &info("other-host", 42, 0)).unwrap();
        assert!(matches!(DbLock::acquire(&db).unwrap(), LockStatus::Acquired(_)));
        std::fs::write(&path, b"not json").unwrap();
        assert!(matches!(DbLock::acquire(&db).unwrap(), LockStatus::Acquired(_)));
    }

    #[test]
    fn drop_leaves_a_lock_taken_over_by_someone_else() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("connections.sqlite");
        let LockStatus::Acquired(lock) = DbLock::acquire(&db).unwrap() else {
            panic!("free")
        };
        write(lock.lock_path(), &info("other-host", 42, NOW)).unwrap();
        let path = lock.lock_path().to_path_buf();
        drop(lock);
        assert!(path.exists());
    }

    #[test]
    fn this_process_is_alive() {
        assert!(process_alive(std::process::id()));
    }
}
