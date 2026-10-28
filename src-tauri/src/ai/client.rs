use crate::ai::sse::{SseEvent, SseParser};
use crate::diagnostics::EngineHttpRecord;
use crate::error::AppError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

/// E-1: default per-request timeout for engine HTTP calls. Without it a
/// stalled generation hangs the analysis worker forever (reqwest has no
/// default timeout). `/health` overrides this with a shorter 5 s probe.
pub const AI_REQUEST_TIMEOUT_SECS: u64 = 300;

#[derive(Clone, Debug)]
struct ToolCallState {
    id: String,
    name: String,
    arguments: String,
}

fn merge_tool_call_delta(calls: &mut Vec<ToolCallState>, call: &serde_json::Value) {
    let idx = call["index"].as_u64().unwrap_or(0) as usize;
    let id = call["id"]
        .as_str()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let name = call["function"]["name"]
        .as_str()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let args_val = call["function"]["arguments"].clone();

    if calls.len() <= idx {
        calls.resize_with(idx + 1, || ToolCallState {
            id: String::new(),
            name: String::new(),
            arguments: String::new(),
        });
    }

    let state = &mut calls[idx];
    if let Some(id) = id {
        if !id.is_empty() {
            state.id = id;
        }
    }
    if let Some(name) = name {
        if !name.is_empty() {
            state.name = name;
        }
    }
    if !args_val.is_null() {
        if let Some(s) = args_val.as_str() {
            state.arguments.push_str(s);
        } else {
            state.arguments = args_val.to_string();
        }
    }
    if state.id.is_empty() {
        state.id = format!("call_{}", Uuid::new_v4().simple());
    }
}

pub struct LlamaClient {
    http: Client,
    chat_base: String,
    chat_model: String,
    embed_base: String,
    embed_model: String,
    api_key: String,
    /// Opened on definitive evidence the embed listener is not in
    /// --embeddings mode (HTTP 501 from /v1/embeddings, or a failed role
    /// check during provisioning) so analysis workers stop retrying a
    /// known-broken endpoint; closed when provisioning succeeds or the
    /// embed server is restarted.
    embed_circuit_open: AtomicBool,
    /// Engine-HTTP telemetry ring (settings observability).
    telemetry: Arc<crate::diagnostics::TelemetryRing<EngineHttpRecord>>,
    /// Optional event emitter so engine requests can be live-streamed through
    /// the single `app_event` channel (AppEvent::LogEvent).
    on_event: Option<Arc<dyn Fn(crate::events::AppEvent) + Send + Sync>>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct ModelListResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

/// One service health probe: reachability plus round-trip latency.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceHealth {
    pub ok: bool,
    pub latency_ms: u64,
}

/// Build the single-shot JSON request used by the analysis worker. Kept
/// separate so its shape (non-streaming + JSON mode, no tool-choice) is
/// regression-tested — parsing a streamed body with `Response::json()` fails.
fn build_analysis_request(
    model: String,
    messages: Vec<ChatMessage>,
    tools: Option<Vec<serde_json::Value>>,
) -> ChatRequest {
    ChatRequest {
        model,
        messages,
        stream: false,
        tools,
        parallel_tool_calls: None,
        tool_choice: None,
        response_format: Some(serde_json::json!({"type": "json_object"})),
    }
}

impl LlamaClient {
    pub fn new(
        chat_url: &str,
        embed_url: &str,
        chat_model: &str,
        embed_model: &str,
        api_key: &str,
        telemetry: Arc<crate::diagnostics::TelemetryRing<EngineHttpRecord>>,
        on_event: Option<Arc<dyn Fn(crate::events::AppEvent) + Send + Sync>>,
    ) -> Self {
        Self::with_timeout(
            chat_url,
            embed_url,
            chat_model,
            embed_model,
            api_key,
            telemetry,
            on_event,
            std::time::Duration::from_secs(AI_REQUEST_TIMEOUT_SECS),
        )
    }

    /// Construct with an explicit request timeout. Production code uses
    /// [`AI_REQUEST_TIMEOUT_SECS`]; tests pin a short value.
    #[allow(clippy::too_many_arguments)]
    pub fn with_timeout(
        chat_url: &str,
        embed_url: &str,
        chat_model: &str,
        embed_model: &str,
        api_key: &str,
        telemetry: Arc<crate::diagnostics::TelemetryRing<EngineHttpRecord>>,
        on_event: Option<Arc<dyn Fn(crate::events::AppEvent) + Send + Sync>>,
        timeout: std::time::Duration,
    ) -> Self {
        // E-1: a stalled generation (server accepted the request but never
        // produced a body / the connection was silently dropped) must not hang
        // the analysis worker forever. A default request timeout makes a lost
        // call surface as a reqwest::Error → AiError → worker mark_failed.
        // `/health` keeps its own shorter 5 s override at the call site; the
        // long default accommodates real generation latency on big prompts.
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest client build must succeed");
        Self {
            http,
            chat_base: chat_url.trim_end_matches('/').to_string(),
            embed_base: embed_url.trim_end_matches('/').to_string(),
            chat_model: chat_model.to_string(),
            embed_model: embed_model.to_string(),
            api_key: api_key.to_string(),
            embed_circuit_open: AtomicBool::new(false),
            telemetry,
            on_event,
        }
    }

    /// Stop calling /v1/embeddings until a successful provision or restart.
    pub fn embed_circuit_trip(&self) {
        self.embed_circuit_open.store(true, Ordering::Relaxed);
    }

    /// Embeddings verified working again — resume calls.
    pub fn embed_circuit_reset(&self) {
        self.embed_circuit_open.store(false, Ordering::Relaxed);
    }

    /// Send a request, recording method/path/status/duration into the ring.
    async fn execute(
        &self,
        service: &str,
        method: &str,
        path: &str,
        req: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let start = Instant::now();
        let result = req.send().await;
        let duration_ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(resp) => {
                let status = resp.status().as_u16();
                self.log_request(service, method, path, Some(status), duration_ms, None);
                Ok(resp)
            }
            Err(e) => {
                self.log_request(
                    service,
                    method,
                    path,
                    None,
                    duration_ms,
                    Some(e.to_string()),
                );
                Err(e)
            }
        }
    }

    /// Push an HTTP telemetry record and live-stream it via LogEvent.
    fn log_request(
        &self,
        service: &str,
        method: &str,
        path: &str,
        status: Option<u16>,
        duration_ms: u64,
        error: Option<String>,
    ) {
        let record = EngineHttpRecord {
            at: chrono::Utc::now().to_rfc3339(),
            service: service.to_string(),
            method: method.to_string(),
            path: path.to_string(),
            status,
            duration_ms,
            error: error.clone(),
        };
        self.telemetry.push(record);

        let level = if error.is_some() {
            "error"
        } else {
            match status {
                Some(s) if s >= 500 => "error",
                Some(s) if s >= 300 => "warn",
                _ => "info",
            }
        };
        let status_str = match status {
            Some(s) => s.to_string(),
            None => "err".to_string(),
        };
        let mut message = format!(
            "{} {} {} ({}) — {}ms",
            method, path, status_str, service, duration_ms
        );
        if let Some(e) = error {
            message.push_str(&format!(" — {}", e));
        }
        if let Some(cb) = &self.on_event {
            cb(crate::events::AppEvent::LogEvent {
                timestamp: chrono::Utc::now().to_rfc3339(),
                level: level.to_string(),
                source: "engine".to_string(),
                message,
            });
        }
    }

    pub async fn check_health(&self) -> Result<(bool, bool), AppError> {
        let (chat, embed) = self.check_health_detailed().await;
        Ok((chat.ok, embed.ok))
    }

    pub async fn check_health_detailed(&self) -> (ServiceHealth, ServiceHealth) {
        let chat = self.service_health("chat").await;
        let embed = self.service_health("embed").await;
        (chat, embed)
    }

    async fn service_health(&self, service: &str) -> ServiceHealth {
        let base = if service == "chat" {
            self.chat_base.as_str()
        } else {
            self.embed_base.as_str()
        };
        let start = Instant::now();
        let req = self
            .http
            .get(format!("{}/health", base))
            .bearer_auth(&self.api_key)
            .timeout(std::time::Duration::from_secs(5));
        match self.execute(service, "GET", "/health", req).await {
            Ok(resp) => ServiceHealth {
                ok: resp.status().is_success(),
                latency_ms: start.elapsed().as_millis() as u64,
            },
            Err(_) => ServiceHealth {
                ok: false,
                latency_ms: start.elapsed().as_millis() as u64,
            },
        }
    }

    #[allow(dead_code)]
    pub async fn list_local_models(&self) -> Result<Vec<String>, AppError> {
        let resp: ModelListResponse = self
            .execute(
                "chat",
                "GET",
                "/v1/models",
                self.http
                    .get(format!("{}/v1/models", self.chat_base))
                    .bearer_auth(&self.api_key),
            )
            .await
            .map_err(|e| AppError::AiError(format!("Failed to list models: {}", e)))?
            .json()
            .await
            .map_err(|e| AppError::AiError(format!("Failed to parse model list: {}", e)))?;

        Ok(resp.data.into_iter().map(|m| m.id).collect())
    }

    pub async fn generate_embeddings(&self, text: &str) -> Result<Vec<f32>, AppError> {
        if self.embed_circuit_open.load(Ordering::Relaxed) {
            return Err(AppError::AiError(
                "Embeddings circuit open: /v1/embeddings was rejected earlier (chat-mode llama-server on the embed port). Run the system check or restart the engine to retry."
                    .into(),
            ));
        }
        let body = serde_json::json!({
            "model": self.embed_model,
            "input": text,
        });

        let resp = self
            .execute(
                "embed",
                "POST",
                "/v1/embeddings",
                self.http
                    .post(format!("{}/v1/embeddings", self.embed_base))
                    .bearer_auth(&self.api_key)
                    .json(&body),
            )
            .await
            .map_err(|e| AppError::AiError(format!("Embedding request failed: {}", e)))?;

        // 501 is definitive: the listener is a llama-server without
        // --embeddings (a chat-mode process squatting the embed port).
        // Anything else (e.g. 503 while loading) stays transient.
        if resp.status() == reqwest::StatusCode::NOT_IMPLEMENTED {
            self.embed_circuit_open.store(true, Ordering::Relaxed);
            return Err(AppError::AiError(
                "Embed server returned 501 Not Implemented — it is not running in --embeddings mode (chat-mode llama-server on the embed port)."
                    .into(),
            ));
        }

        let parsed: EmbeddingResponse = resp
            .json()
            .await
            .map_err(|e| AppError::AiError(format!("Embedding parse failed: {}", e)))?;

        parsed
            .data
            .first()
            .map(|d| d.embedding.clone())
            .ok_or_else(|| AppError::AiError("No embedding returned".into()))
    }

    pub async fn verify_embed_role(&self) -> bool {
        let body = serde_json::json!({
            "model": self.embed_model,
            "input": "role-verification",
        });
        match self
            .execute(
                "embed",
                "POST",
                "/v1/embeddings",
                self.http
                    .post(format!("{}/v1/embeddings", self.embed_base))
                    .bearer_auth(&self.api_key)
                    .json(&body),
            )
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<EmbeddingResponse>().await {
                    Ok(r) => r
                        .data
                        .first()
                        .map(|d| !d.embedding.is_empty())
                        .unwrap_or(false),
                    Err(_) => false,
                }
            }
            _ => false,
        }
    }

    pub async fn analyze_journal(
        &self,
        messages: Vec<(String, String)>,
        tools: Option<Vec<serde_json::Value>>,
    ) -> Result<String, AppError> {
        let chat_messages: Vec<ChatMessage> = messages
            .into_iter()
            .map(|(role, content)| ChatMessage {
                role,
                content,
                tool_calls: None,
                tool_call_id: None,
            })
            .collect();

        let request = build_analysis_request(self.chat_model.clone(), chat_messages, tools);

        let resp: serde_json::Value = self
            .execute(
                "chat",
                "POST",
                "/v1/chat/completions",
                self.http
                    .post(format!("{}/v1/chat/completions", self.chat_base))
                    .bearer_auth(&self.api_key)
                    .json(&request),
            )
            .await
            .map_err(|e| AppError::AiError(format!("Analysis request failed: {}", e)))?
            .json()
            .await
            .map_err(|e| AppError::AiError(format!("Analysis parse failed: {}", e)))?;

        let message = &resp["choices"][0]["message"];
        if let Some(s) = message["content"].as_str() {
            if !s.trim().is_empty() {
                return Ok(s.to_string());
            }
        }
        // Reasoning models spend tokens on `reasoning_content` before the
        // visible answer. Prefer it over a misleading "no content" error.
        if let Some(s) = message["reasoning_content"].as_str() {
            if !s.trim().is_empty() {
                return Ok(s.to_string());
            }
        }
        Err(AppError::AiError("No content in analysis response".into()))
    }

    pub async fn chat_with_tools_stream(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Option<Vec<serde_json::Value>>,
        on_token: impl FnMut(String),
        on_reasoning: impl FnMut(String),
        on_tool_call: impl FnMut(String, String, String),
    ) -> Result<String, AppError> {
        let chat_messages: Vec<ChatMessage> = messages
            .into_iter()
            .filter_map(|m| {
                let role = m["role"].as_str()?.to_string();
                let content = m["content"].as_str().unwrap_or("").to_string();
                let tool_calls = m
                    .get("tool_calls")
                    .and_then(|v| v.as_array())
                    .filter(|v| !v.is_empty())
                    .cloned();
                let tool_call_id = m
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                Some(ChatMessage {
                    role,
                    content,
                    tool_calls,
                    tool_call_id,
                })
            })
            .collect();

        // Keep behavior explicit and predictable: one tool call per turn and
        // auto selection. Some templates (e.g. Hermes) are more reliable with
        // tool_choice auto when parallel calls are disabled.
        let request = ChatRequest {
            model: self.chat_model.clone(),
            messages: chat_messages,
            stream: true,
            tools,
            parallel_tool_calls: Some(false),
            tool_choice: Some("auto".to_string()),
            response_format: None,
        };

        let resp = self
            .execute(
                "chat",
                "POST",
                "/v1/chat/completions",
                self.http
                    .post(format!("{}/v1/chat/completions", self.chat_base))
                    .bearer_auth(&self.api_key)
                    .json(&request),
            )
            .await
            .map_err(|e| AppError::AiError(format!("Chat request failed: {}", e)))?;

        let mut parser = SseParser::new();
        let mut on_token = on_token;
        let mut on_reasoning = on_reasoning;
        let mut on_tool_call = on_tool_call;
        let mut full_content = String::new();
        let mut full_reasoning = String::new();
        let mut tool_calls: Vec<ToolCallState> = Vec::new();

        let mut stream = resp.bytes_stream();
        use futures_util::StreamExt;

        'stream: while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| AppError::AiError(format!("Stream error: {}", e)))?;
            let events = parser.feed(&chunk[..]);

            for event in events {
                match event {
                    SseEvent::Done => break 'stream,
                    SseEvent::Error(e) => return Err(AppError::AiError(e)),
                    SseEvent::Data(json_str) => {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                            if let Some(choices) = val["choices"].as_array() {
                                for choice in choices {
                                    if let Some(delta) = choice.get("delta") {
                                        if let Some(content) = delta["content"].as_str() {
                                            full_content.push_str(content);
                                            on_token(content.to_string());
                                        }
                                        if let Some(reasoning) = delta["reasoning_content"].as_str()
                                        {
                                            if !reasoning.is_empty() {
                                                full_reasoning.push_str(reasoning);
                                                on_reasoning(reasoning.to_string());
                                            }
                                        }
                                        if let Some(tc) = delta["tool_calls"].as_array() {
                                            for call in tc {
                                                merge_tool_call_delta(&mut tool_calls, call);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(tc) = tool_calls.first() {
            if !tc.name.is_empty() {
                on_tool_call(tc.id.clone(), tc.name.clone(), tc.arguments.clone());
            }
        }

        // A reasoning model may spend its entire generation on
        // `reasoning_content`. Return that rather than an empty turn
        // (unless a tool call is the actual answer).
        if tool_calls.is_empty()
            && full_content.trim().is_empty()
            && !full_reasoning.trim().is_empty()
        {
            return Ok(full_reasoning);
        }

        Ok(full_content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    fn test_client_with_timeout(chat_url: &str, timeout: std::time::Duration) -> LlamaClient {
        LlamaClient::with_timeout(
            chat_url,
            "http://127.0.0.1:1",
            "chat-model",
            "embed-model",
            "test-key",
            Arc::new(crate::diagnostics::TelemetryRing::new(8)),
            None,
            timeout,
        )
    }

    // E-1 regression: a server that accepts the connection but never sends a
    // response body must NOT hang the caller forever. Without a request
    // timeout this test would never complete (the original bug that wedged the
    // analysis worker). We use a short 3 s client timeout and a 15 s outer
    // guard: if the per-request timeout is not wired the outer guard fires and
    // the test fails instead of hanging the suite.
    #[tokio::test]
    async fn stalled_generation_times_out_instead_of_hanging() {
        // Black-hole server: accept one connection, read the request, then
        // never write a response.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf);
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
        });

        let client = test_client_with_timeout(
            &format!("http://{}", addr),
            std::time::Duration::from_secs(3),
        );
        let messages = vec![("user".to_string(), "hello".to_string())];

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            client.analyze_journal(messages, None),
        )
        .await;

        // The outer timeout must NOT have fired — the inner per-request client
        // timeout is what should end the wait.
        let inner = result.expect("analyze_journal hung past the outer guard");
        assert!(inner.is_err(), "stalled generation should return an error");
    }

    #[test]
    fn merge_tool_call_deltas_preserves_id_name_and_fragmented_args() {
        let mut calls = Vec::new();
        merge_tool_call_delta(
            &mut calls,
            &serde_json::json!({
                "index": 0,
                "id": "call_1",
                "type": "function",
                "function": {"name": "list_tasks", "arguments": ""}
            }),
        );
        merge_tool_call_delta(
            &mut calls,
            &serde_json::json!({"index": 0, "function": {"arguments": "{\"status\":"}}),
        );
        merge_tool_call_delta(
            &mut calls,
            &serde_json::json!({"index": 0, "function": {"arguments": "\"todo\"}"}}),
        );

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "list_tasks");
        assert_eq!(calls[0].arguments, "{\"status\":\"todo\"}");
    }

    #[test]
    fn merge_tool_call_deltas_fills_late_id_and_name() {
        let mut calls = Vec::new();
        merge_tool_call_delta(
            &mut calls,
            &serde_json::json!({"index": 0, "function": {"arguments": "{\"id\":\"t1\"}"}}),
        );
        merge_tool_call_delta(
            &mut calls,
            &serde_json::json!({
                "index": 0,
                "id": "call_2",
                "function": {"name": "complete_task", "arguments": ""}
            }),
        );

        assert_eq!(calls[0].id, "call_2");
        assert_eq!(calls[0].name, "complete_task");
        assert_eq!(calls[0].arguments, "{\"id\":\"t1\"}");
    }

    #[test]
    fn merge_tool_call_deltas_accepts_object_arguments_and_multiple_calls() {
        let mut calls = Vec::new();
        merge_tool_call_delta(
            &mut calls,
            &serde_json::json!({
                "index": 0,
                "id": "a",
                "function": {"name": "list_tasks", "arguments": {"status": "todo"}}
            }),
        );
        merge_tool_call_delta(
            &mut calls,
            &serde_json::json!({"index": 1, "id": "b", "function": {"name": "list_events"}}),
        );

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].arguments, "{\"status\":\"todo\"}");
        assert_eq!(calls[1].name, "list_events");
    }

    #[test]
    fn tool_call_gets_a_fallback_id_when_server_omits_one() {
        let mut calls = Vec::new();
        merge_tool_call_delta(
            &mut calls,
            &serde_json::json!({"index": 0, "function": {"name": "list_tasks"}}),
        );
        assert!(
            !calls[0].id.is_empty(),
            "every tool call needs an id for the tool response"
        );
    }

    #[test]
    fn request_timeout_is_configured() {
        // Guard the constant so a future edit can't silently drop back to an
        // unbounded client (the exact regression E-1 describes).
        assert!(AI_REQUEST_TIMEOUT_SECS >= 30);
        assert!(AI_REQUEST_TIMEOUT_SECS <= 600);
    }

    #[test]
    fn analysis_request_is_non_streaming_json_mode() {
        // Regression: the analysis worker parses the body with
        // `Response::json()`, so the request MUST be non-streaming and in JSON
        // mode. `stream:true` (an SSE body) makes every analysis fail.
        let req = build_analysis_request(
            "chat-model".to_string(),
            vec![ChatMessage {
                role: "user".to_string(),
                content: "analyze".to_string(),
                tool_calls: None,
                tool_call_id: None,
            }],
            None,
        );
        let value = serde_json::to_value(&req).unwrap();
        assert_eq!(value["stream"], serde_json::json!(false));
        assert_eq!(
            value["response_format"],
            serde_json::json!({"type": "json_object"})
        );
        assert!(value.get("tool_choice").is_none());
        assert!(value.get("parallel_tool_calls").is_none());
    }
}
