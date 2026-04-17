//! System connectors: system_info, ollama_admin.

use crate::Connector;
use serde_json::{json, Value};
use std::process::Command;

fn get_str<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or("")
}

fn run_cmd(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run {program}: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("{program} failed: {stderr}"))
    }
}

// ---------------------------------------------------------------------------
// System info
// ---------------------------------------------------------------------------

pub struct SystemInfo;

impl Connector for SystemInfo {
    fn name(&self) -> &str {
        "system_info"
    }
    fn description(&self) -> &str {
        "Get CPU, RAM, disk, network, and process information for this machine"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "enum": ["overview", "cpu", "memory", "disk", "network", "processes"],
                    "description": "What system information to retrieve"
                }
            },
            "required": ["query"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let query = get_str(input, "query");
        match query {
            "overview" => {
                let mut parts = Vec::new();
                if let Ok(uname) = run_cmd("uname", &["-a"]) {
                    parts.push(format!("System: {}", uname.trim()));
                }
                if let Ok(uptime) = run_cmd("uptime", &[]) {
                    parts.push(format!("Uptime: {}", uptime.trim()));
                }
                if cfg!(target_os = "macos") {
                    if let Ok(hw) = run_cmd("sysctl", &["-n", "hw.memsize"]) {
                        let bytes: u64 = hw.trim().parse().unwrap_or(0);
                        let gb = bytes / (1024 * 1024 * 1024);
                        parts.push(format!("Total RAM: {gb} GB"));
                    }
                    if let Ok(cpu) = run_cmd("sysctl", &["-n", "machdep.cpu.brand_string"]) {
                        parts.push(format!("CPU: {}", cpu.trim()));
                    }
                }
                Ok(parts.join("\n"))
            }
            "cpu" => {
                if cfg!(target_os = "macos") {
                    let mut parts = Vec::new();
                    if let Ok(brand) = run_cmd("sysctl", &["-n", "machdep.cpu.brand_string"]) {
                        parts.push(format!("CPU: {}", brand.trim()));
                    }
                    if let Ok(cores) = run_cmd("sysctl", &["-n", "hw.ncpu"]) {
                        parts.push(format!("Cores: {}", cores.trim()));
                    }
                    Ok(parts.join("\n"))
                } else {
                    run_cmd("lscpu", &[])
                }
            }
            "memory" => {
                if cfg!(target_os = "macos") {
                    run_cmd("vm_stat", &[])
                } else {
                    run_cmd("free", &["-h"])
                }
            }
            "disk" => run_cmd("df", &["-h"]),
            "network" => {
                if cfg!(target_os = "macos") {
                    run_cmd("ifconfig", &[])
                } else {
                    run_cmd("ip", &["addr"])
                }
            }
            "processes" => run_cmd("ps", &["aux", "--sort=-%mem"]),
            _ => Err(format!("unknown system_info query: {query}")),
        }
    }
    fn is_available(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Ollama admin (manage models on the Ollama host)
// ---------------------------------------------------------------------------

pub struct OllamaAdmin;

impl Connector for OllamaAdmin {
    fn name(&self) -> &str {
        "ollama_admin"
    }
    fn description(&self) -> &str {
        "List, pull, or delete models on the Ollama host; check running models"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "running", "pull", "show"],
                    "description": "Ollama admin action"
                },
                "model": { "type": "string", "description": "Model name (for pull/show)" }
            },
            "required": ["action"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let action = get_str(input, "action");
        let model = get_str(input, "model");

        let host = std::env::var("OLLAMA_BASE_URL")
            .or_else(|_| std::env::var("OLLAMA_HOST"))
            .unwrap_or_else(|_| "http://localhost:11434".into());
        let host = host.trim_end_matches('/');

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;

        match action {
            "list" => {
                let resp: Value = client
                    .get(format!("{host}/api/tags"))
                    .send()
                    .map_err(|e| e.to_string())?
                    .json()
                    .map_err(|e| e.to_string())?;
                let models = resp["models"].as_array();
                let mut lines = Vec::new();
                if let Some(arr) = models {
                    for m in arr {
                        let name = m["name"].as_str().unwrap_or("?");
                        let size = m["size"].as_u64().unwrap_or(0);
                        let gb = size as f64 / 1_073_741_824.0;
                        lines.push(format!("{name} ({gb:.1} GB)"));
                    }
                }
                if lines.is_empty() {
                    Ok("No models installed.".into())
                } else {
                    Ok(lines.join("\n"))
                }
            }
            "running" => {
                let resp: Value = client
                    .get(format!("{host}/api/ps"))
                    .send()
                    .map_err(|e| e.to_string())?
                    .json()
                    .map_err(|e| e.to_string())?;
                let models = resp["models"].as_array();
                let mut lines = Vec::new();
                if let Some(arr) = models {
                    for m in arr {
                        let name = m["name"].as_str().unwrap_or("?");
                        let size_vram = m["size_vram"].as_u64().unwrap_or(0);
                        let gb = size_vram as f64 / 1_073_741_824.0;
                        lines.push(format!("{name} (VRAM: {gb:.1} GB)"));
                    }
                }
                if lines.is_empty() {
                    Ok("No models currently loaded.".into())
                } else {
                    Ok(lines.join("\n"))
                }
            }
            "pull" => {
                if model.is_empty() {
                    return Err("model name is required for pull".into());
                }
                // Note: pull can take a long time. We start it and return status.
                let resp = client
                    .post(format!("{host}/api/pull"))
                    .json(&json!({"name": model, "stream": false}))
                    .timeout(std::time::Duration::from_secs(600))
                    .send()
                    .map_err(|e| format!("pull request failed: {e}"))?;
                if resp.status().is_success() {
                    Ok(format!("Model '{model}' pull initiated. This may take several minutes depending on model size."))
                } else {
                    Err(format!(
                        "Failed to pull model '{model}': HTTP {}",
                        resp.status()
                    ))
                }
            }
            "show" => {
                if model.is_empty() {
                    return Err("model name is required for show".into());
                }
                let resp: Value = client
                    .post(format!("{host}/api/show"))
                    .json(&json!({"name": model}))
                    .send()
                    .map_err(|e| e.to_string())?
                    .json()
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
            }
            _ => Err(format!("unknown ollama_admin action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        // Check if we can reach the Ollama host
        let host = std::env::var("OLLAMA_BASE_URL")
            .or_else(|_| std::env::var("OLLAMA_HOST"))
            .unwrap_or_else(|_| "http://localhost:11434".into());
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .ok()
            .and_then(|c| c.get(&host).send().ok())
            .is_some_and(|r| r.status().is_success())
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn connectors() -> Vec<Box<dyn Connector>> {
    vec![Box::new(SystemInfo), Box::new(OllamaAdmin)]
}
