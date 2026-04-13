//! Minimal GitHub client: create branch, create PR.

use async_trait::async_trait;
use serde_json::Value;

use crate::error::{Error, Result};
use super::{Tool, ToolResult, ToolSchema};

#[derive(Clone, Debug)]
pub struct GitHubClient {
    token: String,
    http: reqwest::Client,
}

impl GitHubClient {
    pub fn from_env() -> Option<Self> {
        let token = std::env::var("GITHUB_TOKEN").ok()?;
        let http = reqwest::Client::builder()
            .user_agent("flacoAi/2.0")
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .ok()?;
        Some(Self { token, http })
    }

    pub async fn create_pr(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        head: &str,
        base: &str,
        body: &str,
    ) -> Result<Value> {
        let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls");
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .json(&serde_json::json!({
                "title": title, "head": head, "base": base, "body": body,
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let code = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Other(format!("github {code}: {body}")));
        }
        Ok(resp.json().await?)
    }
}

pub struct GithubCreatePr { pub client: GitHubClient }

#[async_trait]
impl Tool for GithubCreatePr {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "github_create_pr".into(),
            description: "Create a GitHub pull request.".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "owner":{"type":"string"},
                    "repo":{"type":"string"},
                    "title":{"type":"string"},
                    "head":{"type":"string","description":"source branch"},
                    "base":{"type":"string","description":"target branch, e.g. main"},
                    "body":{"type":"string"}
                },
                "required":["owner","repo","title","head","base"]
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let owner = args.get("owner").and_then(Value::as_str).unwrap_or("");
        let repo = args.get("repo").and_then(Value::as_str).unwrap_or("");
        let title = args.get("title").and_then(Value::as_str).unwrap_or("");
        let head = args.get("head").and_then(Value::as_str).unwrap_or("");
        let base = args.get("base").and_then(Value::as_str).unwrap_or("main");
        let body = args.get("body").and_then(Value::as_str).unwrap_or("");
        if owner.is_empty() || repo.is_empty() || title.is_empty() || head.is_empty() {
            return Ok(ToolResult::err("owner, repo, title, head required"));
        }
        let v = self.client.create_pr(owner, repo, title, head, base, body).await?;
        let url = v.get("html_url").and_then(Value::as_str).unwrap_or("?");
        Ok(ToolResult::ok_text(format!("opened PR: {url}")).with_structured(v))
    }
}
