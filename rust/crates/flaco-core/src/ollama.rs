//! Minimal Ollama client used by flaco-core. Only the pieces we need:
//! non-streaming chat with tool calling, plus a streaming chat that emits
//! token chunks for the web/TUI surfaces.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Clone, Debug)]
pub struct OllamaClient {
    base_url: String,
    model: String,
    http: reqwest::Client,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_name: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into(), tool_calls: vec![], tool_name: None }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into(), tool_calls: vec![], tool_name: None }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into(), tool_calls: vec![], tool_name: None }
    }
    pub fn tool(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
            tool_calls: vec![],
            tool_name: Some(name.into()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default)]
    pub id: Option<String>,
    pub function: ToolCallFunction,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<serde_json::Value>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChatResponse {
    #[allow(dead_code)]
    #[serde(default)]
    pub model: String,
    pub message: ChatMessage,
    #[serde(default)]
    pub done: bool,
}

impl OllamaClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .expect("reqwest client");
        Self { base_url: base_url.into(), model: model.into(), http }
    }

    pub fn model(&self) -> &str { &self.model }

    pub fn from_env() -> Self {
        let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let port = std::env::var("OLLAMA_PORT").unwrap_or_else(|_| "11434".into());
        let model = std::env::var("FLACO_MODEL").unwrap_or_else(|_| "qwen3:32b-q8_0".into());
        Self::new(format!("http://{host}:{port}"), model)
    }

    /// Non-streaming chat with optional tool calling.
    pub async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
    ) -> Result<ChatResponse> {
        let req = ChatRequest {
            model: self.model.clone(),
            messages,
            tools,
            stream: false,
            options: Some(serde_json::json!({ "temperature": 0.2 })),
        };
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&req)
            .send()
            .await?;
        if !resp.status().is_success() {
            let code = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Ollama(format!("HTTP {code}: {body}")));
        }
        let parsed: ChatResponse = resp.json().await?;
        Ok(parsed)
    }

    /// Simple streaming chat used by surfaces that want token-by-token output.
    /// Returns the accumulated final assistant message. The callback is called
    /// for every content chunk as it arrives.
    pub async fn chat_stream<F>(
        &self,
        messages: Vec<ChatMessage>,
        mut on_chunk: F,
    ) -> Result<ChatMessage>
    where
        F: FnMut(&str),
    {
        use futures_util::StreamExt;
        let req = ChatRequest {
            model: self.model.clone(),
            messages,
            tools: vec![],
            stream: true,
            options: Some(serde_json::json!({ "temperature": 0.3 })),
        };
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&req)
            .send()
            .await?;
        if !resp.status().is_success() {
            let code = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Ollama(format!("HTTP {code}: {body}")));
        }
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut accumulated = String::new();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            buf.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(nl) = buf.find('\n') {
                let line = buf[..nl].to_string();
                buf.drain(..=nl);
                if line.trim().is_empty() { continue; }
                if let Ok(frame) = serde_json::from_str::<ChatResponse>(&line) {
                    if !frame.message.content.is_empty() {
                        on_chunk(&frame.message.content);
                        accumulated.push_str(&frame.message.content);
                    }
                    if frame.done { break; }
                }
            }
        }
        Ok(ChatMessage::assistant(accumulated))
    }
}
