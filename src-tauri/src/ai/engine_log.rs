//! Drains the engine's piped stdio.
//!
//! Two problems solved at once: an unread pipe (~64 KB) eventually blocks the
//! child mid-request, and the stderr stream is exactly where llama.cpp reports
//! device / offload / VRAM state.

use crate::ai::probe::strip_ansi;
use crate::ai::probe::EngineProbe;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::sync::Mutex as AsyncMutex;

/// Lines kept in memory for failure diagnostics.
pub const RING_CAPACITY: usize = 200;
/// Single-generation rotation threshold.
pub const ROTATE_BYTES: u64 = 2 * 1024 * 1024;

pub type EngineLogRing = Arc<Mutex<VecDeque<String>>>;

pub fn new_ring() -> EngineLogRing {
    Arc::new(Mutex::new(VecDeque::with_capacity(RING_CAPACITY)))
}

/// Push a line, dropping the oldest when full.
pub fn push_capped(ring: &Mutex<VecDeque<String>>, line: String) {
    if let Ok(mut guard) = ring.lock() {
        if guard.len() >= RING_CAPACITY {
            guard.pop_front();
        }
        guard.push_back(line);
    }
}

/// Move an oversized log aside to `<name>.1` (one generation). Never fatal.
pub fn rotate_if_needed(path: &Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > ROTATE_BYTES {
            let mut backup = path.as_os_str().to_owned();
            backup.push(".1");
            let backup = PathBuf::from(backup);
            // Windows `rename` refuses to overwrite an existing destination, so
            // drop the previous generation first (one generation only).
            let _ = std::fs::remove_file(&backup);
            if let Err(e) = std::fs::rename(path, &backup) {
                tracing::warn!(target: "ai::engine_log", error = %e, "engine log rotation failed");
            }
        }
    }
}

/// Drain a stream that carries no diagnostics (llama-server keeps stdout
/// almost silent, but the pipe must still be read).
pub fn spawn_output_drain<R>(stream: R) -> tokio::task::JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Some(result) = lines.next_line().await.transpose() {
            match result {
                Ok(raw) => {}
                Err(e) => {
                    tracing::warn!(target: "ai::engine_log", error = %e, "stdout drain read error");
                    break;
                }
            }
        }
    })
}

/// Read stderr line by line: append to `log_path`, keep the tail in `ring`,
/// and feed the probe that backs the GPU status row.
pub fn spawn_engine_log_reader<R>(
    stream: R,
    log_path: PathBuf,
    ring: EngineLogRing,
    probe: Arc<AsyncMutex<Box<dyn EngineProbe>>>,
) -> tokio::task::JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        rotate_if_needed(&log_path);
        if let Some(parent) = log_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                tracing::warn!(target: "ai::engine_log", error = %e, "engine log directory could not be created");
            }
        }
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
        {
            Ok(f) => Some(f),
            Err(e) => {
                tracing::warn!(target: "ai::engine_log", error = %e, "engine log file could not be opened");
                None
            }
        };
        let mut lines = BufReader::new(stream).lines();
        loop {
            let raw = match lines.next_line().await {
                Ok(Some(raw)) => raw,
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(target: "ai::engine_log", error = %e, "engine log read error");
                    break;
                }
            };
            let line = strip_ansi(&raw);
            if line.trim().is_empty() {
                continue;
            }
            let mut write_failed = false;
            if let Some(handle) = file.as_mut() {
                write_failed = handle
                    .write_all(format!("{}\n", line).as_bytes())
                    .await
                    .is_err();
            }
            if write_failed {
                file = None;
            }
            push_capped(&ring, line.clone());
            probe.lock().await.feed(&line);
        }
    })
}

/// Last `n` lines, oldest first.
pub fn tail(ring: &Mutex<VecDeque<String>>, n: usize) -> Vec<String> {
    match ring.lock() {
        Ok(guard) => guard.iter().rev().take(n).rev().cloned().collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_caps_at_capacity() {
        let ring = std::sync::Mutex::new(std::collections::VecDeque::new());
        for i in 0..250 {
            push_capped(&ring, format!("line-{}", i));
        }
        let guard = ring.lock().unwrap();
        assert_eq!(guard.len(), RING_CAPACITY);
        assert_eq!(guard.front().map(String::as_str), Some("line-50"));
        assert_eq!(guard.back().map(String::as_str), Some("line-249"));
    }

    #[test]
    fn rotation_renames_an_oversized_log() {
        let dir = std::env::temp_dir().join(format!("arnold-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llama-chat.log");
        std::fs::write(&path, vec![b'x'; (ROTATE_BYTES + 1) as usize]).unwrap();
        rotate_if_needed(&path);
        assert!(!path.exists(), "original must be moved away");
        let mut backup = path.clone().into_os_string();
        backup.push(".1");
        assert!(std::path::PathBuf::from(backup).exists(), ".1 must exist");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rotation_leaves_a_small_log_alone() {
        let dir = std::env::temp_dir().join(format!("arnold-log-small-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llama-embed.log");
        std::fs::write(&path, b"small").unwrap();
        rotate_if_needed(&path);
        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn reader_creates_log_directory_and_persists_lines() {
        use tokio::io::AsyncWriteExt;

        let dir = std::env::temp_dir().join(format!("arnold-log-parent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("logs").join("llama-chat.log");
        let (mut writer, reader) = tokio::io::duplex(1024);
        let probe: Arc<AsyncMutex<Box<dyn EngineProbe>>> =
            Arc::new(AsyncMutex::new(crate::ai::probe::default_probe()));
        let task = spawn_engine_log_reader(reader, path.clone(), new_ring(), probe);

        writer.write_all(b"engine startup line\n").await.unwrap();
        drop(writer);
        task.await.unwrap();

        let log = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(log.contains("engine startup line"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
