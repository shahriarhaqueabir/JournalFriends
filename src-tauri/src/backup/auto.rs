#![allow(dead_code)]

use crate::db::pool;
use crate::error::AppError;
use crate::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;

/// Maximum number of automatic backups to retain.
const MAX_AUTO_BACKUPS: usize = 20;

/// Run an automatic backup — called on app open, close, and on schedule.
///
/// `password` is the SQLCipher key (`x'<hex>'`) so the backup file is
/// encrypted like the source (T-02). `None` keeps the legacy behaviour of
/// plaintext backups (only reachable if the vault is still locked).
pub async fn run_auto_backup(
    pool: Arc<pool::DbPool>,
    backup_dir: PathBuf,
    password: Option<String>,
) -> Result<PathBuf, AppError> {
    if !backup_dir.exists() {
        std::fs::create_dir_all(&backup_dir)?;
    }

    let dest_path = crate::backup::manual::generate_backup_name(&backup_dir);
    let result = pool::with_conn(pool, move |conn| {
        crate::backup::manual::create_backup(conn, &dest_path, password.as_deref())
    })
    .await?;

    cleanup_old_backups(&backup_dir)?;

    Ok(result)
}

/// Remove oldest backups beyond MAX_AUTO_BACKUPS.
fn cleanup_old_backups(backup_dir: &std::path::Path) -> Result<(), AppError> {
    let mut backups = crate::backup::manual::list_backups(backup_dir)?;
    while backups.len() > MAX_AUTO_BACKUPS {
        if let Some(oldest) = backups.pop() {
            let _ = std::fs::remove_file(&oldest);
        }
    }
    Ok(())
}

/// Spawn a background task that runs automatic backups on a 30-minute interval.
pub fn spawn_auto_backup_worker(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1800));
        loop {
            interval.tick().await;
            let state = handle.state::<AppState>();
            let pool = match state.db() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let backup_dir = state.paths.backup_dir.clone();
            // T-02: derive the SQLCipher key from the session key. The pool
            // and key are committed together in unlock_vault, so a present
            // pool implies a present key — backups are then encrypted.
            let password = state.key().ok().map(|k| {
                format!(
                    "x'{}'",
                    k.iter().map(|b| format!("{:02x}", b)).collect::<String>()
                )
            });
            if let Err(e) = run_auto_backup(pool, backup_dir, password).await {
                tracing::warn!(target: "backup", error = %e, "auto backup failed");
            }
        }
    });
}
