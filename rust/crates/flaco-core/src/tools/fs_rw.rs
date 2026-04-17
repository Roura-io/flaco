use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;
use tokio::fs;

use crate::error::Result;
use super::{Tool, ToolResult, ToolSchema};

pub struct FsRead;
pub struct FsWrite;

fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return home.join(rest);
        }
    }
    PathBuf::from(p)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[async_trait]
impl Tool for FsRead {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "fs_read".into(),
            description: "Read a text file from disk and return its contents (truncated to 40KB).".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{"path":{"type":"string","description":"Absolute or ~/ path"}},
                "required":["path"]
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let path = args.get("path").and_then(Value::as_str).unwrap_or("");
        if path.is_empty() { return Ok(ToolResult::err("path required")); }
        let p = expand(path);
        match fs::read_to_string(&p).await {
            Ok(mut s) => {
                if s.len() > 40_000 { s.truncate(40_000); s.push_str("\n…[truncated]"); }
                Ok(ToolResult::ok_text(s))
            }
            Err(e) => Ok(ToolResult::err(format!("{}: {e}", p.display()))),
        }
    }
}

#[async_trait]
impl Tool for FsWrite {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "fs_write".into(),
            description: "Write text to a file (creates parent dirs).".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "content":{"type":"string"}
                },
                "required":["path","content"]
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let path = args.get("path").and_then(Value::as_str).unwrap_or("");
        let content = args.get("content").and_then(Value::as_str).unwrap_or("");
        if path.is_empty() { return Ok(ToolResult::err("path required")); }
        let p = expand(path);
        if let Some(parent) = p.parent() { fs::create_dir_all(parent).await.ok(); }
        fs::write(&p, content).await?;
        Ok(ToolResult::ok_text(format!("wrote {} bytes to {}", content.len(), p.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn write_then_read() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("a.txt");
        let w = FsWrite;
        w.call(serde_json::json!({"path": p.to_string_lossy(), "content": "hello"})).await.unwrap();
        let r = FsRead;
        let out = r.call(serde_json::json!({"path": p.to_string_lossy()})).await.unwrap();
        assert!(out.ok);
        assert_eq!(out.output, "hello");
    }
}
