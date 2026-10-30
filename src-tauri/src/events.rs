use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    LogEvent {
        timestamp: String,
        level: String,
        source: String,
        message: String,
    },
    #[allow(dead_code)]
    StartupComplete,
    JournalAnalysisQueued {
        entry_id: String,
    },
    JournalAnalysisProcessing {
        entry_id: String,
    },
    JournalAnalysisCompleted {
        entry_id: String,
        result: crate::ai::AnalysisResult,
    },
    JournalAnalysisError {
        entry_id: String,
        error: String,
    },
    AiModelMissing {
        model: String,
    },
    AiToken {
        conversation_id: String,
        token: String,
    },
    /// Streaming reasoning tokens (model "thinking") before the visible reply.
    AiReasoningToken {
        conversation_id: String,
        token: String,
    },
    AiResponseComplete {
        conversation_id: String,
    },
    AiProposalsReady {
        conversation_id: String,
        count: u32,
    },
    /// First-run: emitted at startup when `onboarding_complete` is unset (P-04).
    OnboardingRequired,
    /// Model provisioning finished successfully (P-03).
    ModelProvisioned {
        model: String,
    },
    /// Model provisioning failed; reason is user-facing (P-03).
    #[allow(dead_code)]
    ModelProvisionFailed {
        model: String,
        reason: String,
    },
    #[allow(dead_code)]
    LlamaEngineStarted {
        service: String,
    },
    #[allow(dead_code)]
    LlamaEngineStopped {
        service: String,
    },
    #[allow(dead_code)]
    LlamaEngineFailed {
        service: String,
        reason: String,
    },
    /// Engine-neutral GPU/offload state after an engine lifecycle change.
    #[allow(dead_code)]
    EngineRuntimeStatus {
        chat: crate::ai::probe::EngineRuntimeInfo,
        embed: crate::ai::probe::EngineRuntimeInfo,
    },
    #[allow(dead_code)]
    LlamaModelImported {
        model_type: String,
        filename: String,
    },
    #[allow(dead_code)]
    LlamaModelMissing {
        model_type: String,
    },
}

pub fn emit(app: &AppHandle, event: AppEvent) {
    if let Err(e) = app.emit("app_event", event) {
        tracing::warn!(target: "events", error = %e, "failed to emit app_event");
    }
}
