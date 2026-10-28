#![allow(dead_code)]

use crate::error::AppError;
use rusqlite::{backup::Backup, Connection};
use std::path::{Path, PathBuf};

/// Apply the SQLCipher `PRAGMA key` to a fresh connection, handling both the
/// raw-string form and the `x'<hex>'` raw-key form. Must be the first
/// statement on the connection. Raw keys are double-quoted
/// (`PRAGMA key = "x'...'"`) — the bare blob literal is a syntax error.
fn apply_key(conn: &Connection, pwd: &str) -> Result<(), rusqlite::Error> {
    let pragma_key = if pwd.starts_with("x'") {
        format!("PRAGMA key = \"{}\"", pwd)
    } else {
        format!("PRAGMA key = '{}'", pwd.replace("'", "''"))
    };
    // PRAGMA key returns a row in this build; execute() would error with
    // ExecuteReturnedResults. query_row discards the row and reports success.
    conn.query_row(&pragma_key, [], |_| Ok(()))?;
    Ok(())
}

/// Create a manual database backup at the specified path.
///
/// Uses `rusqlite::backup::Backup` to ensure WAL pages are included (D-98).
///
/// When `password` is `Some`, the destination is keyed with the same SQLCipher
/// key as the source, so the backup file is encrypted (T-02). Without a key the
/// Backup API copies DECRYPTED pages — silently producing a plaintext copy of
/// the whole vault. The pre-migration safety copy in `vault_migrate` passes
/// `None` deliberately (it snapshots a legacy plaintext DB).
pub fn create_backup(
    source: &Connection,
    dest_path: &Path,
    password: Option<&str>,
) -> Result<PathBuf, AppError> {
    let mut dest = Connection::open(dest_path)?;
    if let Some(pwd) = password {
        apply_key(&dest, pwd)?;
    }
    let backup = Backup::new(source, &mut dest)?;
    backup.run_to_completion(5, std::time::Duration::from_millis(250), None)?;
    Ok(dest_path.to_path_buf())
}

/// List existing backup files in the backups directory, sorted newest-first.
pub fn list_backups(backup_dir: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut backups = Vec::new();
    if backup_dir.exists() {
        for entry in std::fs::read_dir(backup_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "db") {
                backups.push(path);
            }
        }
    }
    backups.sort_by_key(|p| p.metadata().ok().and_then(|m| m.modified().ok()));
    backups.reverse();
    Ok(backups)
}

/// Restore a database from a backup file.
///
/// `password` must match the backup's key: encrypted backups (T-02) require it
/// on BOTH the source (to decrypt the backup) and the target (to stay
/// encrypted after restore).
pub fn restore_backup(
    backup_path: &Path,
    target: &Path,
    password: Option<&str>,
) -> Result<(), AppError> {
    let source = Connection::open(backup_path)?;
    let mut dest = Connection::open(target)?;
    if let Some(pwd) = password {
        apply_key(&source, pwd)?;
        apply_key(&dest, pwd)?;
    }
    let backup = Backup::new(&source, &mut dest)?;
    backup.run_to_completion(5, std::time::Duration::from_millis(250), None)?;
    Ok(())
}

/// Generate a timestamped backup filename in the given directory.
pub fn generate_backup_name(base_dir: &Path) -> PathBuf {
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    base_dir.join(format!("arnold_backup_{}.db", timestamp))
}
