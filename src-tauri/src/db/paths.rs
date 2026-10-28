use std::path::{Path, PathBuf};

/// Resolve the writable root: `ARNOLD_DATA_DIR` overrides the default
/// `<directory containing the executable>/data` location.
pub fn resolve_data_dir() -> PathBuf {
    // T-07: explicit env override remains available for CI/e2e and power users.
    let override_dir = std::env::var_os("ARNOLD_DATA_DIR").map(PathBuf::from);
    let exe_path = std::env::current_exe().unwrap_or_default();
    resolve_data_dir_from(&exe_path, override_dir)
}

fn resolve_data_dir_from(exe_path: &Path, override_dir: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = override_dir {
        return dir;
    }
    exe_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("data")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_data_directory_is_next_to_the_running_executable() {
        if std::env::var_os("ARNOLD_DATA_DIR").is_some() {
            return;
        }
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();

        assert_eq!(resolve_data_dir(), exe_dir.join("data"));
    }

    #[test]
    fn explicit_data_directory_override_takes_precedence() {
        let exe = Path::new(r"E:\Apps\Arnold\arnold.exe");
        let override_dir = PathBuf::from(r"F:\ArnoldData");

        assert_eq!(
            resolve_data_dir_from(exe, Some(override_dir.clone())),
            override_dir
        );
    }

    #[test]
    fn default_path_helper_uses_data_subdirectory_of_executable_parent() {
        let exe = Path::new(r"E:\Apps\Arnold\arnold.exe");
        assert_eq!(
            resolve_data_dir_from(exe, None),
            PathBuf::from(r"E:\Apps\Arnold\data")
        );
    }
}
