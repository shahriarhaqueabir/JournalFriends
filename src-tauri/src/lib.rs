mod ai;
mod backup;
mod db;
mod diagnostics;
mod error;
mod events;
mod logger;
mod paths;
mod preflight;
mod provision;
mod runtime_env;
mod scheduler;

use chrono::Utc;
use error::AppError;
use rusqlite::params;
use serde::Serialize;
use ai::probe::EngineProbe as _;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::{Listener, Manager};
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct AppState {
    // T-06: std::sync::Mutex<Option<..>> instead of OnceCell so the vault
    // password change can swap the pool, in-memory mirror, graph, and session
    // key at runtime. Accessors clone the Arc under a brief lock; no guard is
    // ever held across an await. `None` == vault locked.
    pub db_pool: std::sync::Mutex<Option<Arc<crate::db::pool::DbPool>>>,
    pub mem_db: std::sync::Mutex<Option<Arc<Mutex<rusqlite::Connection>>>>,
    pub graph: std::sync::Mutex<Option<Arc<Mutex<graphqlite::Graph>>>>,
    pub encryption_key: std::sync::Mutex<Option<[u8; 32]>>,
    pub paths: Arc<paths::AppPaths>,
    pub ai_state: Arc<ai::analysis::AnalysisState>,
    pub models: Arc<ai::models::ModelStore>,
    pub llama_client: std::sync::Mutex<Option<Arc<ai::client::LlamaClient>>>,
    pub llama_engine: std::sync::Mutex<Option<Arc<ai::engine::LlamaEngine>>>,
    /// Engine-HTTP telemetry ring (settings observability, in-memory only).
    pub telemetry: Arc<diagnostics::TelemetryRing<diagnostics::EngineHttpRecord>>,
    /// Last health-probe latency/timestamp cache (settings observability).
    pub health: std::sync::Mutex<diagnostics::HealthCache>,
}

fn locked_opt<T: Clone>(m: &std::sync::Mutex<Option<T>>) -> Result<T, AppError> {
    let guard = m.lock().unwrap_or_else(|e| e.into_inner());
    guard.clone().ok_or(AppError::VaultLocked)
}

fn set_opt<T>(m: &std::sync::Mutex<Option<T>>, value: T) {
    let mut guard = m.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(value);
}

impl AppState {
    pub fn db(&self) -> Result<Arc<crate::db::pool::DbPool>, AppError> {
        locked_opt(&self.db_pool)
    }

    pub fn mem_conn(&self) -> Result<Arc<Mutex<rusqlite::Connection>>, AppError> {
        locked_opt(&self.mem_db)
    }

    pub fn graph_db(&self) -> Result<Arc<Mutex<graphqlite::Graph>>, AppError> {
        locked_opt(&self.graph)
    }

    pub fn key(&self) -> Result<[u8; 32], AppError> {
        locked_opt(&self.encryption_key)
    }

    pub fn set_db(&self, pool: Arc<crate::db::pool::DbPool>) {
        set_opt(&self.db_pool, pool);
    }

    pub fn set_mem_db(&self, conn: Arc<Mutex<rusqlite::Connection>>) {
        set_opt(&self.mem_db, conn);
    }

    pub fn set_graph(&self, graph: Arc<Mutex<graphqlite::Graph>>) {
        set_opt(&self.graph, graph);
    }

    pub fn set_key(&self, key: [u8; 32]) {
        set_opt(&self.encryption_key, key);
    }

    pub fn is_unlocked(&self) -> bool {
        self.db().is_ok()
    }

    pub fn models(&self) -> Arc<ai::models::ModelStore> {
        self.models.clone()
    }

    pub fn llama(&self) -> Result<Arc<ai::client::LlamaClient>, AppError> {
        let guard = self.llama_client.lock().unwrap_or_else(|e| e.into_inner());
        guard.clone().ok_or_else(|| {
            AppError::AiError("AI client not available (vault locked or settings missing)".into())
        })
    }

    pub fn engine(&self) -> Result<Arc<ai::engine::LlamaEngine>, AppError> {
        let guard = self.llama_engine.lock().unwrap_or_else(|e| e.into_inner());
        guard.clone().ok_or_else(|| {
            AppError::AiError(
                "LLM engine not available (bundled runtime missing; set llama_exe_path in Settings)".into(),
            )
        })
    }

    pub fn set_llama(&self, c: Arc<ai::client::LlamaClient>) {
        let mut guard = self.llama_client.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(c);
    }

    pub fn set_engine(&self, e: Arc<ai::engine::LlamaEngine>) {
        let mut guard = self.llama_engine.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(e);
    }
}

#[derive(Serialize)]
struct AppDiagnostics {
    generated_at: String,
    data_dir: String,
    db_path: String,
    log_path: String,
    db_size_bytes: u64,
    journal_entries: i64,
    tasks: i64,
    memory_facts: i64,
    conversations: i64,
    analysis_queue_len: usize,
    analysis_statuses: AnalysisStatusCounts,
    engine: EngineDiagSnapshot,
    models: Vec<ModelDiagInfo>,
}

#[derive(Debug, Default, Serialize)]
struct EngineDiagSnapshot {
    chat: ServiceDiagState,
    embed: ServiceDiagState,
    last_health_check: Option<String>,
}

#[derive(Debug, Default, Serialize)]
struct ServiceDiagState {
    status: String,
    pid: Option<u32>,
    latency_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ModelDiagInfo {
    kind: String,
    name: String,
    filename: String,
    size_bytes: u64,
}

#[derive(Default, Serialize)]
struct AnalysisStatusCounts {
    queued: usize,
    processing: usize,
    done: usize,
    failed: usize,
}

fn count_rows(conn: &rusqlite::Connection, table: &str) -> Result<i64, AppError> {
    let sql = format!("SELECT COUNT(*) FROM {}", table);
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(AppError::from)
}

/// Single source of truth for the app version (P-07): Cargo.toml is canonical,
/// tauri.conf.json `version` must match, and the frontend footer renders this
/// value at runtime instead of a hardcoded string.
/// Vault state exposed to the frontend (T-04). `password_set` tells the UI
/// whether a master password already exists (an encrypted DB on disk) — when
/// false the next unlock CREATES the password, so the overlay shows a confirm
/// field and an unrecoverable-data warning instead of a single field.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub unlocked: bool,
    pub password_set: bool,
}

#[tauri::command]
async fn vault_status(state: tauri::State<'_, AppState>) -> Result<VaultStatus, AppError> {
    Ok(VaultStatus {
        unlocked: state.is_unlocked(),
        password_set: db::vault_migrate::is_encrypted(&state.paths.db_path),
    })
}

#[tauri::command]
async fn unlock_vault(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    password: String,
) -> Result<(), AppError> {
    if state.is_unlocked() {
        return Err(AppError::VaultAlreadyUnlocked);
    }

    let db_path = state.paths.db_path.clone();

    // Deriving key (ALE). Held locally until the password is verified — the
    // old order committed it to state BEFORE validation, so a wrong password
    // permanently wedged the vault (VaultAlreadyUnlocked on every retry) and
    // forced an app restart (T-01).
    // T-05: per-install random salt sidecar (`arnold.db.salt`), with a
    // static-salt fallback for pre-T-05 vaults so they stay unlockable.
    let salt = ai::vault::get_or_create_salt(&db_path)?;
    let derived_key = ai::vault::derive_key(&password, &salt);

    // Format for SQLCipher (raw key as hex)
    let hex_key: String = derived_key.iter().map(|b| format!("{:02x}", b)).collect();
    let sqlcipher_key = format!("x'{}'", hex_key);

    // T-03: Migrate a legacy plaintext database to SQLCipher BEFORE opening
    // the keyed pool. A keyed open of a plaintext file fails with "file is not
    // a database" (VaultInvalidPassword), locking every pre-Sprint-7 user out.
    db::vault_migrate::ensure_encrypted(&db_path, &sqlcipher_key, &derived_key)?;

    // Initialise the DB pool. A wrong password on an already-encrypted DB
    // fails HERE (keyed open → VaultInvalidPassword) before any state is
    // committed, so the user can simply retry without restarting.
    let pool = db::init::initialise(&db_path, Some(sqlcipher_key.clone()))?;
    let pool_arc = Arc::new(pool);

    // Password verified — only now commit the session key and pool. The
    // setters overwrite (std::sync::Mutex<Option>), so re-unlock after a
    // failed attempt is naturally idempotent.
    state.set_key(derived_key);
    state.set_db(pool_arc.clone());

    // Initialise Memory DB
    let mem_conn =
        rusqlite::Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;
    mem_conn.execute("CREATE TABLE mem_journal_entries (id TEXT PRIMARY KEY, title TEXT, content TEXT, created_at TEXT, updated_at TEXT)", []).map_err(|e| AppError::Database(e.to_string()))?;
    state.set_mem_db(Arc::new(Mutex::new(mem_conn)));

    // Populate Memory DB from Disk (Decrypting)
    let entries = {
        let db = pool_arc.clone();
        db::pool::with_conn(db, |conn| {
            let mut stmt = conn.prepare("SELECT id, title, content, created_at, updated_at FROM journal_entries WHERE is_deleted = 0")?;
            let rows = stmt.query_map([], |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?
            )))?;
            let mut res = Vec::new();
            for r in rows { res.push(r?); }
            Ok::<_, AppError>(res)
        }).await?
    };

    {
        let key = state.key()?;
        let mem_conn_mu = state.mem_conn()?;
        let mem_conn = mem_conn_mu.lock().await;
        for (id, title, content_enc, created_at, updated_at) in entries {
            // Best effort decryption
            if let Ok(content) = ai::vault::decrypt(&content_enc, &key) {
                let _ = mem_conn.execute("INSERT INTO mem_journal_entries (id, title, content, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, title, content, created_at, updated_at]);
            }
        }
    }

    // Initialise Graph.
    let graph_conn_rus =
        rusqlite::Connection::open(&db_path).map_err(|e| AppError::Database(e.to_string()))?;
    // SQLCipher requires the key via `PRAGMA key = "x'<hex>'"` (double-quoted
    // raw-key form) as the FIRST statement on a fresh connection — the bare
    // `x'...'` blob literal is a syntax error that made every unlock fail.
    // PRAGMA key returns a row; execute() would error (ExecuteReturnedResults),
    // so use query_row and discard the row.
    graph_conn_rus
        .query_row(&format!("PRAGMA key = \"{}\"", sqlcipher_key), [], |_| {
            Ok(())
        })
        .map_err(|e| AppError::Database(e.to_string()))?;

    // Graph::from_connection wraps the standard rusqlite connection to provide Cypher support (T-01)
    let graph_conn = graphqlite::Connection::from_rusqlite(graph_conn_rus)
        .map_err(|e| AppError::Database(e.to_string()))?;
    let graph = graphqlite::Graph::from_connection(graph_conn);
    state.set_graph(Arc::new(Mutex::new(graph)));

    // ── Build LlamaClient + LlamaEngine from settings (available after pool open) ──
    {
        let conn = pool_arc
            .get()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let chat_model = db::settings::get_setting(&conn, "llama_chat_model")
            .ok()
            .flatten()
            .unwrap_or_default();
        let embed_model = db::settings::get_setting(&conn, "llama_embed_model")
            .ok()
            .flatten()
            .unwrap_or_default();
        let chat_port: u16 = db::settings::get_setting(&conn, "llama_chat_port")
            .ok()
            .flatten()
            .unwrap_or_else(|| "8080".into())
            .parse()
            .unwrap_or(8080);
        let embed_port: u16 = db::settings::get_setting(&conn, "llama_embed_port")
            .ok()
            .flatten()
            .unwrap_or_else(|| "8081".into())
            .parse()
            .unwrap_or(8081);
        let gpu_layers: String = db::settings::get_setting(&conn, "llama_ngl")
            .ok()
            .flatten()
            .unwrap_or_else(|| "auto".into());
        let ctx_size: u32 = db::settings::get_setting(&conn, "llama_ctx_size")
            .ok()
            .flatten()
            .unwrap_or_else(|| "16384".into())
            .parse()
            .unwrap_or(16384);
        let exe_path_setting = db::settings::get_setting(&conn, "llama_exe_path")
            .ok()
            .flatten()
            .unwrap_or_default();

        // F-004: generate API key once if not yet persisted
        let api_key = {
            let existing = db::settings::get_setting(&conn, "llama_api_key")
                .ok()
                .flatten()
                .unwrap_or_default();
            if existing.is_empty() {
                let key = ai::engine::generate_api_key();
                let _ = db::settings::set_setting(&conn, "llama_api_key", &key);
                key
            } else {
                existing
            }
        };

        let chat_url = format!("http://127.0.0.1:{}", chat_port);
        let embed_url = format!("http://127.0.0.1:{}", embed_port);

        let engine_event_app = app.clone();
        let client = Arc::new(ai::client::LlamaClient::new(
            &chat_url,
            &embed_url,
            &chat_model,
            &embed_model,
            &api_key,
            state.telemetry.clone(),
            Some(Arc::new(move |ev| {
                crate::events::emit(&engine_event_app, ev);
            })),
        ));
        state.set_llama(client);

        // Prefer the app-local engine override, then the packaged runtime
        // resource (exe plus adjacent DLLs), then the user's saved path.
        let engine_resource = bundled_engine_resource(&app);
        let exe_path = resolve_engine_exe(
            &state.paths.data_dir,
            engine_resource.as_deref(),
            &exe_path_setting,
        );
        if !exe_path.is_empty() {
            let engine = Arc::new(ai::engine::LlamaEngine::new(ai::engine::EngineConfig {
                exe_path: exe_path.into(),
                chat_model: effective_model_path(&state.models(), &chat_model, false),
                embed_model: effective_model_path(&state.models(), &embed_model, true),
                chat_port,
                embed_port,
                gpu_layers,
                ctx_size,
                api_key,
                log_dir: state.paths.data_dir.join("logs"),
            }));
            state.set_engine(engine);
        }
    }

    // Start background workers
    let handle = app.clone();
    crate::scheduler::reminders::spawn_reminder_worker(handle.clone());
    crate::backup::auto::spawn_auto_backup_worker(handle.clone());
    crate::scheduler::weekly_report::spawn_weekly_review_worker(handle.clone());

    // Check for onboarding
    {
        let onboarding_handle = handle.clone();
        tauri::async_runtime::spawn(async move {
            let state = onboarding_handle.state::<AppState>();
            let db = match state.db() {
                Ok(d) => d,
                Err(_) => return,
            };
            let done = db::pool::with_conn(db, |conn| {
                db::settings::get_setting(conn, "onboarding_complete")
            })
            .await
            .ok()
            .flatten()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);

            if !done {
                crate::events::emit(
                    &onboarding_handle,
                    crate::events::AppEvent::OnboardingRequired,
                );
            }
        });
    }

    // T-06: Provisioning needs the vault unlocked (db_set_model reads/writes
    // settings), so re-run it now that we have a working pool. The startup
    // call stays for the already-unlocked/fresh path.
    let prov_handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = crate::provision::provision_model(&prov_handle).await;
        tracing::info!(target: "provision", outcome = %outcome.summary(), "provisioning after vault unlock");
    });

    Ok(())
}

#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// T-06: change the vault master password while the app is running.
///
/// The DB-level `change_vault_password` rekeys the whole SQLCipher file and
/// re-encrypts journal content under the new derived key (same per-install
/// salt — D-114). Here we then rebuild the pool, in-memory mirror, and graph
/// connection with the new key and swap them into state. Handlers and workers
/// fetch fresh Arcs from `state` each call, so a swap propagates safely.
#[tauri::command]
async fn change_vault_password(
    state: tauri::State<'_, AppState>,
    old_password: String,
    new_password: String,
) -> Result<(), AppError> {
    if !state.is_unlocked() {
        return Err(AppError::VaultLocked);
    }

    let db_path = state.paths.db_path.clone();
    let salt = ai::vault::get_or_create_salt(&db_path)?;
    let new_key =
        db::vault_migrate::change_vault_password(&db_path, &salt, &old_password, &new_password)?;

    // Rebuild the pool with the new key and swap state in.
    let hex_key: String = new_key.iter().map(|b| format!("{:02x}", b)).collect();
    let sqlcipher_key = format!("x'{}'", hex_key);
    let pool = db::init::initialise(&db_path, Some(sqlcipher_key.clone()))?;
    let pool_arc = Arc::new(pool);
    state.set_key(new_key);
    state.set_db(pool_arc.clone());

    // Rebuild the in-memory journal mirror under the new key.
    let mem_conn =
        rusqlite::Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;
    mem_conn.execute("CREATE TABLE mem_journal_entries (id TEXT PRIMARY KEY, title TEXT, content TEXT, created_at TEXT, updated_at TEXT)", []).map_err(|e| AppError::Database(e.to_string()))?;
    let entries = {
        let db = pool_arc.clone();
        db::pool::with_conn(db, |conn| {
            let mut stmt = conn.prepare("SELECT id, title, content, created_at, updated_at FROM journal_entries WHERE is_deleted = 0")?;
            let rows = stmt.query_map([], |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?
            )))?;
            let mut res = Vec::new();
            for r in rows { res.push(r?); }
            Ok::<_, AppError>(res)
        }).await?
    };
    {
        let mem_conn_mu = Arc::new(Mutex::new(mem_conn));
        for (id, title, content_enc, created_at, updated_at) in entries {
            if let Ok(content) = ai::vault::decrypt(&content_enc, &new_key) {
                let _ = mem_conn_mu.lock().await.execute(
                    "INSERT INTO mem_journal_entries (id, title, content, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, title, content, created_at, updated_at],
                );
            }
        }
        state.set_mem_db(mem_conn_mu);
    }

    // Rebuild the graph connection with the new key (PRAGMA key first
    // statement, query_row form — same pattern as unlock_vault).
    let graph_conn_rus =
        rusqlite::Connection::open(&db_path).map_err(|e| AppError::Database(e.to_string()))?;
    graph_conn_rus
        .query_row(&format!("PRAGMA key = \"{}\"", sqlcipher_key), [], |_| {
            Ok(())
        })
        .map_err(|e| AppError::Database(e.to_string()))?;
    let graph_conn = graphqlite::Connection::from_rusqlite(graph_conn_rus)
        .map_err(|e| AppError::Database(e.to_string()))?;
    state.set_graph(Arc::new(Mutex::new(graphqlite::Graph::from_connection(
        graph_conn,
    ))));

    tracing::info!(target: "vault", "vault password changed");
    Ok(())
}

// -- snip: many command handlers --
// For brevity, reconstruct only the parts we modified: save_journal_entry, ai analysis worker wiring, and run()

#[tauri::command]
async fn save_journal_entry(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    id: String,
    title: Option<String>,
    content: String,
) -> Result<String, AppError> {
    tracing::debug!(
        target: "journal",
        id = %id,
        title_present = title.as_ref().map(|t| !t.trim().is_empty()).unwrap_or(false),
        content_chars = content.chars().count(),
        "save_journal_entry requested"
    );

    let now = Utc::now().to_rfc3339();
    let is_new = id.is_empty();
    let entry_id = if id.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        id.clone()
    };

    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    let word_count = content.split_whitespace().count() as i64;

    let title_clone = title.clone();
    let entry_id_clone = entry_id.clone();
    let content_plain = content.clone();
    let now_clone = now.clone();

    // Encrypt content for disk
    let key = state.key()?;
    let content_enc = ai::vault::encrypt(&content, &key)?;

    let entry = db::journal::JournalEntry {
        id: entry_id.clone(),
        title,
        content: content_enc,
        word_count,
        created_at: now.clone(),
        updated_at: now,
    };

    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::journal::upsert_entry(conn, &entry)).await?;

    // Update in-memory DB
    if let Ok(mem_conn_mu) = state.mem_conn() {
        let mem_conn = mem_conn_mu.lock().await;
        let _ = mem_conn.execute(
            "INSERT INTO mem_journal_entries (id, title, content, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET title=excluded.title, content=excluded.content, updated_at=excluded.updated_at",
            params![entry_id_clone, title_clone, content_plain, now_clone, now_clone],
        );
    }

    if is_new {
        if let Err(e) = enqueue_analysis(&state, &app, &entry_id).await {
            tracing::warn!(target: "analysis", entry_id = %entry_id, error = %e, "new-entry analysis enqueue failed");
        }
    }

    tracing::debug!(target: "journal", entry_id = %entry_id, "journal entry saved");
    Ok(entry_id)
}

// Minimal placeholder implementations for functions referenced elsewhere so compilation succeeds
#[tauri::command]
async fn journal_get(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<db::journal::JournalEntry>, AppError> {
    let mem_conn_mu = state.mem_conn()?;
    let mem_conn = mem_conn_mu.lock().await;

    let entry = mem_conn.query_row(
        "SELECT id, title, content, created_at, updated_at FROM mem_journal_entries WHERE id = ?1",
        params![id],
        |row| {
            Ok(db::journal::JournalEntry {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                word_count: 0, // Not stored in memory for now
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        },
    );

    match entry {
        Ok(e) => Ok(Some(e)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}

#[tauri::command]
async fn journal_list(
    state: tauri::State<'_, AppState>,
    cursor: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<db::journal::JournalEntrySummary>, AppError> {
    let limit = limit.unwrap_or(20);
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        let entries = db::journal::list_entries(conn, cursor.as_deref(), limit)?;
        Ok(entries)
    })
    .await
}

#[tauri::command]
async fn journal_delete(state: tauri::State<'_, AppState>, id: String) -> Result<bool, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::journal::soft_delete(conn, &id)).await
}

#[tauri::command]
async fn ai_chat(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    conversation_id: Option<String>,
    message: String,
    source_entry_id: Option<String>,
    source: Option<String>,
) -> Result<String, AppError> {
    let source = source.unwrap_or_else(|| "ai_tab".to_string());
    if !matches!(
        source.as_str(),
        "sidebar" | "ai_tab" | "analysis" | "weekly_plan"
    ) {
        return Err(AppError::InvalidInput(format!(
            "invalid conversation source '{source}' (allowed: sidebar, ai_tab, analysis, weekly_plan)"
        )));
    }
    let start = std::time::Instant::now();
    tracing::info!(target: "ai_chat", conversation_id = ?conversation_id, source_entry_id = ?source_entry_id, message_len = message.len(), "IPC: ai_chat started");

    let conv_id = conversation_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let now = Utc::now().to_rfc3339();
    let pool = state.db()?;

    // Create or update conversation
    {
        let pool = pool.clone();
        let conv_id = conv_id.clone();
        let source_entry_id = source_entry_id.clone();
        let title = if conversation_id.is_some() {
            None
        } else {
            Some(message.chars().take(40).collect::<String>())
        };
        let model = db::pool::with_conn(pool.clone(), |conn| {
            db::settings::get_setting(conn, "llama_chat_model")
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".into());
        db::pool::with_conn(pool, move |conn| {
            db::ai::upsert_conversation(
                conn,
                &conv_id,
                title.as_deref(),
                &model,
                &source,
                source_entry_id.as_deref(),
                Some("user"),
            )
        })
        .await?;
    }

    // Save user message
    let user_msg = db::ai::AiMessage {
        id: Uuid::new_v4().to_string(),
        conversation_id: conv_id.clone(),
        role: "user".to_string(),
        content: message.clone(),
        tool_name: None,
        tool_args: None,
        tool_result: None,
        tool_call_id: None,
        confirmed: None,
        model: Some(
            db::pool::with_conn(pool.clone(), |conn| {
                db::settings::get_setting(conn, "llama_chat_model")
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "unknown".into()),
        ),
        tokens_in: None,
        tokens_out: None,
        created_at: now.clone(),
        source_entry_id: None,
        created_by: Some("user".to_string()),
        last_reinforced: None,
    };
    {
        let pool = pool.clone();
        db::pool::with_conn(pool, move |conn| db::ai::insert_message(conn, &user_msg)).await?;
    }

    // Load recent messages for conversation context
    let recent_messages = {
        let pool = pool.clone();
        let conv_id = conv_id.clone();
        db::pool::with_conn(pool, move |conn| {
            db::ai::recent_messages(conn, &conv_id, 20)
        })
        .await?
    };
    let recent_messages_json: Vec<serde_json::Value> = db::ai::replay_messages(&recent_messages);

    // Build context: memory items + journal snippets (Hybrid Search) + open tasks
    let facts = {
        let pool = pool.clone();
        db::pool::with_conn(pool, |conn| {
            crate::db::knowledge::list_items(conn, None, None, None)
        })
        .await
        .unwrap_or_default()
    };

    // Resolve the user's name from their memory items so the prompt can state it
    // unambiguously. Prefer explicit "name"/"call you" items, then any short
    // "person"-category item, so the model never substitutes its own name.
    // Resolve the user's name. Prefer an explicit "name" item (saved from
    // chat) over the onboarding "What should I call you?" nickname, then any
    // short person-category item. Facts are ordered by updated_at DESC, so the
    // most recently saved name wins within each tier.
    let user_name: Option<String> = {
        let mut candidate: Option<String> = None;
        // Tier 1: explicit name items (title contains "name" but not the
        // onboarding "call you" phrasing, which may hold a nickname).
        for item in &facts {
            let title_lc = item.title.to_lowercase();
            if title_lc.contains("name") && !title_lc.contains("call you") {
                let c = item.content.trim();
                if !c.is_empty() && c.chars().count() <= 60 {
                    candidate = Some(c.to_string());
                    break;
                }
            }
        }
        // Tier 2: onboarding "What should I call you?" (may be a nickname).
        if candidate.is_none() {
            for item in &facts {
                let title_lc = item.title.to_lowercase();
                if title_lc.contains("call you") || title_lc.contains("what should i call") {
                    let c = item.content.trim();
                    if !c.is_empty() && c.chars().count() <= 60 {
                        candidate = Some(c.to_string());
                        break;
                    }
                }
            }
        }
        // Tier 3: any short person-category item.
        if candidate.is_none() {
            for item in &facts {
                if item.category == "person" {
                    let c = item.content.trim();
                    if !c.is_empty() && c.chars().count() <= 40 && !c.contains('.') {
                        candidate = Some(c.to_string());
                        break;
                    }
                }
            }
        }
        candidate
    };

    // T-02/T-03: Hybrid Context (Keyword + Semantic)
    let journal_context = {
        let mut results = Vec::new();
        let mut seen_ids = HashSet::new();

        // 1. Keyword Search (LIKE on memory DB) — use extracted keywords instead of
        // the raw message so multi-word queries actually match journal entries.
        let keyword_results = {
            let mem_mu = state.mem_conn()?;
            let mem = mem_mu.lock().await;
            let keywords = crate::ai::keywords::unique_keywords(&message, 5);
            // Fall back to the whole message if no keywords survived filtering.
            let search_terms: Vec<String> = if keywords.is_empty() {
                vec![message.clone()]
            } else {
                keywords
            };

            let mut seen = HashSet::new();
            let mut res = Vec::new();
            let mut stmt = mem
                .prepare(
                    "SELECT id, title, substr(content, 1, 280) as snippet, created_at
                     FROM mem_journal_entries
                     WHERE content LIKE ?1 OR title LIKE ?1
                     LIMIT 5",
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            for term in &search_terms {
                let search_term = format!("%{}%", term);
                let rows = stmt
                    .query_map(params![search_term], |row| {
                        Ok(db::journal::JournalContextSnippet {
                            id: row.get(0)?,
                            title: row.get(1)?,
                            snippet: row.get(2)?,
                            created_at: row.get(3)?,
                        })
                    })
                    .map_err(|e| AppError::Database(e.to_string()))?;
                for row in rows {
                    let r = row.map_err(|e| AppError::Database(e.to_string()))?;
                    if seen.insert(r.id.clone()) {
                        res.push(r);
                        if res.len() >= 5 {
                            break;
                        }
                    }
                }
                if res.len() >= 5 {
                    break;
                }
            }
            res
        };

        for r in keyword_results {
            seen_ids.insert(r.id.clone());
            results.push(r);
        }

        // 2. Semantic Search (Embeddings on Disk DB)
        if let Ok(client) = state.llama() {
            if let Ok(query_vec) = client.generate_embeddings(&message).await {
                let pool_sem = pool.clone();
                let semantic_ids = db::pool::with_conn(pool_sem, move |conn| {
                    db::vectors::semantic_search(conn, &query_vec, 5)
                })
                .await
                .unwrap_or_default();

                for (entry_id, _score) in semantic_ids {
                    if !seen_ids.contains(&entry_id) {
                        // Fetch snippet from memory DB
                        if let Ok(mem_mu) = state.mem_conn() {
                            let mem = mem_mu.lock().await;
                            let snippet_opt = mem.query_row(
                            "SELECT id, title, substr(content, 1, 280), created_at FROM mem_journal_entries WHERE id = ?1",
                            params![entry_id],
                            |row| Ok(db::journal::JournalContextSnippet {
                                id: row.get(0)?,
                                title: row.get(1)?,
                                snippet: row.get(2)?,
                                created_at: row.get(3)?,
                            })
                        ).ok();

                            if let Some(snippet) = snippet_opt {
                                seen_ids.insert(entry_id);
                                results.push(snippet);
                            }
                        }
                    }
                }
            }
        }
        results
    };
    let task_context = {
        let pool = pool.clone();
        db::pool::with_conn(pool, |conn| crate::db::tasks::list_tasks(conn, false))
            .await
            .unwrap_or_default()
    };

    let event_context = {
        let pool = pool.clone();
        let now = Utc::now().to_rfc3339();
        db::pool::with_conn(pool, move |conn| {
            db::events::list_events(conn, Some(&now), Some(10))
        })
        .await
        .unwrap_or_default()
    };

    let weekly_review = {
        let pool = pool.clone();
        db::pool::with_conn(pool, db::reports::get_weekly_review)
            .await
            .ok()
    };

    let ai_voice = {
        let pool = pool.clone();
        db::pool::with_conn(pool, move |conn| {
            db::settings::get_setting(conn, "ai_voice")
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
    };

    let mood_context = {
        let pool = pool.clone();
        let now_local = chrono::Local::now();
        let start = now_local.date_naive() - chrono::Duration::days(7);
        let end = now_local.date_naive();
        let start_s = start.format("%Y-%m-%d").to_string();
        let end_s = end.format("%Y-%m-%d").to_string();
        db::pool::with_conn(pool, move |conn| {
            let manual = crate::db::mood::manual_days(conn, &start_s, &end_s)?;
            let inferred = crate::db::mood::inferred_days(conn, &start_s, &end_s)?;
            Ok::<_, AppError>(crate::db::mood::merge_days(manual, inferred))
        })
        .await
        .unwrap_or_default()
    };
    let mood_context_str: String = if mood_context.is_empty() {
        String::new()
    } else {
        mood_context
            .iter()
            .map(|d| format!("{}: {} ({})", d.date, d.mood, d.source))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let system_prompt = crate::ai::prompt::get_chat_system_prompt(
        &facts,
        &journal_context,
        &task_context,
        &event_context,
        weekly_review.as_ref(),
        &ai_voice,
        &mood_context_str,
        user_name.as_deref(),
    );

    // Build tool executor and call the tool-aware chat pipeline
    let executor = crate::ai::tools::ToolExecutor::new_with_vault(pool.clone(), state.mem_conn()?);
    let tool_defs = crate::ai::tools::get_tools();
    let client = state.llama()?;

    // Build full-fidelity message list (Vec<Value>) for correct tool_calls round-tripping
    let mut msgs: Vec<serde_json::Value> =
        crate::ai::build_chat_messages(&system_prompt, &recent_messages_json);

    let mut full_response = String::new();
    let max_turns = 10u32;
    let mut received_final_turn = false;

    for _turn in 0..max_turns {
        let mut did_tool = false;
        let mut tool_call_id = String::new();
        let mut tool_name = String::new();
        let mut tool_args = String::new();
        // Per-turn buffer (M2): only the final non-tool turn becomes the
        // persisted assistant message; tool-turn preambles must not leak in.
        let mut turn_response = String::new();

        let resp = client
            .chat_with_tools_stream(
                msgs.clone(),
                Some(tool_defs.clone()),
                |token| {
                    turn_response.push_str(&token);
                    crate::events::emit(
                        &app,
                        crate::events::AppEvent::AiToken {
                            conversation_id: conv_id.clone(),
                            token,
                        },
                    );
                },
                |reasoning_tok| {
                    crate::events::emit(
                        &app,
                        crate::events::AppEvent::AiReasoningToken {
                            conversation_id: conv_id.clone(),
                            token: reasoning_tok,
                        },
                    );
                },
                |id, name, args| {
                    did_tool = true;
                    tool_call_id = id;
                    tool_name = name;
                    tool_args = args;
                },
            )
            .await?;

        if !did_tool {
            // Never persist `resp` here: the streaming client may return its
            // reasoning text when no visible answer was generated (D-139).
            full_response = crate::ai::terminal_chat_response(&turn_response, false);
            if turn_response.trim().is_empty() {
                crate::events::emit(
                    &app,
                    crate::events::AppEvent::AiToken {
                        conversation_id: conv_id.clone(),
                        token: full_response.clone(),
                    },
                );
            }
            received_final_turn = true;
            break;
        }

        // Tool call turn: push assistant tool_calls + result, persist, continue.
        let assistant_call_msg = serde_json::json!({
            "role": "assistant",
            "content": resp.clone(),
            "tool_calls": [{"id": tool_call_id, "type":"function","function":{"name":tool_name,"arguments":tool_args}}],
        });
        msgs.push(assistant_call_msg);

        let args_val: serde_json::Value =
            serde_json::from_str(&tool_args).unwrap_or(serde_json::json!({}));
        let args_val = crate::ai::percent_decode_value(args_val);
        let result = executor.execute(&tool_name, args_val).await;
        let tool_content = result.to_model_content();
        msgs.push(serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "content": tool_content,
        }));

        // Persist the assistant tool-call turn and its tool result
        let tool_turn_time = Utc::now().to_rfc3339();
        let model_name = db::pool::with_conn(pool.clone(), |conn| {
            db::settings::get_setting(conn, "llama_chat_model")
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".into());
        let assistant_call_row = db::ai::AiMessage {
            id: Uuid::new_v4().to_string(),
            conversation_id: conv_id.clone(),
            role: "assistant".to_string(),
            // Preserve any user-visible preamble streamed with this tool call;
            // it is replayed as the content of the assistant tool_calls row.
            content: resp,
            tool_name: Some(tool_name.clone()),
            tool_args: Some(tool_args.clone()),
            tool_result: None,
            tool_call_id: Some(tool_call_id.clone()),
            confirmed: None,
            model: Some(model_name.clone()),
            tokens_in: None,
            tokens_out: None,
            created_at: tool_turn_time.clone(),
            source_entry_id: None,
            created_by: Some("ai".to_string()),
            last_reinforced: None,
        };
        let tool_result_row = db::ai::AiMessage {
            id: Uuid::new_v4().to_string(),
            conversation_id: conv_id.clone(),
            role: "tool".to_string(),
            content: String::new(),
            tool_name: Some(tool_name.clone()),
            tool_args: None,
            tool_result: Some(tool_content),
            tool_call_id: Some(tool_call_id.clone()),
            confirmed: None,
            model: Some(model_name),
            tokens_in: None,
            tokens_out: None,
            created_at: tool_turn_time,
            source_entry_id: None,
            created_by: Some("ai".to_string()),
            last_reinforced: None,
        };
        {
            db::pool::with_conn(pool.clone(), move |conn| {
                db::ai::insert_message(conn, &assistant_call_row)?;
                db::ai::insert_message(conn, &tool_result_row)
            })
            .await?;
        }
    }

    // The model can keep issuing tools until the loop budget is exhausted.
    // Surface and persist an explicit terminal response instead of completing
    // with an empty assistant row.
    if !received_final_turn {
        full_response = crate::ai::terminal_chat_response("", true);
        crate::events::emit(
            &app,
            crate::events::AppEvent::AiToken {
                conversation_id: conv_id.clone(),
                token: full_response.clone(),
            },
        );
    }

    let _response = full_response;

    // Check for pending proposals and emit event if any
    {
        let pool = pool.clone();
        let proposal_count: Result<u32, _> = db::pool::with_conn(pool, |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM proposed_inbox WHERE status = 'pending' AND created_by = 'ai'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))
        })
        .await;
        if let Ok(count) = proposal_count {
            if count > 0 {
                crate::events::emit(
                    &app,
                    crate::events::AppEvent::AiProposalsReady {
                        conversation_id: conv_id.clone(),
                        count,
                    },
                );
            }
        }
    }

    // Save assistant response (persisted from latest ai_message entry)
    let now = Utc::now().to_rfc3339();
    let assistant_msg = db::ai::AiMessage {
        id: Uuid::new_v4().to_string(),
        conversation_id: conv_id.clone(),
        role: "assistant".to_string(),
        content: _response,
        tool_name: None,
        tool_args: None,
        tool_result: None,
        tool_call_id: None,
        confirmed: None,
        model: Some(
            db::pool::with_conn(pool.clone(), |conn| {
                db::settings::get_setting(conn, "llama_chat_model")
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "unknown".into()),
        ),
        tokens_in: None,
        tokens_out: None,
        created_at: now,
        source_entry_id: None,
        created_by: Some("ai".to_string()),
        last_reinforced: None,
    };
    {
        db::pool::with_conn(pool, move |conn| {
            db::ai::insert_message(conn, &assistant_msg)
        })
        .await?;
    }

    // Completion means the final answer has been persisted. Request errors
    // return through the IPC error path, which renders its own error state.
    crate::events::emit(
        &app,
        crate::events::AppEvent::AiResponseComplete {
            conversation_id: conv_id.clone(),
        },
    );

    let duration = start.elapsed();
    tracing::info!(target: "ai_chat", conversation_id = %conv_id, duration_ms = duration.as_millis(), "IPC: ai_chat completed");
    Ok(conv_id)
}

#[tauri::command]
async fn app_diagnostics(state: tauri::State<'_, AppState>) -> Result<AppDiagnostics, AppError> {
    let pool = state.db()?;
    let (journal_entries, tasks, memory_facts, conversations) = db::pool::with_conn(pool, |conn| {
        Ok((
            count_rows(conn, "journal_entries").unwrap_or(0),
            count_rows(conn, "tasks").unwrap_or(0),
            count_rows(conn, "knowledge_items").unwrap_or(0),
            count_rows(conn, "ai_conversations").unwrap_or(0),
        ))
    })
    .await?;

    // E-2: source the "Analysis queue" cell from the persisted queue table —
    // the same store the queue card reads — so both numbers agree. The
    // in-memory deque is transient and drains as items finish.
    let analysis_queue_len = {
        let pool = state.db()?;
        db::pool::with_conn(pool, |conn| crate::db::analysis_queue::count_all(conn))
            .await
            .map(|n| n.max(0) as usize)
            .unwrap_or(0)
    };
    let statuses = state.ai_state.status.lock().await;
    let mut analysis_statuses = AnalysisStatusCounts::default();
    for s in statuses.values() {
        match s {
            ai::AnalysisStatus::Queued => analysis_statuses.queued += 1,
            ai::AnalysisStatus::Processing => analysis_statuses.processing += 1,
            ai::AnalysisStatus::Done => analysis_statuses.done += 1,
            ai::AnalysisStatus::Failed => analysis_statuses.failed += 1,
        }
    }
    drop(statuses);

    // Engine snapshot: configured → pids/status from state; health latency
    // comes from the last check_engine_health probe.
    let mut engine = EngineDiagSnapshot::default();
    if let Ok(engine_opt) = state.engine() {
        engine.chat.status = engine_status_str(engine_opt.chat_status().await);
        engine.embed.status = engine_status_str(engine_opt.embed_status().await);
        engine.chat.pid = engine_opt.chat_pid().await;
        engine.embed.pid = engine_opt.embed_pid().await;
    } else {
        engine.chat.status = "not_configured".to_string();
        engine.embed.status = "not_configured".to_string();
    }
    {
        let health = state.health.lock().unwrap_or_else(|e| e.into_inner());
        engine.chat.latency_ms = health.chat_latency_ms;
        engine.embed.latency_ms = health.embed_latency_ms;
        engine.last_health_check = health.last_check.clone();
    }

    let mut models: Vec<ModelDiagInfo> = Vec::new();
    for m in state.models().scan_chat() {
        models.push(ModelDiagInfo {
            kind: "chat".to_string(),
            name: m.name,
            filename: m.filename,
            size_bytes: m.size_bytes,
        });
    }
    for m in state.models().scan_embed() {
        models.push(ModelDiagInfo {
            kind: "embed".to_string(),
            name: m.name,
            filename: m.filename,
            size_bytes: m.size_bytes,
        });
    }

    Ok(AppDiagnostics {
        generated_at: Utc::now().to_rfc3339(),
        data_dir: state.paths.data_dir.to_string_lossy().to_string(),
        db_path: state.paths.db_path.to_string_lossy().to_string(),
        log_path: state.paths.log_path.to_string_lossy().to_string(),
        db_size_bytes: std::fs::metadata(&state.paths.db_path)
            .map(|m| m.len())
            .unwrap_or(0),
        journal_entries,
        tasks,
        memory_facts,
        conversations,
        analysis_queue_len,
        analysis_statuses,
        engine,
        models,
    })
}

#[tauri::command]
async fn report_bug(
    state: tauri::State<'_, AppState>,
    user_feedback: String,
) -> Result<String, AppError> {
    let exports_dir = state.paths.export_dir.clone();
    std::fs::create_dir_all(&exports_dir)
        .map_err(|e| AppError::Io(format!("Failed to create exports dir: {}", e)))?;

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("bug_report_{}.md", timestamp);
    let path = exports_dir.join(&filename);

    let system_info = format!(
        "OS: Windows\n\
         App Version: {}\n\
         Data Dir: {}\n\n",
        env!("CARGO_PKG_VERSION"),
        state.paths.data_dir.display()
    );

    let engine_log = state.telemetry.snapshot();
    let engine_section = format!(
        "## Engine HTTP Log\n{}\n\n",
        serde_json::to_string_pretty(&engine_log).unwrap_or_else(|_| "[]".to_string())
    );
    let tail = diagnostics::tail_log_file(&state.paths.log_path, 200);
    let log_section = format!("## App Log Tail\n```\n{}\n```\n\n", tail.lines.join("\n"));

    let report = format!(
        "# Bug Report\n\n\
         Generated: {}\n\n\
         ## System Info\n{}\
         {}\
         {}\
         ## User Feedback\n{}\n",
        Utc::now().to_rfc3339(),
        system_info,
        engine_section,
        log_section,
        user_feedback,
    );

    std::fs::write(&path, &report)
        .map_err(|e| AppError::Io(format!("Failed to write bug report: {}", e)))?;

    tracing::info!(target: "bug_report", path = %path.display(), "bug report written");
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
async fn propose_item(
    state: tauri::State<'_, AppState>,
    item_type: String,
    proposed_title: String,
    proposed_data: String,
    source_entry_id: Option<String>,
    source_text: Option<String>,
    confidence: f64,
) -> Result<String, AppError> {
    let pool = state.db()?;
    let proposal_id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();

    let id_clone = proposal_id.clone();
    db::pool::with_conn(pool, move |conn| {
        db::ai::insert_proposed_item(
            conn,
            &id_clone,
            None, // conversation_id
            source_entry_id.as_deref(),
            &item_type,
            &proposed_title,
            &proposed_data,
            source_text.as_deref(),
            Some(confidence),
            Some("user"),
            &created_at,
        )
    })
    .await?;

    Ok(proposal_id)
}

#[tauri::command]
async fn proposed_list(
    state: tauri::State<'_, AppState>,
    status: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<crate::db::ai::ProposedInboxItem>, AppError> {
    let limit = limit.unwrap_or(50);
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        let items = crate::db::ai::list_proposed(conn, status.as_deref(), limit)?;
        Ok(items)
    })
    .await
}

#[tauri::command]
async fn memory_list(
    state: tauri::State<'_, AppState>,
    kind: Option<String>,
    category: Option<String>,
    search: Option<String>,
) -> Result<Vec<crate::db::knowledge::KnowledgeItem>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        crate::db::knowledge::list_items(
            conn,
            kind.as_deref(),
            category.as_deref(),
            search.as_deref(),
        )
    })
    .await
}

#[tauri::command]
async fn memory_get(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<crate::db::knowledge::KnowledgeItemDetail, AppError> {
    let db = state.db()?;
    let item_id = id.clone();
    let detail = db::pool::with_conn(db, move |conn| {
        crate::db::knowledge::get_item_detail(conn, &item_id)
    })
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Memory item {} not found", id)))?;
    Ok(detail)
}

#[tauri::command]
async fn memory_create(
    state: tauri::State<'_, AppState>,
    input: crate::db::knowledge::KnowledgeItem,
) -> Result<crate::db::knowledge::KnowledgeItem, AppError> {
    if !crate::db::knowledge::validate_kind(&input.kind) {
        return Err(AppError::InvalidInput(format!(
            "Invalid kind: {}",
            input.kind
        )));
    }
    if !crate::db::knowledge::validate_category(&input.category) {
        return Err(AppError::InvalidInput(format!(
            "Invalid category: {}",
            input.category
        )));
    }
    let now = Utc::now().to_rfc3339();
    let mut item = input;
    item.id = if item.id.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        item.id.clone()
    };
    item.provenance = if item.provenance.is_empty() {
        "manual".into()
    } else {
        item.provenance
    };
    item.created_by = if item.created_by.is_empty() {
        "user".into()
    } else {
        item.created_by
    };
    item.confidence = item.confidence.clamp(0.0, 1.0);
    item.created_at = now.clone();
    item.updated_at = now;

    let db = state.db()?;
    let item_clone = item.clone();
    db::pool::with_conn(db, move |conn| {
        crate::db::knowledge::create_item(conn, &item_clone)
    })
    .await?;
    Ok(item)
}

#[tauri::command]
async fn memory_update(
    state: tauri::State<'_, AppState>,
    input: crate::db::knowledge::KnowledgeItem,
) -> Result<crate::db::knowledge::KnowledgeItem, AppError> {
    if !crate::db::knowledge::validate_kind(&input.kind) {
        return Err(AppError::InvalidInput(format!(
            "Invalid kind: {}",
            input.kind
        )));
    }
    if !crate::db::knowledge::validate_category(&input.category) {
        return Err(AppError::InvalidInput(format!(
            "Invalid category: {}",
            input.category
        )));
    }
    let db = state.db()?;
    let item_id = input.id.clone();
    let existing = db::pool::with_conn(db.clone(), move |conn| {
        crate::db::knowledge::get_item(conn, &item_id)
    })
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Memory item {} not found", input.id)))?;

    let mut updated = existing.clone();
    updated.title = input.title;
    updated.content = input.content;
    updated.kind = input.kind;
    updated.category = input.category;
    updated.tags = input.tags;
    updated.item_date = input.item_date;
    updated.confidence = input.confidence.clamp(0.0, 1.0);
    updated.updated_at = Utc::now().to_rfc3339();

    let updated_clone = updated.clone();
    db::pool::with_conn(db, move |conn| {
        crate::db::knowledge::update_item(conn, &updated_clone)
    })
    .await?;
    Ok(updated)
}

#[tauri::command]
async fn memory_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| crate::db::knowledge::delete_item(conn, &id)).await?;
    Ok(())
}

#[tauri::command]
async fn memory_link(
    state: tauri::State<'_, AppState>,
    source_id: String,
    target_id: String,
    relation: String,
    created_by: String,
) -> Result<crate::db::knowledge::KnowledgeLink, AppError> {
    if !crate::db::knowledge::validate_relation(&relation) {
        return Err(AppError::InvalidInput(format!(
            "Invalid relation: {}",
            relation
        )));
    }
    if source_id == target_id {
        return Err(AppError::InvalidInput(
            "Cannot link an item to itself".into(),
        ));
    }
    let now = Utc::now().to_rfc3339();
    let link = crate::db::knowledge::KnowledgeLink {
        id: Uuid::new_v4().to_string(),
        source_id,
        target_id,
        relation,
        created_by: if created_by.is_empty() {
            "user".into()
        } else {
            created_by
        },
        created_at: now,
    };
    let db = state.db()?;
    let link_clone = link.clone();
    db::pool::with_conn(db, move |conn| {
        crate::db::knowledge::link_items(conn, &link_clone)
    })
    .await?;
    Ok(link)
}

#[tauri::command]
async fn memory_unlink(state: tauri::State<'_, AppState>, link_id: String) -> Result<(), AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        crate::db::knowledge::unlink_item(conn, &link_id)
    })
    .await?;
    Ok(())
}

#[tauri::command]
async fn proposed_action(
    _app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    action: String,
) -> Result<(), AppError> {
    use crate::db::ai;
    use crate::db::tasks;

    let db = state.db()?;
    db::pool::with_tx(db, move |tx| {
        // Fetch proposal
        let proposal = ai::get_proposal(tx, &id)?
            .ok_or_else(|| AppError::NotFound(format!("Proposal {} not found", id)))?;

        match action.as_str() {
            "accept" => {
                // Idempotency: a repeated accept must not create a duplicate
                // record, and a dismissed proposal can never be applied.
                if proposal.status == "accepted" {
                    return Ok(());
                }
                if proposal.status == "dismissed" {
                    return Err(AppError::InvalidInput(
                        "This proposal was dismissed and cannot be accepted.".into(),
                    ));
                }
                match proposal.item_type.as_str() {
                    "task" => {
                        // Parse proposed_data as JSON for task fields
                        let data: serde_json::Value = serde_json::from_str(&proposal.proposed_data)
                            .unwrap_or(serde_json::json!({}));
                        let data = crate::ai::percent_decode_value(data);

                        // `create_task` proposals carry no disambiguating "type";
                        // `update_task`/`complete_task` set data.type to
                        // "task_update"/"task_complete" and must mutate the
                        // referenced task instead of creating a new one.
                        let proposal_kind =
                            data.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if proposal_kind == "task_update" || proposal_kind == "task_complete" {
                            let task_id = tasks::apply_task_proposal(tx, proposal_kind, &data)?;
                            ai::update_proposal_status(
                                tx,
                                &id,
                                "accepted",
                                Some(&task_id),
                                Some("user"),
                                Some(&Utc::now().to_rfc3339()),
                            )?;
                            return Ok(());
                        }

                        let title = data.get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&proposal.proposed_title)
                            .to_string();
                        let description = data.get("description")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let priority = data.get("priority")
                            .and_then(|v| v.as_str())
                            .unwrap_or("medium")
                            .to_string();
                        let due_date = data.get("due_date")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string());

                        let now = Utc::now().to_rfc3339();
                        let task = tasks::Task {
                            id: Uuid::new_v4().to_string(),
                            parent_task_id: None,
                            title,
                            description,
                            status: "todo".to_string(),
                            priority,
                            due_date,
                            due_time: None,
                            reminder_at: None,
                            reminder_fired: false,
                            time_estimate: None,
                            time_logged: 0,
                            tags: "[]".to_string(),
                            labels: "[]".to_string(),
                            category: None,
                            project: None,
                            energy_level: None,
                            context_tag: None,
                            linked_url: None,
                            ai_created: true,
                            recurrence_rule: None,
                            is_blocked: false,
                            created_at: now.clone(),
                            updated_at: now,
                            completed_at: None,
                            archived: false,
                        };
                        tasks::create_task(tx, &task)?;

                        ai::update_proposal_status(tx, &id, "accepted", Some(&task.id), Some("user"), Some(&Utc::now().to_rfc3339()))?;
                    }
                    "fact" => {
                        let data: serde_json::Value = serde_json::from_str(&proposal.proposed_data)
                            .unwrap_or(serde_json::json!({}));
                        let fact_key = data.get("fact_key")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&proposal.proposed_title)
                            .to_string();
                        let content = data.get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&proposal.proposed_title)
                            .to_string();
                        let category = data.get("category")
                            .and_then(|v| v.as_str())
                            .unwrap_or("other")
                            .to_string();
                        if !crate::db::knowledge::validate_category(&category) {
                            return Err(crate::error::AppError::InvalidInput(format!(
                                "Invalid fact category: {}", category
                            )));
                        }
                        let confidence = data.get("confidence")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.8)
                            .clamp(0.0, 1.0);

                        let now = Utc::now().to_rfc3339();
                        let item = crate::db::knowledge::KnowledgeItem {
                            id: Uuid::new_v4().to_string(),
                            title: fact_key,
                            content,
                            kind: "information".to_string(),
                            category,
                            tags: "[]".to_string(),
                            item_date: None,
                            confidence,
                            provenance: "ai".to_string(),
                            created_by: proposal.created_by.clone().unwrap_or_else(|| "ai".to_string()),
                            source_entry_id: proposal.source_entry_id.clone(),
                            created_at: now.clone(),
                            updated_at: now,
                        };
                        crate::db::knowledge::create_item(tx, &item)?;

                        ai::update_proposal_status(tx, &id, "accepted", None, Some("user"), Some(&Utc::now().to_rfc3339()))?;
                    }
                    "memory" => {
                        let data: serde_json::Value = serde_json::from_str(&proposal.proposed_data)
                            .unwrap_or(serde_json::json!({}));
                        let sub_action = data.get("memory_action")
                            .and_then(|v| v.as_str())
                            .unwrap_or("create");
                        let now = Utc::now().to_rfc3339();

                        match sub_action {
                            "update" => {
                                let item_id = data.get("item_id").and_then(|v| v.as_str()).unwrap_or("");
                                if item_id.is_empty() {
                                    return Err(crate::error::AppError::InvalidInput(
                                        "memory update proposal missing item_id".into(),
                                    ));
                                }
                                let item = crate::db::knowledge::get_item(tx, item_id)?
                                    .ok_or_else(|| crate::error::AppError::NotFound(format!("Memory item {} not found", item_id)))?;
                                let mut updated = item.clone();
                                if let Some(t) = data.get("title").and_then(|v| v.as_str()) {
                                    updated.title = t.to_string();
                                }
                                if let Some(c) = data.get("content").and_then(|v| v.as_str()) {
                                    updated.content = c.to_string();
                                }
                                if let Some(k) = data.get("kind").and_then(|v| v.as_str()) {
                                    if !crate::db::knowledge::validate_kind(k) {
                                        return Err(crate::error::AppError::InvalidInput(format!("Invalid kind: {}", k)));
                                    }
                                    updated.kind = k.to_string();
                                }
                                if let Some(c) = data.get("category").and_then(|v| v.as_str()) {
                                    if !crate::db::knowledge::validate_category(c) {
                                        return Err(crate::error::AppError::InvalidInput(format!("Invalid category: {}", c)));
                                    }
                                    updated.category = c.to_string();
                                }
                                if let Some(t) = data.get("tags").and_then(|v| v.as_array()) {
                                    updated.tags = serde_json::Value::Array(t.clone()).to_string();
                                }
                                if data.get("item_date").is_some() {
                                    updated.item_date = data.get("item_date").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
                                }
                                if let Some(c) = data.get("confidence").and_then(|v| v.as_f64()) {
                                    updated.confidence = c.clamp(0.0, 1.0);
                                }
                                updated.updated_at = now.clone();
                                crate::db::knowledge::update_item(tx, &updated)?;
                            }
                            "delete" => {
                                let item_id = data.get("item_id").and_then(|v| v.as_str()).unwrap_or("");
                                if item_id.is_empty() {
                                    return Err(crate::error::AppError::InvalidInput(
                                        "memory delete proposal missing item_id".into(),
                                    ));
                                }
                                crate::db::knowledge::delete_item(tx, item_id)?;
                            }
                            "link" => {
                                let source_id = data.get("source_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let target_id = data.get("target_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let relation = data.get("relation").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                if source_id.is_empty() || target_id.is_empty() || !crate::db::knowledge::validate_relation(&relation) {
                                    return Err(crate::error::AppError::InvalidInput(
                                        "memory link proposal missing or invalid fields".into(),
                                    ));
                                }
                                if source_id == target_id {
                                    return Err(crate::error::AppError::InvalidInput(
                                        "Cannot link an item to itself".into(),
                                    ));
                                }
                                let link = crate::db::knowledge::KnowledgeLink {
                                    id: Uuid::new_v4().to_string(),
                                    source_id,
                                    target_id,
                                    relation,
                                    created_by: "ai".to_string(),
                                    created_at: now,
                                };
                                crate::db::knowledge::link_items(tx, &link)?;
                            }
                            _ => {
                                let title = data.get("title")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&proposal.proposed_title)
                                    .to_string();
                                let content = data.get("content")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&title)
                                    .to_string();
                                let kind = data.get("kind")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("information")
                                    .to_string();
                                if !crate::db::knowledge::validate_kind(&kind) {
                                    return Err(crate::error::AppError::InvalidInput(format!("Invalid kind: {}", kind)));
                                }
                                let category = data.get("category")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("other")
                                    .to_string();
                                if !crate::db::knowledge::validate_category(&category) {
                                    return Err(crate::error::AppError::InvalidInput(format!("Invalid category: {}", category)));
                                }
                                let confidence = data.get("confidence")
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(1.0)
                                    .clamp(0.0, 1.0);
                                let tags = data.get("tags")
                                    .and_then(|v| v.as_array())
                                    .map(|a| serde_json::Value::Array(a.clone()).to_string())
                                    .unwrap_or_else(|| "[]".to_string());
                                let item_date = data.get("item_date")
                                    .and_then(|v| v.as_str())
                                    .filter(|s| !s.is_empty())
                                    .map(|s| s.to_string());

                                let item = crate::db::knowledge::KnowledgeItem {
                                    id: Uuid::new_v4().to_string(),
                                    title,
                                    content,
                                    kind,
                                    category,
                                    tags,
                                    item_date,
                                    confidence,
                                    provenance: "ai".to_string(),
                                    created_by: proposal.created_by.clone().unwrap_or_else(|| "ai".to_string()),
                                    source_entry_id: proposal.source_entry_id.clone(),
                                    created_at: now.clone(),
                                    updated_at: now,
                                };
                                crate::db::knowledge::create_item(tx, &item)?;
                            }
                        }

                        ai::update_proposal_status(tx, &id, "accepted", None, Some("user"), Some(&Utc::now().to_rfc3339()))?;
                    }
                    "event" => {
                        let data: serde_json::Value = serde_json::from_str(&proposal.proposed_data).unwrap_or(serde_json::json!({}));
                        let data = crate::ai::percent_decode_value(data);
                        // `event_create`/`event_update`/`event_delete` are the
                        // chat-tool markers; legacy analysis proposals carry no
                        // disambiguating type and are treated as a create.
                        let proposal_kind = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        let action = match proposal_kind {
                            "event_update" => "update",
                            "event_delete" => "delete",
                            _ => "create",
                        };
                        let record_id = db::events::apply_event_proposal(
                            tx,
                            action,
                            &data,
                            proposal.source_entry_id.as_deref(),
                        )?;
                        ai::update_proposal_status(tx, &id, "accepted", Some(&record_id), Some("user"), Some(&Utc::now().to_rfc3339()))?;
                    }
                    "note" => {
                        let data: serde_json::Value = serde_json::from_str(&proposal.proposed_data).unwrap_or(serde_json::json!({}));
                        let data = crate::ai::percent_decode_value(data);
                        let proposal_kind = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        let action = match proposal_kind {
                            "note_update" => "update",
                            "note_delete" => "delete",
                            _ => "create",
                        };
                        let record_id = db::events::apply_note_proposal(
                            tx,
                            action,
                            &data,
                            proposal.source_entry_id.as_deref(),
                        )?;
                        ai::update_proposal_status(tx, &id, "accepted", Some(&record_id), Some("user"), Some(&Utc::now().to_rfc3339()))?;
                    }
                    other => {
                        // Generic accept — just mark accepted
                        ai::update_proposal_status(tx, &id, "accepted", None, Some("user"), Some(&Utc::now().to_rfc3339()))?;
                        tracing::warn!(target: "proposed_action", item_type = %other, "accepted unknown proposal type without action");
                    }
                }
            }
            "dismiss" => {
                // Only a pending proposal can be dismissed; dismissing an
                // already-accepted proposal must not delete the applied record.
                if proposal.status == "pending" {
                    ai::update_proposal_status(tx, &id, "dismissed", None, None, None)?;
                }
            }
            _ => {
                return Err(AppError::InvalidInput(format!("Unknown action: {}. Use 'accept' or 'dismiss'", action)));
            }
        }

        Ok(())
    }).await?;

    Ok(())
}

#[tauri::command]
async fn event_get(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<db::events::Event>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::events::get_event(conn, &id)).await
}

#[tauri::command]
async fn event_list(
    state: tauri::State<'_, AppState>,
    start_after: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<db::events::Event>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        db::events::list_events(conn, start_after.as_deref(), limit)
    })
    .await
}

#[tauri::command]
async fn event_save(
    state: tauri::State<'_, AppState>,
    event: db::events::Event,
) -> Result<String, AppError> {
    let mut event = event;
    if event.id.is_empty() {
        event.id = Uuid::new_v4().to_string();
    }
    let now = Utc::now().to_rfc3339();
    if event.created_at.is_empty() {
        event.created_at = now.clone();
    }
    event.updated_at = now;

    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        // Try update first
        if db::events::get_event(conn, &event.id)?.is_some() {
            db::events::update_event(conn, &event)?;
        } else {
            db::events::create_event(conn, &event)?;
        }
        Ok(event.id)
    })
    .await
}

#[tauri::command]
async fn event_delete(state: tauri::State<'_, AppState>, id: String) -> Result<bool, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::events::soft_delete_event(conn, &id)).await
}

#[tauri::command]
async fn note_list(
    state: tauri::State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<db::events::Note>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::events::list_notes(conn, limit)).await
}

#[tauri::command]
async fn note_save(
    state: tauri::State<'_, AppState>,
    note: db::events::Note,
) -> Result<String, AppError> {
    let mut note = note;
    if note.id.is_empty() {
        note.id = Uuid::new_v4().to_string();
    }
    let now = Utc::now().to_rfc3339();
    if note.created_at.is_empty() {
        note.created_at = now.clone();
    }
    note.updated_at = now;

    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        if db::events::get_note(conn, &note.id)?.is_some() {
            db::events::update_note(conn, &note)?;
        } else {
            db::events::create_note(conn, &note)?;
        }
        Ok(note.id)
    })
    .await
}

#[tauri::command]
async fn note_delete(state: tauri::State<'_, AppState>, id: String) -> Result<bool, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::events::soft_delete_note(conn, &id)).await
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineHealth {
    pub chat: bool,
    pub embed: bool,
    pub chat_latency_ms: Option<u64>,
    pub embed_latency_ms: Option<u64>,
    pub checked_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineStatusSnapshot {
    pub chat: String,
    pub embed: String,
}

fn engine_status_str(s: crate::ai::engine::EngineStatus) -> String {
    match s {
        crate::ai::engine::EngineStatus::Stopped => "stopped".to_string(),
        crate::ai::engine::EngineStatus::Starting => "starting".to_string(),
        crate::ai::engine::EngineStatus::Running => "running".to_string(),
        crate::ai::engine::EngineStatus::Failed(e) => format!("failed: {}", e),
    }
}

#[tauri::command]
async fn list_chat_models(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::ai::models::ModelInfo>, AppError> {
    Ok(state.models().scan_chat())
}

#[tauri::command]
async fn list_embed_models(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::ai::models::ModelInfo>, AppError> {
    Ok(state.models().scan_embed())
}

#[tauri::command]
async fn import_model(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    source: String,
    model_type: String,
) -> Result<crate::ai::models::ModelInfo, AppError> {
    if model_type != "chat" && model_type != "embed" {
        return Err(AppError::AiError(
            "model_type must be 'chat' or 'embed'".to_string(),
        ));
    }
    let info = state
        .models()
        .import_model(std::path::Path::new(&source), &model_type)
        .map_err(|e| {
            if e.contains("Copy failed") && e.contains("user-mapped section") {
                AppError::AiError(
                    "The model file is currently loaded by a running engine, so it can't be copied. If it's already in the chat/embed list, just select it there instead of importing again. Otherwise stop the engine (Restart/Stop) before importing."
                        .into(),
                )
            } else {
                AppError::AiError(e)
            }
        })?;
    crate::events::emit(
        &app,
        crate::events::AppEvent::LlamaModelImported {
            model_type: model_type.clone(),
            filename: info.filename.clone(),
        },
    );
    Ok(info)
}

#[tauri::command]
async fn check_engine_health(state: tauri::State<'_, AppState>) -> Result<EngineHealth, AppError> {
    let client = state.llama()?;
    let (chat, embed) = client.check_health_detailed().await;
    let checked_at = Utc::now().to_rfc3339();
    {
        let mut cache = state.health.lock().unwrap_or_else(|e| e.into_inner());
        cache.chat_latency_ms = Some(chat.latency_ms);
        cache.embed_latency_ms = Some(embed.latency_ms);
        cache.last_check = Some(checked_at.clone());
    }
    Ok(EngineHealth {
        chat: chat.ok,
        embed: embed.ok,
        chat_latency_ms: Some(chat.latency_ms),
        embed_latency_ms: Some(embed.latency_ms),
        checked_at: Some(checked_at),
    })
}

/// App-local llama.cpp runtime override (D-128).
fn bundled_engine_exe(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("engines").join("llama-server.exe")
}

/// Resolve the packaged llama.cpp runtime from Tauri's app resource directory.
/// The full runtime directory is bundled together so Windows can load the DLLs
/// adjacent to `llama-server.exe`.
fn bundled_engine_resource(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    let packaged = app
        .path()
        .resolve(
            "engines/llama-server.exe",
            tauri::path::BaseDirectory::Resource,
        )
        .ok();
    if packaged.as_ref().is_some_and(|path| path.is_file()) {
        return packaged;
    }

    // `cargo tauri dev` can use the staged runtime in the repository root
    // before the app bundle's resource directory exists.
    #[cfg(debug_assertions)]
    {
        let dev_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../llama-b10952-bin-win-cuda-x64/llama-server.exe");
        if dev_path.is_file() {
            return Some(dev_path);
        }
    }

    packaged
}

/// Where the resolved llama-server executable came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum EnginePathSource {
    /// `<data>/engines/llama-server.exe` staged at runtime (D-128).
    AppLocal,
    /// `engines/llama-server.exe` shipped next to the installed app.
    BundledResource,
    /// The user's `llama_exe_path` setting.
    Saved,
    /// Nothing usable was found.
    None,
}

impl EnginePathSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AppLocal => "app_local",
            Self::BundledResource => "bundled_resource",
            Self::Saved => "saved",
            Self::None => "none",
        }
    }
}

/// Resolve the llama-server executable to use, plus where it came from.
///
/// Precedence: app-local `<data>/engines`, packaged Tauri resource, saved path.
pub(crate) fn resolve_engine_exe_with_source(
    data_dir: &std::path::Path,
    resource_exe: Option<&std::path::Path>,
    saved: &str,
) -> (String, EnginePathSource) {
    let bundled = bundled_engine_exe(data_dir);
    if bundled.is_file() {
        return (
            bundled.to_string_lossy().into_owned(),
            EnginePathSource::AppLocal,
        );
    }
    if let Some(resource) = resource_exe.filter(|path| path.is_file()) {
        return (
            resource.to_string_lossy().into_owned(),
            EnginePathSource::BundledResource,
        );
    }
    if !saved.is_empty() && std::path::Path::new(saved).is_file() {
        return (saved.to_string(), EnginePathSource::Saved);
    }
    (String::new(), EnginePathSource::None)
}

/// Resolve the llama-server executable to use.
pub(crate) fn resolve_engine_exe(
    data_dir: &std::path::Path,
    resource_exe: Option<&std::path::Path>,
    saved: &str,
) -> String {
    resolve_engine_exe_with_source(data_dir, resource_exe, saved).0
}

/// Wire shape for `engine_path_info` (D-148: snake_case keys).
#[derive(Debug, Serialize)]
struct EnginePathInfo {
    /// Executable the app will launch; empty when no runtime was found.
    resolved: String,
    /// One of `app_local`, `bundled_resource`, `saved`, `none`.
    source: String,
    /// User override from Settings (empty when unset or the vault is locked).
    saved: String,
}

/// Report the llama-server executable the app will actually launch, so the UI
/// can show the bundled runtime path instead of a placeholder. Does not require
/// an unlocked vault — the saved override is simply empty while locked.
#[tauri::command]
async fn engine_path_info(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<EnginePathInfo, AppError> {
    let saved = match state.db() {
        Ok(pool) => db::pool::with_conn(pool, |conn| {
            db::settings::get_setting(conn, "llama_exe_path")
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default(),
        Err(_) => String::new(),
    };
    let resource = bundled_engine_resource(&app);
    let (resolved, source) =
        resolve_engine_exe_with_source(&state.paths.data_dir, resource.as_deref(), &saved);
    Ok(EnginePathInfo {
        resolved,
        source: source.as_str().to_string(),
        saved,
    })
}

/// Live GPU/offload state for both services (D-148: snake_case keys).
#[derive(Debug, Serialize)]
struct EngineRuntimeSnapshot {
    chat: ai::probe::EngineRuntimeInfo,
    embed: ai::probe::EngineRuntimeInfo,
}

/// Report what the running engine is actually doing: GPU, partial, or CPU.
#[tauri::command]
async fn engine_runtime_info(
    state: tauri::State<'_, AppState>,
) -> Result<EngineRuntimeSnapshot, AppError> {
    match state.engine() {
        Ok(engine) => Ok(EngineRuntimeSnapshot {
            chat: engine.chat_info().await,
            embed: engine.embed_info().await,
        }),
        Err(_) => Ok(EngineRuntimeSnapshot {
            chat: crate::ai::probe::EngineRuntimeInfo::default(),
            embed: crate::ai::probe::EngineRuntimeInfo::default(),
        }),
    }
}

/// Everything the requirements table shows: engine label, accelerators,
/// MSVC runtime presence, and the two user-supplied model files.
#[derive(Debug, Serialize)]
struct RequirementsReport {
    engine: String,
    devices: Vec<ai::probe::DeviceInfo>,
    msvc_runtime: bool,
    chat_model: String,
    embed_model: String,
}

/// Requirements disclosure for onboarding and System. Works with a locked
/// vault: model names simply come back empty.
#[tauri::command]
async fn engine_requirements(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RequirementsReport, AppError> {
    let (saved_exe, chat_model, embed_model) = match state.db() {
        Ok(pool) => db::pool::with_conn(pool, |conn| {
            Ok::<_, AppError>((
                db::settings::get_setting(conn, "llama_exe_path")?.unwrap_or_default(),
                db::settings::get_setting(conn, "llama_chat_model")?.unwrap_or_default(),
                db::settings::get_setting(conn, "llama_embed_model")?.unwrap_or_default(),
            ))
        })
        .await
        .unwrap_or_default(),
        Err(_) => (String::new(), String::new(), String::new()),
    };

    let probe = ai::probe::default_probe();
    let engine_label = probe.engine_label().to_string();
    let resource = bundled_engine_resource(&app);
    let resolved = resolve_engine_exe(&state.paths.data_dir, resource.as_deref(), &saved_exe);

    let devices = if resolved.is_empty() {
        Vec::new()
    } else {
        let exe = std::path::PathBuf::from(resolved);
        match tokio::task::spawn_blocking(move || probe.list_devices(&exe)).await {
            Ok(Ok(devices)) => devices,
            Ok(Err(e)) => {
                tracing::warn!(target: "engine", error = %e, "device listing failed");
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    };

    Ok(RequirementsReport {
        engine: engine_label,
        devices,
        msvc_runtime: runtime_env::msvc_runtime_present(),
        chat_model,
        embed_model,
    })
}

///
/// Priority: the saved `llama_chat_model`/`llama_embed_model` setting when the
/// file still exists in the store; otherwise auto-detect from the bundled
/// models directory — for chat take the largest GGUF, for embed the smallest
/// (deterministic even when a stray second file sits in the chat folder).
pub(crate) fn effective_model_path(
    store: &ai::models::ModelStore,
    saved: &str,
    is_embed: bool,
) -> std::path::PathBuf {
    // Any instruct-tuned chat GGUF is accepted (Qwythos recommended). Ignore a
    // saved value that looks like an embedding model so a stray file in
    // models/chat/ cannot take over the reasoning role.
    let saved = if !is_embed && !ai::models::is_supported_chat_model(saved) {
        ""
    } else {
        saved
    };
    if !saved.is_empty() {
        let p = if is_embed {
            store.resolve_embed_path(saved)
        } else {
            store.resolve_chat_path(saved)
        };
        if p.is_file() {
            return p;
        }
    }
    let mut scanned = if is_embed {
        store.scan_embed()
    } else {
        store.scan_chat()
    };
    if scanned.is_empty() {
        return if is_embed {
            store.get_embed_path(saved)
        } else {
            store.get_chat_path(saved)
        };
    }
    scanned.sort_by_key(|m| m.size_bytes);
    let chosen = if is_embed {
        scanned.first()
    } else {
        scanned.last()
    }
    .expect("scanned is non-empty");
    if is_embed {
        store.get_embed_path(&chosen.filename)
    } else {
        store.get_chat_path(&chosen.filename)
    }
}

/// Rebuild the app-managed engine from the CURRENT settings (not the
/// startup snapshot), so a freshly-saved llama_exe_path applies without
/// an app restart. Returns Ok(false) when the exe path is unset.
async fn rebuild_engine(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, AppState>,
) -> Result<bool, AppError> {
    let db = state.db()?;
    let (chat_model, embed_model, chat_port, embed_port, gpu_layers, ctx_size, exe_path, api_key) =
        db::pool::with_conn(db, |conn| {
            let chat_model = db::settings::get_setting(conn, "llama_chat_model")
                .ok()
                .flatten()
                .unwrap_or_default();
            let embed_model = db::settings::get_setting(conn, "llama_embed_model")
                .ok()
                .flatten()
                .unwrap_or_default();
            let chat_port: u16 = db::settings::get_setting(conn, "llama_chat_port")
                .ok()
                .flatten()
                .unwrap_or_else(|| "8080".into())
                .parse()
                .unwrap_or(8080);
            let embed_port: u16 = db::settings::get_setting(conn, "llama_embed_port")
                .ok()
                .flatten()
                .unwrap_or_else(|| "8081".into())
                .parse()
                .unwrap_or(8081);
            let gpu_layers: String = db::settings::get_setting(conn, "llama_ngl")
                .ok()
                .flatten()
                .unwrap_or_else(|| "auto".into());
            let ctx_size: u32 = db::settings::get_setting(conn, "llama_ctx_size")
                .ok()
                .flatten()
                .unwrap_or_else(|| "16384".into())
                .parse()
                .unwrap_or(16384);
            let exe_path = db::settings::get_setting(conn, "llama_exe_path")
                .ok()
                .flatten()
                .unwrap_or_default();
            let api_key = db::settings::get_setting(conn, "llama_api_key")
                .ok()
                .flatten()
                .unwrap_or_default();
            Ok::<_, AppError>((
                chat_model,
                embed_model,
                chat_port,
                embed_port,
                gpu_layers,
                ctx_size,
                exe_path,
                api_key,
            ))
        })
        .await?;

    let engine_resource = bundled_engine_resource(app);
    let executable =
        resolve_engine_exe(&state.paths.data_dir, engine_resource.as_deref(), &exe_path);
    if executable.is_empty() {
        return Ok(false);
    }
    let engine = Arc::new(ai::engine::LlamaEngine::new(ai::engine::EngineConfig {
        exe_path: executable.into(),
        chat_model: effective_model_path(&state.models(), &chat_model, false),
        embed_model: effective_model_path(&state.models(), &embed_model, true),
        chat_port,
        embed_port,
        gpu_layers,
        ctx_size,
        api_key,
        log_dir: state.paths.data_dir.join("logs"),
    }));
    state.set_engine(engine);
    Ok(true)
}

/// What a Stop / Restart actually did, so the UI can report the truth instead
/// of assuming the port was free.
#[derive(Debug, Serialize)]
struct EngineActionOutcome {
    service: String,
    /// Stray llama-server reaped to free the port (leftover from a previous run).
    reaped: Option<ai::ports::PortOwner>,
    /// Human summary for the toast / status line.
    note: String,
}

fn port_role(service: &str) -> Result<ai::ports::PortRole, AppError> {
    match service {
        "chat" => Ok(ai::ports::PortRole::Chat),
        "embed" => Ok(ai::ports::PortRole::Embed),
        _ => Err(AppError::AiError("Unknown service".into())),
    }
}

fn reaped_owner(clearance: &ai::ports::PortClearance) -> Option<ai::ports::PortOwner> {
    match clearance {
        ai::ports::PortClearance::Reaped(owner) => Some(owner.clone()),
        _ => None,
    }
}

fn action_note(verb: &str, service: &str, clearance: &ai::ports::PortClearance) -> String {
    match clearance {
        ai::ports::PortClearance::Reaped(owner) => format!(
            "{} engine {} - freed port {} by stopping a stray llama-server (pid {})",
            service, verb, owner.port, owner.pid
        ),
        _ => format!("{} engine {}", service, verb),
    }
}

/// Free a service port after its own child was stopped. `own_pid` is the pid the
/// current process owned (so a failed kill cannot be mistaken for a squatter),
/// `other_pid` the sibling engine (a shared port means a config mistake).
async fn free_engine_port(
    role: ai::ports::PortRole,
    port: u16,
    own_pid: Option<u32>,
    other_pid: Option<u32>,
) -> Result<ai::ports::PortClearance, AppError> {
    let clearance = ai::ports::clear_port(role, port, own_pid, other_pid).await;
    match &clearance {
        ai::ports::PortClearance::Blocked { reason, .. } => {
            Err(AppError::AiError(reason.clone()))
        }
        ai::ports::PortClearance::OwnEngine(owner) => Err(AppError::AiError(format!(
            "Port {} is still held by Arnold's own {} engine (pid {}) - it did not exit. Press Stop, then Start.",
            owner.port,
            role.label(),
            owner.pid
        ))),
        _ => Ok(clearance),
    }
}

#[tauri::command]
async fn restart_engine(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    service: String,
) -> Result<EngineActionOutcome, AppError> {
    let role = port_role(&service)?;
    let configured = rebuild_engine(&app, &state).await?;
    if !configured {
        return Err(AppError::AiError(
            "LLM engine not available (bundled runtime missing; set llama_exe_path in Settings)"
                .into(),
        ));
    }
    let engine = state.engine()?;
    let (port, own_pid, other_pid) = match role {
        ai::ports::PortRole::Chat => (
            engine.chat_port(),
            engine.chat_pid().await,
            engine.embed_pid().await,
        ),
        ai::ports::PortRole::Embed => (
            engine.embed_port(),
            engine.embed_pid().await,
            engine.chat_pid().await,
        ),
    };

    // Stop the engine this process owns, then free the port. A leftover
    // llama-server from a force-killed previous run is reaped here; anything
    // else is refused with a reason the UI can act on.
    match role {
        ai::ports::PortRole::Chat => engine.stop_chat().await,
        ai::ports::PortRole::Embed => engine.stop_embed().await,
    }
    let clearance = free_engine_port(role, port, own_pid, other_pid).await?;

    match role {
        ai::ports::PortRole::Chat => engine.start_chat().await.map_err(AppError::AiError)?,
        ai::ports::PortRole::Embed => {
            if let Err(e) = engine.start_embed().await {
                let reason =
                    crate::provision::embed_start_failure_reason(state.llama().ok().as_ref(), e)
                        .await;
                return Err(AppError::AiError(reason));
            }
            if let Ok(client) = state.llama() {
                client.embed_circuit_reset();
            }
        }
    }
    crate::events::emit(
        &app,
        crate::events::AppEvent::LlamaEngineStarted {
            service: service.clone(),
        },
    );
    crate::events::emit(
        &app,
        crate::events::AppEvent::EngineRuntimeStatus {
            chat: engine.chat_info().await,
            embed: engine.embed_info().await,
        },
    );
    Ok(EngineActionOutcome {
        reaped: reaped_owner(&clearance),
        note: action_note("restarted", &service, &clearance),
        service,
    })
}

#[tauri::command]
async fn stop_engine(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    service: String,
) -> Result<EngineActionOutcome, AppError> {
    let role = port_role(&service)?;
    let engine = state.engine()?;
    let (port, own_pid, other_pid) = match role {
        ai::ports::PortRole::Chat => (
            engine.chat_port(),
            engine.chat_pid().await,
            engine.embed_pid().await,
        ),
        ai::ports::PortRole::Embed => (
            engine.embed_port(),
            engine.embed_pid().await,
            engine.chat_pid().await,
        ),
    };
    match role {
        ai::ports::PortRole::Chat => engine.stop_chat().await,
        ai::ports::PortRole::Embed => engine.stop_embed().await,
    }
    let clearance = free_engine_port(role, port, own_pid, other_pid).await?;
    crate::events::emit(
        &app,
        crate::events::AppEvent::LlamaEngineStopped {
            service: service.clone(),
        },
    );
    crate::events::emit(
        &app,
        crate::events::AppEvent::EngineRuntimeStatus {
            chat: engine.chat_info().await,
            embed: engine.embed_info().await,
        },
    );
    Ok(EngineActionOutcome {
        reaped: reaped_owner(&clearance),
        note: action_note("stopped", &service, &clearance),
        service,
    })
}

/// Wire shape for `engine_port_info` (D-148: snake_case keys).
#[derive(Debug, Serialize)]
struct EnginePortInfo {
    chat_port: u16,
    embed_port: u16,
    /// Pid the app itself owns for each service (None when stopped).
    chat_pid: Option<u32>,
    embed_pid: Option<u32>,
    /// Whoever is LISTENING on the port, including the app's own engine.
    chat_owner: Option<ai::ports::PortOwner>,
    embed_owner: Option<ai::ports::PortOwner>,
}

/// Report who holds the two engine ports so System can show it and offer an
/// explicit "Clear port" when the holder is not a llama-server.
#[tauri::command]
async fn engine_port_info(state: tauri::State<'_, AppState>) -> Result<EnginePortInfo, AppError> {
    let (chat_port, embed_port, chat_pid, embed_pid) = match state.engine() {
        Ok(engine) => (
            engine.chat_port(),
            engine.embed_port(),
            engine.chat_pid().await,
            engine.embed_pid().await,
        ),
        // Engine not built yet (locked vault / missing runtime): report the
        // seeded defaults so a squatter is still visible pre-configuration.
        Err(_) => (8080, 8081, None, None),
    };
    let (chat_owner, embed_owner) = tokio::join!(
        ai::ports::owner_of(chat_port),
        ai::ports::owner_of(embed_port)
    );
    Ok(EnginePortInfo {
        chat_port,
        embed_port,
        chat_pid,
        embed_pid,
        chat_owner,
        embed_owner,
    })
}

/// Explicit, user-confirmed kill of whatever holds a service port (any process,
/// not just llama-server). Refuses Arnold's own live engine — use Stop for that.
#[tauri::command]
async fn force_clear_port(
    state: tauri::State<'_, AppState>,
    service: String,
) -> Result<Option<ai::ports::PortOwner>, AppError> {
    let role = port_role(&service)?;
    let engine = state.engine().map_err(|_| {
        AppError::AiError("LLM engine not configured - nothing to clear for.".to_string())
    })?;
    let (port, own_pid) = match role {
        ai::ports::PortRole::Chat => (engine.chat_port(), engine.chat_pid().await),
        ai::ports::PortRole::Embed => (engine.embed_port(), engine.embed_pid().await),
    };
    ai::ports::force_clear(role, port, own_pid)
        .await
        .map_err(AppError::AiError)
}

#[tauri::command]
async fn get_engine_status(
    state: tauri::State<'_, AppState>,
) -> Result<EngineStatusSnapshot, AppError> {
    match state.engine() {
        Ok(engine) => Ok(EngineStatusSnapshot {
            chat: engine_status_str(engine.chat_status().await),
            embed: engine_status_str(engine.embed_status().await),
        }),
        Err(_) => Ok(EngineStatusSnapshot {
            chat: "not_configured".to_string(),
            embed: "not_configured".to_string(),
        }),
    }
}

#[tauri::command]
async fn report_activity(
    state: tauri::State<'_, AppState>,
    start_date: String,
    end_date: String,
) -> Result<Vec<db::reports::ActivityReportRow>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        db::reports::get_activity_report(conn, &start_date, &end_date)
    })
    .await
}

#[tauri::command]
async fn report_mood(
    state: tauri::State<'_, AppState>,
    start_date: String,
    end_date: String,
) -> Result<Vec<db::reports::MoodTrendReportRow>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        db::reports::get_mood_trend_report(conn, &start_date, &end_date)
    })
    .await
}

#[tauri::command]
async fn report_weekly(
    state: tauri::State<'_, AppState>,
) -> Result<db::reports::WeeklyReviewReport, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::reports::get_weekly_review(conn)).await
}

#[tauri::command]
async fn report_task_completion(
    state: tauri::State<'_, AppState>,
    period: String,
    start_date: String,
    end_date: String,
) -> Result<Vec<db::reports::TaskCompletionReportRow>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        db::reports::get_task_completion_report(conn, &period, &start_date, &end_date)
    })
    .await
}

#[tauri::command]
async fn task_create(
    state: tauri::State<'_, AppState>,
    title: String,
    parent_task_id: Option<String>,
    due_date: Option<String>,
    priority: Option<String>,
) -> Result<String, AppError> {
    let now = Utc::now().to_rfc3339();
    if let Some(parent) = parent_task_id.clone() {
        // D-40: subtask depth is two levels maximum — enforced here, not just UI.
        let db = state.db()?;
        let depth_ok =
            db::pool::with_conn(db, move |conn| match db::tasks::get_task(conn, &parent) {
                Ok(Some(t)) => Ok::<bool, AppError>(t.parent_task_id.is_none()),
                Ok(None) => Ok(true),
                Err(e) => Err(e),
            })
            .await?;
        if !depth_ok {
            return Err(AppError::InvalidInput(
                "subtask depth is limited to two levels".to_string(),
            ));
        }
    }
    let task = db::tasks::Task {
        id: Uuid::new_v4().to_string(),
        parent_task_id,
        title,
        description: None,
        status: "todo".into(),
        priority: priority.unwrap_or_else(|| "medium".into()),
        due_date,
        due_time: None,
        reminder_at: None,
        reminder_fired: false,
        time_estimate: None,
        time_logged: 0,
        tags: "[]".into(),
        labels: "[]".into(),
        category: None,
        project: None,
        energy_level: None,
        context_tag: None,
        linked_url: None,
        ai_created: false,
        recurrence_rule: None,
        is_blocked: false,
        created_at: now.clone(),
        updated_at: now,
        completed_at: None,
        archived: false,
    };
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::tasks::create_task(conn, &task)).await
}

#[tauri::command]
async fn task_list(
    state: tauri::State<'_, AppState>,
    include_completed: Option<bool>,
) -> Result<Vec<db::tasks::Task>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        db::tasks::list_tasks(conn, include_completed.unwrap_or(false))
    })
    .await
}

#[tauri::command]
async fn task_update_status(
    state: tauri::State<'_, AppState>,
    id: String,
    status: String,
) -> Result<(), AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        db::tasks::update_task_status(conn, &id, &status)
    })
    .await
}

#[tauri::command]
async fn task_get(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<db::tasks::Task>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::tasks::get_task(conn, &id)).await
}

#[tauri::command]
async fn task_update(
    state: tauri::State<'_, AppState>,
    task: db::tasks::Task,
) -> Result<(), AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::tasks::update_task(conn, &task)).await
}

#[tauri::command]
async fn task_search(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<Vec<db::tasks::Task>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::tasks::search_tasks(conn, &query)).await
}

#[tauri::command]
async fn task_delete(state: tauri::State<'_, AppState>, id: String) -> Result<bool, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::tasks::soft_delete(conn, &id)).await
}

#[tauri::command]
async fn task_add_dependency(
    state: tauri::State<'_, AppState>,
    blocked_id: String,
    blocking_id: String,
) -> Result<(), AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        db::tasks::add_dependency(conn, &blocked_id, &blocking_id)
    })
    .await
}

#[tauri::command]
async fn task_remove_dependency(
    state: tauri::State<'_, AppState>,
    blocked_id: String,
    blocking_id: String,
) -> Result<(), AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        db::tasks::remove_dependency(conn, &blocked_id, &blocking_id)
    })
    .await
}

#[tauri::command]
async fn task_get_dependencies(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<Vec<db::tasks::Task>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::tasks::get_dependencies(conn, &task_id)).await
}

#[tauri::command]
async fn conversation_list(
    state: tauri::State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<db::ai::AiConversation>, AppError> {
    let limit = limit.unwrap_or(50);
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::ai::list_conversations(conn, limit)).await
}

#[tauri::command]
async fn setting_get(
    state: tauri::State<'_, AppState>,
    key: String,
) -> Result<Option<String>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| db::settings::get_setting(conn, &key)).await
}

#[tauri::command]
async fn setting_set(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), AppError> {
    // The chat role takes any instruct-tuned GGUF except an embedding model;
    // reject the obvious mismatch at the settings boundary (no downloads).
    if key == "llama_chat_model"
        && !value.is_empty()
        && !crate::ai::models::is_supported_chat_model(&value)
    {
        return Err(AppError::InvalidInput(format!(
            "That looks like an embedding model, so it cannot serve the chat role. Pick an instruct-tuned chat GGUF (recommended: {}.gguf).",
            crate::ai::models::RECOMMENDED_CHAT_MODEL
        )));
    }
    let db_key = key.clone();
    let db_value = value.clone();
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        db::settings::set_setting(conn, &db_key, &db_value)
    })
    .await?;

    // Settings audit trail (settings observability) — key only, never value.
    crate::events::emit(
        &app,
        crate::events::AppEvent::LogEvent {
            timestamp: Utc::now().to_rfc3339(),
            level: "info".to_string(),
            source: "settings".to_string(),
            message: format!("setting '{}' updated", key),
        },
    );

    Ok(())
}

#[tauri::command]
async fn background_read_image(path: String) -> Result<String, AppError> {
    let bytes = std::fs::read(&path).map_err(AppError::from)?;
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "image/*",
    };
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{};base64,{}", mime, b64))
}

#[tauri::command]
async fn restart_onboarding(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, |conn| {
        db::settings::set_setting(conn, "onboarding_complete", "")?;
        // Also clear engine config to ensure a clean slate
        db::settings::set_setting(conn, "llama_exe_path", "")?;
        db::settings::set_setting(conn, "llama_chat_model", "")?;
        db::settings::set_setting(conn, "llama_embed_model", "")
    })
    .await?;

    crate::events::emit(&app, crate::events::AppEvent::OnboardingRequired);
    Ok(())
}

#[tauri::command]
async fn check_onboarding(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let db = match state.db() {
        Ok(d) => d,
        Err(_) => return Ok(()), // Locked, can't check yet
    };
    let done = db::pool::with_conn(db, |conn| {
        db::settings::get_setting(conn, "onboarding_complete")
    })
    .await
    .ok()
    .flatten()
    .map(|v| !v.trim().is_empty())
    .unwrap_or(false);

    if !done {
        crate::events::emit(&app, crate::events::AppEvent::OnboardingRequired);
    }
    Ok(())
}

#[tauri::command]
async fn mood_checkin(
    state: tauri::State<'_, AppState>,
    mood: String,
    date: String,
    note: Option<String>,
) -> Result<(), AppError> {
    if !crate::db::mood::validate_mood(&mood) {
        return Err(AppError::InvalidInput(format!("Unknown mood: {mood}")));
    }
    if chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
        return Err(AppError::InvalidInput(format!("Invalid date: {date}")));
    }
    let db = state.db()?;
    let id = Uuid::new_v4().to_string();
    let note_owned = note.unwrap_or_default();
    db::pool::with_tx(db, move |tx| {
        crate::db::mood::checkin(tx, &id, &mood, &note_owned, &date, &Utc::now().to_rfc3339())
    })
    .await
}

#[tauri::command]
async fn mood_list(
    state: tauri::State<'_, AppState>,
    start: String,
    end: String,
) -> Result<Vec<crate::db::mood::MoodDay>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        let manual = crate::db::mood::manual_days(conn, &start, &end)?;
        let inferred = crate::db::mood::inferred_days(conn, &start, &end)?;
        Ok(crate::db::mood::merge_days(manual, inferred))
    })
    .await
}

#[tauri::command]
async fn mood_delete(state: tauri::State<'_, AppState>, date: String) -> Result<(), AppError> {
    let db = state.db()?;
    db::pool::with_tx(db, move |tx| crate::db::mood::delete(tx, &date)).await
}

// ── System tab commands (spec 2.4) ───────────────────────────────

/// Backup Now — same Backup API path as the auto-backup worker (D-98).
/// Returns the produced file path (lossy display form for the UI).
#[tauri::command]
async fn backup_now(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    let pool = state.db()?;
    let backup_dir = state.paths.backup_dir.clone();
    // T-02: same `x'<hex>'` key derivation as spawn_auto_backup_worker
    // (backup/auto.rs:64-69) so the backup file is encrypted like the source.
    let password = state.key().ok().map(|k| {
        format!(
            "x'{}'",
            k.iter().map(|b| format!("{:02x}", b)).collect::<String>()
        )
    });
    let path = crate::backup::auto::run_auto_backup(pool, backup_dir, password).await?;
    Ok(path.to_string_lossy().into_owned())
}

/// System tab — Analysis queue card.
#[tauri::command]
async fn analysis_queue_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::db::analysis_queue::QueueRow>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| crate::db::analysis_queue::list_all(conn)).await
}

/// System tab / strip Retry — re-enqueue a failed analysis. `reset_failed`
/// gates on status='failed' (NotFound otherwise); `enqueue_analysis` then does
/// the in-memory push + `JournalAnalysisQueued` emission exactly as a fresh save.
#[tauri::command]
async fn analysis_requeue(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    entry_id: String,
) -> Result<(), AppError> {
    let db = state.db()?;
    let id = entry_id.clone();
    let reset = db::pool::with_conn(db, move |conn| {
        crate::db::analysis_queue::reset_failed(conn, &id)
    })
    .await?;
    if !reset {
        return Err(AppError::NotFound(format!(
            "no failed analysis for entry {entry_id}"
        )));
    }
    enqueue_analysis(&state, &app, &entry_id).await
}

/// Wire shape for the System tab SQL runner. `changes` is structurally always
/// 0: the guard only permits read-only statements (D-147), and `conn.changes()`
/// would report the pooled connection's *previous* write — a lie.
#[derive(Serialize)]
struct SqlRunResult {
    columns: Vec<String>,
    rows: Vec<Vec<serde_json::Value>>,
    changes: u64,
}

/// D-147 gate: the first keyword (after any leading `--`/`/* */` comments)
/// must be one of select | with | pragma | explain.
fn ensure_read_only(query: &str) -> Result<(), AppError> {
    let mut s = query.trim_start();
    loop {
        if let Some(rest) = s.strip_prefix("--") {
            s = match rest.find('\n') {
                Some(i) => rest[i + 1..].trim_start(),
                None => "",
            };
        } else if let Some(rest) = s.strip_prefix("/*") {
            match rest.find("*/") {
                Some(i) => s = rest[i + 2..].trim_start(),
                None => return Err(AppError::InvalidInput("unterminated block comment".into())),
            }
        } else {
            break;
        }
    }
    let first = s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match first.as_str() {
        "select" | "with" | "pragma" | "explain" => Ok(()),
        "" => Err(AppError::InvalidInput("empty query".into())),
        other => Err(AppError::InvalidInput(format!(
            "only read-only statements are allowed (SELECT/WITH/PRAGMA/EXPLAIN), got `{other}`"
        ))),
    }
}

fn sql_value_to_json(row: &rusqlite::Row<'_>, i: usize) -> Result<serde_json::Value, AppError> {
    let v: rusqlite::types::Value = row.get(i)?;
    Ok(match v {
        rusqlite::types::Value::Null => serde_json::Value::Null,
        rusqlite::types::Value::Integer(n) => serde_json::json!(n),
        rusqlite::types::Value::Real(f) => serde_json::json!(f), // NaN/Inf → Null (serde_json), no panic
        rusqlite::types::Value::Text(t) => serde_json::Value::String(t),
        rusqlite::types::Value::Blob(b) => {
            serde_json::Value::String(format!("<blob {} bytes>", b.len()))
        }
    })
}

/// System tab SQL runner — read-only statements only (D-147), 500-row cap.
#[tauri::command]
async fn sql_run(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<SqlRunResult, AppError> {
    ensure_read_only(&query)?;
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        // Backstop: even a keyword-allowlisted statement must report read-only
        // (blocks `PRAGMA journal_mode = DELETE`, `EXPLAIN INSERT`, etc.).
        let mut stmt = conn.prepare(&query)?;
        if !stmt.readonly() {
            return Err(AppError::InvalidInput(
                "statement rejected by the read-only backstop".into(),
            ));
        }
        let columns: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(|c| c.to_string())
            .collect();
        let col_count = columns.len();
        let mut rows_stmt = stmt.query([])?;
        let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
        const MAX_ROWS: usize = 500;
        while let Some(row) = rows_stmt.next()? {
            if rows.len() >= MAX_ROWS {
                break;
            }
            let mut vals = Vec::with_capacity(col_count);
            for i in 0..col_count {
                vals.push(sql_value_to_json(row, i)?);
            }
            rows.push(vals);
        }
        Ok(SqlRunResult {
            columns,
            rows,
            changes: 0,
        })
    })
    .await
}

async fn enqueue_analysis(
    state: &AppState,
    app: &tauri::AppHandle,
    entry_id: &str,
) -> Result<(), AppError> {
    let db = state.db()?;
    let entry_id = entry_id.to_string();
    {
        let pool = db.clone();
        let id_db = entry_id.clone();
        let _ =
            db::pool::with_conn(pool, move |conn| db::analysis_queue::enqueue(conn, &id_db)).await;
    }
    let mut queue = state.ai_state.queue.lock().await;
    let mut queued_ids = state.ai_state.queued_ids.lock().await;
    if queue.len() < crate::ai::analysis::MAX_QUEUE {
        queue.retain(|existing_id| existing_id != &entry_id);
        queue.push_back(entry_id.clone());
        queued_ids.insert(entry_id.clone());
    }
    state
        .ai_state
        .status
        .lock()
        .await
        .insert(entry_id.clone(), crate::ai::AnalysisStatus::Queued);
    crate::events::emit(
        app,
        crate::events::AppEvent::JournalAnalysisQueued {
            entry_id: entry_id.clone(),
        },
    );
    crate::events::emit(
        app,
        crate::events::AppEvent::LogEvent {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: "info".to_string(),
            source: "analysis".to_string(),
            message: format!("journal entry {entry_id} queued for analysis"),
        },
    );
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // P-01: resolve every path once, up front
    let app_paths = paths::AppPaths::resolve();
    app_paths
        .ensure_dirs()
        .expect("Failed to create app data directories");

    crate::logger::init(&app_paths.log_path);
    tracing::info!(
        target: "startup",
        data_dir = ?app_paths.data_dir,
        "starting Arnold"
    );

    let models = Arc::new(ai::models::ModelStore::new(
        app_paths.data_dir.join("models"),
    ));
    models
        .ensure_dirs()
        .expect("Failed to create model directories");

    let ai_state = Arc::new(ai::analysis::AnalysisState::new());

    let app_state = AppState {
        db_pool: std::sync::Mutex::new(None),
        mem_db: std::sync::Mutex::new(None),
        graph: std::sync::Mutex::new(None),
        encryption_key: std::sync::Mutex::new(None),
        paths: Arc::new(app_paths),
        ai_state,
        models,
        llama_client: std::sync::Mutex::new(None),
        llama_engine: std::sync::Mutex::new(None),
        telemetry: Arc::new(diagnostics::TelemetryRing::new(500)),
        health: std::sync::Mutex::new(diagnostics::HealthCache::default()),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .manage(app_state)
        .setup(|app| {
            // T-04: Windows "Mica" UI — Apply native vibrancy effect
            #[cfg(target_os = "windows")]
            {
                use window_vibrancy::apply_mica;
                if let Some(window) = app.get_webview_window("main") {
                    let _ = apply_mica(&window, None);
                }
            }

            let handle = app.handle().clone();

            // P-03: Model Provisioning Task (starts chat+embed llama.cpp engines)
            {
                let setup_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let outcome = crate::provision::provision_model(&setup_handle).await;
                    tracing::info!(
                        target: "ai_provision",
                        outcome = %outcome.summary(),
                        "startup model provisioning finished"
                    );
                });
            }

            let worker_handle = handle.clone();

            // Restore pending analysis items (waits for DB)
            {
                let restore_handle = worker_handle.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        let state = restore_handle.state::<AppState>();
                        let db = match state.db() {
                            Ok(d) => d,
                            Err(_) => continue,
                        };

                        let ids: Result<Vec<String>, _> = db::pool::with_conn(db, |conn| {
                            db::analysis_queue::list_queued(conn)
                        }).await;

                        if let Ok(ids) = ids {
                            let mut queue = state.ai_state.queue.lock().await;
                            let mut queued_ids = state.ai_state.queued_ids.lock().await;
                            let mut statuses = state.ai_state.status.lock().await;
                            for id in ids.into_iter() {
                                if queue.len() >= crate::ai::analysis::MAX_QUEUE {
                                    break;
                                }
                                if !queued_ids.contains(&id) {
                                    queued_ids.insert(id.clone());
                                    queue.push_back(id.clone());
                                    statuses.insert(id.clone(), crate::ai::AnalysisStatus::Queued);
                                }
                            }
                        }
                        break; // Successfully initialized
                    }
                });
            }

            // Worker loop (waits for DB)
            {
                let worker_handle = worker_handle.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                        let state = worker_handle.state::<AppState>();
                        let db = match state.db() {
                            Ok(d) => d,
                            Err(_) => continue,
                        };

                        let entry_id: Option<String> = {
                            let mut queue = state.ai_state.queue.lock().await;
                            let mut queued_ids = state.ai_state.queued_ids.lock().await;
                            let mut processing_ids = state.ai_state.processing_ids.lock().await;

                            if let Some(id) = queue.pop_front() {
                                if processing_ids.contains(&id) {
                                    // Already being worked on by another thread (race), skip
                                    queue.push_back(id.clone());
                                    None
                                } else {
                                    queued_ids.remove(&id);
                                    processing_ids.insert(id.clone());
                                    Some(id)
                                }
                            } else {
                                None
                            }
                        };

                        if let Some(id) = entry_id {
                            // Persist that we're processing this entry before doing heavy work
                            {
                                let pool = db.clone();
                                let id_db = id.clone();
                                if let Err(e) = db::pool::with_conn(pool, move |conn| {
                                    db::analysis_queue::mark_processing(conn, &id_db)
                                }).await {
                                    tracing::warn!(target: "analysis", entry_id = %id, error = %e, "failed to mark processing in DB; re-queueing");
                                    // push back into in-memory queue and continue
                                    let mut queue = state.ai_state.queue.lock().await;
                                    let mut queued_ids = state.ai_state.queued_ids.lock().await;
                                    queue.push_front(id.clone());
                                    queued_ids.insert(id.clone());
                                    continue;
                                } else {
                                    state.ai_state.status.lock().await.insert(id.clone(), crate::ai::AnalysisStatus::Processing);
                                    crate::events::emit(
                                        &worker_handle,
                                        crate::events::AppEvent::JournalAnalysisProcessing {
                                            entry_id: id.clone(),
                                        },
                                    );
                                }
                            }

                            // Load entry
                            let entry_opt = {
                                let pool = db.clone();
                                let id = id.clone();
                                db::pool::with_conn(pool, move |conn| {
                                    db::journal::get_entry_by_id(conn, &id)
                                }).await
                            };

                            if let Ok(Some(entry)) = entry_opt {
                                // Decrypt content for AI tasks (T-01 Security Layer)
                                let key = match state.key() {
                                    Ok(k) => k,
                                    Err(_) => continue,
                                };
                                let decrypted_content = match ai::vault::decrypt(&entry.content, &key) {
                                    Ok(c) => c,
                                    Err(e) => {
                                        tracing::error!(target: "analysis", entry_id = %id, error = %e, "decryption failed");
                                        continue;
                                    }
                                };

                                let client = match state.llama() {
                                    Ok(c) => c,
                                    Err(_) => continue,
                                };

                                // Embeddings (1-arg signature)
                                if let Ok(vector) = client.generate_embeddings(&decrypted_content).await {
                                    let pool = db.clone();
                                    let id_vec = id.clone();
                                    let _ = db::pool::with_conn(pool, move |conn| {
                                        db::vectors::upsert_vector(conn, &id_vec, &vector, "nomic-embed-text:latest")
                                    }).await;
                                    tracing::info!(target: "analysis", entry_id = %id, "embeddings generated and stored");
                                }

                                // Journal analysis: build (role,content) tuples, parse RawAnalysis
                                let system_prompt = crate::ai::prompt::get_analysis_system_prompt().to_string();
                                let messages = vec![
                                    ("system".to_string(), system_prompt),
                                    ("user".to_string(),   decrypted_content.clone()),
                                ];
                                match client.analyze_journal(messages, None).await {
                                    Ok(txt) => {
                                        let json_str = crate::ai::clean_json_string(&txt);
                                        let raw: crate::ai::RawAnalysis = match serde_json::from_str(json_str) {
                                            Ok(r) => r,
                                            Err(e) => {
                                                tracing::warn!(target: "analysis", entry_id = %id, error = %e, "failed to parse analysis JSON");
                                                state.ai_state.status.lock().await.insert(id.clone(), crate::ai::AnalysisStatus::Done);
                                                state.ai_state.processing_ids.lock().await.remove(&id);
                                                continue;
                                            }
                                        };
                                        let result = crate::ai::AnalysisResult::from_raw(raw, id.clone());

                                        // Update graph (best-effort)
                                        if let Ok(graph_mu) = state.graph_db() {
                                            let graph = graph_mu.lock().await;
                                            for (s, p, o) in &result.triplets {
                                                let _ = graph.upsert_node(s, Vec::<(String, String)>::new(), "Entity");
                                                let _ = graph.upsert_node(o, Vec::<(String, String)>::new(), "Entity");
                                                let _ = graph.upsert_edge(s, o, Vec::<(String, String)>::new(), p);
                                            }
                                        }

                                        // Persist proposals — same structure
                                        let conv_id = Uuid::new_v4().to_string();
                                        let created_at = chrono::Utc::now().to_rfc3339();
                                        let proposed_count = (result.tasks.len()
                                            + result.facts.len()
                                            + result.events.len()
                                            + result.notes.len()) as u32;
                                        let tx_ok;
                                        {
                                            let pool = db.clone();
                                            let id = id.clone();
                                            let conv_id = conv_id.clone();
                                            let result = result.clone();
                                            let content = decrypted_content.clone();
                                            let model: String = db::pool::with_conn(db.clone(), |conn|
                                                db::settings::get_setting(conn, "llama_chat_model")
                                            ).await.ok().flatten().unwrap_or_else(|| "unknown".into());
                                            tx_ok = db::pool::with_tx(pool, move |tx| {
                                                db::ai::upsert_conversation(tx, &conv_id, Some("auto-analysis"), &model, "analysis", Some(&id), Some("ai"))?;

                                                for task in result.tasks.iter().filter(|t| !t.title.trim().is_empty()) {
                                                    let proposal_id = Uuid::new_v4().to_string();
                                                    let title = crate::ai::percent_decode_str(&task.title);
                                                    let due_date = task.due_date.as_deref().map(crate::ai::percent_decode_str);
                                                    let priority = task.priority.as_deref().map(crate::ai::percent_decode_str);
                                                    let data = serde_json::to_string(&serde_json::json!({"type": "task", "title": title, "due_date": due_date, "priority": priority})).unwrap_or_else(|_| "{}".into());
                                                    let snippet: String = content.chars().take(512).collect();
                                                    db::ai::insert_proposed_item(tx, &proposal_id, Some(&conv_id), Some(&id), "task", &title, &data, Some(&snippet), None, Some("ai"), &created_at)?;
                                                }
                                                for fact in result.facts.iter().filter(|f| {
                                                    let k = f.fact_key.trim().to_lowercase();
                                                    !k.is_empty() && k != "true" && k != "false" && k != "null" && k != "none" && !f.content.trim().is_empty()
                                                }) {
                                                    let proposal_id = Uuid::new_v4().to_string();
                                                    let data = serde_json::to_string(&serde_json::json!({"type": "fact", "fact_key": fact.fact_key, "content": fact.content, "category": fact.category})).unwrap_or_else(|_| "{}".into());
                                                    let snippet: String = content.chars().take(512).collect();
                                                    db::ai::insert_proposed_item(tx, &proposal_id, Some(&conv_id), Some(&id), "fact", &fact.content, &data, Some(&snippet), None, Some("ai"), &created_at)?;
                                                }
                                                for ev in result.events.iter().filter(|e| {
                                                    !e.title.trim().is_empty() && !e.start_at.trim().is_empty() && chrono::DateTime::parse_from_rfc3339(&e.start_at).is_ok()
                                                }) {
                                                    let proposal_id = Uuid::new_v4().to_string();
                                                    let data = serde_json::to_string(&serde_json::json!({"type": "event", "title": ev.title, "start_at": ev.start_at, "location": ev.location})).unwrap_or_else(|_| "{}".into());
                                                    let snippet: String = content.chars().take(512).collect();
                                                    db::ai::insert_proposed_item(tx, &proposal_id, Some(&conv_id), Some(&id), "event", &ev.title, &data, Some(&snippet), None, Some("ai"), &created_at)?;
                                                }
                                                for note in result.notes.iter().filter(|n| !n.content.trim().is_empty()) {
                                                    let proposal_id = Uuid::new_v4().to_string();
                                                    let data = serde_json::to_string(&serde_json::json!({"type": "note", "title": note.title, "content": note.content, "tags": note.tags})).unwrap_or_else(|_| "{}".into());
                                                    let snippet: String = content.chars().take(512).collect();
                                                    db::ai::insert_proposed_item(tx, &proposal_id, Some(&conv_id), Some(&id), "note", note.title.as_deref().unwrap_or("New Note"), &data, Some(&snippet), None, Some("ai"), &created_at)?;
                                                }
                                                Ok::<(), AppError>(())
                                            })
                                            .await
                                            .is_ok();
                                        }

                                        if tx_ok && proposed_count > 0 {
                                            crate::events::emit(
                                                &worker_handle,
                                                crate::events::AppEvent::AiProposalsReady {
                                                    // Conversation id is unused by the frontend handler
                                                    // (ai-sidebar ai_proposals_ready) — from the analysis
                                                    // worker it carries the journal entry id.
                                                    conversation_id: id.clone(),
                                                    count: proposed_count,
                                                },
                                            );
                                        }

                                        // Mark done
                                        state.ai_state.status.lock().await.insert(id.clone(), crate::ai::AnalysisStatus::Done);
                                        let pool = db.clone();
                                        let id_db = id.clone();
                                        if let Err(e) = db::pool::with_conn(pool, move |conn| {
                                            db::analysis_queue::mark_done(conn, &id_db)
                                        }).await {
                                            tracing::warn!(target: "analysis", entry_id = %id, error = %e, "failed to mark done in DB");
                                        }
                                        crate::events::emit(&worker_handle, crate::events::AppEvent::JournalAnalysisCompleted { entry_id: id.clone(), result });
                                        crate::events::emit(&worker_handle, crate::events::AppEvent::LogEvent {
                                            timestamp: chrono::Utc::now().to_rfc3339(),
                                            level: "info".to_string(),
                                            source: "analysis".to_string(),
                                            message: format!("journal entry {} analysis completed", id),
                                        });
                                    }
                                    Err(e) => {
                                        let error_msg = e.to_string();
                                        tracing::warn!(target: "analysis", entry_id = %id, error = %e, "journal analysis failed");
                                        let pool = db.clone();
                                        let id_db = id.clone();
                                        let _ = db::pool::with_conn(pool, move |conn| {
                                            db::analysis_queue::mark_failed(conn, &id_db)
                                        }).await;
                                        state.ai_state.status.lock().await.insert(id.clone(), crate::ai::AnalysisStatus::Failed);
                                        crate::events::emit(&worker_handle, crate::events::AppEvent::JournalAnalysisError { entry_id: id.clone(), error: error_msg });
                                        crate::events::emit(&worker_handle, crate::events::AppEvent::LogEvent {
                                            timestamp: chrono::Utc::now().to_rfc3339(),
                                            level: "error".to_string(),
                                            source: "analysis".to_string(),
                                            message: format!("journal entry {} analysis failed", id),
                                        });
                                    }
                                }
                                state.ai_state.processing_ids.lock().await.remove(&id);
                            } else {
                                // If entry wasn't found or decryption failed, remove from processing anyway
                                state.ai_state.processing_ids.lock().await.remove(&id);
                            }
                        }
                    }
                });
            }

            // Listener (waits for DB)
            {
                let listener_handle = handle.clone();
                app.listen_any("journal_analysis_queued", move |event| {
                    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                        if let Some(id) = payload["id"].as_str() {
                            let task_handle = listener_handle.clone();
                            let id_clone = id.to_string();
                            tauri::async_runtime::spawn(async move {
                                let state = task_handle.state::<AppState>();
                                let _ = enqueue_analysis(&state, &task_handle, &id_clone).await;
                            });
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            vault_status,
            unlock_vault,
            change_vault_password,
            save_journal_entry,
            journal_get,
            journal_list,
            journal_delete,
            ai_chat,
            app_diagnostics,
            report_bug,
            event_list,
            event_save,
            event_delete,
            note_list,
            note_save,
            note_delete,
            event_get,
            report_activity,
            report_mood,
            report_weekly,
            report_task_completion,
            task_create,
            task_list,
            task_update_status,
            task_get,
            task_update,
            task_search,
            task_delete,
            task_add_dependency,
            task_remove_dependency,
            task_get_dependencies,
            conversation_list,
            list_chat_models,
            list_embed_models,
            import_model,
            check_engine_health,
            engine_path_info,
            engine_runtime_info,
            engine_requirements,
            restart_engine,
            stop_engine,
            engine_port_info,
            force_clear_port,
            get_engine_status,
            setting_get,
            setting_set,
            background_read_image,
            diagnostics::engine_http_log,
            diagnostics::log_tail,
            diagnostics::telemetry_clear,
            preflight::preflight_run,
            provision::provision_model_cmd,
            app_version,
            restart_onboarding,
            check_onboarding,
            proposed_list,
            propose_item,
            proposed_action,
            memory_list,
            memory_get,
            memory_create,
            memory_update,
            memory_delete,
            memory_link,
            memory_unlink,
            mood_checkin,
            mood_list,
            mood_delete,
            backup_now,
            analysis_queue_list,
            analysis_requeue,
            sql_run,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod engine_path_tests {
    use super::resolve_engine_exe;
    use super::{resolve_engine_exe_with_source, EnginePathSource};
    use std::path::Path;

    #[test]
    fn engine_resolution_prefers_app_local_then_packaged_then_saved() {
        let root =
            std::env::temp_dir().join(format!("arnold-engine-path-test-{}", uuid::Uuid::new_v4()));
        let data_dir = root.join("data");
        let local_engine = data_dir.join("engines/llama-server.exe");
        let resource_engine = root.join("resources/engines/llama-server.exe");
        let saved_engine = root.join("custom/llama-server.exe");

        for path in [&local_engine, &resource_engine, &saved_engine] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"test runtime").unwrap();
        }

        assert_eq!(
            std::path::PathBuf::from(resolve_engine_exe(
                &data_dir,
                Some(&resource_engine),
                saved_engine.to_str().unwrap(),
            )),
            local_engine
        );
        std::fs::remove_file(&local_engine).unwrap();
        assert_eq!(
            std::path::PathBuf::from(resolve_engine_exe(
                &data_dir,
                Some(&resource_engine),
                saved_engine.to_str().unwrap(),
            )),
            resource_engine
        );
        std::fs::remove_file(&resource_engine).unwrap();
        assert_eq!(
            std::path::PathBuf::from(resolve_engine_exe(
                &data_dir,
                Some(Path::new("missing.exe")),
                saved_engine.to_str().unwrap(),
            )),
            saved_engine
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn engine_resolution_reports_where_the_path_came_from() {
        let root =
            std::env::temp_dir().join(format!("arnold-engine-source-{}", uuid::Uuid::new_v4()));
        let data_dir = root.join("data");
        let local_engine = data_dir.join("engines/llama-server.exe");
        let resource_engine = root.join("resources/engines/llama-server.exe");
        let saved_engine = root.join("custom/llama-server.exe");
        for path in [&local_engine, &resource_engine, &saved_engine] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"test runtime").unwrap();
        }
        let saved = saved_engine.to_str().unwrap();

        let (path, source) =
            resolve_engine_exe_with_source(&data_dir, Some(&resource_engine), saved);
        assert_eq!(std::path::PathBuf::from(&path), local_engine);
        assert_eq!(source, EnginePathSource::AppLocal);
        assert_eq!(source.as_str(), "app_local");

        std::fs::remove_file(&local_engine).unwrap();
        let (path, source) =
            resolve_engine_exe_with_source(&data_dir, Some(&resource_engine), saved);
        assert_eq!(std::path::PathBuf::from(&path), resource_engine);
        assert_eq!(source, EnginePathSource::BundledResource);
        assert_eq!(source.as_str(), "bundled_resource");

        let (path, source) =
            resolve_engine_exe_with_source(&data_dir, Some(Path::new("missing.exe")), saved);
        assert_eq!(std::path::PathBuf::from(&path), saved_engine);
        assert_eq!(source, EnginePathSource::Saved);
        assert_eq!(source.as_str(), "saved");

        std::fs::remove_file(&saved_engine).unwrap();
        let (path, source) = resolve_engine_exe_with_source(&data_dir, None, saved);
        assert!(path.is_empty());
        assert_eq!(source, EnginePathSource::None);
        assert_eq!(source.as_str(), "none");

        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;

    #[test]
    fn app_diagnostics_serializes_snake_case_wire_keys() {
        let d = AppDiagnostics {
            generated_at: "2026-10-08T00:00:00Z".to_string(),
            data_dir: "/data".to_string(),
            db_path: "/data/arnold.db".to_string(),
            log_path: "/data/arnold-debug.log".to_string(),
            db_size_bytes: 1,
            journal_entries: 1,
            tasks: 1,
            memory_facts: 1,
            conversations: 1,
            analysis_queue_len: 1,
            analysis_statuses: AnalysisStatusCounts::default(),
            engine: EngineDiagSnapshot::default(),
            models: Vec::new(),
        };
        let v = serde_json::to_value(&d).expect("AppDiagnostics must serialize");
        for key in [
            "generated_at",
            "data_dir",
            "db_path",
            "log_path",
            "db_size_bytes",
            "journal_entries",
            "tasks",
            "memory_facts",
            "conversations",
            "analysis_queue_len",
            "analysis_statuses",
            "engine",
            "models",
        ] {
            assert!(v.get(key).is_some(), "missing wire key: {key}");
        }
        for key in [
            "dataDir",
            "dbPath",
            "logPath",
            "dbSizeBytes",
            "journalEntries",
            "memoryFacts",
            "analysisQueueLen",
        ] {
            assert!(v.get(key).is_none(), "camelCase leak on wire: {key}");
        }
        // Nested engine/status shapes the System tab reads (snake_case too).
        assert!(v["engine"].get("last_health_check").is_some());
        assert!(v["engine"]["chat"].get("latency_ms").is_some());
        assert!(v["engine"]["embed"].get("pid").is_some());
        assert!(v["analysis_statuses"].get("queued").is_some());
        assert!(v["analysis_statuses"].get("failed").is_some());

        // New analysis-worker events: exact wire shapes consumed by the
        // frontend (app.js / ai-sidebar dispatch on event.payload.type).
        let processing = serde_json::to_value(crate::events::AppEvent::JournalAnalysisProcessing {
            entry_id: "e1".to_string(),
        })
        .expect("JournalAnalysisProcessing must serialize");
        assert_eq!(processing["type"], "journal_analysis_processing");
        assert_eq!(processing["entry_id"], "e1");
        assert!(
            processing.get("entryId").is_none(),
            "camelCase leak on wire: entryId"
        );

        let ready = serde_json::to_value(crate::events::AppEvent::AiProposalsReady {
            conversation_id: "e1".to_string(),
            count: 3,
        })
        .expect("AiProposalsReady must serialize");
        assert_eq!(ready["type"], "ai_proposals_ready");
        assert_eq!(ready["conversation_id"], "e1");
        assert_eq!(ready["count"], 3);
        assert!(
            ready.get("conversationId").is_none(),
            "camelCase leak on wire: conversationId"
        );
    }

    #[test]
    fn engine_path_info_serializes_snake_case_wire_keys() {
        let info = EnginePathInfo {
            resolved: "C:/app/resources/engines/llama-server.exe".to_string(),
            source: "bundled_resource".to_string(),
            saved: String::new(),
        };
        let v = serde_json::to_value(&info).expect("EnginePathInfo must serialize");
        for key in ["resolved", "source", "saved"] {
            assert!(v.get(key).is_some(), "missing wire key: {key}");
        }
        assert_eq!(v["source"], "bundled_resource");
        // The UI reads snake_case (D-148); a camelCase leak would blank the hint.
        assert!(v.get("savedPath").is_none(), "camelCase leak on wire");
    }

    #[test]
    fn engine_port_info_serializes_snake_case_wire_keys() {
        let owner = crate::ai::ports::PortOwner {
            port: 8080,
            pid: 13008,
            name: "llama-server.exe".to_string(),
            exe: Some("C:/app/engines/llama-server.exe".to_string()),
            command: String::new(),
            is_llama_server: true,
        };
        let info = EnginePortInfo {
            chat_port: 8080,
            embed_port: 8081,
            chat_pid: Some(13008),
            embed_pid: None,
            chat_owner: Some(owner),
            embed_owner: None,
        };
        let v = serde_json::to_value(&info).expect("EnginePortInfo must serialize");
        for key in [
            "chat_port",
            "embed_port",
            "chat_pid",
            "embed_pid",
            "chat_owner",
            "embed_owner",
        ] {
            assert!(v.get(key).is_some(), "missing wire key: {key}");
        }
        assert_eq!(v["chat_owner"]["is_llama_server"], true);
        assert_eq!(v["chat_owner"]["pid"], 13008);
        assert!(v.get("chatPort").is_none(), "camelCase leak on wire");
        assert!(v["chat_owner"].get("isLlamaServer").is_none());
        // The System tab branches on a null owner, so it must serialize as null.
        assert!(v["embed_owner"].is_null());
    }

    #[test]
    fn engine_action_outcome_serializes_snake_case_wire_keys() {
        let outcome = EngineActionOutcome {
            service: "chat".to_string(),
            reaped: None,
            note: "chat engine restarted".to_string(),
        };
        let v = serde_json::to_value(&outcome).expect("EngineActionOutcome must serialize");
        for key in ["service", "reaped", "note"] {
            assert!(v.get(key).is_some(), "missing wire key: {key}");
        }
        assert!(v.get("reapedPid").is_none(), "camelCase leak on wire");
    }

    #[test]
    fn engine_runtime_info_serializes_snake_case_wire_keys() {
        let snapshot = EngineRuntimeSnapshot {
            chat: crate::ai::probe::EngineRuntimeInfo::default(),
            embed: crate::ai::probe::EngineRuntimeInfo::default(),
        };
        let v = serde_json::to_value(&snapshot).expect("must serialize");
        for key in ["chat", "embed"] {
            assert!(v.get(key).is_some(), "missing wire key: {key}");
        }
        for key in [
            "engine",
            "backend",
            "device",
            "device_name",
            "layers_offloaded",
            "layers_total",
            "model_mib",
            "kv_mib",
            "vram_total_mib",
            "vram_free_mib",
        ] {
            assert!(v["chat"].get(key).is_some(), "missing nested key: {key}");
        }
        assert!(v["chat"].get("layersOffloaded").is_none(), "camelCase leak");
    }

    #[test]
    fn requirements_report_serializes_snake_case_wire_keys() {
        let report = RequirementsReport {
            engine: "llama.cpp".into(),
            devices: Vec::new(),
            msvc_runtime: true,
            chat_model: "m.gguf".into(),
            embed_model: String::new(),
        };
        let v = serde_json::to_value(&report).expect("must serialize");
        for key in ["engine", "devices", "msvc_runtime", "chat_model", "embed_model"] {
            assert!(v.get(key).is_some(), "missing wire key: {key}");
        }
        assert!(v.get("msvcRuntime").is_none(), "camelCase leak");
    }
}

#[cfg(test)]
mod sql_run_tests {
    use super::{ensure_read_only, AppError};

    #[test]
    fn ensure_read_only_accepts_read_verbs() {
        for ok in [
            "SELECT 1",
            "  select 1",
            " SELECT 1  ",
            "-- leading line comment\nSELECT 1",
            "-- a\n   -- b\nSELECT 1",
            "/* block */ SELECT 1",
            "/* a */ /* b */\nSELECT * FROM tasks",
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "PRAGMA table_info(tasks)",
            "EXPLAIN SELECT 1",
            " \n\t SELECT 1",
        ] {
            assert!(ensure_read_only(ok).is_ok(), "expected accept: {ok:?}");
        }
    }

    #[test]
    fn ensure_read_only_rejects_writes() {
        for bad in [
            "",
            "   ",
            "INSERT INTO tasks (id) VALUES ('x')",
            "insert into tasks (id) VALUES ('x')",
            "UPDATE tasks SET title = 'x'",
            "DELETE FROM tasks",
            "DROP TABLE tasks",
            "ALTER TABLE tasks ADD COLUMN x TEXT",
            "CREATE TABLE evil(a)",
            "REPLACE INTO tasks VALUES ('x')",
            "VACUUM",
            "ATTACH DATABASE 'x.db' AS x",
            "BEGIN",
            "/* unterminated",
            " \n\t INSERT INTO tasks VALUES (1)",
        ] {
            let err = ensure_read_only(bad).expect_err(&format!("expected reject: {bad:?}"));
            assert!(
                matches!(err, AppError::InvalidInput(_)),
                "wrong variant: {err}"
            );
        }
    }

    #[test]
    fn stmt_readonly_backstop_rejects_writes() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t(a TEXT); INSERT INTO t VALUES ('x');")
            .unwrap();
        // Probe-verified behavior (see Task 4 Step 4 note).
        assert!(conn.prepare("SELECT * FROM t").unwrap().readonly());
        assert!(conn
            .prepare("WITH x AS (SELECT 1) SELECT * FROM x")
            .unwrap()
            .readonly());
        assert!(conn.prepare("PRAGMA table_info(t)").unwrap().readonly());
        assert!(conn.prepare("EXPLAIN SELECT * FROM t").unwrap().readonly());
        assert!(!conn
            .prepare("INSERT INTO t VALUES ('y')")
            .unwrap()
            .readonly());
        assert!(!conn.prepare("UPDATE t SET a = 'z'").unwrap().readonly());
        assert!(!conn.prepare("DELETE FROM t").unwrap().readonly());
        assert!(!conn.prepare("DROP TABLE t").unwrap().readonly());
        assert!(!conn.prepare("VACUUM").unwrap().readonly());
        assert!(!conn
            .prepare("EXPLAIN INSERT INTO t VALUES ('y')")
            .unwrap()
            .readonly());
        assert!(!conn
            .prepare("PRAGMA journal_mode = DELETE")
            .unwrap()
            .readonly());
    }
}
