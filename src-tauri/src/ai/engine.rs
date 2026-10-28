use base64::Engine as _;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;
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
    pub ngl: u32,
    pub ctx_size: u32,
    pub api_key: String,
}

pub struct LlamaEngine {
    config: EngineConfig,
    chat_child: Arc<Mutex<Option<Child>>>,
    embed_child: Arc<Mutex<Option<Child>>>,
}

/// Chat-server spawn arguments. Kept pure so the tool-capable flags are
/// regression-tested without starting a model.
fn chat_args(config: &EngineConfig) -> Vec<std::ffi::OsString> {
    let args: Vec<std::ffi::OsString> = vec![
        "-m".into(),
        config.chat_model.clone().into_os_string(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        config.chat_port.to_string().into(),
        "-ngl".into(),
        config.ngl.to_string().into(),
        "-c".into(),
        config.ctx_size.to_string().into(),
        "--api-key".into(),
        config.api_key.clone().into(),
        "--no-warmup".into(),
        // Tool calling needs a Jinja chat template. Build 10952 enables this
        // by default, but pass it explicitly so an older bundled binary or a
        // changed default cannot silently disable tool calls.
        "--jinja".into(),
    ];
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
                "Embed port {} is already in use by another process. Stop it, or change llama_embed_port in Settings.",
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
        }
    }

    pub async fn start_chat(&self) -> Result<(), String> {
        let mut child = self.chat_child.lock().await;
        if child.is_some() {
            return Ok(());
        }
        if Self::port_in_use(self.config.chat_port) {
            return Err(format!(
                "Chat port {} is already in use by another process. Stop it, or change llama_chat_port in Settings.",
                self.config.chat_port
            ));
        }

        let output = Command::new(&self.config.exe_path)
            .args(chat_args(&self.config))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn chat server: {}", e))?;

        *child = Some(output);
        Ok(())
    }

    pub async fn start_embed(&self) -> Result<(), EmbedStartError> {
        let mut child = self.embed_child.lock().await;
        if child.is_some() {
            return Ok(());
        }
        if Self::port_in_use(self.config.embed_port) {
            return Err(EmbedStartError::PortInUse(self.config.embed_port));
        }

        let output = Command::new(&self.config.exe_path)
            .arg("-m")
            .arg(&self.config.embed_model)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(self.config.embed_port.to_string())
            .arg("--embeddings")
            .arg("--pooling")
            .arg("cls")
            .arg("-ngl")
            .arg(self.config.ngl.to_string())
            .arg("--api-key")
            .arg(&self.config.api_key)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| EmbedStartError::Spawn(e.to_string()))?;

        *child = Some(output);
        Ok(())
    }

    pub async fn stop_chat(&self) {
        let mut child = self.chat_child.lock().await;
        if let Some(ref mut proc) = *child {
            let _ = proc.kill();
        }
        *child = None;
    }

    pub async fn stop_embed(&self) {
        let mut child = self.embed_child.lock().await;
        if let Some(ref mut proc) = *child {
            let _ = proc.kill();
        }
        *child = None;
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
                Ok(None) => Some(proc.id()),
                Err(_) => Some(proc.id()),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> EngineConfig {
        EngineConfig {
            exe_path: PathBuf::from("llama-server.exe"),
            chat_model: PathBuf::from("chat.gguf"),
            embed_model: PathBuf::from("embed.gguf"),
            chat_port: 18080,
            embed_port: 18081,
            ngl: 99,
            ctx_size: 16384,
            api_key: "k".into(),
        }
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
    fn chat_args_include_jinja_and_configured_values() {
        let cfg = test_config();
        let args = chat_args(&cfg);
        assert!(args.iter().any(|a| a.to_string_lossy() == "--jinja"));
        assert_eq!(arg_value(&args, "-m"), "chat.gguf");
        assert_eq!(arg_value(&args, "--host"), "127.0.0.1");
        assert_eq!(arg_value(&args, "--port"), "18080");
        assert_eq!(arg_value(&args, "-ngl"), "99");
        assert_eq!(arg_value(&args, "-c"), "16384");
        assert_eq!(arg_value(&args, "--api-key"), "k");
        assert!(args.iter().any(|a| a.to_string_lossy() == "--no-warmup"));
    }

    #[test]
    fn embed_start_error_display_messages() {
        assert_eq!(
            EmbedStartError::PortInUse(8081).to_string(),
            "Embed port 8081 is already in use by another process. Stop it, or change llama_embed_port in Settings."
        );
        assert_eq!(
            EmbedStartError::Spawn("access is denied".into()).to_string(),
            "Failed to spawn embed server: access is denied"
        );
    }
}
