use crate::db::pool;
use crate::error::AppError;
use crate::AppState;
use chrono::Utc;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use sysinfo::Disks;

/// Result of one preflight check.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreflightCheck {
    /// Stable machine-readable id, e.g. `llama_connection`.
    pub id: String,
    /// Human-readable label shown in the UI.
    pub label: String,
    pub status: CheckStatus,
    /// Detail message rendered under the label.
    pub message: String,
    /// True when the check has a user action associated (e.g. "start llama-server").
    pub actionable: bool,
}

impl PreflightCheck {
    fn new(id: &str, label: &str, status: CheckStatus, message: String) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            status,
            message,
            actionable: false,
        }
    }

    fn pass(id: &str, label: &str, message: impl Into<String>) -> Self {
        Self::new(id, label, CheckStatus::Pass, message.into())
    }

    fn warn(id: &str, label: &str, message: impl Into<String>) -> Self {
        Self::new(id, label, CheckStatus::Warn, message.into())
    }

    fn fail(id: &str, label: &str, message: impl Into<String>) -> Self {
        Self::new(id, label, CheckStatus::Fail, message.into())
    }

    fn with_action(mut self, actionable: bool) -> Self {
        self.actionable = actionable;
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PreflightReport {
    pub app_version: String,
    pub generated_at: String,
    pub checks: Vec<PreflightCheck>,
}

impl PreflightReport {
    pub fn is_healthy(&self) -> bool {
        !self.checks.iter().any(|c| c.status == CheckStatus::Fail)
    }

    pub fn has_warnings(&self) -> bool {
        self.checks.iter().any(|c| c.status == CheckStatus::Warn)
    }
}

fn disk_free_space(data_dir: &std::path::Path) -> Option<u64> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| data_dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().components().count())
        .map(|d| d.available_space())
}

/// Check 1: the data directory exists, is writable, and the marker file round-trips.
fn check_data_dir_writable(data_dir: &Path) -> PreflightCheck {
    let marker = data_dir.join(".preflight-write-test");
    let result = std::fs::write(&marker, "ok").and_then(|_| std::fs::remove_file(&marker));
    match result {
        Ok(_) => PreflightCheck::pass(
            "data_dir_writable",
            "Data directory",
            format!("Writable: {}", data_dir.display()),
        ),
        Err(e) => PreflightCheck::fail(
            "data_dir_writable",
            "Data directory",
            format!("Not writable: {}", e),
        )
        .with_action(true),
    }
}

/// Check 2: the database pool opens and a real query succeeds.
async fn check_database(pool: Arc<pool::DbPool>) -> PreflightCheck {
    match pool::with_conn(pool, |conn| {
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM journal_entries", [], |r| r.get(0))?;
        let _ = conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))?;
        Ok::<i64, AppError>(count)
    })
    .await
    {
        Ok(count) => PreflightCheck::pass(
            "database",
            "Database",
            format!("Opens and readable ({} journal entries)", count),
        ),
        Err(e) => PreflightCheck::fail(
            "database",
            "Database",
            format!("Failed to query database: {}", e),
        ),
    }
}

/// Check 7: the engine binary can enumerate a GPU without loading a model.
fn gpu_check_from(
    result: Result<Vec<crate::ai::probe::DeviceInfo>, crate::ai::probe::ProbeError>,
) -> PreflightCheck {
    match result {
        Ok(devices) if !devices.is_empty() => {
            let d = &devices[0];
            PreflightCheck::pass(
                "gpu_offload",
                "GPU offload",
                format!(
                    "{} ({}, {} MiB free) - available for GPU offload",
                    d.name, d.id, d.free_mib
                ),
            )
        }
        Ok(_) => PreflightCheck::warn(
            "gpu_offload",
            "GPU offload",
            "No GPU device found - Arnold will run on CPU (slower, still fully offline)."
                .to_string(),
        )
        .with_action(true),
        Err(e) => PreflightCheck::fail(
            "gpu_offload",
            "GPU offload",
            format!("The engine could not be executed: {}", e),
        )
        .with_action(true),
    }
}

/// Check 8: the Microsoft Visual C++ runtime the bundled engine imports.
fn check_msvc_runtime() -> PreflightCheck {
    if crate::runtime_env::msvc_runtime_present() {
        PreflightCheck::pass(
            "msvc_runtime",
            "Visual C++ runtime",
            "Microsoft Visual C++ 2015-2022 x64 runtime detected",
        )
    } else {
        PreflightCheck::warn(
            "msvc_runtime",
            "Visual C++ runtime",
            "Missing Microsoft Visual C++ 2015-2022 x64 runtime - the engine cannot start. Install it from https://aka.ms/vs/17/release/vc_redist.x64.exe"
                .to_string(),
        )
        .with_action(true)
    }
}

/// Run every preflight check and return the full report.
pub async fn run_preflight(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<PreflightReport, AppError> {
    let mut checks: Vec<PreflightCheck> = Vec::new();
    let paths = &state.paths;

    // 1. Data directory writable
    checks.push(check_data_dir_writable(&paths.data_dir));

    // 2. Database opens and is usable
    if let Ok(db) = state.db() {
        checks.push(check_database(db).await);
    } else {
        checks.push(
            PreflightCheck::fail(
                "database",
                "Database",
                "Vault is locked. Please enter your master password to access the database.",
            )
            .with_action(true),
        );
    }

    // 3. LLM backend reachable. /health passes for ANY llama-server, so when
    // both answer, confirm the embed listener actually supports /v1/embeddings
    // (a chat-mode process squatting the embed port answers /health too).
    {
        match state.llama() {
            Ok(client) => {
                let (chat_ok, embed_ok) = client.check_health().await.unwrap_or((false, false));
                let embed_role_ok = embed_ok && client.verify_embed_role().await;
                if chat_ok && embed_role_ok {
                    checks.push(PreflightCheck::pass(
                        "llama_connection",
                        "LLM backend",
                        "Chat + embed servers reachable".to_string(),
                    ));
                } else if embed_ok && !embed_role_ok {
                    checks.push(
                        PreflightCheck::warn(
                            "llama_connection",
                            "LLM backend",
                            "Embed port answers /health but rejects /v1/embeddings — a chat-mode llama-server (no --embeddings) is using the embed port. Stop that process, then re-run the system check."
                                .to_string(),
                        )
                        .with_action(true),
                    );
                } else if chat_ok || embed_ok {
                    let detail = if chat_ok { "chat only" } else { "embed only" };
                    checks.push(
                        PreflightCheck::warn(
                            "llama_connection",
                            "LLM backend",
                            format!("Partial: {} server reachable, other offline", detail),
                        )
                        .with_action(true),
                    );
                } else {
                    checks.push(PreflightCheck::fail(
                        "llama_connection",
                        "LLM backend",
                        "Both servers unreachable. Start llama.cpp or configure Settings > Engine.".to_string(),
                    ).with_action(true));
                }
            }
            Err(_) => {
                checks.push(
                    PreflightCheck::warn(
                        "llama_connection",
                        "LLM backend",
                        "AI engine not configured — the bundled llama-server.exe was not found. Reinstall, or point Settings > Engine at a llama.cpp build."
                            .to_string(),
                    )
                    .with_action(true),
                );
            }
        }
    }

    // 3b. GPU availability + the VC++ runtime the bundled engine imports.
    {
        let resource = crate::bundled_engine_resource(app_handle);
        // No `?` here: a locked vault must still produce a full report (check 2
        // already reports it), exactly like `engine_requirements` does.
        let saved = match state.db() {
            Ok(pool) => crate::db::pool::with_conn(pool, |conn| {
                crate::db::settings::get_setting(conn, "llama_exe_path")
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_default(),
            Err(_) => String::new(),
        };
        let resolved = crate::resolve_engine_exe(&paths.data_dir, resource.as_deref(), &saved);
        let result = if resolved.is_empty() {
            Err(crate::ai::probe::ProbeError(
                "no llama-server.exe found".into(),
            ))
        } else {
            let probe = crate::ai::probe::default_probe();
            let exe = std::path::PathBuf::from(resolved);
            tokio::task::spawn_blocking(move || probe.list_devices(&exe))
                .await
                .unwrap_or_else(|e| Err(crate::ai::probe::ProbeError(e.to_string())))
        };
        checks.push(gpu_check_from(result));
        checks.push(check_msvc_runtime());
    }

    // 4. Configured model present
    {
        let model: String = crate::db::pool::with_conn(state.db()?, |conn| {
            crate::db::settings::get_setting(conn, "llama_chat_model")
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

        if model.is_empty() {
            checks.push(
                PreflightCheck::warn(
                    "model_present",
                    "AI model",
                    "No llama_chat_model configured. Import a GGUF via Settings.".to_string(),
                )
                .with_action(true),
            );
        } else {
            let store = state.models();
            let found = store
                .scan_chat()
                .iter()
                .any(|m| m.name == model || m.filename == model);
            if found {
                checks.push(PreflightCheck::pass(
                    "model_present",
                    "AI model",
                    format!("GGUF '{}' found in models/chat/", model),
                ));
            } else {
                checks.push(
                    PreflightCheck::warn(
                        "model_present",
                        "AI model",
                        format!(
                            "'{}' not in models/chat/ — import the GGUF via Settings.",
                            model
                        ),
                    )
                    .with_action(true),
                );
            }
        }
    }

    // 5. Disk space
    {
        match disk_free_space(&paths.data_dir) {
            Some(bytes) if bytes > 500 * 1024 * 1024 => checks.push(PreflightCheck::pass(
                "disk_space",
                "Disk space",
                format!("{:.1} GB free", bytes as f64 / (1024.0 * 1024.0 * 1024.0)),
            )),
            Some(bytes) => checks.push(
                PreflightCheck::warn(
                    "disk_space",
                    "Disk space",
                    format!(
                        "Only {:.0} MB free — model downloads may fail",
                        bytes as f64 / (1024.0 * 1024.0)
                    ),
                )
                .with_action(true),
            ),
            None => checks.push(PreflightCheck::warn(
                "disk_space",
                "Disk space",
                "Could not determine free disk space".to_string(),
            )),
        }
    }

    // 6. App version
    checks.push(PreflightCheck::pass(
        "app_version",
        "Application version",
        env!("CARGO_PKG_VERSION").to_string(),
    ));

    let report = PreflightReport {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        generated_at: Utc::now().to_rfc3339(),
        checks,
    };
    tracing::info!(
        target: "preflight",
        healthy = report.is_healthy(),
        warnings = report.has_warnings(),
        checks = report.checks.len(),
        "system preflight complete"
    );
    Ok(report)
}

#[tauri::command]
pub async fn preflight_run(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<PreflightReport, AppError> {
    run_preflight(&app, &state).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msvc_check_is_a_warn_or_pass_with_a_message() {
        let check = check_msvc_runtime();
        assert_eq!(check.id, "msvc_runtime");
        assert!(matches!(check.status, CheckStatus::Pass | CheckStatus::Warn));
        assert!(!check.message.is_empty());
        if check.status == CheckStatus::Warn {
            assert!(check.actionable, "a missing runtime needs an action");
        }
    }

    #[test]
    fn gpu_check_reports_the_probe_outcome() {
        let devices = vec![crate::ai::probe::DeviceInfo {
            id: "CUDA0".into(),
            name: "NVIDIA GeForce RTX 3060".into(),
            total_mib: 12287,
            free_mib: 11255,
        }];
        let check = gpu_check_from(Ok(devices));
        assert_eq!(check.id, "gpu_offload");
        assert_eq!(check.status, CheckStatus::Pass);
        assert!(check.message.contains("RTX 3060"));

        let none = gpu_check_from(Ok(Vec::new()));
        assert_eq!(none.status, CheckStatus::Warn);
        assert!(none.actionable);

        let broken = gpu_check_from(Err(crate::ai::probe::ProbeError("os error 126".into())));
        assert_eq!(broken.status, CheckStatus::Fail);
        assert!(broken.message.contains("126"));
    }
}
