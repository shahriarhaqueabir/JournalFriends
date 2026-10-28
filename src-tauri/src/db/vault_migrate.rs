//! Plaintext → SQLCipher migration for pre-Sprint-7 databases (T-03).
//!
//! Before the vault switch, `arnold.db` was created by the stock SQLite
//! driver as a plaintext file with plaintext journal content. Opening such a
//! file with `PRAGMA key` fails with "file is not a database", which locked
//! every existing user out of the vault. `ensure_encrypted` detects the legacy
//! format, takes a safety backup (D-98: Backup API), exports the database into
//! a fresh keyed file via `ATTACH` + `sqlcipher_export`, swaps it into place,
//! and re-encrypts journal content that is still plaintext.
//!
//! NOTE: `PRAGMA rekey` cannot encrypt a plaintext database in this SQLCipher
//! 4.x build (it is a silent no-op on an unkeyed connection — verified
//! empirically). The `sqlcipher_export` approach is the supported migration
//! path for plaintext → encrypted.

use crate::ai::vault;
use crate::error::AppError;
use rusqlite::Connection;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// SQLite file header magic (16 bytes at offset 0).
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Ensure `db_path` is SQLCipher-encrypted, migrating a legacy plaintext file
/// in place when needed.
///
/// Returns `Ok(true)` if a migration ran, `Ok(false)` if the file was already
/// encrypted (or absent — the pool will create it encrypted on first use).
///
/// `sqlcipher_key` is the full `x'<hex>'` PRAGMA value; `aes_key` is the raw
/// derived key used for per-field journal content encryption.
pub fn ensure_encrypted(
    db_path: &Path,
    sqlcipher_key: &str,
    aes_key: &[u8; 32],
) -> Result<bool, AppError> {
    // No file → fresh database; the keyed pool creates it encrypted.
    if !db_path.exists() {
        return Ok(false);
    }

    // Detect the format from the header: a legacy plaintext DB keeps the
    // SQLite magic intact and reserves 0 bytes per page (offset 20). A
    // SQLCipher 4.x file has ciphertext where the magic was (full-header
    // encryption) and a non-zero reserved byte. Anything else (too small,
    // corrupt) is left alone — the keyed pool surfaces the real error.
    let mut header = [0u8; 32];
    let mut file = std::fs::File::open(db_path).map_err(|e| AppError::Io(e.to_string()))?;
    let n = file
        .read(&mut header)
        .map_err(|e| AppError::Io(e.to_string()))?;
    if n < 16 || &header[..16] != SQLITE_MAGIC || header[20] != 0 {
        // Not a legacy plaintext file (already SQLCipher, or not SQLite).
        return Ok(false);
    }

    tracing::info!(target: "vault", path = %db_path.display(), "legacy plaintext database detected — migrating to SQLCipher");

    // 1. Safety backup BEFORE any mutation (D-98: Backup API, never fs::copy).
    let backup_path = db_path.with_extension("db.pre-vault");
    {
        let src = Connection::open(db_path).map_err(|e| AppError::Database(e.to_string()))?;
        // Deliberately keyless: this snapshots the legacy PLAINTEXT DB before
        // mutation (T-02 — encrypted backups are only for post-migration files).
        crate::backup::manual::create_backup(&src, &backup_path, None)?;
        tracing::info!(target: "vault", backup = %backup_path.display(), "pre-migration backup created");
    }

    // 2. Export the plaintext DB into a fresh keyed file. `rekey` is a no-op
    //    on an unkeyed connection in SQLCipher 4.x, so the supported path is:
    //    ATTACH a new database with the key, then `sqlcipher_export` copies
    //    schema + data (including FTS virtual tables) while encrypting pages.
    //    WAL content is included because the export reads through the live
    //    connection. The temp file lives beside `db_path` so the swap below is
    //    same-volume (rename is otherwise not atomic on Windows).
    let enc_tmp = db_path.with_extension("db.enc-tmp");
    let _ = std::fs::remove_file(&enc_tmp);
    let _ = std::fs::remove_file(format!("{}-wal", enc_tmp.display()));
    let _ = std::fs::remove_file(format!("{}-shm", enc_tmp.display()));
    {
        let src = Connection::open(db_path).map_err(|e| AppError::Database(e.to_string()))?;
        src.execute_batch(&format!(
            "ATTACH DATABASE '{}' AS enc KEY \"{}\";",
            enc_tmp.display(),
            sqlcipher_key
        ))
        .map_err(|e| AppError::Database(format!("attach keyed target: {e}")))?;
        src.execute_batch("SELECT sqlcipher_export('enc');")
            .map_err(|e| AppError::Database(format!("sqlcipher_export: {e}")))?;
        src.execute_batch("DETACH DATABASE enc;")
            .map_err(|e| AppError::Database(format!("detach: {e}")))?;
        tracing::info!(target: "vault", tmp = %enc_tmp.display(), "plaintext DB exported to keyed temp file");
    }

    // 3. Verify the keyed temp file before swapping it over the original.
    {
        let keyed = Connection::open(&enc_tmp).map_err(|e| AppError::Database(e.to_string()))?;
        keyed
            .query_row(&format!("PRAGMA key = \"{}\"", sqlcipher_key), [], |_| {
                Ok(())
            })
            .map_err(|e| AppError::Database(format!("verify keyed open: {e}")))?;
        let count: i64 = keyed
            .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))
            .map_err(|e| AppError::Database(format!("verify read schema: {e}")))?;
        if count == 0 {
            return Err(AppError::Database(
                "migration verification failed: keyed connection cannot read schema".into(),
            ));
        }
        tracing::info!(target: "vault", tables = count, "keyed temp file verified");
    }

    // 4. Swap: remove the legacy file (the pre-vault backup is our safety
    //    net) and move the verified encrypted file into place.
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));
    std::fs::rename(&enc_tmp, db_path)
        .map_err(|e| AppError::Io(format!("swap encrypted file into place: {e}")))?;
    tracing::info!(target: "vault", path = %db_path.display(), "encrypted file swapped into place");

    // 5. Re-encrypt journal content that predates the AES-GCM switch. Rows
    //    that already decrypt are left untouched. Note: the journal_fts
    //    triggers from 002_fts_triggers.sql fire on these UPDATEs and index
    //    the ciphertext — harmless (no runtime FTS consumers today).
    let keyed = Connection::open(db_path).map_err(|e| AppError::Database(e.to_string()))?;
    keyed
        .query_row(&format!("PRAGMA key = \"{}\"", sqlcipher_key), [], |_| {
            Ok(())
        })
        .map_err(|e| AppError::Database(e.to_string()))?;

    let has_journal: i64 = keyed
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='journal_entries'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

    if has_journal > 0 {
        let mut stmt = keyed
            .prepare("SELECT id, content FROM journal_entries")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut pending: Vec<(String, String)> = Vec::new();
        for row in rows {
            let (id, content) = row.map_err(|e| AppError::Database(e.to_string()))?;
            // Encrypted rows decrypt cleanly; plaintext rows error out.
            if vault::decrypt(&content, aes_key).is_err() {
                pending.push((id, content));
            }
        }
        drop(stmt);

        let mut reencrypted = 0u64;
        for (id, content) in pending {
            let encrypted = vault::encrypt(&content, aes_key)?;
            keyed
                .execute(
                    "UPDATE journal_entries SET content = ?1 WHERE id = ?2",
                    rusqlite::params![encrypted, id],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            reencrypted += 1;
        }
        tracing::info!(target: "vault", reencrypted, "journal content re-encrypted (AES-GCM)");
    }

    // 6. Migration fully succeeded and the keyed DB reads correctly — the
    //    pre-vault copy would otherwise be a permanent PLAINTEXT artifact
    //    beside the encrypted DB (T-03). Delete it (and any WAL/SHM remnants)
    //    only now; on any earlier error the copy stays as a safety net.
    let _ = std::fs::remove_file(&backup_path);
    let _ = std::fs::remove_file(format!("{}-wal", backup_path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", backup_path.display()));

    Ok(true)
}

/// True when `db_path` already has a SQLCipher master password established
/// (an existing encrypted file). Plaintext-legacy files, missing files, and
/// non-SQLite files return `false` — in all of those cases the next unlock
/// SETS the password (T-04 drives the create-password UX from this).
///
/// Detection: bundled-sqlcipher is SQLCipher 4.x, which ships with
/// `cipher_plaintext_header_size = 0` — the ENTIRE page-1 header is
/// encrypted, so the 16-byte `SQLite format 3\0` magic is ciphertext on an
/// encrypted file (verified empirically: every keyed file shows garbled
/// magic + non-zero reserved byte at offset 20). Plaintext SQLite keeps the
/// magic intact and reserved-space byte at offset 20 == 0. So:
///   - magic intact  → plaintext SQLite (or SQLCipher 3.x) → check offset 20
///   - magic garbled → SQLCipher 4.x encrypted (fail-closed: a corrupt file
///     is also flagged so the vault demands the password instead of offering
///     to set a new one over it)
pub fn is_encrypted(db_path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(db_path) else {
        return false;
    };
    let mut header = [0u8; 32];
    let Ok(n) = file.read(&mut header) else {
        return false;
    };
    // Fewer than 21 bytes is not a real SQLite database.
    if n < 21 {
        return false;
    }
    if &header[..16] == SQLITE_MAGIC {
        // Plaintext-style header. Reserved space byte (offset 20) is 0 for
        // stock SQLite; non-zero means SQLCipher 3.x-style reserved pages.
        header[20] != 0
    } else {
        // Magic is ciphertext → SQLCipher 4.x full-header encryption.
        true
    }
}

/// Apply the SQLCipher `PRAGMA key` to a fresh connection, handling both the
/// raw-string form and the `x'<hex>'` raw-key form (same quoting rules as
/// `backup::manual::apply_key`; duplicated here to keep module layering).
fn apply_key(conn: &Connection, pwd: &str) -> Result<(), rusqlite::Error> {
    let pragma_key = if pwd.starts_with("x'") {
        format!("PRAGMA key = \"{}\"", pwd)
    } else {
        format!("PRAGMA key = '{}'", pwd.replace('\'', "''"))
    };
    // PRAGMA key returns a row in this build; execute() would error with
    // ExecuteReturnedResults. query_row discards the row and reports success.
    conn.query_row(&pragma_key, [], |_| Ok(()))?;
    Ok(())
}

fn hex(key: &[u8; 32]) -> String {
    key.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Change the master password (T-06).
///
/// Runs on a standalone connection so the live pool is untouched until the
/// caller swaps it. Steps:
/// 1. Derive old + new keys from (password, salt). The salt is per-install
///    and NOT rotated on password change — salt sidecar, journal AES keys, and
///    the SQLCipher key must all stay coherent (D-114).
/// 2. Open with the CURRENT key and verify a schema read. A wrong current
///    password fails here with `VaultInvalidPassword`, before any mutation.
/// 3. `PRAGMA rekey` re-encrypts every page with the new key. On an
///    already-keyed connection this WORKS — unlike plaintext migration where
///    rekey is a silent no-op (see module docs; the ATTACH + sqlcipher_export
///    path is only needed for plaintext → encrypted).
/// 4. Verify a fresh keyed open with the NEW key.
/// 5. Re-encrypt journal `content` (AES-GCM) with the new derived key.
///
/// Returns the NEW 32-byte derived key so the caller can rebuild the pool,
/// graph, in-memory mirror, and session key from it.
pub fn change_vault_password(
    db_path: &Path,
    salt: &str,
    current_password: &str,
    new_password: &str,
) -> Result<[u8; 32], AppError> {
    if new_password.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "new password must not be empty".into(),
        ));
    }

    let old_key = vault::derive_key(current_password, salt);
    let new_key = vault::derive_key(new_password, salt);
    let old_sqlcipher = format!("x'{}'", hex(&old_key));
    let new_sqlcipher = format!("x'{}'", hex(&new_key));

    // 1. Verify the CURRENT key before mutating anything. The live app pool
    //    holds the file open in WAL mode, so give the connection a busy
    //    timeout — the rekey below needs a brief exclusive lock.
    let mut conn = Connection::open(db_path).map_err(|e| AppError::Database(e.to_string()))?;
    let _ = conn.busy_timeout(Duration::from_secs(10));
    apply_key(&conn, &old_sqlcipher)?;
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .map_err(|_| AppError::VaultInvalidPassword)?;

    // 2. Rekey to the new key. Like `PRAGMA key`, this build returns a row —
    //    execute() errors with ExecuteReturnedResults; query_row discards it.
    conn.query_row(&format!("PRAGMA rekey = \"{}\"", new_sqlcipher), [], |_| {
        Ok(())
    })
    .map_err(|e| AppError::Database(format!("rekey failed: {e}")))?;

    // 3. Verify with a fresh connection + new key before touching content.
    {
        let verify = Connection::open(db_path).map_err(|e| AppError::Database(e.to_string()))?;
        apply_key(&verify, &new_sqlcipher)?;
        verify
            .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .map_err(|_| AppError::VaultInvalidPassword)?;
    }

    // 4. Re-encrypt journal content with the new AES key (same salt).
    reencrypt_journal_content(&mut conn, &old_key, &new_key)?;

    Ok(new_key)
}

/// Re-encrypt journal `content` with a new AES key. Rows that fail to decrypt
/// with the old key are skipped (best effort) — a failure here must not abort
/// the password change after the rekey already succeeded. The transaction
/// keeps the re-encryption atomic.
fn reencrypt_journal_content(
    conn: &mut Connection,
    old_key: &[u8; 32],
    new_key: &[u8; 32],
) -> Result<(), AppError> {
    let has_journal: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='journal_entries'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
    if has_journal == 0 {
        return Ok(());
    }

    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(e.to_string()))?;
    let mut stmt = tx
        .prepare("SELECT id, content FROM journal_entries")
        .map_err(|e| AppError::Database(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| AppError::Database(e.to_string()))?;

    let mut pending: Vec<(String, String)> = Vec::new();
    for row in rows {
        let (id, content_opt) = row.map_err(|e| AppError::Database(e.to_string()))?;
        let Some(content) = content_opt else {
            continue;
        };
        match vault::decrypt(&content, old_key) {
            Ok(plain) => pending.push((id, vault::encrypt(&plain, new_key)?)),
            Err(e) => {
                tracing::warn!(target: "vault", entry_id = %id, error = %e, "skipping content re-encrypt");
            }
        }
    }
    drop(stmt);

    for (id, content) in pending {
        tx.execute(
            "UPDATE journal_entries SET content = ?1 WHERE id = ?2",
            rusqlite::params![content, id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
    }
    tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}
