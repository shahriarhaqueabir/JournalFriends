use crate::error::AppError;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Type alias for the SQLite connection pool.
pub type DbPool = Pool<SqliteConnectionManager>;

/// Initialize a connection pool for the SQLite database at `path`.
///
/// If `password` is provided, applies `PRAGMA key` for SQLCipher encryption.
///
/// Sets per-connection PRAGMAs via `ConnectionCustomizer`:
/// - `key = <password>` (if provided)
/// - `foreign_keys = ON`
/// - `temp_store = MEMORY`
/// - `busy_timeout = 5000ms`
///
/// File-level settings (`journal_mode = WAL`, `synchronous = 1`) are set once
/// on an initial connection, as they persist across all connections.
pub fn init_pool(path: &Path, max_size: u32, password: Option<String>) -> Result<DbPool, AppError> {
    // Hardening (encryption at rest): when a vault password is set and the DB
    // file pre-exists, the file MUST already be SQLCipher-encrypted on disk.
    // Check the header BEFORE the keyed open — a keyed open of a legacy
    // plaintext file fails with a confusing "Invalid master password", and a
    // silently-swallowed PRAGMA key error would otherwise create/leave a
    // PLAINTEXT database while the user believes it is encrypted. A freshly
    // created file is trusted (SQLCipher encrypts the header on first write).
    if password.is_some() && path.exists() && !crate::db::vault_migrate::is_encrypted(path) {
        return Err(AppError::Database(format!(
            "vault password set but database file is not encrypted on disk: {}",
            path.display()
        )));
    }

    let manager = SqliteConnectionManager::file(path);

    let pool = Pool::builder()
        .max_size(max_size)
        .connection_customizer(Box::new(SqliteCustomizer { password }))
        .build(manager)
        .map_err(|e| AppError::Database(e.to_string()))?;

    // Set file-level PRAGMAs once (these persist across all connections)
    let conn = pool.get().map_err(|e| {
        // If this fails with "file is not a database", it's likely a bad key
        let msg = e.to_string();
        if msg.contains("file is not a database") {
            AppError::VaultInvalidPassword
        } else {
            AppError::Database(msg)
        }
    })?;

    // journal_mode and synchronous return the new value, so we use query_row to avoid "Execute returned results" error
    let _ = conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()));
    let _ = conn.query_row("PRAGMA synchronous = 1", [], |_| Ok(()));

    // Verify we can read from the schema (confirms key is correct)
    let _ = conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .map_err(|_| AppError::VaultInvalidPassword)?;

    Ok(pool)
}

/// Acquire a connection from the pool and run a synchronous closure on it
/// inside `tokio::task::spawn_blocking`.
///
/// This keeps the async runtime responsive while DB work runs on a blocking
/// thread. Replaces the old `state.conn.lock().await` pattern.
pub async fn with_conn<F, R>(pool: Arc<DbPool>, f: F) -> Result<R, AppError>
where
    F: FnOnce(&rusqlite::Connection) -> Result<R, AppError> + Send + 'static,
    R: Send + 'static,
{
    let pool = pool.clone();
    let res = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| AppError::Database(e.to_string()))?;
        f(&conn)
    })
    .await
    .map_err(|e| AppError::JoinError(e.to_string()))?;
    res
}

/// Acquire a connection, begin a transaction, run the closure, and commit.
///
/// Automatically rolls back if the closure returns an error.
pub async fn with_tx<F, R>(pool: Arc<DbPool>, f: F) -> Result<R, AppError>
where
    F: FnOnce(&rusqlite::Transaction) -> Result<R, AppError> + Send + 'static,
    R: Send + 'static,
{
    let pool = pool.clone();
    let res = tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(|e| AppError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        let r = f(&tx)?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(r)
    })
    .await
    .map_err(|e| AppError::JoinError(e.to_string()))?;
    res
}

// ---------------------------------------------------------------------------
// Connection customizer — runs on every connection checked out from the pool
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct SqliteCustomizer {
    password: Option<String>,
}

impl r2d2::CustomizeConnection<rusqlite::Connection, rusqlite::Error> for SqliteCustomizer {
    fn on_acquire(&self, conn: &mut rusqlite::Connection) -> Result<(), rusqlite::Error> {
        if let Some(ref pwd) = self.password {
            // SQLCipher PRAGMA key must be the first thing executed on a connection
            // If the password starts with x', it's a raw hex key and must be
            // DOUBLE-quoted (`PRAGMA key = "x'<hex>'"`) — the bare blob
            // literal is a syntax error in SQLCipher.
            let pragma_key = if pwd.starts_with("x'") {
                format!("PRAGMA key = \"{}\"", pwd)
            } else {
                format!("PRAGMA key = '{}'", pwd.replace("'", "''"))
            };
            // PRAGMA key returns a row; execute() would error (and the error
            // must NOT be swallowed — that silently creates a PLAINTEXT DB).
            let _ = conn.query_row(&pragma_key, [], |_| Ok(()));
        }
        let _ = conn.execute("PRAGMA foreign_keys = ON", []);
        let _ = conn.query_row("PRAGMA temp_store = MEMORY", [], |_| Ok(()));
        conn.busy_timeout(Duration::from_millis(5000))?;
        Ok(())
    }
}
