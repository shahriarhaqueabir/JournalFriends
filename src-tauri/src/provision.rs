use crate::ai::engine::EmbedStartError;
use crate::error::AppError;
use crate::events::{emit, AppEvent};
use crate::AppState;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// Explain an embed start failure. A busy port is diagnosed as a squatter only
/// when the listener actually fails /v1/embeddings (a working embed server on
/// the port keeps the generic message — the app never adopts a process it did
/// not start, since it cannot verify what else that process serves). A failed
/// role check trips the embeddings circuit so workers stop hammering 501s.
pub async fn embed_start_failure_reason(
    client: Option<&Arc<crate::ai::client::LlamaClient>>,
    e: EmbedStartError,
) -> String {
    if let EmbedStartError::PortInUse(_) = e {
        if let Some(c) = client {
            if !c.verify_embed_role().await {
                c.embed_circuit_trip();
                return "Embed port is in use by a chat-mode llama-server (no --embeddings): /health passes but /v1/embeddings returns 501. Stop that process, or change llama_embed_port in Settings, then retry provisioning."
                    .to_string();
            }
        }
    }
    e.to_string()
}

/// What happened during a provisioning attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum ProvisionOutcome {
    /// Configured models found and chat+embed servers started (or already running).
    AlreadyPresent,
    /// Could not provision; reason explains what the user can do.
    Unavailable { reason: String },
}

impl ProvisionOutcome {
    pub fn summary(&self) -> String {
        match self {
            ProvisionOutcome::AlreadyPresent => "LLM engine running".to_string(),
            ProvisionOutcome::Unavailable { reason } => reason.clone(),
        }
    }
}

/// Ensure the configured llama.cpp engine is running. Safe to call at startup,
/// after vault unlock, and on demand from the UI (P-03). Never panics — all
/// failures become an `Unavailable` outcome the frontend can surface.
pub async fn provision_model(app: &AppHandle) -> ProvisionOutcome {
    let state = app.state::<AppState>();

    // Engine must be configured (llama_exe_path set — engine built at unlock).
    let engine = match state.engine() {
        Ok(e) => e,
        Err(_) => {
            let reason = "LLM engine not configured. Set the llama.cpp executable path in Settings > AI Engine.".to_string();
            emit(
                app,
                AppEvent::LlamaEngineFailed {
                    service: "engine".to_string(),
                    reason: reason.clone(),
                },
            );
            return ProvisionOutcome::Unavailable { reason };
        }
    };

    // Vault must be unlocked (settings live in the encrypted DB).
    let db = match state.db() {
        Ok(d) => d,
        Err(e) => {
            let reason = format!("Vault not unlocked: {}", e);
            emit(
                app,
                AppEvent::LlamaEngineFailed {
                    service: "engine".to_string(),
                    reason: reason.clone(),
                },
            );
            return ProvisionOutcome::Unavailable { reason };
        }
    };

    let (chat_model, embed_model) = match crate::db::pool::with_conn(db, |conn| {
        let chat = crate::db::settings::get_setting(conn, "llama_chat_model")?;
        let embed = crate::db::settings::get_setting(conn, "llama_embed_model")?;
        Ok::<(Option<String>, Option<String>), AppError>((chat, embed))
    })
    .await
    {
        Ok((c, e)) => (c.unwrap_or_default(), e.unwrap_or_default()),
        Err(e) => {
            let reason = format!("Failed to read model settings: {}", e);
            emit(
                app,
                AppEvent::LlamaEngineFailed {
                    service: "engine".to_string(),
                    reason: reason.clone(),
                },
            );
            return ProvisionOutcome::Unavailable { reason };
        }
    };

    // Resolve the models the engine will actually use (bundled store wins
    // when saved settings are empty/point at missing files) and verify the
    // resolved GGUFs exist on disk before spawning.
    let store = state.models();
    if !crate::effective_model_path(&store, &chat_model, false).is_file() {
        let reason =
            "No chat model available. Import a GGUF into models/chat/ via Settings > AI Models."
                .to_string();
        emit(
            app,
            AppEvent::LlamaModelMissing {
                model_type: "chat".to_string(),
            },
        );
        return ProvisionOutcome::Unavailable { reason };
    }
    if !crate::effective_model_path(&store, &embed_model, true).is_file() {
        let reason = "No embed model available. Import a GGUF into models/embed/ (e.g. nomic-embed-text) via Settings > AI Models.".to_string();
        emit(
            app,
            AppEvent::LlamaModelMissing {
                model_type: "embed".to_string(),
            },
        );
        return ProvisionOutcome::Unavailable { reason };
    }

    // Start servers (idempotent — start_* returns Ok(()) if already running).
    if let Err(e) = engine.start_chat().await {
        let reason = format!("Failed to start chat server: {}", e);
        emit(
            app,
            AppEvent::LlamaEngineFailed {
                service: "chat".to_string(),
                reason: reason.clone(),
            },
        );
        return ProvisionOutcome::Unavailable { reason };
    }
    if let Err(e) = engine.start_embed().await {
        let reason = embed_start_failure_reason(state.llama().ok().as_ref(), e).await;
        emit(
            app,
            AppEvent::LlamaEngineFailed {
                service: "embed".to_string(),
                reason: reason.clone(),
            },
        );
        return ProvisionOutcome::Unavailable { reason };
    }

    // Wait for health — poll up to ~15s (llama.cpp may take seconds to boot).
    let client = match state.llama() {
        Ok(c) => c,
        Err(_) => {
            let reason = "AI client not configured".to_string();
            return ProvisionOutcome::Unavailable { reason };
        }
    };
    let mut chat_ok = false;
    let mut embed_ok = false;
    for _ in 0..15 {
        match client.check_health().await {
            Ok((c, e)) => {
                chat_ok = c;
                embed_ok = e;
            }
            Err(_) => { /* not up yet */ }
        }
        if chat_ok && embed_ok {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    if chat_ok {
        emit(
            app,
            AppEvent::LlamaEngineStarted {
                service: "chat".to_string(),
            },
        );
    }
    if embed_ok {
        emit(
            app,
            AppEvent::LlamaEngineStarted {
                service: "embed".to_string(),
            },
        );
    }

    // /health passes for ANY llama-server — including a chat-mode process
    // squatting on the embed port. The authoritative role check is an
    // actual /v1/embeddings call.
    if embed_ok && !client.verify_embed_role().await {
        client.embed_circuit_trip();
        let reason = "Embed port answered /health but rejected /v1/embeddings — it is serving a chat-mode llama-server (not in --embeddings mode). Stop the process on the embed port and retry."
            .to_string();
        emit(
            app,
            AppEvent::LlamaEngineFailed {
                service: "embed".to_string(),
                reason: reason.clone(),
            },
        );
        return ProvisionOutcome::Unavailable { reason };
    }

    if !chat_ok {
        let reason = "Chat server did not become healthy within 15s".to_string();
        emit(
            app,
            AppEvent::LlamaEngineFailed {
                service: "chat".to_string(),
                reason: reason.clone(),
            },
        );
        return ProvisionOutcome::Unavailable { reason };
    }
    if !embed_ok {
        let reason = "Embed server did not become healthy within 15s".to_string();
        emit(
            app,
            AppEvent::LlamaEngineFailed {
                service: "embed".to_string(),
                reason: reason.clone(),
            },
        );
        return ProvisionOutcome::Unavailable { reason };
    }

    client.embed_circuit_reset();
    emit(app, AppEvent::ModelProvisioned { model: chat_model });
    ProvisionOutcome::AlreadyPresent
}

#[tauri::command]
pub async fn provision_model_cmd(app: tauri::AppHandle) -> Result<String, AppError> {
    let outcome = provision_model(&app).await;
    Ok(outcome.summary())
}
