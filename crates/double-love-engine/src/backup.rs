use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io,
    path::Path,
    time::Duration,
};

#[cfg(target_os = "macos")]
use std::{ffi::CString, os::unix::ffi::OsStrExt};

use rusqlite::{Connection, OpenFlags, backup::Backup};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum DatabaseBackupError {
    #[error("sqlite backup error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database backup filesystem error: {0}")]
    Filesystem(#[from] io::Error),
}

#[cfg(target_os = "macos")]
fn publish_without_replacement(temporary: &Path, destination: &Path) -> io::Result<()> {
    let temporary = CString::new(temporary.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "temporary path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "backup path contains NUL"))?;
    // SAFETY: both pointers are valid NUL-terminated paths, and RENAME_EXCL prevents replacement.
    let result = unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            temporary.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn publish_without_replacement(temporary: &Path, destination: &Path) -> io::Result<()> {
    fs::hard_link(temporary, destination)?;
    fs::remove_file(temporary)
}

/// Creates an idempotent, point-in-time SQLite backup without opening a [`crate::ProjectStore`].
///
/// The destination is published without replacement. If it already exists, this function leaves
/// it untouched and returns success.
pub fn backup_sqlite_database(
    source: &Path,
    destination: &Path,
) -> Result<(), DatabaseBackupError> {
    match fs::symlink_metadata(destination) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let source_connection = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let file_name = destination.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "database backup destination must have a file name",
        )
    })?;
    let mut temporary_name = OsString::from(".");
    temporary_name.push(file_name);
    temporary_name.push(format!(".{}.tmp", Uuid::new_v4()));
    let temporary = parent.join(temporary_name);

    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;

    let backup_result = (|| -> Result<(), DatabaseBackupError> {
        let mut destination_connection =
            Connection::open_with_flags(&temporary, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        {
            let backup = Backup::new(&source_connection, &mut destination_connection)?;
            backup.run_to_completion(100, Duration::from_millis(10), None)?;
        }
        drop(destination_connection);
        OpenOptions::new().read(true).open(&temporary)?.sync_all()?;
        Ok(())
    })();

    if let Err(error) = backup_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    match publish_without_replacement(&temporary, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(temporary)?;
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(temporary);
            Err(DatabaseBackupError::Filesystem(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    use rusqlite::{Connection, OpenFlags};

    use super::backup_sqlite_database;
    use crate::{CanvasSpec, ProjectStore};

    fn temp_database(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "double-love-{label}-{}-{unique}.sqlite",
            std::process::id()
        ))
    }

    #[test]
    fn online_backup_is_readable_and_never_replaced() {
        let source = temp_database("backup-source");
        let destination = temp_database("backup-destination");
        let store = ProjectStore::open(&source).expect("source store opens");
        store
            .set_project_id("pre-electron-project")
            .expect("project id writes");

        backup_sqlite_database(&source, &destination).expect("online backup succeeds");

        let read_only = Connection::open_with_flags(&destination, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("backup opens read-only");
        let schema_version: u32 = read_only
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("backup schema version");
        assert_eq!(schema_version, 11);
        let project_id: String = read_only
            .query_row(
                "SELECT value FROM project_meta WHERE key = 'project_id'",
                [],
                |row| row.get(0),
            )
            .expect("backup project id");
        assert_eq!(project_id, "pre-electron-project");
        drop(read_only);

        store
            .set_canvas_spec(&CanvasSpec::default())
            .expect("source changes after backup");
        backup_sqlite_database(&source, &destination).expect("existing backup is skipped");
        drop(store);

        let fallback = ProjectStore::open(&destination).expect("ProjectStore opens the backup");
        assert_eq!(
            fallback
                .project_id()
                .expect("fallback project id")
                .as_deref(),
            Some("pre-electron-project")
        );
        assert_eq!(fallback.revision().expect("fallback revision"), 0);
        drop(fallback);

        fs::remove_file(source).expect("source removed");
        fs::remove_file(destination).expect("backup removed");
    }
}
