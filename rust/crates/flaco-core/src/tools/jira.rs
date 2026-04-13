//! Minimal Jira REST client — just enough to create issues, epics, and links.

use async_trait::async_trait;
use base64::Engine as _;
use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, Result};
use super::{Tool, ToolResult, ToolSchema};

#[derive(Clone, Debug, Serialize)]
pub struct JiraIssueSummary {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub priority: Option<String>,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct JiraClient {
    base: String,
    email: String,
    token: String,
    http: reqwest::Client,
}

impl JiraClient {
    pub fn from_env() -> Option<Self> {
        let base = std::env::var("JIRA_URL").ok()?;
        let email = std::env::var("JIRA_EMAIL").ok()?;
        let token = std::env::var("JIRA_API_TOKEN").ok()?;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .ok()?;
        Some(Self { base, email, token, http })
    }

    fn auth(&self) -> String {
        let raw = format!("{}:{}", self.email, self.token);
        format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(raw))
    }

    pub async fn create_issue(
        &self,
        project_key: &str,
        summary: &str,
        description: &str,
        issue_type: &str,
    ) -> Result<Value> {
        let body = serde_json::json!({
            "fields": {
                "project": {"key": project_key},
                "summary": summary,
                "description": adf_paragraph(description),
                "issuetype": {"name": issue_type}
            }
        });
        let url = format!("{}/rest/api/3/issue", self.base);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let code = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Other(format!("jira {code}: {text}")));
        }
        Ok(resp.json().await?)
    }

    /// Search with JQL. Returns the parsed JSON response; callers pluck the
    /// `issues` array. Limits to a handful of fields to keep the payload tiny.
    pub async fn search(&self, jql: &str, max_results: u32) -> Result<Value> {
        let url = format!("{}/rest/api/3/search", self.base);
        let body = serde_json::json!({
            "jql": jql,
            "maxResults": max_results,
            "fields": ["summary", "status", "priority", "issuetype", "updated"],
        });
        let resp = self
            .http
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let code = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Other(format!("jira search {code}: {text}")));
        }
        Ok(resp.json().await?)
    }

    /// Pretty-print open issues assigned to `currentUser()` as one per line.
    /// Falls back to a friendly string when the JQL finds nothing.
    pub async fn my_open_issues(&self, max: u32) -> Result<Vec<JiraIssueSummary>> {
        let jql = "assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC";
        let resp = self.search(jql, max).await?;
        let mut out = Vec::new();
        if let Some(arr) = resp.get("issues").and_then(Value::as_array) {
            for issue in arr {
                let key = issue.get("key").and_then(Value::as_str).unwrap_or("?").to_string();
                let fields = issue.get("fields");
                let summary = fields
                    .and_then(|f| f.get("summary"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let status = fields
                    .and_then(|f| f.get("status"))
                    .and_then(|s| s.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string();
                let priority = fields
                    .and_then(|f| f.get("priority"))
                    .and_then(|p| p.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let kind = fields
                    .and_then(|f| f.get("issuetype"))
                    .and_then(|t| t.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("Task")
                    .to_string();
                out.push(JiraIssueSummary { key, summary, status, priority, kind });
            }
        }
        Ok(out)
    }

    pub async fn list_projects(&self) -> Result<Value> {
        let url = format!("{}/rest/api/3/project", self.base);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(Error::Other(format!("jira HTTP {}", resp.status())));
        }
        Ok(resp.json().await?)
    }
}

fn adf_paragraph(text: &str) -> Value {
    serde_json::json!({
        "type": "doc",
        "version": 1,
        "content": [
            {
                "type": "paragraph",
                "content": [{"type":"text","text": text}]
            }
        ]
    })
}

pub struct JiraCreateIssue { pub client: JiraClient }

#[async_trait]
impl Tool for JiraCreateIssue {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "jira_create_issue".into(),
            description: "Create a Jira issue in the given project.".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "project_key":{"type":"string","description":"e.g. FLACO"},
                    "summary":{"type":"string"},
                    "description":{"type":"string"},
                    "issue_type":{"type":"string","description":"Task|Story|Epic|Sub-task", "default":"Task"}
                },
                "required":["project_key","summary"]
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let project_key = args.get("project_key").and_then(Value::as_str).unwrap_or("");
        let summary = args.get("summary").and_then(Value::as_str).unwrap_or("");
        let description = args.get("description").and_then(Value::as_str).unwrap_or("");
        let issue_type = args.get("issue_type").and_then(Value::as_str).unwrap_or("Task");
        if project_key.is_empty() || summary.is_empty() {
            return Ok(ToolResult::err("project_key and summary required"));
        }
        let resp = self.client.create_issue(project_key, summary, description, issue_type).await?;
        let key = resp.get("key").and_then(Value::as_str).unwrap_or("?");
        Ok(ToolResult::ok_text(format!("Created {key}: {summary}")).with_structured(resp))
    }
}
