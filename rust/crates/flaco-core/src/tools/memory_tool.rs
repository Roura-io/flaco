//! Memory as a tool — the model can `remember`, `recall`, and `list_memories`
//! to persist and retrieve facts across surfaces.

use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;
use crate::memory::Memory;
use super::{Tool, ToolResult, ToolSchema};

pub struct Remember { pub memory: Memory, pub default_user: String }
pub struct Recall   { pub memory: Memory, pub default_user: String }
pub struct ListMemories { pub memory: Memory, pub default_user: String }

#[async_trait]
impl Tool for Remember {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "remember".into(),
            description: "Save a fact or preference for the user. Use for durable info: preferences, team names, recurring tasks, API keys owners, etc.".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "content":{"type":"string"},
                    "kind":{"type":"string","enum":["fact","preference","note"],"default":"fact"},
                    "user":{"type":"string","description":"Optional override"}
                },
                "required":["content"]
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let content = args.get("content").and_then(Value::as_str).unwrap_or("").trim();
        if content.is_empty() { return Ok(ToolResult::err("content required")); }
        let kind = args.get("kind").and_then(Value::as_str).unwrap_or("fact");
        // Ignore any `user` override the model sends — each surface pins its
        // own user, and case mismatches silently create ghost memory silos.
        let user = &self.default_user;

        // Idempotency: if a fact with this exact content already exists for
        // this user, don't create a duplicate. This is the second line of
        // defense against the tool-loop duplication bug — even if the
        // runtime dedup fails to catch a repeat call (because the model
        // sent a slightly different arg shape), the memory store itself
        // refuses to double-write.
        if let Ok(existing) = self.memory.all_facts(user, 10_000) {
            if let Some(row) = existing.iter().find(|f| f.content == content) {
                return Ok(ToolResult::ok_text(format!(
                    "already remembered #{} [{}]: {}",
                    row.id, row.kind, content
                )));
            }
        }

        let id = self.memory.remember_fact(user, kind, content, None)?;
        Ok(ToolResult::ok_text(format!("remembered #{id} [{kind}]: {content}")))
    }
}

#[async_trait]
impl Tool for Recall {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "recall".into(),
            description: "Search the unified memory store for facts about a topic. Use to check what you already know about a user/project before answering.".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string"},
                    "limit":{"type":"integer","default":10},
                    "user":{"type":"string"}
                },
                "required":["query"]
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let q = args.get("query").and_then(Value::as_str).unwrap_or("");
        // Ignore any `user` override the model sends — each surface pins
        // its own user, and case-mismatches (Chris vs chris) were silently
        // hiding memories. Always use the surface default.
        let user = &self.default_user;
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
        let mut hits = self.memory.search_facts(user, q, limit)?;
        if hits.is_empty() {
            // Fall back to the most-recent facts so the model always gets
            // some context instead of a dead-end "no match".
            hits = self.memory.all_facts(user, limit)?;
            if hits.is_empty() { return Ok(ToolResult::ok_text("(no memories stored)")); }
        }
        let mut s = String::new();
        for h in &hits {
            s.push_str(&format!("#{} [{}] {}\n", h.id, h.kind, h.content));
        }
        Ok(ToolResult::ok_text(s).with_structured(serde_json::to_value(hits)?))
    }
}

#[async_trait]
impl Tool for ListMemories {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "list_memories".into(),
            description: "List every stored memory for the user, newest first.".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "limit":{"type":"integer","default":50},
                    "user":{"type":"string"}
                }
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let user = &self.default_user;
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;
        let hits = self.memory.all_facts(user, limit)?;
        if hits.is_empty() { return Ok(ToolResult::ok_text("(no memories yet)")); }
        let mut s = String::new();
        for h in &hits {
            s.push_str(&format!("#{} [{}] {}\n", h.id, h.kind, h.content));
        }
        Ok(ToolResult::ok_text(s))
    }
}
