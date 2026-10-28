use crate::db::{migrations, pool, settings};
use crate::error::AppError;

/// Initialise the database and return a connection pool.
///
/// 1. Creates/opens the SQLite database at the provided path (from `AppPaths`).
/// 2. Sets file-level PRAGMAs (journal_mode=WAL, synchronous=1).
/// 3. Runs pending migrations.
/// 4. Seeds default settings.
///
/// Returns a `DbPool` which manages a connection pool for concurrent access.
pub fn initialise(
    db_path: &std::path::Path,
    password: Option<String>,
) -> Result<pool::DbPool, AppError> {
    // Create the pool with per-connection PRAGMAs (foreign_keys, temp_store,
    // busy_timeout) applied by the built-in ConnectionCustomizer. File-level
    // PRAGMAs (journal_mode, synchronous) are set inside init_pool.
    let db_pool = pool::init_pool(db_path, 10, password)?;

    // Run migrations on a connection from the pool.
    let mut conn = db_pool
        .get()
        .map_err(|e| AppError::Database(e.to_string()))?;
    migrations::run_pending(&mut conn)?;
    settings::seed_defaults(&conn)?;

    println!("[DB] Initialised pool at {:?}", db_path);
    Ok(db_pool)
}
