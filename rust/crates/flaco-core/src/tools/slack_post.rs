//! Tool that lets the model post a message to a Slack channel via the bot
//! token. Useful for "remind me to…" / cross-surface notifications.

use async_trait::async_trait;
use serde_json::Value;

use crate::error::{Error, Result};
use super::{Tool, ToolResult, ToolSchema};

pub struct SlackPost {
    pub bot_token: String,
    pub default_channel: String,
    pub http: reqwest::Client,
}

impl SlackPost {
    pub fn from_env() -> Option<Self> {
        let bot_token = std::env::var("SLACK_BOT_TOKEN").ok()?;
        let default_channel = std::env::var("SLACK_CHANNEL")
            .unwrap_or_else(|_| "flaco-general".into());
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .ok()?;
        Some(Self { bot_token, default_channel, http })
    }
}

#[async_trait]
impl Tool for SlackPost {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "slack_post".into(),
            description: "Post a message to a Slack channel. Use for notifications, reminders, daily briefs that Chris should see in Slack.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "channel": {"type":"string","description":"Channel name or ID (default flaco-general)"},
                    "text": {"type":"string"}
                },
                "required": ["text"]
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let channel = args
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or(&self.default_channel)
            .to_string();
        let text = args.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if text.is_empty() { return Ok(ToolResult::err("text required")); }
        let body = serde_json::json!({"channel": channel, "text": text});
        let resp = self
            .http
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.bot_token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        let j: Value = resp.json().await?;
        if j.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(ToolResult::ok_text(format!("posted to #{channel}")))
        } else {
            Err(Error::Other(format!("slack_post failed: {j}")))
        }
    }
}
