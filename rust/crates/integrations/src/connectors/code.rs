//! Code & development connectors: GitHub, git_ops, Docker, npm, crates_io, Homebrew.

use crate::Connector;
use serde_json::{json, Value};
use std::process::Command;

// ---------------------------------------------------------------------------
// Helper: run a CLI command and return stdout
// ---------------------------------------------------------------------------

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

fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn get_str<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or("")
}

// ---------------------------------------------------------------------------
// GitHub (via `gh` CLI)
// ---------------------------------------------------------------------------

pub struct GitHub;

impl Connector for GitHub {
    fn name(&self) -> &str {
        "github"
    }
    fn description(&self) -> &str {
        "Search repos, list issues/PRs, view code on GitHub (via gh CLI)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search_repos", "list_issues", "list_prs", "view_issue", "view_pr", "repo_info"],
                    "description": "The GitHub action to perform"
                },
                "query": { "type": "string", "description": "Search query or repo slug (owner/repo)" },
                "number": { "type": "integer", "description": "Issue or PR number (for view actions)" }
            },
            "required": ["action"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let action = get_str(input, "action");
        let query = get_str(input, "query");
        match action {
            "search_repos" => run_cmd("gh", &["search", "repos", query, "--limit", "10"]),
            "list_issues" => run_cmd("gh", &["issue", "list", "-R", query, "--limit", "20"]),
            "list_prs" => run_cmd("gh", &["pr", "list", "-R", query, "--limit", "20"]),
            "view_issue" => {
                let num = input["number"].as_i64().unwrap_or(0).to_string();
                run_cmd("gh", &["issue", "view", &num, "-R", query])
            }
            "view_pr" => {
                let num = input["number"].as_i64().unwrap_or(0).to_string();
                run_cmd("gh", &["pr", "view", &num, "-R", query])
            }
            "repo_info" => run_cmd("gh", &["repo", "view", query]),
            _ => Err(format!("unknown github action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        command_exists("gh")
    }
}

// ---------------------------------------------------------------------------
// Git operations (advanced)
// ---------------------------------------------------------------------------

pub struct GitOps;

impl Connector for GitOps {
    fn name(&self) -> &str {
        "git_ops"
    }
    fn description(&self) -> &str {
        "Advanced git operations: log, blame, diff, stash, cherry-pick preview"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["log", "blame", "diff_stat", "stash_list", "shortlog", "branch_list"],
                    "description": "Git operation to perform"
                },
                "args": { "type": "string", "description": "Additional arguments (file path, range, etc.)" }
            },
            "required": ["action"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let action = get_str(input, "action");
        let args = get_str(input, "args");
        match action {
            "log" => {
                let extra = if args.is_empty() {
                    "--oneline -20"
                } else {
                    args
                };
                let parts: Vec<&str> = extra.split_whitespace().collect();
                let mut cmd_args = vec!["log"];
                cmd_args.extend(parts);
                run_cmd("git", &cmd_args)
            }
            "blame" => run_cmd("git", &["blame", args]),
            "diff_stat" => run_cmd("git", &["diff", "--stat"]),
            "stash_list" => run_cmd("git", &["stash", "list"]),
            "shortlog" => run_cmd("git", &["shortlog", "-sn", "--all", "--no-merges"]),
            "branch_list" => run_cmd("git", &["branch", "-a", "--sort=-committerdate"]),
            _ => Err(format!("unknown git_ops action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        command_exists("git")
    }
}

// ---------------------------------------------------------------------------
// Docker
// ---------------------------------------------------------------------------

pub struct Docker;

impl Connector for Docker {
    fn name(&self) -> &str {
        "docker"
    }
    fn description(&self) -> &str {
        "List/start/stop containers, view logs, inspect images (via docker CLI)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["ps", "images", "logs", "inspect", "stats"],
                    "description": "Docker action"
                },
                "target": { "type": "string", "description": "Container or image name/ID" },
                "tail": { "type": "integer", "description": "Number of log lines (for logs action)" }
            },
            "required": ["action"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let action = get_str(input, "action");
        let target = get_str(input, "target");
        match action {
            "ps" => run_cmd(
                "docker",
                &[
                    "ps",
                    "--format",
                    "table {{.Names}}\t{{.Status}}\t{{.Ports}}",
                ],
            ),
            "images" => run_cmd(
                "docker",
                &[
                    "images",
                    "--format",
                    "table {{.Repository}}\t{{.Tag}}\t{{.Size}}",
                ],
            ),
            "logs" => {
                let tail = input["tail"].as_i64().unwrap_or(50).to_string();
                run_cmd("docker", &["logs", "--tail", &tail, target])
            }
            "inspect" => run_cmd("docker", &["inspect", target]),
            "stats" => run_cmd(
                "docker",
                &[
                    "stats",
                    "--no-stream",
                    "--format",
                    "table {{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}",
                ],
            ),
            _ => Err(format!("unknown docker action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        command_exists("docker")
    }
}

// ---------------------------------------------------------------------------
// npm registry (HTTP, no auth)
// ---------------------------------------------------------------------------

pub struct NpmRegistry;

impl Connector for NpmRegistry {
    fn name(&self) -> &str {
        "npm_registry"
    }
    fn description(&self) -> &str {
        "Search npm packages, check versions, read package info"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search", "info"],
                    "description": "npm registry action"
                },
                "package": { "type": "string", "description": "Package name or search query" }
            },
            "required": ["action", "package"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let action = get_str(input, "action");
        let package = get_str(input, "package");
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        match action {
            "search" => {
                let url = format!("https://registry.npmjs.org/-/v1/search?text={package}&size=10");
                let resp: Value = client
                    .get(&url)
                    .send()
                    .map_err(|e| e.to_string())?
                    .json()
                    .map_err(|e| e.to_string())?;
                let objects = resp["objects"].as_array();
                let mut lines = Vec::new();
                if let Some(arr) = objects {
                    for obj in arr {
                        let name = obj["package"]["name"].as_str().unwrap_or("?");
                        let version = obj["package"]["version"].as_str().unwrap_or("?");
                        let desc = obj["package"]["description"].as_str().unwrap_or("");
                        lines.push(format!("{name}@{version} — {desc}"));
                    }
                }
                Ok(lines.join("\n"))
            }
            "info" => {
                let url = format!("https://registry.npmjs.org/{package}/latest");
                let resp: Value = client
                    .get(&url)
                    .send()
                    .map_err(|e| e.to_string())?
                    .json()
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
            }
            _ => Err(format!("unknown npm action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        true // HTTP-based, always available
    }
}

// ---------------------------------------------------------------------------
// crates.io (HTTP, no auth)
// ---------------------------------------------------------------------------

pub struct CratesIo;

impl Connector for CratesIo {
    fn name(&self) -> &str {
        "crates_io"
    }
    fn description(&self) -> &str {
        "Search Rust crates, check versions on crates.io"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search", "info"],
                    "description": "crates.io action"
                },
                "query": { "type": "string", "description": "Crate name or search query" }
            },
            "required": ["action", "query"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let action = get_str(input, "action");
        let query = get_str(input, "query");
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("flacoai/0.1.0")
            .build()
            .map_err(|e| e.to_string())?;
        match action {
            "search" => {
                let url = format!("https://crates.io/api/v1/crates?q={query}&per_page=10");
                let resp: Value = client
                    .get(&url)
                    .send()
                    .map_err(|e| e.to_string())?
                    .json()
                    .map_err(|e| e.to_string())?;
                let crates = resp["crates"].as_array();
                let mut lines = Vec::new();
                if let Some(arr) = crates {
                    for c in arr {
                        let name = c["name"].as_str().unwrap_or("?");
                        let version = c["newest_version"].as_str().unwrap_or("?");
                        let desc = c["description"].as_str().unwrap_or("");
                        let dl = c["downloads"].as_i64().unwrap_or(0);
                        lines.push(format!("{name} v{version} ({dl} downloads) — {desc}"));
                    }
                }
                Ok(lines.join("\n"))
            }
            "info" => {
                let url = format!("https://crates.io/api/v1/crates/{query}");
                let resp: Value = client
                    .get(&url)
                    .send()
                    .map_err(|e| e.to_string())?
                    .json()
                    .map_err(|e| e.to_string())?;
                let c = &resp["crate"];
                Ok(format!(
                    "{} v{}\nDescription: {}\nDownloads: {}\nRepository: {}\nDocumentation: {}",
                    c["name"].as_str().unwrap_or("?"),
                    c["newest_version"].as_str().unwrap_or("?"),
                    c["description"].as_str().unwrap_or(""),
                    c["downloads"].as_i64().unwrap_or(0),
                    c["repository"].as_str().unwrap_or("n/a"),
                    c["documentation"].as_str().unwrap_or("n/a"),
                ))
            }
            _ => Err(format!("unknown crates_io action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Homebrew (HTTP, no auth)
// ---------------------------------------------------------------------------

pub struct Homebrew;

impl Connector for Homebrew {
    fn name(&self) -> &str {
        "homebrew"
    }
    fn description(&self) -> &str {
        "Search and get info on Homebrew formulae"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search", "info"],
                    "description": "Homebrew action"
                },
                "formula": { "type": "string", "description": "Formula name or search query" }
            },
            "required": ["action", "formula"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let action = get_str(input, "action");
        let formula = get_str(input, "formula");
        match action {
            "search" => run_cmd("brew", &["search", formula]),
            "info" => run_cmd("brew", &["info", formula]),
            _ => Err(format!("unknown homebrew action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        command_exists("brew")
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn connectors() -> Vec<Box<dyn Connector>> {
    vec![
        Box::new(GitHub),
        Box::new(GitOps),
        Box::new(Docker),
        Box::new(NpmRegistry),
        Box::new(CratesIo),
        Box::new(Homebrew),
    ]
}
