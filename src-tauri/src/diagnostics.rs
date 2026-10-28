//! Settings observability — under-the-hood diagnostics.
//!
//! In-memory telemetry only (no schema): a bounded ring for engine HTTP
//! records, a health-latency cache, log-tail reading, and the three Tauri
//! commands that expose them. Durability comes from the existing tracing log
//! file, surfaced in-app via `log_tail`.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

/// Bounded, in-memory ring buffer for telemetry records.
///
/// Newest item is appended to the back; when full, the oldest item is dropped.
/// `snapshot()` returns items in insertion order (oldest → newest) and never
/// blocks on a popped guard.
pub struct TelemetryRing<T> {
    inner: Mutex<RingInner<T>>,
}

struct RingInner<T> {
    capacity: usize,
    items: VecDeque<T>,
}

impl<T> TelemetryRing<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(RingInner {
                capacity,
                items: VecDeque::new(),
            }),
        }
    }

    pub fn push(&self, item: T) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.items.len() >= inner.capacity {
            inner.items.pop_front();
        }
        inner.items.push_back(item);
    }

    pub fn snapshot(&self) -> Vec<T>
    where
        T: Clone,
    {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.items.iter().cloned().collect()
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.items.clear();
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .items
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One engine HTTP request as observed by `LlamaClient`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineHttpRecord {
    pub at: String,
    pub service: String,
    pub method: String,
    pub path: String,
    pub status: Option<u16>,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// Last engine-health probe (latency + timestamp), written by
/// `check_engine_health` and read by `app_diagnostics`.
#[derive(Debug, Default)]
pub struct HealthCache {
    pub chat_latency_ms: Option<u64>,
    pub embed_latency_ms: Option<u64>,
    pub last_check: Option<String>,
}

/// Response shape for the `log_tail` command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogTailResponse {
    pub lines: Vec<String>,
    pub error: Option<String>,
}

/// Last `wanted_lines` lines of the tracing log file.
///
/// A missing/rotated file returns empty lines (no error) so the UI can show a
/// friendly empty state; other IO failures surface as `error`.
pub fn tail_log_file(path: &Path, wanted_lines: usize) -> LogTailResponse {
    let max_lines = wanted_lines.clamp(1, 2000);
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            let all: Vec<String> = contents.lines().map(|s| s.to_string()).collect();
            let start = all.len().saturating_sub(max_lines);
            LogTailResponse {
                lines: all[start..].to_vec(),
                error: None,
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LogTailResponse {
            lines: Vec::new(),
            error: None,
        },
        Err(e) => LogTailResponse {
            lines: Vec::new(),
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
pub async fn engine_http_log(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<EngineHttpRecord>, crate::error::AppError> {
    Ok(state.telemetry.snapshot())
}

#[tauri::command]
pub async fn log_tail(
    state: tauri::State<'_, crate::AppState>,
    lines: Option<u32>,
) -> Result<LogTailResponse, crate::error::AppError> {
    Ok(tail_log_file(
        &state.paths.log_path,
        lines.unwrap_or(200) as usize,
    ))
}

#[tauri::command]
pub async fn telemetry_clear(
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), crate::error::AppError> {
    state.telemetry.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_pushes_below_cap() {
        let ring = TelemetryRing::new(3);
        ring.push(1);
        ring.push(2);
        assert_eq!(ring.len(), 2);
        assert_eq!(ring.snapshot(), vec![1, 2]);
    }

    #[test]
    fn ring_trims_oldest_at_cap() {
        let ring = TelemetryRing::new(3);
        for i in 1..=5 {
            ring.push(i);
        }
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.snapshot(), vec![3, 4, 5]);
    }

    #[test]
    fn ring_snapshot_preserves_order_and_clones() {
        let ring = TelemetryRing::new(64);
        for i in 1..=4 {
            ring.push(i);
        }
        let snap = ring.snapshot();
        assert_eq!(snap, vec![1, 2, 3, 4]);
        ring.clear();
        assert!(ring.is_empty());
        // snapshot was a clone — unaffected by clear
        assert_eq!(snap, vec![1, 2, 3, 4]);
    }

    #[test]
    fn ring_clear_empties() {
        let ring = TelemetryRing::new(5);
        ring.push("a");
        ring.push("b");
        ring.clear();
        assert!(ring.is_empty());
        assert_eq!(ring.snapshot().len(), 0);
    }

    #[test]
    fn engine_http_record_serde_round_trip() {
        let rec = EngineHttpRecord {
            at: "2026-09-15T10:00:00Z".to_string(),
            service: "chat".to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            status: Some(200),
            duration_ms: 42,
            error: None,
        };
        let json = serde_json::to_string(&rec).expect("serialize");
        assert!(json.contains("\"durationMs\":42"));
        let back: EngineHttpRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.service, "chat");
        assert_eq!(back.status, Some(200));
    }

    #[test]
    fn tail_log_file_handles_missing_path() {
        let resp = tail_log_file(Path::new("definitely/missing.log"), 10);
        assert!(resp.error.is_none());
        assert!(resp.lines.is_empty());
    }
}
