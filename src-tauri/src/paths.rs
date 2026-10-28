use std::fs;
use std::path::{Path, PathBuf};

/// Centralized app path resolution (D-47).
///
/// Every filesystem location the app touches derives from a single resolved
/// data directory. Never build paths from `Path::new("..")` relative guesses
/// elsewhere — they break when the app is installed (CWD != src-tauri).
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// Root writable data directory (`<exe_dir>/data` by default).
    pub data_dir: PathBuf,
    /// SQLite database file.
    pub db_path: PathBuf,
    /// tracing debug log file.
    pub log_path: PathBuf,
    /// Automatic + manual backup destination.
    pub backup_dir: PathBuf,
    /// Bug reports / JSON export destination.
    pub export_dir: PathBuf,
    /// User attachments destination.
    pub attachments_dir: PathBuf,
    /// Directory containing `*.Modelfile` files (bundled resource or dev checkout).
    pub modelfiles_dir: PathBuf,
}

impl AppPaths {
    /// Resolve all paths from the executable-relative data directory.
    pub fn resolve() -> Self {
        Self::from_data_dir(crate::db::paths::resolve_data_dir())
    }

    /// Build the full set of paths from an explicit data directory.
    pub fn from_data_dir(data_dir: PathBuf) -> Self {
        let db_path = data_dir.join("arnold.db");
        let log_path = data_dir.join("arnold-debug.log");
        let backup_dir = data_dir.join("backups");
        let export_dir = data_dir.join("exports");
        let attachments_dir = data_dir.join("attachments");

        let modelfiles_dir = resolve_modelfiles_dir(&data_dir);

        Self {
            data_dir,
            db_path,
            log_path,
            backup_dir,
            export_dir,
            attachments_dir,
            modelfiles_dir,
        }
    }

    /// Create every directory this app writes to. Idempotent.
    pub fn ensure_dirs(&self) -> Result<(), std::io::Error> {
        for dir in [
            &self.data_dir,
            &self.backup_dir,
            &self.export_dir,
            &self.attachments_dir,
        ] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// Locate the Modelfile directory without requiring the Tauri app handle.
///
/// Priority:
/// 1. `<exe_dir>/modelfiles`   — bundled resources (MSI/NSIS/portable)
/// 2. `<exe_dir>/../../../modelfiles` — dev checkout (exe at src-tauri/target/debug)
/// 3. `<data_dir>/modelfiles`  — writable fallback
fn resolve_modelfiles_dir(data_dir: &Path) -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    if let Some(exe_dir) = exe_dir {
        let bundled = exe_dir.join("modelfiles");
        if bundled.is_dir() {
            return bundled;
        }
        let dev = exe_dir.join("../../../modelfiles");
        if dev.is_dir() {
            return dev;
        }
    }

    data_dir.join("modelfiles")
}
