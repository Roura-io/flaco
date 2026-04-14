use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use crate::error::Result;
use super::{Tool, ToolResult, ToolSchema};

/// Minimal `bash` tool. Intentionally conservative — refuses a small deny-list
/// to avoid obvious footguns. Chris can loosen this later.
pub struct Bash {
    pub workdir: Option<std::path::PathBuf>,
}

impl Bash {
    pub fn new() -> Self { Self { workdir: None } }
    pub fn with_workdir(dir: impl Into<std::path::PathBuf>) -> Self {
        Self { workdir: Some(dir.into()) }
    }
}

const DENY: &[&str] = &[
    "rm -rf /",
    "mkfs",
    ":(){:|:&};:",
    "dd if=/dev/zero of=/dev/",
    "> /dev/sda",
];

#[async_trait]
impl Tool for Bash {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "bash".into(),
            description: "Run a shell command on the local machine. Returns stdout+stderr. Use for simple, safe commands (git, ls, curl, cat). Do NOT destructive ops.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type":"string", "description":"The shell command to run"},
                    "timeout_seconds": {"type":"integer", "description":"Optional timeout, default 60"}
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Value) -> Result<ToolResult> {
        let cmd = args.get("command").and_then(Value::as_str).unwrap_or("").trim();
        if cmd.is_empty() {
            return Ok(ToolResult::err("empty command"));
        }
        for bad in DENY {
            if cmd.contains(bad) {
                return Ok(ToolResult::err(format!("command denied by flaco-core: {bad}")));
            }
        }
        let timeout = args
            .get("timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(60);

        let mut c = Command::new("bash");
        c.arg("-lc").arg(cmd);
        if let Some(dir) = &self.workdir {
            c.current_dir(dir);
        }
        let fut = c.output();
        let output = match tokio::time::timeout(std::time::Duration::from_secs(timeout), fut).await {
            Ok(r) => r?,
            Err(_) => return Ok(ToolResult::err(format!("timeout after {timeout}s"))),
        };
        let mut body = String::new();
        body.push_str(&String::from_utf8_lossy(&output.stdout));
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            body.push_str("\n[stderr]\n");
            body.push_str(&stderr);
        }
        if body.len() > 60_000 {
            body.truncate(60_000);
            body.push_str("\n…[truncated]");
        }
        Ok(ToolResult {
            ok: output.status.success(),
            output: body,
            structured: Some(serde_json::json!({"exit_code": output.status.code()})),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bash_echoes() {
        let t = Bash::new();
        let r = t.call(serde_json::json!({"command":"echo hello"})).await.unwrap();
        assert!(r.ok);
        assert!(r.output.contains("hello"));
    }

    #[tokio::test]
    async fn bash_denies_rmrf_root() {
        let t = Bash::new();
        let r = t.call(serde_json::json!({"command":"rm -rf /tmp/no && rm -rf /"})).await.unwrap();
        assert!(!r.ok);
    }
}
