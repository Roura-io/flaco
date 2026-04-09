//! Productivity connectors: macOS Calendar, Reminders, Notes, Contacts, Slack, Jira, Notion.

use crate::Connector;
use serde_json::{json, Value};
use std::process::Command;

fn get_str<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or("")
}

fn run_osascript(script: &str) -> Result<String, String> {
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .map_err(|e| format!("osascript failed: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn env_or_empty(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// macOS Calendar (via osascript)
// ---------------------------------------------------------------------------

pub struct Calendar;

impl Connector for Calendar {
    fn name(&self) -> &str {
        "calendar"
    }
    fn description(&self) -> &str {
        "Read macOS Calendar events for today or a date range"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["today", "upcoming"],
                    "description": "Calendar action"
                }
            },
            "required": ["action"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let action = get_str(input, "action");
        match action {
            "today" | "upcoming" => {
                let script = r#"
                    tell application "Calendar"
                        set today to current date
                        set endDate to today + (7 * days)
                        set output to ""
                        repeat with cal in calendars
                            set evts to (every event of cal whose start date ≥ today and start date ≤ endDate)
                            repeat with evt in evts
                                set output to output & (summary of evt) & " | " & (start date of evt as string) & linefeed
                            end repeat
                        end repeat
                        return output
                    end tell
                "#;
                run_osascript(script)
            }
            _ => Err(format!("unknown calendar action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        cfg!(target_os = "macos")
    }
}

// ---------------------------------------------------------------------------
// macOS Reminders
// ---------------------------------------------------------------------------

pub struct Reminders;

impl Connector for Reminders {
    fn name(&self) -> &str {
        "reminders"
    }
    fn description(&self) -> &str {
        "Read and create macOS Reminders"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "create"],
                    "description": "Reminders action"
                },
                "list_name": { "type": "string", "description": "Reminder list name (default: Reminders)" },
                "title": { "type": "string", "description": "Reminder title (for create)" }
            },
            "required": ["action"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let action = get_str(input, "action");
        let list_name = if get_str(input, "list_name").is_empty() {
            "Reminders"
        } else {
            get_str(input, "list_name")
        };
        match action {
            "list" => {
                let script = format!(
                    r#"tell application "Reminders"
                        set output to ""
                        set theList to list "{list_name}"
                        repeat with r in (reminders of theList whose completed is false)
                            set output to output & (name of r) & linefeed
                        end repeat
                        return output
                    end tell"#
                );
                run_osascript(&script)
            }
            "create" => {
                let title = get_str(input, "title");
                if title.is_empty() {
                    return Err("title is required for create".into());
                }
                let script = format!(
                    r#"tell application "Reminders"
                        tell list "{list_name}"
                            make new reminder with properties {{name:"{title}"}}
                        end tell
                        return "Created reminder: {title}"
                    end tell"#
                );
                run_osascript(&script)
            }
            _ => Err(format!("unknown reminders action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        cfg!(target_os = "macos")
    }
}

// ---------------------------------------------------------------------------
// macOS Notes
// ---------------------------------------------------------------------------

pub struct Notes;

impl Connector for Notes {
    fn name(&self) -> &str {
        "notes"
    }
    fn description(&self) -> &str {
        "Search and read macOS Notes"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "search"],
                    "description": "Notes action"
                },
                "query": { "type": "string", "description": "Search query" }
            },
            "required": ["action"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let action = get_str(input, "action");
        match action {
            "list" => {
                let script = r#"
                    tell application "Notes"
                        set output to ""
                        repeat with n in (notes of default account)
                            set output to output & (name of n) & linefeed
                            if (count of output) > 2000 then exit repeat
                        end repeat
                        return output
                    end tell
                "#;
                run_osascript(script)
            }
            "search" => {
                let query = get_str(input, "query");
                let script = format!(
                    r#"tell application "Notes"
                        set output to ""
                        repeat with n in (notes of default account whose name contains "{query}")
                            set output to output & (name of n) & " | " & (body of n) & linefeed
                            if (count of output) > 4000 then exit repeat
                        end repeat
                        return output
                    end tell"#
                );
                run_osascript(&script)
            }
            _ => Err(format!("unknown notes action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        cfg!(target_os = "macos")
    }
}

// ---------------------------------------------------------------------------
// macOS Contacts
// ---------------------------------------------------------------------------

pub struct Contacts;

impl Connector for Contacts {
    fn name(&self) -> &str {
        "contacts"
    }
    fn description(&self) -> &str {
        "Search macOS Contacts by name"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Name to search for" }
            },
            "required": ["name"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let name = get_str(input, "name");
        let script = format!(
            r#"tell application "Contacts"
                set output to ""
                set matches to (every person whose name contains "{name}")
                repeat with p in matches
                    set output to output & (name of p)
                    if (count of emails of p) > 0 then
                        set output to output & " | " & (value of first email of p)
                    end if
                    if (count of phones of p) > 0 then
                        set output to output & " | " & (value of first phone of p)
                    end if
                    set output to output & linefeed
                end repeat
                return output
            end tell"#
        );
        run_osascript(&script)
    }
    fn is_available(&self) -> bool {
        cfg!(target_os = "macos")
    }
}

// ---------------------------------------------------------------------------
// Slack (needs SLACK_TOKEN env var)
// ---------------------------------------------------------------------------

pub struct Slack;

impl Connector for Slack {
    fn name(&self) -> &str {
        "slack"
    }
    fn description(&self) -> &str {
        "Send messages and read channels in Slack (set SLACK_TOKEN to enable)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["send", "read_channel"],
                    "description": "Slack action"
                },
                "channel": { "type": "string", "description": "Channel name or ID" },
                "message": { "type": "string", "description": "Message text (for send)" }
            },
            "required": ["action", "channel"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let token = env_or_empty("SLACK_TOKEN");
        if token.is_empty() {
            return Err("SLACK_TOKEN environment variable is not set. Set it to your Slack bot token to enable this integration.".into());
        }
        let action = get_str(input, "action");
        let channel = get_str(input, "channel");
        let client = reqwest::blocking::Client::new();
        match action {
            "send" => {
                let message = get_str(input, "message");
                let resp = client
                    .post("https://slack.com/api/chat.postMessage")
                    .bearer_auth(&token)
                    .json(&json!({"channel": channel, "text": message}))
                    .send()
                    .map_err(|e| e.to_string())?;
                let body: Value = resp.json().map_err(|e| e.to_string())?;
                if body["ok"].as_bool() == Some(true) {
                    Ok("Message sent.".into())
                } else {
                    Err(format!(
                        "Slack API error: {}",
                        body["error"].as_str().unwrap_or("unknown")
                    ))
                }
            }
            "read_channel" => {
                let resp = client
                    .get("https://slack.com/api/conversations.history")
                    .bearer_auth(&token)
                    .query(&[("channel", channel), ("limit", "10")])
                    .send()
                    .map_err(|e| e.to_string())?;
                let body: Value = resp.json().map_err(|e| e.to_string())?;
                let messages = body["messages"].as_array();
                let mut lines = Vec::new();
                if let Some(arr) = messages {
                    for msg in arr {
                        let text = msg["text"].as_str().unwrap_or("");
                        let user = msg["user"].as_str().unwrap_or("?");
                        lines.push(format!("[{user}] {text}"));
                    }
                }
                Ok(lines.join("\n"))
            }
            _ => Err(format!("unknown slack action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        !env_or_empty("SLACK_TOKEN").is_empty()
    }
}

// ---------------------------------------------------------------------------
// Jira (needs JIRA_URL, JIRA_EMAIL, JIRA_TOKEN env vars)
// ---------------------------------------------------------------------------

pub struct Jira;

impl Connector for Jira {
    fn name(&self) -> &str {
        "jira"
    }
    fn description(&self) -> &str {
        "Search and create Jira issues (set JIRA_URL, JIRA_EMAIL, JIRA_TOKEN to enable)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search", "view"],
                    "description": "Jira action"
                },
                "query": { "type": "string", "description": "JQL query or issue key" }
            },
            "required": ["action", "query"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let base_url = env_or_empty("JIRA_URL");
        let email = env_or_empty("JIRA_EMAIL");
        let token = env_or_empty("JIRA_TOKEN");
        if base_url.is_empty() || token.is_empty() {
            return Err("Set JIRA_URL, JIRA_EMAIL, and JIRA_TOKEN environment variables to enable Jira integration.".into());
        }
        let action = get_str(input, "action");
        let query = get_str(input, "query");
        let client = reqwest::blocking::Client::new();
        match action {
            "search" => {
                let url = format!("{base_url}/rest/api/3/search");
                let resp: Value = client
                    .get(&url)
                    .basic_auth(&email, Some(&token))
                    .query(&[("jql", query), ("maxResults", "10")])
                    .send()
                    .map_err(|e| e.to_string())?
                    .json()
                    .map_err(|e| e.to_string())?;
                let issues = resp["issues"].as_array();
                let mut lines = Vec::new();
                if let Some(arr) = issues {
                    for issue in arr {
                        let key = issue["key"].as_str().unwrap_or("?");
                        let summary = issue["fields"]["summary"].as_str().unwrap_or("");
                        let status = issue["fields"]["status"]["name"].as_str().unwrap_or("");
                        lines.push(format!("{key}: {summary} [{status}]"));
                    }
                }
                Ok(lines.join("\n"))
            }
            "view" => {
                let url = format!("{base_url}/rest/api/3/issue/{query}");
                let resp: Value = client
                    .get(&url)
                    .basic_auth(&email, Some(&token))
                    .send()
                    .map_err(|e| e.to_string())?
                    .json()
                    .map_err(|e| e.to_string())?;
                let key = resp["key"].as_str().unwrap_or("?");
                let summary = resp["fields"]["summary"].as_str().unwrap_or("");
                let status = resp["fields"]["status"]["name"].as_str().unwrap_or("");
                let desc = resp["fields"]["description"]
                    .as_str()
                    .unwrap_or("(no description)");
                Ok(format!("{key}: {summary}\nStatus: {status}\n\n{desc}"))
            }
            _ => Err(format!("unknown jira action: {action}")),
        }
    }
    fn is_available(&self) -> bool {
        !env_or_empty("JIRA_URL").is_empty() && !env_or_empty("JIRA_TOKEN").is_empty()
    }
}

// ---------------------------------------------------------------------------
// Notion (needs NOTION_TOKEN env var)
// ---------------------------------------------------------------------------

pub struct Notion;

impl Connector for Notion {
    fn name(&self) -> &str {
        "notion"
    }
    fn description(&self) -> &str {
        "Search and read Notion pages (set NOTION_TOKEN to enable)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search"],
                    "description": "Notion action"
                },
                "query": { "type": "string", "description": "Search query" }
            },
            "required": ["action", "query"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let token = env_or_empty("NOTION_TOKEN");
        if token.is_empty() {
            return Err(
                "Set NOTION_TOKEN environment variable to enable Notion integration.".into(),
            );
        }
        let query = get_str(input, "query");
        let client = reqwest::blocking::Client::new();
        let resp: Value = client
            .post("https://api.notion.com/v1/search")
            .bearer_auth(&token)
            .header("Notion-Version", "2022-06-28")
            .json(&json!({"query": query, "page_size": 10}))
            .send()
            .map_err(|e| e.to_string())?
            .json()
            .map_err(|e| e.to_string())?;
        let results = resp["results"].as_array();
        let mut lines = Vec::new();
        if let Some(arr) = results {
            for page in arr {
                let title = page["properties"]["title"]["title"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|t| t["plain_text"].as_str())
                    .or_else(|| {
                        page["properties"]["Name"]["title"]
                            .as_array()
                            .and_then(|a| a.first())
                            .and_then(|t| t["plain_text"].as_str())
                    })
                    .unwrap_or("Untitled");
                let url = page["url"].as_str().unwrap_or("");
                lines.push(format!("{title}\n  {url}"));
            }
        }
        if lines.is_empty() {
            Ok("No results found.".into())
        } else {
            Ok(lines.join("\n"))
        }
    }
    fn is_available(&self) -> bool {
        !env_or_empty("NOTION_TOKEN").is_empty()
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn connectors() -> Vec<Box<dyn Connector>> {
    vec![
        Box::new(Calendar),
        Box::new(Reminders),
        Box::new(Notes),
        Box::new(Contacts),
        Box::new(Slack),
        Box::new(Jira),
        Box::new(Notion),
    ]
}
