use crate::ai::engine_log::{self, EngineLogRing};
use crate::ai::probe::{default_probe, EngineProbe, EngineRuntimeInfo};
use base64::Engine as _;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

// F-004: generate a fresh API key from OS entropy. 32 random bytes → base64.
pub fn generate_api_key() -> String {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub exe_path: PathBuf,
    pub chat_model: PathBuf,
    pub embed_model: PathBuf,
    pub chat_port: u16,
    pub embed_port: u16,
    /// Raw `llama_ngl` setting. Normalised by the probe, never here.
    pub gpu_layers: String,
    pub ctx_size: u32,
    pub api_key: String,
    /// Directory for `<data>/logs/llama-<service>.log`.
    pub log_dir: PathBuf,
}

pub struct LlamaEngine {
    config: EngineConfig,
    chat_child: Arc<Mutex<Option<Child>>>,
    embed_child: Arc<Mutex<Option<Child>>>,
    chat_probe: Arc<Mutex<Box<dyn EngineProbe>>>,
    embed_probe: Arc<Mutex<Box<dyn EngineProbe>>>,
    chat_ring: EngineLogRing,
    embed_ring: EngineLogRing,
    chat_reader: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    embed_reader: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

/// Chat-server spawn arguments. Kept pure so the tool-capable flags are
/// regression-tested without starting a model.
fn chat_args(config: &EngineConfig, probe: &dyn EngineProbe) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = Vec::with_capacity(16);
    args.push("-m".into());
    args.push(config.chat_model.clone().into_os_string());
    args.push("--host".into());
    args.push("127.0.0.1".into());
    args.push("--port".into());
    args.push(config.chat_port.to_string().into());
    // Engine-specific GPU flags (llama.cpp: -ngl <value> -lv 4).
    args.extend(probe.gpu_args(&config.gpu_layers));
    args.push("-c".into());
    args.push(config.ctx_size.to_string().into());
    args.push("--api-key".into());
    args.push(config.api_key.clone().into());
    args.push("--no-warmup".into());
    // Tool calling needs a Jinja chat template. Build 10952 enables this by
    // default, but pass it explicitly so an older bundled binary or a changed
    // default cannot silently disable tool calls.
    args.push("--jinja".into());
    args
}

/// Embedding-server spawn arguments (pure, same reason as `chat_args`).
fn embed_args(config: &EngineConfig, probe: &dyn EngineProbe) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = Vec::with_capacity(12);
    args.push("-m".into());
    args.push(config.embed_model.clone().into_os_string());
    args.push("--host".into());
    args.push("127.0.0.1".into());
    args.push("--port".into());
    args.push(config.embed_port.to_string().into());
    args.push("--embeddings".into());
    args.push("--pooling".into());
    args.push("cls".into());
    args.extend(probe.gpu_args(&config.gpu_layers));
    args.push("--api-key".into());
    args.push(config.api_key.clone().into());
    args
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineStatus {
    Stopped,
    Starting,
    Running,
    Failed(String),
}

/// Typed failure for embed-server startup so callers can distinguish a busy
/// port (worth diagnosing: a chat-mode llama-server squatting the embed port
/// answers /health but 501s /v1/embeddings) from a spawn error.
#[derive(Debug, PartialEq, Eq)]
pub enum EmbedStartError {
    PortInUse(u16),
    Spawn(String),
}

impl std::fmt::Display for EmbedStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbedStartError::PortInUse(port) => write!(
                f,
                "Embed port {} is already in use by another process. Restart the engine (it reaps stray llama-server processes), use \"Clear port\" in System, or change llama_embed_port in Settings.",
                port
            ),
            EmbedStartError::Spawn(e) => write!(f, "Failed to spawn embed server: {}", e),
        }
    }
}

impl LlamaEngine {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            chat_child: Arc::new(Mutex::new(None)),
            embed_child: Arc::new(Mutex::new(None)),
            chat_probe: Arc::new(Mutex::new(default_probe())),
            embed_probe: Arc::new(Mutex::new(default_probe())),
            chat_ring: engine_log::new_ring(),
            embed_ring: engine_log::new_ring(),
            chat_reader: Arc::new(Mutex::new(None)),
            embed_reader: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start_chat(&self) -> Result<(), String> {
        // Compute args first (brief probe lock) so we never hold chat_child and
        // chat_probe simultaneously — that ordering would deadlock against
        // chat_info, which takes them in the opposite order.
        let args = {
            let probe = self.chat_probe.lock().await;
            chat_args(&self.config, probe.as_ref())
        };

        let mut child = self.chat_child.lock().await;
        if child.is_some() {
            return Ok(());
        }
        if Self::port_in_use(self.config.chat_port) {
            return Err(format!(
                "Chat port {} is already in use by another process. Restart the engine (it reaps stray llama-server processes), use \"Clear port\" in System, or change llama_chat_port in Settings.",
                self.config.chat_port
            ));
        }

        let mut proc = Command::new(&self.config.exe_path)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn chat server: {}", e))?;

        let stdout = proc.stdout.take();
        let stderr = proc.stderr.take();
        *child = Some(proc);
        drop(child);

        self.start_reader("chat", stdout, stderr).await;
        Ok(())
    }

    /// Fresh probe state per start, then drain both pipes.
    async fn start_reader(
        &self,
        service: &'static str,
        stdout: Option<tokio::process::ChildStdout>,
        stderr: Option<tokio::process::ChildStderr>,
    ) {
        let (probe, ring, slot, file_name) = if service == "chat" {
            (
                self.chat_probe.clone(),
                self.chat_ring.clone(),
                self.chat_reader.clone(),
                "llama-chat.log",
            )
        } else {
            (
                self.embed_probe.clone(),
                self.embed_ring.clone(),
                self.embed_reader.clone(),
                "llama-embed.log",
            )
        };
        if let Some(handle) = slot.lock().await.take() {
            handle.abort();
        }
        probe.lock().await.reset();
        if let Some(out) = stdout {
            engine_log::spawn_output_drain(out);
        }
        if let Some(err) = stderr {
            let handle = engine_log::spawn_engine_log_reader(
                err,
                self.config.log_dir.join(file_name),
                ring,
                probe,
            );
            *slot.lock().await = Some(handle);
        }
    }

    pub async fn start_embed(&self) -> Result<(), EmbedStartError> {
        let args = {
            let probe = self.embed_probe.lock().await;
            embed_args(&self.config, probe.as_ref())
        };

        let mut child = self.embed_child.lock().await;
        if child.is_some() {
            return Ok(());
        }
        if Self::port_in_use(self.config.embed_port) {
            return Err(EmbedStartError::PortInUse(self.config.embed_port));
        }

        let mut proc = Command::new(&self.config.exe_path)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| EmbedStartError::Spawn(e.to_string()))?;

        let stdout = proc.stdout.take();
        let stderr = proc.stderr.take();
        *child = Some(proc);
        drop(child);

        self.start_reader("embed", stdout, stderr).await;
        Ok(())
    }

    pub async fn stop_chat(&self) {
        let proc = { self.chat_child.lock().await.take() };
        if let Some(mut proc) = proc {
            let _ = proc.kill().await;
            // Reap the process so the OS releases the port/PID before we spawn
            // a replacement; otherwise restart_engine can leave a stray child.
            let _ = proc.wait().await;
        }
        if let Some(handle) = self.chat_reader.lock().await.take() {
            handle.abort();
        }
        self.chat_probe.lock().await.reset();
    }

    pub async fn stop_embed(&self) {
        let proc = { self.embed_child.lock().await.take() };
        if let Some(mut proc) = proc {
            let _ = proc.kill().await;
            let _ = proc.wait().await;
        }
        if let Some(handle) = self.embed_reader.lock().await.take() {
            handle.abort();
        }
        self.embed_probe.lock().await.reset();
    }

    pub async fn stop_all(&self) {
        self.stop_chat().await;
        self.stop_embed().await;
    }

    pub async fn chat_status(&self) -> EngineStatus {
        let mut child = self.chat_child.lock().await;
        if let Some(ref mut proc) = *child {
            match proc.try_wait() {
                Ok(Some(_)) => EngineStatus::Stopped,
                Ok(None) => EngineStatus::Running,
                Err(e) => EngineStatus::Failed(e.to_string()),
            }
        } else {
            EngineStatus::Stopped
        }
    }

    pub async fn embed_status(&self) -> EngineStatus {
        let mut child = self.embed_child.lock().await;
        if let Some(ref mut proc) = *child {
            match proc.try_wait() {
                Ok(Some(_)) => EngineStatus::Stopped,
                Ok(None) => EngineStatus::Running,
                Err(e) => EngineStatus::Failed(e.to_string()),
            }
        } else {
            EngineStatus::Stopped
        }
    }

    pub async fn chat_pid(&self) -> Option<u32> {
        self.alive_pid(&self.chat_child).await
    }

    pub async fn embed_pid(&self) -> Option<u32> {
        self.alive_pid(&self.embed_child).await
    }

    async fn alive_pid(&self, slot: &Arc<Mutex<Option<Child>>>) -> Option<u32> {
        let mut child = slot.lock().await;
        match child.as_mut() {
            Some(proc) => match proc.try_wait() {
                Ok(Some(_)) => None,
                Ok(None) => proc.id(),
                Err(_) => proc.id(),
            },
            None => None,
        }
    }

    /// True when something is already accepting connections on `port`.
    /// Probed before spawn so a busy port surfaces as a typed, actionable
    /// error instead of a silent bind failure + 15s health timeout.
    fn port_in_use(port: u16) -> bool {
        use std::net::{SocketAddr, TcpStream};
        use std::time::Duration;
        TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_millis(300),
        )
        .is_ok()
    }

    pub fn chat_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.config.chat_port)
    }

    pub fn embed_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.config.embed_port)
    }

    pub fn chat_port(&self) -> u16 {
        self.config.chat_port
    }

    pub fn embed_port(&self) -> u16 {
        self.config.embed_port
    }

    /// Engine-neutral runtime state for the chat server.
    pub async fn chat_info(&self) -> EngineRuntimeInfo {
        // Read status first (releases child lock) before locking the probe,
        // so we never hold chat_probe and chat_child simultaneously in the
        // opposite order from start_chat.
        let running = matches!(self.chat_status().await, EngineStatus::Running);
        let mut probe = self.chat_probe.lock().await;
        if !running {
            probe.reset();
        }
        probe.snapshot()
    }

    /// Engine-neutral runtime state for the embed server.
    pub async fn embed_info(&self) -> EngineRuntimeInfo {
        let running = matches!(self.embed_status().await, EngineStatus::Running);
        let mut probe = self.embed_probe.lock().await;
        if !running {
            probe.reset();
        }
        probe.snapshot()
    }

    /// Last `n` drained log lines for a service (`"chat"` / `"embed"`).
    pub async fn last_log_lines(&self, service: &str, n: usize) -> Vec<String> {
        let ring = if service == "chat" {
            &self.chat_ring
        } else {
            &self.embed_ring
        };
        engine_log::tail(ring, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::probe::LlamaCppProbe;

    fn test_config() -> EngineConfig {
        EngineConfig {
            exe_path: PathBuf::from("llama-server.exe"),
            chat_model: PathBuf::from("chat.gguf"),
            embed_model: PathBuf::from("embed.gguf"),
            chat_port: 18080,
            embed_port: 18081,
            gpu_layers: "auto".into(),
            ctx_size: 16384,
            api_key: "k".into(),
            log_dir: PathBuf::from("logs"),
        }
    }

    fn probe() -> LlamaCppProbe {
        LlamaCppProbe::default()
    }

    fn arg_value<'a>(args: &'a [std::ffi::OsString], flag: &str) -> &'a str {
        let idx = args
            .iter()
            .position(|a| a.to_string_lossy() == flag)
            .expect("flag missing");
        args.get(idx + 1)
            .expect("value missing")
            .to_str()
            .expect("utf-8 value")
    }

    #[test]
    fn chat_args_default_to_auto_gpu_layers() {
        let cfg = test_config();
        let args = chat_args(&cfg, &probe());
        assert!(args.iter().any(|a| a.to_string_lossy() == "--jinja"));
        assert_eq!(arg_value(&args, "-m"), "chat.gguf");
        assert_eq!(arg_value(&args, "--host"), "127.0.0.1");
        assert_eq!(arg_value(&args, "--port"), "18080");
        assert_eq!(arg_value(&args, "-ngl"), "auto");
        assert_eq!(arg_value(&args, "-lv"), "4");
        assert_eq!(arg_value(&args, "-c"), "16384");
        assert_eq!(arg_value(&args, "--api-key"), "k");
        assert!(args.iter().any(|a| a.to_string_lossy() == "--no-warmup"));
    }

    #[test]
    fn chat_args_forward_an_explicit_layer_count() {
        let mut cfg = test_config();
        cfg.gpu_layers = "0".into();
        assert_eq!(arg_value(&chat_args(&cfg, &probe()), "-ngl"), "0");
        cfg.gpu_layers = "nonsense".into();
        assert_eq!(arg_value(&chat_args(&cfg, &probe()), "-ngl"), "auto");
    }

    #[test]
    fn embed_args_request_embeddings_and_gpu_layers() {
        let args = embed_args(&test_config(), &probe());
        assert!(args.iter().any(|a| a.to_string_lossy() == "--embeddings"));
        assert_eq!(arg_value(&args, "--pooling"), "cls");
        assert_eq!(arg_value(&args, "-ngl"), "auto");
        assert_eq!(arg_value(&args, "-lv"), "4");
    }

    #[tokio::test]
    async fn fresh_engine_reports_unknown_runtime_state() {
        let engine = LlamaEngine::new(test_config());
        assert_eq!(engine.chat_info().await.backend, "unknown");
        assert_eq!(engine.embed_info().await.backend, "unknown");
        assert!(engine.last_log_lines("chat", 5).await.is_empty());
    }

    #[tokio::test]
    async fn stopped_engine_does_not_report_stale_gpu_state() {
        let engine = LlamaEngine::new(test_config());
        {
            let mut probe = engine.chat_probe.lock().await;
            probe.feed(
                "llama_prepare_model_devices: using device CUDA0 (RTX 3060) (0000:01:00.0) - 900 MiB free",
            );
            probe.feed("load_tensors: offloaded 33/33 layers to GPU");
        }
        // There is no child in this unit test: chat_info must clear the probe
        // instead of returning the previous process's state.
        assert_eq!(engine.chat_info().await.backend, "unknown");
    }

    #[test]
    fn embed_start_error_display_messages() {
        assert_eq!(
            EmbedStartError::PortInUse(8081).to_string(),
            "Embed port 8081 is already in use by another process. Restart the engine (it reaps stray llama-server processes), use \"Clear port\" in System, or change llama_embed_port in Settings."
        );
        assert_eq!(
            EmbedStartError::Spawn("access is denied".into()).to_string(),
            "Failed to spawn embed server: access is denied"
        );
    }

    #[test]
    fn engine_exposes_configured_ports() {
        let engine = LlamaEngine::new(test_config());
        assert_eq!(engine.chat_port(), 18080);
        assert_eq!(engine.embed_port(), 18081);
    }
}
