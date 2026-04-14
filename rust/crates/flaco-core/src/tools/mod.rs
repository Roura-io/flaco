//! Typed tool registry.
//!
//! Each tool declares a JSON schema (compatible with Ollama's native tool
//! calling format) and an async handler. The registry exposes a single
//! `call` method that looks the tool up and runs it.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

pub mod bash;
pub mod fs_rw;
pub mod web;
pub mod jira;
pub mod github;
pub mod memory_tool;
pub mod shortcut;
pub mod scaffold;
pub mod research;
pub mod save_to_unas;
pub mod slack_post;
pub mod weather;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolSchema {
    pub fn to_ollama(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolResult {
    pub ok: bool,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<Value>,
}

impl ToolResult {
    pub fn ok_text(s: impl Into<String>) -> Self {
        Self { ok: true, output: s.into(), structured: None }
    }
    pub fn with_structured(mut self, v: Value) -> Self {
        self.structured = Some(v);
        self
    }
    pub fn err(s: impl Into<String>) -> Self {
        Self { ok: false, output: s.into(), structured: None }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    async fn call(&self, args: Value) -> Result<ToolResult>;
}

#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ToolRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.schema().name;
        self.tools.insert(name, tool);
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.tools.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn schemas(&self) -> Vec<Value> {
        self.tools.values().map(|t| t.schema().to_ollama()).collect()
    }

    pub fn schema_by_name(&self, name: &str) -> Option<ToolSchema> {
        self.tools.get(name).map(|t| t.schema())
    }

    pub async fn call(&self, name: &str, args: Value) -> Result<ToolResult> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| Error::ToolNotFound(name.into()))?;
        tool.call(args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;
    #[async_trait]
    impl Tool for Echo {
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "echo".into(),
                description: "echo".into(),
                parameters: serde_json::json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            }
        }
        async fn call(&self, args: Value) -> Result<ToolResult> {
            let text = args.get("text").and_then(Value::as_str).unwrap_or("");
            Ok(ToolResult::ok_text(text))
        }
    }

    #[tokio::test]
    async fn registry_calls_tool() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(Echo));
        let out = reg.call("echo", serde_json::json!({"text":"hi"})).await.unwrap();
        assert!(out.ok);
        assert_eq!(out.output, "hi");
        assert!(reg.schemas().iter().any(|s| s["function"]["name"] == "echo"));
    }
}
